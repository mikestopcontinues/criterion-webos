use super::tests::{Database, wait};
use super::*;
use std::{
    sync::{Arc, Mutex, mpsc},
    task::{Context, Poll, Waker},
    time::Duration,
};

#[test]
fn an_unpolled_future_cannot_keep_the_worker_alive_after_disposal() {
    let fence = Db8WriteFence::with_transport(Database(Arc::new(Mutex::new((7, false))))).unwrap();
    let reservation = fence.reserve();
    drop(fence);
    assert!(matches!(wait(reservation), Err(FenceError::Unavailable)));
}

#[test]
fn dropping_an_issued_future_does_not_cancel_or_clear_its_reservation() {
    struct HeldGet {
        database: Database,
        entered: Option<mpsc::SyncSender<()>>,
        release: mpsc::Receiver<()>,
    }
    impl Db8Transport for HeldGet {
        fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(1)).unwrap();
            }
            self.database.get(deadline)
        }
        fn put(
            &mut self,
            revision: u64,
            issued: bool,
            deadline: Instant,
        ) -> Result<Vec<u8>, FenceError> {
            self.database.put(revision, issued, deadline)
        }
    }
    let (entered, entry) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let storage = Arc::new(Mutex::new((7, false)));
    let fence = Db8WriteFence::with_transport(HeldGet {
        database: Database(storage.clone()),
        entered: Some(entered),
        release: released,
    })
    .unwrap();
    let mut reservation = fence.reserve();
    let waker = Waker::noop();
    assert!(matches!(
        reservation.as_mut().poll(&mut Context::from_waker(waker)),
        Poll::Pending
    ));
    entry.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(reservation);
    release.send(()).unwrap();
    assert_eq!(wait(fence.state()), Ok(FenceState::PossiblyIssued));
    assert!(matches!(wait(fence.reserve()), Err(FenceError::Held)));
    drop(fence);
    let restarted = Db8WriteFence::with_transport(Database(storage)).unwrap();
    assert_eq!(wait(restarted.state()), Ok(FenceState::PossiblyIssued));
}

#[test]
fn a_late_put_acknowledgment_never_returns_a_write_reservation() {
    struct LatePut(Database);
    impl Db8Transport for LatePut {
        fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError> {
            self.0.get(deadline)
        }
        fn put(
            &mut self,
            revision: u64,
            issued: bool,
            deadline: Instant,
        ) -> Result<Vec<u8>, FenceError> {
            let acknowledgment = self.0.put(revision, issued, deadline)?;
            // The fixture violates the transport's return bound to exercise the adapter's
            // independent after-call deadline admission. No provider operation is involved.
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(10),
            );
            Ok(acknowledgment)
        }
    }
    let storage = Arc::new(Mutex::new((7, false)));
    let fence = Db8WriteFence::with_transport(LatePut(Database(storage.clone()))).unwrap();
    assert!(matches!(
        wait(fence.reserve()),
        Err(FenceError::Unconfirmed)
    ));
    assert_eq!(wait(fence.state()), Err(FenceError::Unconfirmed));
    drop(fence);
    let restarted = Db8WriteFence::with_transport(Database(storage)).unwrap();
    assert_eq!(wait(restarted.state()), Ok(FenceState::PossiblyIssued));
    assert!(matches!(wait(restarted.reserve()), Err(FenceError::Held)));
}

#[test]
fn every_storage_leg_uses_the_same_original_operation_deadline() {
    struct Deadlines {
        database: Database,
        held: Option<Instant>,
        leg: u8,
    }
    impl Db8Transport for Deadlines {
        fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError> {
            match self.leg {
                0 => self.held = Some(deadline),
                2 | 4 => assert_eq!(Some(deadline), self.held),
                _ => panic!("unexpected get leg"),
            }
            self.leg += 1;
            self.database.get(deadline)
        }
        fn put(
            &mut self,
            revision: u64,
            issued: bool,
            deadline: Instant,
        ) -> Result<Vec<u8>, FenceError> {
            match self.leg {
                1 => assert_eq!(Some(deadline), self.held),
                3 => self.held = Some(deadline),
                _ => panic!("unexpected put leg"),
            }
            self.leg += 1;
            self.database.put(revision, issued, deadline)
        }
    }
    let fence = Db8WriteFence::with_transport(Deadlines {
        database: Database(Arc::new(Mutex::new((7, false)))),
        held: None,
        leg: 0,
    })
    .unwrap();
    let reservation = wait(fence.reserve()).unwrap();
    assert_eq!(wait(fence.complete(reservation)), Ok(()));
}

#[test]
fn disposal_joins_an_issued_storage_write_before_returning() {
    struct HeldPut {
        database: Database,
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Db8Transport for HeldPut {
        fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError> {
            self.database.get(deadline)
        }
        fn put(
            &mut self,
            revision: u64,
            issued: bool,
            deadline: Instant,
        ) -> Result<Vec<u8>, FenceError> {
            let acknowledgment = self.database.put(revision, issued, deadline)?;
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(acknowledgment)
        }
    }
    let (entered, entry) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let storage = Arc::new(Mutex::new((7, false)));
    let fence = Db8WriteFence::with_transport(HeldPut {
        database: Database(storage.clone()),
        entered,
        release: released,
    })
    .unwrap();
    let mut reservation = fence.reserve();
    let waker = Waker::noop();
    assert!(matches!(
        reservation.as_mut().poll(&mut Context::from_waker(waker)),
        Poll::Pending
    ));
    entry.recv_timeout(Duration::from_secs(1)).unwrap();
    let (dropping, started) = mpsc::sync_channel(1);
    let (disposed, done) = mpsc::sync_channel(1);
    let disposal = std::thread::spawn(move || {
        dropping.send(()).unwrap();
        drop(fence);
        disposed.send(()).unwrap();
    });
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        done.recv_timeout(Duration::from_millis(25)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    disposal.join().unwrap();
    // The original issued operation settled; no capability reached completion.
    drop(wait(reservation).unwrap());
    let restarted = Db8WriteFence::with_transport(Database(storage)).unwrap();
    assert_eq!(wait(restarted.state()), Ok(FenceState::PossiblyIssued));
}
