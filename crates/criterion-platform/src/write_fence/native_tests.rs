// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual native owner exercised through the fence with controlled host LS2/GLib calls.
use super::{tests::wait, *};
use crate::main_bus::fixture::{Fixture, Mode};
use std::{
    process::{Command, Stdio},
    task::{Context, Poll, Waker},
    time::Duration,
};

fn open_native_fence() -> Result<Db8WriteFence, FenceError> {
    Ok(Db8WriteFence {
        worker: worker::Worker::start_factory(native::NativeTransport::open)?,
    })
}

#[test]
fn native_fence_round_trip_uses_one_fixed_main_owner_on_the_worker() {
    let fixture = Fixture::new();
    let fence = open_native_fence().unwrap();
    assert_eq!(wait(fence.state()), Ok(FenceState::Clean));
    assert!(matches!(open_native_fence(), Err(FenceError::Unavailable)));
    let reservation = wait(fence.reserve()).unwrap();
    assert_eq!(wait(fence.complete(reservation)), Ok(()));
    assert_eq!(wait(fence.state()), Ok(FenceState::Clean));
    drop(fence);
    fixture.assert_closed(&[
        "luna://com.palm.db/get",
        "luna://com.palm.db/get",
        "luna://com.palm.db/put",
        "luna://com.palm.db/get",
        "luna://com.palm.db/put",
        "luna://com.palm.db/get",
        "luna://com.palm.db/get",
    ]);
}

#[test]
fn dropped_native_future_retains_issued_reservation_until_worker_settlement() {
    let fixture = Fixture::new();
    let fence = open_native_fence().unwrap();
    let (entered, release) = fixture.hold_dispatch();
    let mut reservation = fence.reserve();
    assert!(matches!(
        reservation
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(reservation);
    release.send(()).unwrap();
    assert_eq!(wait(fence.state()), Ok(FenceState::PossiblyIssued));
    assert!(matches!(wait(fence.reserve()), Err(FenceError::Held)));
    drop(fence);
    fixture.assert_closed(&[
        "luna://com.palm.db/get",
        "luna://com.palm.db/put",
        "luna://com.palm.db/get",
    ]);
}

#[test]
fn native_reply_requires_exact_db8_sender_and_current_token() {
    for mode in [Mode::WrongSender, Mode::WrongToken] {
        let fixture = Fixture::new();
        fixture.mode(mode);
        let fence = open_native_fence().unwrap();
        assert_eq!(wait(fence.state()), Err(FenceError::Unconfirmed));
        assert_eq!(wait(fence.state()), Err(FenceError::Unconfirmed));
        drop(fence);
        fixture.assert_closed(&["luna://com.palm.db/get"]);
    }
}

#[test]
fn failed_unregister_holds_the_main_claim_until_process_exit() {
    const CHILD: &str = "CRITERION_MAIN_UNREGISTER_FIXTURE";
    if std::env::var_os(CHILD).is_some() {
        let fixture = Fixture::new();
        fixture.mode(Mode::WrongSender);
        fixture.fail_unregister();
        let first = open_native_fence().unwrap();
        assert_eq!(wait(first.state()), Err(FenceError::Unconfirmed));
        drop(first);
        assert!(matches!(open_native_fence(), Err(FenceError::Unavailable)));
        fixture.assert_quarantined();
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "write_fence::native_tests::failed_unregister_holds_the_main_claim_until_process_exit",
        ])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "quarantine fixture subprocess failed");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("quarantine fixture subprocess did not settle");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
