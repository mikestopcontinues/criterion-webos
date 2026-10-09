use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
struct Sequence(Mutex<Vec<Response>>);
impl Transport for Sequence {
    async fn post(&self, _request: Request) -> Result<Response, Error> {
        Ok(self.0.lock().unwrap().remove(0))
    }
}
fn json(body: &str) -> Response {
    Response {
        status: 200,
        body: SecretBody::new(body.as_bytes().to_vec()),
    }
}
#[tokio::test]
async fn linking_exposes_issuer_instructions_and_waits_before_polling() {
    let now = Arc::new(AtomicU64::new(100));
    let transport = Sequence(Mutex::new(vec![
        json(
            r#"{"device_code":"device-secret","user_code":"ABCD-EFGH","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD-EFGH","expires_in":900,"interval":5}"#,
        ),
        json(
            r#"{"access_token":"access-secret","refresh_token":"refresh-secret","expires_in":3600}"#,
        ),
    ]));
    let session =
        Session::with_transport(Configuration::production(), transport, Clock(now.clone()));
    let instructions = session.start_link().await.unwrap();
    assert_eq!(instructions.user_code.expose(), "ABCD-EFGH");
    assert_eq!(
        instructions.verification_uri_complete.expose(),
        "https://login.criterion.com/activate?user_code=ABCD-EFGH"
    );
    assert_eq!(instructions.expires_at, Duration::from_secs(1000));
    assert_eq!(
        session.poll_once().await,
        Ok(PollOutcome::WaitUntil(Duration::from_secs(105)))
    );
    now.store(105, Ordering::SeqCst);
    assert_eq!(session.poll_once().await, Ok(PollOutcome::Authorized));
}

#[derive(Clone)]
struct Held {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl Transport for Held {
    async fn post(&self, request: Request) -> Result<Response, Error> {
        if request.endpoint == Endpoint::DeviceCode {
            return Ok(json(
                r#"{"device_code":"device-secret","user_code":"ABCD-EFGH","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD-EFGH","expires_in":900,"interval":5}"#,
            ));
        }
        self.entered.notify_one();
        self.release.notified().await;
        Ok(json(
            r#"{"access_token":"late-access-secret","refresh_token":"late-refresh-secret","expires_in":3600}"#,
        ))
    }
}

#[tokio::test]
async fn cancelled_poll_drops_late_tokens_and_never_reopens_the_session() {
    let now = Arc::new(AtomicU64::new(0));
    let held = Held {
        entered: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    };
    let session = Session::with_transport(
        Configuration::production(),
        held.clone(),
        Clock(now.clone()),
    );
    session.start_link().await.unwrap();
    now.store(5, Ordering::SeqCst);
    let poll = session.poll_once();
    tokio::pin!(poll);
    tokio::select! {
        value = &mut poll => panic!("held token request returned early: {value:?}"),
        () = held.entered.notified() => {}
    }
    session.cancel();
    held.release.notify_one();
    assert_eq!(poll.await, Err(Error::Stale));
    assert_eq!(session.status(), Status::Cancelled);
    assert_eq!(session.with_access_token(|_| ()), Err(Error::NoSession));
}

struct Script(Mutex<Vec<Result<Response, Error>>>);
impl Transport for Script {
    async fn post(&self, _request: Request) -> Result<Response, Error> {
        self.0.lock().unwrap().remove(0)
    }
}
fn failure(code: &str) -> Response {
    Response {
        status: 400,
        body: SecretBody::new(format!("{{\"error\":\"{code}\"}}").into_bytes()),
    }
}
fn device() -> Response {
    json(
        r#"{"device_code":"device-secret","user_code":"ABCD-EFGH","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD-EFGH","expires_in":900,"interval":5}"#,
    )
}

#[tokio::test]
async fn pending_slowdown_and_network_backoff_obey_the_issuer_then_denial_terminates() {
    let now = Arc::new(AtomicU64::new(100));
    let session = Session::with_transport(
        Configuration::production(),
        Script(Mutex::new(vec![
            Ok(device()),
            Ok(failure("authorization_pending")),
            Ok(failure("slow_down")),
            Err(Error::Deadline),
            Ok(failure("authorization_pending")),
            Ok(failure("access_denied")),
        ])),
        Clock(now.clone()),
    );
    session.start_link().await.unwrap();
    now.store(105, Ordering::SeqCst);
    assert_eq!(
        session.poll_once().await,
        Ok(PollOutcome::Pending(Duration::from_secs(110)))
    );
    now.store(110, Ordering::SeqCst);
    assert_eq!(
        session.poll_once().await,
        Ok(PollOutcome::Pending(Duration::from_secs(120)))
    );
    now.store(120, Ordering::SeqCst);
    assert_eq!(
        session.poll_once().await,
        Ok(PollOutcome::RetryAt(Duration::from_secs(140)))
    );
    now.store(140, Ordering::SeqCst);
    assert_eq!(
        session.poll_once().await,
        Ok(PollOutcome::Pending(Duration::from_secs(150)))
    );
    now.store(150, Ordering::SeqCst);
    assert_eq!(session.poll_once().await, Err(Error::Denied));
    assert_eq!(session.status(), Status::Denied);
    assert_eq!(session.poll_once().await, Err(Error::NoSession));
}

#[derive(Clone)]
struct Recorder {
    responses: Arc<Mutex<Vec<Result<Response, Error>>>>,
    forms: Arc<Mutex<Vec<String>>>,
}
impl Transport for Recorder {
    async fn post(&self, request: Request) -> Result<Response, Error> {
        // Only literal fixture values cross this test recording seam.
        self.forms
            .lock()
            .unwrap()
            .push(String::from_utf8(request.body.expose().to_vec()).unwrap());
        self.responses.lock().unwrap().remove(0)
    }
}
#[tokio::test]
async fn refresh_rotation_replaces_the_secret_before_the_next_refresh_and_revoke() {
    let now = Arc::new(AtomicU64::new(0));
    let recorder = Recorder {
        responses: Arc::new(Mutex::new(vec![
            Ok(device()),
            Ok(json(
                r#"{"access_token":"old-access","refresh_token":"first-refresh","expires_in":3600}"#,
            )),
            Ok(json(
                r#"{"access_token":"new-access","refresh_token":"rotated-refresh","expires_in":3600}"#,
            )),
            Ok(json(
                r#"{"access_token":"newest-access","refresh_token":null,"expires_in":3600}"#,
            )),
            Ok(Response {
                status: 200,
                body: SecretBody::new(Vec::new()),
            }),
        ])),
        forms: Arc::new(Mutex::new(Vec::new())),
    };
    let session = Session::with_transport(
        Configuration::production(),
        recorder.clone(),
        Clock(now.clone()),
    );
    session.start_link().await.unwrap();
    now.store(5, Ordering::SeqCst);
    session.poll_once().await.unwrap();
    session.refresh().await.unwrap();
    assert_eq!(
        session.with_access_token(str::to_owned),
        Ok("new-access".to_owned())
    );
    session.refresh().await.unwrap();
    assert_eq!(
        session.with_access_token(str::to_owned),
        Ok("newest-access".to_owned())
    );
    session.logout().await.unwrap();
    assert_eq!(session.status(), Status::SignedOut);
    assert_eq!(session.with_access_token(|_| ()), Err(Error::NoSession));
    let forms = recorder.forms.lock().unwrap();
    let refresh_one: Vec<_> = url::form_urlencoded::parse(forms[2].as_bytes()).collect();
    let refresh_two: Vec<_> = url::form_urlencoded::parse(forms[3].as_bytes()).collect();
    let revoke: Vec<_> = url::form_urlencoded::parse(forms[4].as_bytes()).collect();
    assert!(refresh_one.contains(&("refresh_token".into(), "first-refresh".into())));
    assert!(refresh_two.contains(&("refresh_token".into(), "rotated-refresh".into())));
    assert!(revoke.contains(&("token".into(), "rotated-refresh".into())));
    assert_eq!(refresh_one.len(), 4);
    assert_eq!(revoke.len(), 2);
}

async fn linked<T: Transport>(session: &Session<T, Clock>, now: &AtomicU64) {
    session.start_link().await.unwrap();
    now.store(5, Ordering::SeqCst);
    assert_eq!(session.poll_once().await, Ok(PollOutcome::Authorized));
}
fn tokens() -> Response {
    json(r#"{"access_token":"access-secret","refresh_token":"refresh-secret","expires_in":3600}"#)
}
#[tokio::test]
async fn consumed_malformed_token_success_requires_a_new_sign_in() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![
            device(),
            json(r#"{"access_token":"partial-secret","expires_in":3600}"#),
        ])),
        Clock(now.clone()),
    );
    session.start_link().await.unwrap();
    now.store(5, Ordering::SeqCst);
    assert_eq!(
        session.poll_once().await,
        Err(Error::ReauthenticationRequired)
    );
    assert_eq!(session.status(), Status::ReauthenticationRequired);
    assert_eq!(session.poll_once().await, Err(Error::NoSession));
}
#[tokio::test]
async fn clock_regression_clears_credentials_and_reports_reauthentication() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![device(), tokens()])),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    now.store(4, Ordering::SeqCst);
    assert_eq!(session.status(), Status::ReauthenticationRequired);
}
#[tokio::test]
async fn access_expiry_reports_refresh_required_without_exposing_expired_token() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![device(), tokens()])),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    now.store(3605, Ordering::SeqCst);
    assert_eq!(session.status(), Status::RefreshRequired);
    assert_eq!(session.with_access_token(|_| ()), Err(Error::Expired));
}
#[derive(Clone)]
struct HeldRefresh {
    held: Held,
    token_requests: Arc<AtomicU64>,
}
impl Transport for HeldRefresh {
    async fn post(&self, request: Request) -> Result<Response, Error> {
        if request.endpoint == Endpoint::DeviceCode {
            return Ok(device());
        }
        if self.token_requests.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(tokens());
        }
        self.held.post(request).await
    }
}
fn held_refresh() -> HeldRefresh {
    HeldRefresh {
        held: Held {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        },
        token_requests: Arc::new(AtomicU64::new(0)),
    }
}
#[tokio::test]
async fn cancellation_serializes_and_invalidates_an_in_flight_refresh() {
    let now = Arc::new(AtomicU64::new(0));
    let transport = held_refresh();
    let session = Session::with_transport(
        Configuration::production(),
        transport.clone(),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    let mut refresh = Box::pin(session.refresh());
    tokio::select! { value = &mut refresh => panic!("refresh returned early: {value:?}"), () = transport.held.entered.notified() => {} }
    assert_eq!(session.refresh().await, Err(Error::Busy));
    session.cancel();
    transport.held.release.notify_one();
    assert_eq!(refresh.await, Err(Error::Stale));
    assert_eq!(session.status(), Status::ReauthenticationRequired);
    assert_eq!(session.with_access_token(|_| ()), Err(Error::NoSession));
}
#[tokio::test]
async fn refresh_time_overflow_cannot_retain_a_possibly_rotated_token() {
    let now = Arc::new(AtomicU64::new(0));
    let transport = held_refresh();
    let session = Session::with_transport(
        Configuration::production(),
        transport.clone(),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    now.store(u64::MAX - 1, Ordering::SeqCst);
    let mut refresh = Box::pin(session.refresh());
    tokio::select! { value = &mut refresh => panic!("refresh returned early: {value:?}"), () = transport.held.entered.notified() => {} }
    transport.held.release.notify_one();
    assert_eq!(refresh.await, Err(Error::ReauthenticationRequired));
    assert_eq!(session.status(), Status::ReauthenticationRequired);
    assert_eq!(session.refresh().await, Err(Error::NoSession));
}

struct MemoryStore(Option<StoredSession>);
impl SecureSessionStore for MemoryStore {
    async fn take(&mut self) -> Result<Option<StoredSession>, Error> {
        Ok(self.0.take())
    }
    async fn replace(&mut self, session: StoredSession) -> Result<(), Error> {
        self.0 = Some(session);
        Ok(())
    }
    async fn clear(&mut self) -> Result<(), Error> {
        self.0 = None;
        Ok(())
    }
}
#[tokio::test]
async fn checkpoint_take_restore_requires_refresh_and_is_bound_to_registration() {
    let now = Arc::new(AtomicU64::new(0));
    let original = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![device(), tokens()])),
        Clock(now.clone()),
    );
    linked(&original, &now).await;
    let record = original.checkpoint().unwrap();
    assert!(!format!("{record:?}").contains("refresh-secret"));
    let payload = record.encode();
    assert!(!format!("{payload:?}").contains("refresh-secret"));
    let decoded = StoredSession::decode(payload).unwrap();
    let mut store = MemoryStore(None);
    store.replace(decoded).await.unwrap();
    let restored = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![json(
            r#"{"access_token":"restored-access","refresh_token":"rotated-refresh","expires_in":3600}"#,
        )])),
        Clock(now.clone()),
    );
    restored
        .restore(store.take().await.unwrap().unwrap())
        .unwrap();
    assert!(store.take().await.unwrap().is_none());
    assert_eq!(restored.status(), Status::RefreshRequired);
    assert_eq!(restored.with_access_token(|_| ()), Err(Error::NoSession));
    restored.refresh().await.unwrap();
    assert_eq!(
        restored.with_access_token(str::to_owned),
        Ok("restored-access".to_owned())
    );
    store.replace(restored.checkpoint().unwrap()).await.unwrap();
    store.clear().await.unwrap();
    assert!(store.take().await.unwrap().is_none());
    let wrong = format!(
        r#"{{"version":1,"issuer":"https://elsewhere.invalid/","client_id":"{CLIENT_ID}","refresh_token":"refresh-secret"}}"#
    );
    assert!(matches!(
        StoredSession::decode(SecretBody::new(wrong.into_bytes())),
        Err(Error::InvalidResponse)
    ));
}

#[tokio::test]
async fn unknown_refresh_completion_erases_the_old_token_and_does_not_retry() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Script(Mutex::new(vec![
            Ok(device()),
            Ok(tokens()),
            Err(Error::Deadline),
        ])),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    assert_eq!(
        session.refresh().await,
        Err(Error::ReauthenticationRequired)
    );
    assert_eq!(session.refresh().await, Err(Error::NoSession));
    assert!(matches!(session.checkpoint(), Err(Error::NoSession)));
}
#[tokio::test]
async fn dropping_refresh_future_releases_ownership_and_forces_reauthentication() {
    let now = Arc::new(AtomicU64::new(0));
    let transport = held_refresh();
    let session = Session::with_transport(
        Configuration::production(),
        transport.clone(),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    let mut refresh = Box::pin(session.refresh());
    tokio::select! { value = &mut refresh => panic!("refresh returned early: {value:?}"), () = transport.held.entered.notified() => {} }
    drop(refresh);
    assert_eq!(session.status(), Status::ReauthenticationRequired);
    assert_eq!(session.refresh().await, Err(Error::NoSession));
    session.start_link().await.unwrap();
}
#[tokio::test]
async fn logout_during_refresh_removes_local_credentials_and_rejects_late_tokens() {
    let now = Arc::new(AtomicU64::new(0));
    let transport = held_refresh();
    let session = Session::with_transport(
        Configuration::production(),
        transport.clone(),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    let mut refresh = Box::pin(session.refresh());
    tokio::select! { value = &mut refresh => panic!("refresh returned early: {value:?}"), () = transport.held.entered.notified() => {} }
    assert_eq!(session.logout().await, Err(Error::RevocationUnconfirmed));
    transport.held.release.notify_one();
    assert_eq!(refresh.await, Err(Error::Stale));
    assert_eq!(session.status(), Status::SignedOut);
    assert!(matches!(session.checkpoint(), Err(Error::NoSession)));
}
#[tokio::test]
async fn grant_expiry_after_await_and_disposal_reject_late_tokens() {
    for disposed in [false, true] {
        let now = Arc::new(AtomicU64::new(0));
        let held = Held {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        let session = Session::with_transport(
            Configuration::production(),
            held.clone(),
            Clock(now.clone()),
        );
        session.start_link().await.unwrap();
        now.store(5, Ordering::SeqCst);
        let mut poll = Box::pin(session.poll_once());
        tokio::select! { value = &mut poll => panic!("poll returned early: {value:?}"), () = held.entered.notified() => {} }
        if disposed {
            session.dispose();
        } else {
            now.store(900, Ordering::SeqCst);
        }
        held.release.notify_one();
        assert_eq!(
            poll.await,
            Err(if disposed {
                Error::Disposed
            } else {
                Error::Expired
            })
        );
        assert_eq!(
            session.status(),
            if disposed {
                Status::Disposed
            } else {
                Status::Expired
            }
        );
        assert!(session.with_access_token(|_| ()).is_err());
    }
}
#[tokio::test]
async fn callback_panic_cannot_prevent_secret_cleanup_or_disposal() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![device(), tokens()])),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || session.with_access_token::<()>(|_| panic!("synthetic callback panic"))
        ))
        .is_err()
    );
    assert_eq!(session.status(), Status::ReauthenticationRequired);
    assert!(matches!(session.checkpoint(), Err(Error::NoSession)));
    session.dispose();
    assert_eq!(session.status(), Status::Disposed);
}
#[tokio::test]
async fn hostile_activation_uri_and_native_schema_bounds_are_rejected() {
    for uri in [
        "http://login.criterion.com/activate",
        "https://login.criterion.com.evil.invalid/activate",
        "https://name@login.criterion.com/activate",
        "https://login.criterion.com:444/activate",
        "https://login.criterion.com/elsewhere",
        "https://login.criterion.com/activate#secret",
    ] {
        let body = serde_json::to_string(&serde_json::json!({"device_code":"synthetic-device", "user_code":"ABCD", "verification_uri_complete":uri, "expires_in":900,"interval":5})).unwrap();
        let session = Session::with_transport(
            Configuration::production(),
            Sequence(Mutex::new(vec![json(&body)])),
            Clock(Arc::new(AtomicU64::new(0))),
        );
        assert!(matches!(
            session.start_link().await,
            Err(Error::InvalidResponse)
        ));
        assert_eq!(session.status(), Status::SignedOut);
    }
    for (expires, interval) in [(0, 5), (3601, 5), (900, 0), (900, 61)] {
        let body = format!(
            r#"{{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate","expires_in":{expires},"interval":{interval}}}"#
        );
        let session = Session::with_transport(
            Configuration::production(),
            Sequence(Mutex::new(vec![json(&body)])),
            Clock(Arc::new(AtomicU64::new(0))),
        );
        assert!(matches!(
            session.start_link().await,
            Err(Error::InvalidResponse)
        ));
    }
}
#[test]
fn maximum_escaped_checkpoint_round_trips_and_all_secret_diagnostics_are_redacted() {
    let token: Secret =
        serde_json::from_str(&serde_json::to_string(&"\"".repeat(crate::wire::MAX_TOKEN)).unwrap())
            .unwrap();
    let record = StoredSession {
        refresh_token: token,
    };
    let payload = record.encode();
    assert!(payload.expose().len() > 32_768);
    let decoded = StoredSession::decode(payload).unwrap();
    assert_eq!(decoded.refresh_token.expose().len(), crate::wire::MAX_TOKEN);
    let secret: Secret = serde_json::from_str("\"synthetic-diagnostic-secret\"").unwrap();
    assert_eq!(format!("{secret:?}/{secret}"), "[redacted]/[redacted]");
    let instructions = LinkInstructions {
        user_code: secret,
        verification_uri_complete: serde_json::from_str(
            "\"https://login.criterion.com/activate?user_code=synthetic-private\"",
        )
        .unwrap(),
        expires_at: Duration::ZERO,
    };
    assert!(!format!("{instructions:?}").contains("synthetic"));
}

#[tokio::test]
async fn next_poll_wake_is_bounded_by_grant_expiry_even_near_clock_limit() {
    let started = u64::MAX - 901;
    let now = Arc::new(AtomicU64::new(started));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![device(), failure("authorization_pending")])),
        Clock(now.clone()),
    );
    session.start_link().await.unwrap();
    now.store(u64::MAX - 2, Ordering::SeqCst);
    assert_eq!(
        session.poll_once().await,
        Ok(PollOutcome::Pending(Duration::from_secs(u64::MAX - 1)))
    );
    now.store(u64::MAX - 1, Ordering::SeqCst);
    assert_eq!(session.poll_once().await, Err(Error::Expired));
}

#[tokio::test]
async fn interrupted_revoke_settlement_is_explicitly_unconfirmed() {
    let now = Arc::new(AtomicU64::new(0));
    let transport = held_refresh();
    let session = Session::with_transport(
        Configuration::production(),
        transport.clone(),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    let mut logout = Box::pin(session.logout());
    tokio::select! { value = &mut logout => panic!("revoke returned early: {value:?}"), () = transport.held.entered.notified() => {} }
    session.dispose();
    transport.held.release.notify_one();
    assert_eq!(logout.await, Err(Error::RevocationUnconfirmed));
    assert_eq!(session.status(), Status::Disposed);
}

#[tokio::test]
async fn device_authorization_requires_an_object_response() {
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![json(
            r#"["device-secret","ABCD-EFGH","https://login.criterion.com/activate?user_code=ABCD-EFGH",900,5]"#,
        )])),
        Clock(Arc::new(AtomicU64::new(0))),
    );
    assert!(matches!(
        session.start_link().await,
        Err(Error::InvalidResponse)
    ));
    assert_eq!(session.status(), Status::SignedOut);
    assert_eq!(session.poll_once().await, Err(Error::NoSession));
}

#[tokio::test]
async fn token_array_success_requires_a_new_sign_in_without_credential_admission() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![
            device(),
            json(r#"["access-secret","refresh-secret",3600]"#),
        ])),
        Clock(now.clone()),
    );
    session.start_link().await.unwrap();
    now.store(5, Ordering::SeqCst);
    assert_eq!(
        session.poll_once().await,
        Err(Error::ReauthenticationRequired)
    );
    assert_eq!(session.status(), Status::ReauthenticationRequired);
    assert_eq!(session.with_access_token(|_| ()), Err(Error::NoSession));
    assert!(matches!(session.checkpoint(), Err(Error::NoSession)));
}

#[tokio::test]
async fn refresh_array_success_erases_possibly_rotated_credentials() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![
            device(),
            tokens(),
            json(r#"["new-access","rotated-refresh",3600]"#),
        ])),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    assert_eq!(
        session.refresh().await,
        Err(Error::ReauthenticationRequired)
    );
    assert_eq!(session.status(), Status::ReauthenticationRequired);
    assert_eq!(session.with_access_token(|_| ()), Err(Error::NoSession));
    assert!(matches!(session.checkpoint(), Err(Error::NoSession)));
    assert_eq!(session.refresh().await, Err(Error::NoSession));
}

#[tokio::test]
async fn issuer_error_array_cannot_extend_a_device_authorization_transaction() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![
            device(),
            Response {
                status: 400,
                body: SecretBody::new(br#"["authorization_pending"]"#.to_vec()),
            },
        ])),
        Clock(now.clone()),
    );
    session.start_link().await.unwrap();
    now.store(5, Ordering::SeqCst);
    assert_eq!(session.poll_once().await, Err(Error::InvalidResponse));
    assert_eq!(session.status(), Status::Cancelled);
    assert_eq!(session.poll_once().await, Err(Error::NoSession));
}

#[test]
fn authenticated_checkpoint_plaintext_requires_an_object_record() {
    let payload = format!(r#"[1,"{ISSUER}","{CLIENT_ID}","refresh-secret"]"#);
    assert!(matches!(
        StoredSession::decode(SecretBody::new(payload.into_bytes())),
        Err(Error::InvalidResponse)
    ));
}

#[tokio::test]
async fn token_object_validation_rejects_wrong_fields_and_trailing_documents() {
    for body in [
        "null",
        "true",
        "42",
        r#""synthetic-scalar""#,
        r#"{"access_token":{"value":"access-secret"},"refresh_token":"refresh-secret","expires_in":3600}"#,
        r#"{"access_token":["access-secret"],"refresh_token":"refresh-secret","expires_in":3600}"#,
        r#"{"access_token":"access-secret","refresh_token":"refresh-secret","expires_in":{"value":3600}}"#,
        r#"{"access_token":"access-secret","access_token":"other-secret","refresh_token":"refresh-secret","expires_in":3600}"#,
        r#"{"access_token":"access-secret","refresh_token":"refresh-secret","expires_in":3600} {}"#,
        r#"{"access_token":"access-secret","refresh_token":"refresh-secret","expires_in":3600} false"#,
    ] {
        let now = Arc::new(AtomicU64::new(0));
        let session = Session::with_transport(
            Configuration::production(),
            Sequence(Mutex::new(vec![device(), json(body)])),
            Clock(now.clone()),
        );
        session.start_link().await.unwrap();
        now.store(5, Ordering::SeqCst);
        assert_eq!(
            session.poll_once().await,
            Err(Error::ReauthenticationRequired)
        );
        assert_eq!(session.with_access_token(|_| ()), Err(Error::NoSession));
        assert!(matches!(session.checkpoint(), Err(Error::NoSession)));
    }
}

#[tokio::test]
async fn object_response_extension_and_omitted_refresh_token_preserve_the_session() {
    let now = Arc::new(AtomicU64::new(0));
    let recorder = Recorder {
        responses: Arc::new(Mutex::new(vec![
            Ok(device()),
            Ok(json(
                r#" {"access_token":"access-secret","refresh_token":"refresh-secret","expires_in":3600,"issuer_extension":{"items":[{"enabled":true}]}}
"#,
            )),
            Ok(json(
                r#"{"access_token":"new-access","expires_in":3600,"issuer_extension":[{"enabled":true}]}"#,
            )),
            Ok(Response {
                status: 200,
                body: SecretBody::new(Vec::new()),
            }),
        ])),
        forms: Arc::new(Mutex::new(Vec::new())),
    };
    let session = Session::with_transport(
        Configuration::production(),
        recorder.clone(),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    session.refresh().await.unwrap();
    assert_eq!(
        session.with_access_token(str::to_owned),
        Ok("new-access".to_owned())
    );
    session.logout().await.unwrap();
    let forms = recorder.forms.lock().unwrap();
    let revoke: Vec<_> = url::form_urlencoded::parse(forms[3].as_bytes()).collect();
    assert!(revoke.contains(&("token".into(), "refresh-secret".into())));
    assert_eq!(session.status(), Status::SignedOut);
}

#[tokio::test]
async fn duplicate_optional_refresh_field_cannot_confirm_rotation() {
    let now = Arc::new(AtomicU64::new(0));
    let session = Session::with_transport(
        Configuration::production(),
        Sequence(Mutex::new(vec![
            device(),
            tokens(),
            json(
                r#"{"access_token":"new-access","refresh_token":null,"refresh_token":"rotated-refresh","expires_in":3600}"#,
            ),
        ])),
        Clock(now.clone()),
    );
    linked(&session, &now).await;
    assert_eq!(
        session.refresh().await,
        Err(Error::ReauthenticationRequired)
    );
    assert_eq!(session.with_access_token(|_| ()), Err(Error::NoSession));
    assert!(matches!(session.checkpoint(), Err(Error::NoSession)));
}

#[test]
fn checkpoint_object_validation_rejects_changed_shape_and_trailing_documents() {
    let valid = format!(
        r#"{{"version":1,"issuer":"{ISSUER}","client_id":"{CLIENT_ID}","refresh_token":"refresh-secret"}}"#
    );
    let invalid = [
        "null".to_owned(),
        "true".to_owned(),
        "42".to_owned(),
        r#""synthetic-scalar""#.to_owned(),
        valid.replace(r#""refresh-secret""#, r#"{"value":"refresh-secret"}"#),
        valid.replace(r#""refresh-secret""#, r#"["refresh-secret"]"#),
        valid.replace(r#""version":1"#, r#""version":1,"version":1"#),
        valid.replace(
            r#""refresh_token":"refresh-secret""#,
            r#""refresh_token":"refresh-secret","refresh_token":"other-secret""#,
        ),
        valid.replace(r#""version":1"#, r#""version":1,"extra":{"items":[true]}"#),
        format!("{valid} {{}}"),
        format!("{valid} false"),
    ];
    for body in invalid {
        assert!(matches!(
            StoredSession::decode(SecretBody::new(body.into_bytes())),
            Err(Error::InvalidResponse)
        ));
    }
    assert!(StoredSession::decode(SecretBody::new(format!(" \n{valid}\t ").into_bytes())).is_ok());
}
