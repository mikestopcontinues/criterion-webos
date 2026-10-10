use super::tests::{Database, wait};
use super::*;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

fn record(revision: u64, issued: bool) -> Vec<u8> {
    format!(r#"{{"returnValue":true,"results":[{{"_id":"crit.write.v1","_kind":"com.mikestopcontinues.criterion.unofficial.issuedwrite:1","_rev":{revision},"version":1,"possiblyIssued":{issued}}}]}}"#).into_bytes()
}
fn acknowledgment(revision: u64) -> Vec<u8> {
    format!(r#"{{"returnValue":true,"results":[{{"id":"crit.write.v1","rev":{revision}}}]}}"#)
        .into_bytes()
}
enum Call {
    Get(Result<Vec<u8>, FenceError>),
    Put(u64, bool, Result<Vec<u8>, FenceError>),
}
struct Replies(VecDeque<Call>);
impl Db8Transport for Replies {
    fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError> {
        assert!(deadline > Instant::now());
        let Some(Call::Get(result)) = self.0.pop_front() else {
            panic!("unexpected DB8 get")
        };
        result
    }
    fn put(
        &mut self,
        revision: u64,
        issued: bool,
        deadline: Instant,
    ) -> Result<Vec<u8>, FenceError> {
        assert!(deadline > Instant::now());
        let Some(Call::Put(expected_revision, expected_issued, result)) = self.0.pop_front() else {
            panic!("unexpected DB8 put")
        };
        assert_eq!((revision, issued), (expected_revision, expected_issued));
        result
    }
}
fn scripted(calls: impl IntoIterator<Item = Call>) -> Db8WriteFence {
    Db8WriteFence::with_transport(Replies(calls.into_iter().collect())).unwrap()
}

#[test]
fn missing_corrupt_and_unavailable_startup_cannot_default_to_clean() {
    let invalid = [
        br#"{"returnValue":true,"results":[]}"#.to_vec(),
        b"{bad".to_vec(),
        record(0, false),
        String::from_utf8(record(7, false))
            .unwrap()
            .replace("\"version\":1", "\"version\":2")
            .into_bytes(),
        String::from_utf8(record(7, false))
            .unwrap()
            .replace("\"version\":1", "\"version\":1,\"extra\":false")
            .into_bytes(),
        String::from_utf8(record(7, false))
            .unwrap()
            .replace("\"version\":1", "\"version\":1,\"version\":1")
            .into_bytes(),
        String::from_utf8(record(7, false))
            .unwrap()
            .replace("crit.write.v1", "other.write.v1")
            .into_bytes(),
        String::from_utf8(record(7, false))
            .unwrap()
            .replace("possiblyIssued\":false", "possiblyIssued\":null")
            .into_bytes(),
        vec![b' '; 4097],
    ];
    for bytes in invalid {
        let fence = scripted([Call::Get(Ok(bytes))]);
        assert!(wait(fence.state()).is_err());
        assert!(matches!(
            wait(fence.reserve()),
            Err(FenceError::Unconfirmed)
        ));
        assert_eq!(wait(fence.state()), Err(FenceError::Unconfirmed));
    }
    let fence = scripted([Call::Get(Err(FenceError::Unavailable))]);
    assert_eq!(wait(fence.state()), Err(FenceError::Unavailable));
    assert!(matches!(
        wait(fence.reserve()),
        Err(FenceError::Unconfirmed)
    ));
}

#[test]
fn reservation_requires_exact_current_acknowledgment_and_readback() {
    for (ack, readback) in [
        (Ok(acknowledgment(8)), Some(record(9, true))),
        (Ok(acknowledgment(8)), Some(record(8, false))),
        (Ok(acknowledgment(7)), None),
        (
            Ok(br#"{"returnValue":true,"results":[{"id":"other","rev":8}]}"#.to_vec()),
            None,
        ),
        (
            Ok(br#"{"returnValue":true,"results":[],"extra":true}"#.to_vec()),
            None,
        ),
        (Err(FenceError::Unconfirmed), None),
        (Err(FenceError::Invalid), None),
    ] {
        let mut calls = vec![Call::Get(Ok(record(7, false))), Call::Put(7, true, ack)];
        if let Some(bytes) = readback {
            calls.push(Call::Get(Ok(bytes)));
        }
        let fence = scripted(calls);
        assert!(matches!(
            wait(fence.reserve()),
            Err(FenceError::Unconfirmed)
        ));
        assert_eq!(wait(fence.state()), Err(FenceError::Unconfirmed));
        assert!(matches!(
            wait(fence.reserve()),
            Err(FenceError::Unconfirmed)
        ));
    }
}

#[test]
fn completion_requires_the_held_revision_and_exact_clean_readback() {
    for (ack, readback) in [
        (Err(FenceError::Invalid), None),
        (Err(FenceError::Unconfirmed), None),
        (Ok(acknowledgment(9)), Some(record(10, false))),
        (Ok(acknowledgment(9)), Some(record(9, true))),
    ] {
        let mut calls = vec![
            Call::Get(Ok(record(7, false))),
            Call::Put(7, true, Ok(acknowledgment(8))),
            Call::Get(Ok(record(8, true))),
            Call::Put(8, false, ack),
        ];
        if let Some(bytes) = readback {
            calls.push(Call::Get(Ok(bytes)));
        }
        let fence = scripted(calls);
        let held = wait(fence.reserve()).unwrap();
        assert_eq!(wait(fence.complete(held)), Err(FenceError::Unconfirmed));
        assert_eq!(wait(fence.state()), Err(FenceError::Unconfirmed));
        assert!(matches!(
            wait(fence.reserve()),
            Err(FenceError::Unconfirmed)
        ));
    }
}

#[test]
fn restart_cannot_clear_an_issued_or_lost_acknowledgment_record() {
    let storage = Arc::new(Mutex::new((7, false)));
    let fence = Db8WriteFence::with_transport(Database(storage.clone())).unwrap();
    let held = wait(fence.reserve()).unwrap();
    drop(held);
    drop(fence);
    let restarted = Db8WriteFence::with_transport(Database(storage)).unwrap();
    assert_eq!(wait(restarted.state()), Ok(FenceState::PossiblyIssued));
    assert!(matches!(wait(restarted.reserve()), Err(FenceError::Held)));
    drop(restarted);

    struct LostAck(Database);
    impl Db8Transport for LostAck {
        fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError> {
            self.0.get(deadline)
        }
        fn put(
            &mut self,
            revision: u64,
            issued: bool,
            deadline: Instant,
        ) -> Result<Vec<u8>, FenceError> {
            self.0.put(revision, issued, deadline)?;
            Err(FenceError::Unconfirmed)
        }
    }
    let storage = Arc::new(Mutex::new((7, false)));
    let fence = Db8WriteFence::with_transport(LostAck(Database(storage.clone()))).unwrap();
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
