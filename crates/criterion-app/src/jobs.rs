// SPDX-License-Identifier: GPL-3.0-or-later
//! One main-thread publication slot for cancellable HTTP/decode work.
//! Futures never publish directly into the application or own native resources.
use std::{future::Future, pin::Pin};
use tokio::{runtime::Handle, task::JoinHandle};

pub(crate) struct Jobs<T> {
    active: Option<JoinHandle<T>>,
    retiring: bool,
    pending: Option<Pending<T>>,
}
struct Pending<T> {
    runtime: Handle,
    work: Pin<Box<dyn Future<Output = T> + Send>>,
}

impl<T: Send + 'static> Jobs<T> {
    pub(crate) fn new() -> Self {
        Self {
            active: None,
            retiring: false,
            pending: None,
        }
    }

    pub(crate) fn replace(
        &mut self,
        runtime: &Handle,
        work: impl Future<Output = T> + Send + 'static,
    ) {
        self.cancel();
        if self.active.is_some() {
            self.pending = Some(Pending {
                runtime: runtime.clone(),
                work: Box::pin(work),
            });
        } else {
            self.active = Some(runtime.spawn(work));
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.pending = None;
        if let Some(job) = &self.active {
            job.abort();
            self.retiring = true;
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some() || self.pending.is_some()
    }

    pub(crate) async fn take_ready(&mut self) -> Option<Result<T, tokio::task::JoinError>> {
        if !self.active.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        let result = match self.active.take() {
            Some(job) => job.await,
            None => return None,
        };
        if std::mem::take(&mut self.retiring) {
            if let Some(pending) = self.pending.take() {
                self.active = Some(pending.runtime.spawn(pending.work));
            }
            None
        } else {
            Some(result)
        }
    }

    /// Complete an already issued bounded operation during explicit disposal. The
    /// owning transport must supply its total deadline; this method adds no retry.
    pub(crate) async fn finish(&mut self) -> Option<Result<T, tokio::task::JoinError>> {
        if self.retiring {
            if let Some(job) = self.active.take() {
                let _ = job.await;
            }
            self.retiring = false;
            if let Some(pending) = self.pending.take() {
                self.active = Some(pending.runtime.spawn(pending.work));
            }
        }
        match self.active.take() {
            Some(job) => Some(job.await),
            None => None,
        }
    }
}

impl<T> Drop for Jobs<T> {
    fn drop(&mut self) {
        if let Some(job) = self.active.take() {
            job.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::oneshot;

    struct Retire(Arc<AtomicUsize>);
    impl Drop for Retire {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    async fn pump() {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }
    async fn latest<T: Send + 'static>(owner: &mut Jobs<T>) -> T {
        for _ in 0..32 {
            if let Some(result) = owner.take_ready().await {
                return result.unwrap();
            }
            tokio::task::yield_now().await;
        }
        panic!("latest job did not become ready");
    }

    #[tokio::test]
    async fn replacement_retires_started_work_and_only_latest_can_publish() {
        let retired = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = oneshot::channel();
        let (hold_tx, hold_rx) = oneshot::channel::<()>();
        let guard = Retire(retired.clone());
        let mut owner = Jobs::new();
        owner.replace(&Handle::current(), async move {
            let _guard = guard;
            let _ = started_tx.send(());
            let _ = hold_rx.await;
            "old"
        });
        pump().await;
        assert!(started_rx.await.is_ok(), "old request must actually start");
        owner.replace(&Handle::current(), async { "latest" });
        pump().await;
        assert_eq!(retired.load(Ordering::SeqCst), 1);
        assert!(hold_tx.send(()).is_err());
        assert_eq!(latest(&mut owner).await, "latest");
        assert!(owner.take_ready().await.is_none());
    }

    #[tokio::test]
    async fn cancellation_discards_already_finished_unpublished_result() {
        let mut owner = Jobs::new();
        owner.replace(&Handle::current(), async { 7 });
        pump().await;
        owner.cancel();
        assert!(owner.take_ready().await.is_none());
        owner.replace(&Handle::current(), async { 9 });
        pump().await;
        assert_eq!(latest(&mut owner).await, 9);
    }

    #[tokio::test]
    async fn pending_read_does_not_wait_and_drop_retires_started_work() {
        let retired = Arc::new(AtomicUsize::new(0));
        let guard = Retire(retired.clone());
        let (started_tx, started_rx) = oneshot::channel();
        let (_hold_tx, hold_rx) = oneshot::channel::<()>();
        let mut owner = Jobs::new();
        owner.replace(&Handle::current(), async move {
            let _guard = guard;
            let _ = started_tx.send(());
            let _ = hold_rx.await;
            3
        });
        pump().await;
        assert!(started_rx.await.is_ok());
        assert!(owner.take_ready().await.is_none());
        drop(owner);
        pump().await;
        assert_eq!(retired.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn replacement_waits_for_old_transport_retirement() {
        use std::sync::{Condvar, Mutex, mpsc};
        struct Fence {
            gate: Arc<(Mutex<bool>, Condvar)>,
            entered: mpsc::Sender<()>,
        }
        impl Drop for Fence {
            fn drop(&mut self) {
                let _ = self.entered.send(());
                let (lock, wake) = &*self.gate;
                drop(
                    wake.wait_while(lock.lock().unwrap(), |released| !*released)
                        .unwrap(),
                );
            }
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (retiring_tx, retiring_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let (replacement_tx, replacement_rx) = mpsc::channel();
        let (_hold_tx, hold_rx) = oneshot::channel::<()>();
        let fence = Fence {
            gate: gate.clone(),
            entered: retiring_tx,
        };
        let mut owner = Jobs::new();
        owner.replace(runtime.handle(), async move {
            let _fence = fence;
            let _ = started_tx.send(());
            let _ = hold_rx.await;
            "old"
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        owner.replace(runtime.handle(), async move {
            let _ = replacement_tx.send(());
            "replacement"
        });
        retiring_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let started_before_retirement = replacement_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_ok();
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert!(
            !started_before_retirement,
            "replacement entered while old request still owned its transport permit"
        );
    }
}
