use criterion_native_caller_probe::{
    CloseWait, Failure, KEYMANAGER_ID, KeyAdmission, SERVICE_ID, Snapshot, State, admit_bridge,
    admit_keymanager,
};

#[test]
fn fixed_protocol_reply_admits_the_running_broker_snapshot() {
    let payload = br#"{"returnValue":true,"version":"0.1.0","state":"running","counter":7,"pid":42,"subscribers":1,"stopAcknowledged":false,"cleanupConfirmed":false,"exitCode":null}"#;
    let reply = admit_bridge(Some(SERVICE_ID), 11, 11, payload).unwrap();
    assert_eq!((reply.pid, reply.counter, reply.subscribers), (42, 7, 1));
}

fn closed(pid: u32) -> Snapshot {
    Snapshot {
        pid,
        counter: 8,
        subscribers: 0,
        state: State::Closed,
        stop_acknowledged: true,
        cleanup_confirmed: true,
        exit_code: Some(0),
    }
}

#[test]
fn close_failure_and_wrong_process_keep_completion_until_the_deadline() {
    let mut wait = CloseWait::new(Some(closed(42)), 3000);
    wait.observe(1000, Err(Failure::ServiceRejected));
    wait.observe(1000, Ok(closed(43)));
    assert!(!wait.complete(2999));
    assert!(!wait.retired());
    wait.observe(1000, Ok(closed(42)));
    assert!(wait.complete(2999));
    assert!(wait.retired());
}

#[test]
fn absent_key_reply_proves_only_read_only_call_admission() {
    assert_eq!(
        admit_keymanager(
            Some(KEYMANAGER_ID),
            2,
            2,
            br#"{"returnValue":false,"errorCode":-10001,"errorText":"Key not found"}"#
        ),
        Ok(KeyAdmission::MissingKey)
    );
}

#[test]
fn known_service_rejection_is_coarse_and_does_not_admit_a_snapshot() {
    assert_eq!(
        admit_bridge(
            Some(SERVICE_ID),
            11,
            11,
            br#"{"returnValue":false,"errorCode":"unauthorized"}"#
        ),
        Err(Failure::ServiceRejected)
    );
}

#[test]
fn reply_admission_rejects_untrusted_identity_and_non_object_json() {
    let payload = br#"{"returnValue":false,"errorCode":"unauthorized"}"#;
    for source in [
        None,
        Some("com.mikestopcontinues.criterion.probe.player"),
        Some("com.mikestopcontinues.criterion.probe.player.bridge.extra"),
    ] {
        assert_eq!(
            admit_bridge(source, 11, 11, payload),
            Err(Failure::WrongSource)
        );
    }
    for (token, expected) in [(0, 11), (11, 0), (12, 11)] {
        assert_eq!(
            admit_bridge(Some(SERVICE_ID), token, expected, payload),
            Err(Failure::WrongToken)
        );
    }
    for payload in [
        &br#"[false,"unauthorized"]"#[..],
        br#"{"returnValue":false,"returnValue":false,"errorCode":"unauthorized"}"#,
        br#"{"returnValue":false,"errorCode":"unauthorized","appid":"claimed"}"#,
        br#"{"returnValue":false,"errorCode":"unauthorized"} {}"#,
        br#"{"returnValue":false,"errorCode":"unknown"}"#,
    ] {
        assert_eq!(
            admit_bridge(Some(SERVICE_ID), 11, 11, payload),
            Err(Failure::InvalidResponse)
        );
    }
    assert_eq!(
        admit_bridge(Some(SERVICE_ID), 11, 11, &vec![b' '; 4097]),
        Err(Failure::InvalidResponse)
    );
}

#[test]
fn liveness_uses_the_initial_attach_counter_and_same_process() {
    let initial = Snapshot {
        pid: 42,
        counter: 7,
        subscribers: 1,
        state: State::Running,
        stop_acknowledged: false,
        cleanup_confirmed: false,
        exit_code: None,
    };
    assert!(
        Snapshot {
            counter: 8,
            ..initial
        }
        .pinged_after(initial)
    );
    assert!(!initial.pinged_after(initial));
    assert!(
        !Snapshot {
            pid: 43,
            counter: 8,
            ..initial
        }
        .pinged_after(initial)
    );
    assert!(
        !Snapshot {
            cleanup_confirmed: true,
            counter: 8,
            ..initial
        }
        .pinged_after(initial)
    );
    assert!(!closed(42).pinged_after(initial));
}

#[test]
fn forced_stop_or_exit_without_pipe_cleanup_cannot_claim_retirement() {
    let mut wait = CloseWait::new(Some(closed(42)), 3000);
    for reply in [
        Snapshot {
            cleanup_confirmed: false,
            ..closed(42)
        },
        Snapshot {
            stop_acknowledged: false,
            ..closed(42)
        },
        Snapshot {
            exit_code: Some(1),
            ..closed(42)
        },
        Snapshot {
            subscribers: 1,
            ..closed(42)
        },
    ] {
        wait.observe(1000, Ok(reply));
        assert!(!wait.retired());
    }
    assert!(!wait.complete(2999));
    assert!(wait.complete(3000));
    assert!(!wait.retired());
}

#[test]
fn close_cannot_regress_below_the_highest_acknowledged_counter() {
    let mut wait = CloseWait::new(Some(closed(42)), 3000);
    wait.observe(
        1000,
        Ok(Snapshot {
            counter: 7,
            ..closed(42)
        }),
    );
    assert!(!wait.retired());
    wait.observe(
        1000,
        Ok(Snapshot {
            counter: 0,
            ..closed(42)
        }),
    );
    assert!(!wait.retired());
    wait.observe(1000, Ok(closed(42)));
    assert!(wait.retired());
}

#[test]
fn close_at_or_after_the_original_deadline_cannot_be_admitted() {
    for now in [3000, 3001, u64::MAX] {
        let mut wait = CloseWait::new(Some(closed(42)), 3000);
        wait.observe(now, Ok(closed(42)));
        assert!(!wait.retired());
        assert!(wait.complete(now));
    }
    let mut wait = CloseWait::new(Some(closed(42)), 3000);
    wait.observe(2999, Ok(closed(42)));
    assert!(wait.retired());
}

#[test]
fn keymanager_denial_or_existing_key_never_implies_symmetric_key_ownership() {
    assert_eq!(
        admit_keymanager(
            Some(KEYMANAGER_ID),
            2,
            2,
            br#"{"returnValue":false,"errorCode":-20017,"errorText":"Get error from keymaster"}"#
        ),
        Err(Failure::ServiceRejected)
    );
    assert_eq!(
        admit_keymanager(
            Some(KEYMANAGER_ID),
            2,
            2,
            br#"{"returnValue":true,"pubkey":"YWJj"}"#
        ),
        Ok(KeyAdmission::ExistingPublicKey)
    );
    assert_eq!(
        admit_keymanager(
            Some(KEYMANAGER_ID),
            2,
            2,
            br#"{"returnValue":true,"pubkey":"YWJj","handle":"private"}"#
        ),
        Err(Failure::InvalidResponse)
    );
    assert_eq!(
        admit_keymanager(
            Some(SERVICE_ID),
            2,
            2,
            br#"{"returnValue":false,"errorCode":-10001,"errorText":"Key not found"}"#
        ),
        Err(Failure::WrongSource)
    );
}

#[test]
fn success_schema_requires_every_field_and_bounded_counter_process_and_slots() {
    let payload = r#"{"returnValue":true,"version":"0.1.0","state":"running","counter":7,"pid":42,"subscribers":1,"stopAcknowledged":false,"cleanupConfirmed":false,"exitCode":null}"#;
    for invalid in [
        payload.replace("\"exitCode\":null", "\"unexpected\":null"),
        payload.replace("\"counter\":7", "\"counter\":1000001"),
        payload.replace("\"pid\":42", "\"pid\":0"),
        payload.replace("\"pid\":42", "\"pid\":2147483648"),
        payload.replace("\"subscribers\":1", "\"subscribers\":4"),
        payload.replace("0.1.0", "0.2.0"),
    ] {
        assert_eq!(
            admit_bridge(Some(SERVICE_ID), 11, 11, invalid.as_bytes()),
            Err(Failure::InvalidResponse)
        );
    }
}
