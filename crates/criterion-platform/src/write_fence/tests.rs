use super::*;
use std::{
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

struct Park(std::thread::Thread);
impl Wake for Park {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
pub(super) fn wait<T>(mut future: FenceFuture<T>) -> Result<T, FenceError> {
    let bound = Instant::now() + std::time::Duration::from_secs(12);
    let waker = Waker::from(Arc::new(Park(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => {
                let remaining = bound.saturating_duration_since(Instant::now());
                assert!(
                    !remaining.is_zero(),
                    "fence fixture did not settle within its bound"
                );
                std::thread::park_timeout(remaining);
            }
        }
    }
}

pub(super) struct Database(pub(super) Arc<Mutex<(u64, bool)>>);
impl Db8Transport for Database {
    fn get(&mut self, _: Instant) -> Result<Vec<u8>, FenceError> {
        let (revision, issued) = *self.0.lock().unwrap();
        Ok(format!(r#"{{"returnValue":true,"results":[{{"_id":"crit.write.v1","_kind":"com.mikestopcontinues.criterion.unofficial.issuedwrite:1","_rev":{revision},"version":1,"possiblyIssued":{issued}}}]}}"#).into_bytes())
    }
    fn put(&mut self, revision: u64, issued: bool, _: Instant) -> Result<Vec<u8>, FenceError> {
        let mut record = self.0.lock().unwrap();
        if record.0 != revision {
            return Err(FenceError::Invalid);
        }
        *record = (revision + 1, issued);
        Ok(format!(
            r#"{{"returnValue":true,"results":[{{"id":"crit.write.v1","rev":{}}}]}}"#,
            record.0
        )
        .into_bytes())
    }
}

#[test]
fn an_acknowledged_reservation_holds_all_writes_until_authorized_completion() {
    let storage = Arc::new(Mutex::new((7, false)));
    let fence = Db8WriteFence::with_transport(Database(storage)).unwrap();
    assert_eq!(wait(fence.state()), Ok(FenceState::Clean));
    let reservation = wait(fence.reserve()).unwrap();
    assert_eq!(wait(fence.state()), Ok(FenceState::PossiblyIssued));
    assert!(matches!(wait(fence.reserve()), Err(FenceError::Held)));
    assert_eq!(wait(fence.complete(reservation)), Ok(()));
    assert_eq!(wait(fence.state()), Ok(FenceState::Clean));
}

#[test]
fn an_unpolled_reservation_cannot_issue_a_storage_write() {
    let storage = Arc::new(Mutex::new((7, false)));
    let fence = Db8WriteFence::with_transport(Database(storage)).unwrap();
    let unpolled = fence.reserve();
    assert_eq!(wait(fence.state()), Ok(FenceState::Clean));
    drop(unpolled);
    assert_eq!(wait(fence.state()), Ok(FenceState::Clean));
}
