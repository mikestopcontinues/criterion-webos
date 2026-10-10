// SPDX-License-Identifier: GPL-3.0-or-later
//! An accepted write is never an abortable read job. Its durable reservation
//! outlives publication authority, and only native acknowledgment/NotIssued clears it.
use criterion_account::{AccountClient, Error, WatchListContentType, WriteFailure};
use criterion_platform::write_fence::{Db8WriteFence, FenceState, IssuedWriteFence};
use criterion_provider::MediaId;
use criterion_session::{MonotonicClock, Session, Transport};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::{
    runtime::{Handle, Runtime},
    task::JoinHandle,
};

pub(crate) struct WriteRequest {
    pub(crate) epoch: u64,
    pub(crate) root: MediaId,
    pub(crate) content_type: WatchListContentType,
    pub(crate) remove: bool,
}

/// Coarse lifecycle facts, never an Applied claim or retained provider body.
pub(crate) struct WriteOutcome {
    pub(crate) epoch: u64,
    pub(crate) acknowledged: bool,
    pub(crate) possibly_issued: bool,
    /// No reservation was started, or its authorized completion was acknowledged.
    pub(crate) fence_ready: bool,
}

pub(super) struct Writes {
    fence: Option<Arc<dyn IssuedWriteFence>>,
    initializing: Option<JoinHandle<bool>>,
    initialized: bool,
    ready: bool,
    job: Option<(u64, JoinHandle<WriteOutcome>)>,
    live: Option<Arc<AtomicBool>>,
}

impl Writes {
    pub(super) fn new() -> Self {
        Self::with_fence(
            Db8WriteFence::new()
                .ok()
                .map(|fence| Arc::new(fence) as Arc<dyn IssuedWriteFence>),
        )
    }

    pub(super) fn with_fence(fence: Option<Arc<dyn IssuedWriteFence>>) -> Self {
        Self {
            fence,
            initializing: None,
            initialized: false,
            ready: false,
            job: None,
            live: None,
        }
    }

    pub(super) fn initialize(&mut self, runtime: &Handle) {
        if self.initialized || self.initializing.is_some() {
            return;
        }
        let Some(fence) = self.fence.clone() else {
            self.initialized = true;
            return;
        };
        self.initializing =
            Some(runtime.spawn(async move { fence.state().await == Ok(FenceState::Clean) }));
    }

    pub(super) fn poll_initialization(&mut self, runtime: &Runtime) {
        if self
            .initializing
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            self.ready = runtime
                .block_on(
                    self.initializing
                        .take()
                        .expect("finished startup observation"),
                )
                .unwrap_or(false);
            self.initialized = true;
        }
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready && !self.is_active()
    }
    pub(super) fn is_active(&self) -> bool {
        self.job.is_some()
    }

    pub(super) fn start<
        A: criterion_account::Transport + 'static,
        S: Transport + 'static,
        C: MonotonicClock + 'static,
    >(
        &mut self,
        runtime: &Handle,
        account: Arc<AccountClient<A>>,
        session: Arc<Session<S, C>>,
        request: WriteRequest,
    ) -> Result<(), Error> {
        if !self.is_ready() {
            return Err(Error::Busy);
        }
        let fence = self.fence.clone().ok_or(Error::Unavailable)?;
        let live = Arc::new(AtomicBool::new(true));
        self.live = Some(live.clone());
        self.ready = false;
        self.job = Some((
            request.epoch,
            runtime.spawn(async move {
                let mut outcome = WriteOutcome {
                    epoch: request.epoch,
                    acknowledged: false,
                    possibly_issued: false,
                    fence_ready: false,
                };
                if !live.load(Ordering::SeqCst) {
                    // Startup was Clean and no reservation was polled; the next intent
                    // must still reserve against the current durable revision.
                    outcome.fence_ready = true;
                    return outcome;
                }
                let Ok(reservation) = fence.reserve().await else {
                    return outcome;
                };
                let result = if !live.load(Ordering::SeqCst) {
                    Err(WriteFailure::NotIssued(Error::Stale))
                } else if request.remove {
                    account.remove_watch_list(&session, &request.root).await
                } else {
                    account
                        .add_watch_list(&session, &request.root, request.content_type)
                        .await
                };
                let may_clear = match result {
                    Ok(receipt) => {
                        // sync is a native acknowledgment flag, never evidence of membership.
                        let _ = receipt;
                        outcome.acknowledged = true;
                        outcome.possibly_issued = true;
                        true
                    }
                    Err(WriteFailure::NotIssued(_)) => true,
                    Err(WriteFailure::Unconfirmed(_)) => {
                        outcome.possibly_issued = true;
                        false
                    }
                };
                if may_clear {
                    outcome.fence_ready = fence.complete(reservation).await.is_ok();
                }
                outcome
            }),
        ));
        Ok(())
    }

    pub(super) fn retire(&mut self) {
        if let Some(live) = &self.live {
            live.store(false, Ordering::SeqCst);
        }
    }

    pub(super) fn poll(&mut self, runtime: &Runtime) -> Option<WriteOutcome> {
        if !self
            .job
            .as_ref()
            .is_some_and(|(_, task)| task.is_finished())
        {
            return None;
        }
        self.take(runtime)
    }

    fn take(&mut self, runtime: &Runtime) -> Option<WriteOutcome> {
        let (epoch, task) = self.job.take()?;
        // A task failure cannot supply proof of NotIssued or acknowledged completion.
        // Retire its UI intent and invalidate cached membership without printing an error.
        let result = runtime.block_on(task).unwrap_or(WriteOutcome {
            epoch,
            acknowledged: false,
            possibly_issued: true,
            fence_ready: false,
        });
        self.live = None;
        self.ready = result.fence_ready;
        Some(result)
    }

    pub(super) fn finish(&mut self, runtime: &Runtime) {
        self.retire();
        if self.job.is_some() {
            let _ = self.take(runtime);
        }
        if let Some(initializing) = self.initializing.take() {
            let _ = runtime.block_on(initializing);
            self.initialized = true;
        }
    }
}
