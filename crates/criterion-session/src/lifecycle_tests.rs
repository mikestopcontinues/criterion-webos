//! Synthetic transport and storage fixtures; no at-rest or TV admission.
use crate::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

#[derive(Clone)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

// This fixture models reopening only. Its literal plaintext is never a backend.
#[derive(Clone)]
struct Store(
    Arc<Mutex<Option<SecretBody>>>,
    Arc<Mutex<Vec<&'static str>>>,
);
impl SecureSessionStore for Store {
    async fn take(&mut self) -> Result<Option<StoredSession>, Error> {
        self.1.lock().unwrap().push("take");
        self.0
            .lock()
            .unwrap()
            .take()
            .map(StoredSession::decode)
            .transpose()
    }
    async fn replace(&mut self, value: StoredSession) -> Result<(), Error> {
        self.1.lock().unwrap().push("save");
        *self.0.lock().unwrap() = Some(value.encode());
        Ok(())
    }
    async fn clear(&mut self) -> Result<(), Error> {
        self.1.lock().unwrap().push("clear");
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[derive(Clone)]
struct Issuer(Arc<Mutex<Vec<Response>>>, Arc<Mutex<Vec<&'static str>>>);
impl Transport for Issuer {
    async fn post(&self, request: Request) -> Result<Response, Error> {
        self.1.lock().unwrap().push(match request.endpoint {
            Endpoint::DeviceCode => "link",
            Endpoint::Token => "token",
            Endpoint::Revoke => "revoke",
        });
        Ok(self.0.lock().unwrap().remove(0))
    }
}
struct LifetimeIssuer {
    issuer: Issuer,
    dropped: Arc<AtomicU64>,
}
impl Transport for LifetimeIssuer {
    async fn post(&self, request: Request) -> Result<Response, Error> {
        self.issuer.post(request).await
    }
}
impl Drop for LifetimeIssuer {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
fn json(body: &str) -> Response {
    Response {
        status: 200,
        body: SecretBody::new(body.as_bytes().to_vec()),
    }
}
fn token() -> Response {
    json(r#"{"access_token":"fixture-access","refresh_token":"fixture-current","expires_in":3600}"#)
}
fn saved() -> SecretBody {
    SecretBody::new(format!(r#"{{"version":1,"issuer":"{ISSUER}","client_id":"{CLIENT_ID}","refresh_token":"fixture-old"}}"#).into_bytes())
}

#[tokio::test]
async fn restore_consumes_checkpoint_before_issuer_and_active_drop_keeps_it_absent() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let issuer = Issuer(Arc::new(Mutex::new(vec![token()])), events.clone());
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        issuer,
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    assert!(owner.restore().await.unwrap());
    assert_eq!(
        owner.with_access_token(str::to_owned).unwrap(),
        "fixture-access"
    );
    assert_eq!(*events.lock().unwrap(), ["take", "token"]);
    drop(owner);
    let reopened = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store,
        tokio::runtime::Handle::current(),
    );
    assert!(!reopened.restore().await.unwrap());
    assert_eq!(reopened.with_access_token(|_| ()), Err(Error::NoSession));
    assert_eq!(*events.lock().unwrap(), ["take", "token", "take"]);
}

#[tokio::test]
async fn graceful_stop_saves_current_rotation_but_logout_erases_it() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let issuer = Issuer(
        Arc::new(Mutex::new(vec![
            token(),
            json(
                r#"{"access_token":"fixture-second","refresh_token":"fixture-newest","expires_in":3600}"#,
            ),
        ])),
        events.clone(),
    );
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        issuer,
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    owner.refresh().await.unwrap();
    assert!(store.0.lock().unwrap().is_none());
    owner.graceful_stop().await.unwrap();
    assert_eq!(owner.with_access_token(|_| ()), Err(Error::Disposed));
    let record = store.0.lock().unwrap().take().unwrap();
    assert_eq!(
        StoredSession::decode(record)
            .unwrap()
            .refresh_token
            .expose(),
        "fixture-newest"
    );
    *store.0.lock().unwrap() = Some(saved());
    let issuer = Issuer(
        Arc::new(Mutex::new(vec![token(), json("{}")])),
        events.clone(),
    );
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        issuer,
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    owner.logout().await.unwrap();
    assert!(store.0.lock().unwrap().is_none());
    assert_eq!(owner.with_access_token(|_| ()), Err(Error::Disposed));
    assert_eq!(
        *events.lock().unwrap(),
        [
            "take", "token", "clear", "token", "save", "take", "token", "clear", "revoke"
        ]
    );
}

#[tokio::test]
async fn retirement_rejects_a_completed_but_unclaimed_reply() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let issuer = Issuer(Arc::new(Mutex::new(vec![token()])), events.clone());
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        issuer,
        Clock(Arc::new(AtomicU64::new(0))),
        store,
        tokio::runtime::Handle::current(),
    );
    let reply = owner.restore();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::SignedIn { .. })));
    owner.graceful_stop().await.unwrap();
    assert_eq!(reply.await, Err(Error::Stale));
}

struct Gate {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        })
    }
}
struct Plan {
    response: Result<Response, Error>,
    gate: Option<Arc<Gate>>,
}
#[derive(Clone)]
struct ControlledIssuer {
    plans: Arc<Mutex<std::collections::VecDeque<Plan>>>,
    events: Arc<Mutex<Vec<&'static str>>>,
    revoked: Arc<Mutex<Option<String>>>,
}
impl Transport for ControlledIssuer {
    async fn post(&self, request: Request) -> Result<Response, Error> {
        self.events.lock().unwrap().push(match request.endpoint {
            Endpoint::DeviceCode => "link",
            Endpoint::Token => "token",
            Endpoint::Revoke => "revoke",
        });
        if request.endpoint == Endpoint::Revoke {
            *self.revoked.lock().unwrap() =
                Some(String::from_utf8(request.body.expose().to_vec()).unwrap());
        }
        let plan = self.plans.lock().unwrap().pop_front().unwrap();
        if let Some(gate) = plan.gate {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
        plan.response
    }
}

#[tokio::test]
async fn logout_joins_rotation_rejects_overlap_and_revokes_the_newest_token() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let gate = Gate::new();
    let revoked = Arc::new(Mutex::new(None));
    let issuer = ControlledIssuer {
        plans: Arc::new(Mutex::new(std::collections::VecDeque::from([
            Plan {
                response: Ok(token()),
                gate: None,
            },
            Plan {
                response: Ok(json(
                    r#"{"access_token":"fixture-new","refresh_token":"fixture-rotated","expires_in":3600}"#,
                )),
                gate: Some(gate.clone()),
            },
            Plan {
                response: Ok(json("{}")),
                gate: None,
            },
        ]))),
        events: events.clone(),
        revoked: revoked.clone(),
    };
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        issuer,
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    let refresh = owner.refresh();
    gate.entered.notified().await;
    assert_eq!(owner.refresh().await, Err(Error::Busy));
    let busy_reply = owner.refresh();
    let logout = owner.logout();
    assert_eq!(owner.with_access_token(|_| ()), Err(Error::Disposed));
    assert_eq!(owner.graceful_stop().await, Err(Error::Disposed));
    assert!(store.0.lock().unwrap().is_none());
    assert_eq!(*events.lock().unwrap(), ["take", "token", "clear", "token"]);
    gate.release.notify_one();
    assert_eq!(refresh.await, Err(Error::Stale));
    assert_eq!(busy_reply.await, Err(Error::Stale));
    logout.await.unwrap();
    assert!(
        revoked
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains("token=fixture-rotated")
    );
    assert!(store.0.lock().unwrap().is_none());
    assert_eq!(
        *events.lock().unwrap(),
        ["take", "token", "clear", "token", "clear", "revoke"]
    );
}

#[tokio::test]
async fn uncertain_rotation_cannot_relink_or_claim_confirmed_revocation() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let issuer = ControlledIssuer {
        plans: Arc::new(Mutex::new(std::collections::VecDeque::from([
            Plan {
                response: Ok(token()),
                gate: None,
            },
            Plan {
                response: Err(Error::Deadline),
                gate: None,
            },
        ]))),
        events: events.clone(),
        revoked: Arc::new(Mutex::new(None)),
    };
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        issuer,
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    assert_eq!(owner.refresh().await, Err(Error::ReauthenticationRequired));
    assert_eq!(
        owner.start_link().await.err(),
        Some(Error::ReauthenticationRequired)
    );
    assert_eq!(owner.logout().await, Err(Error::RevocationUnconfirmed));
    assert!(store.0.lock().unwrap().is_none());
    assert_eq!(
        *events.lock().unwrap(),
        ["take", "token", "clear", "token", "clear"]
    );
}

#[derive(Clone)]
struct FaultStore {
    inner: Store,
    fault: Arc<Mutex<Option<&'static str>>>,
    recovery_required: Arc<std::sync::atomic::AtomicBool>,
    hold: Option<(&'static str, Arc<Gate>)>,
}
impl FaultStore {
    fn new(inner: Store) -> Self {
        Self {
            inner,
            fault: Arc::new(Mutex::new(None)),
            recovery_required: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            hold: None,
        }
    }
    // This boolean stands in for an admitted backend's durable recovery fence.
    // It tests the owner contract only, and proves no platform durability.
    async fn begin(&self, operation: &'static str) -> Result<(), Error> {
        if self.recovery_required.swap(true, Ordering::SeqCst) {
            return Err(Error::Unavailable);
        }
        if let Some((name, gate)) = &self.hold
            && *name == operation
        {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
        Ok(())
    }
    fn complete<R>(&self, operation: &'static str, result: Result<R, Error>) -> Result<R, Error> {
        if self.fault.lock().unwrap().as_ref() == Some(&operation) {
            return Err(Error::Unavailable);
        }
        if result.is_ok() {
            self.recovery_required.store(false, Ordering::SeqCst);
        }
        result
    }
}
impl SecureSessionStore for FaultStore {
    async fn take(&mut self) -> Result<Option<StoredSession>, Error> {
        self.begin("take").await?;
        let result = self.inner.take().await;
        self.complete("take", result)
    }
    async fn replace(&mut self, value: StoredSession) -> Result<(), Error> {
        self.begin("save").await?;
        let result = self.inner.replace(value).await;
        self.complete("save", result)
    }
    async fn clear(&mut self) -> Result<(), Error> {
        self.begin("clear").await?;
        let result = self.inner.clear().await;
        self.complete("clear", result)
    }
}

#[tokio::test]
async fn unconfirmed_removal_blocks_issuer_and_restart_depends_on_backend_recovery() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = FaultStore::new(Store(Arc::new(Mutex::new(Some(saved()))), events.clone()));
    *store.fault.lock().unwrap() = Some("take");
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    assert_eq!(owner.restore().await, Err(Error::StorageUnconfirmed));
    assert_eq!(
        owner.start_link().await.err(),
        Some(Error::StorageUnconfirmed)
    );
    assert_eq!(
        owner.with_access_token(|_| ()),
        Err(Error::StorageUnconfirmed)
    );
    drop(owner);
    let reopened = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store,
        tokio::runtime::Handle::current(),
    );
    assert_eq!(reopened.restore().await, Err(Error::StorageUnconfirmed));
    assert_eq!(*events.lock().unwrap(), ["take"]);
}

#[derive(Clone)]
struct BlockingClock {
    block: Arc<std::sync::atomic::AtomicBool>,
    entered: std::sync::mpsc::Sender<()>,
    released: Arc<(Mutex<bool>, std::sync::Condvar)>,
}
impl MonotonicClock for BlockingClock {
    fn now(&self) -> Duration {
        if self.block.swap(false, Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            let (lock, condition) = &*self.released;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
        }
        Duration::ZERO
    }
}

#[tokio::test]
async fn unpolled_busy_reply_cannot_retain_session_after_owner_drops_during_publication() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let dropped = Arc::new(AtomicU64::new(0));
    let (entered, received) = std::sync::mpsc::channel();
    let clock = BlockingClock {
        block: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        entered,
        released: Arc::new((Mutex::new(false), std::sync::Condvar::new())),
    };
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        LifetimeIssuer {
            issuer: Issuer(Arc::new(Mutex::new(vec![token()])), events),
            dropped: dropped.clone(),
        },
        clock.clone(),
        store,
        tokio::runtime::Handle::current(),
    );
    let held = owner.restore();
    let busy = owner.refresh();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::SignedIn { .. })));
    clock.block.store(true, Ordering::SeqCst);
    let reader = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(held)
    });
    received.recv().unwrap();
    drop(owner);
    *clock.released.0.lock().unwrap() = true;
    clock.released.1.notify_one();
    assert_eq!(reader.join().unwrap(), Err(Error::Stale));
    // The transport belongs only to the session. Retaining a public reply must
    // not retain that session after the issued job and observation have settled.
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(busy.await, Err(Error::Stale));
}

#[tokio::test]
async fn status_held_in_clock_cannot_publish_after_retirement_intent() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let (entered, received) = std::sync::mpsc::channel();
    let clock = BlockingClock {
        block: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        entered,
        released: Arc::new((Mutex::new(false), std::sync::Condvar::new())),
    };
    let owner = Arc::new(PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token()])), events.clone()),
        clock.clone(),
        store,
        tokio::runtime::Handle::current(),
    ));
    owner.restore().await.unwrap();
    clock.block.store(true, Ordering::SeqCst);
    let reader = {
        let owner = owner.clone();
        std::thread::spawn(move || owner.status())
    };
    received.recv().unwrap();
    let stop = owner.graceful_stop();
    *clock.released.0.lock().unwrap() = true;
    clock.released.1.notify_one();
    assert_eq!(reader.join().unwrap(), Err(Error::Disposed));
    stop.await.unwrap();
}

fn link() -> Response {
    json(
        r#"{"device_code":"fixture-device","user_code":"FIXTURE","verification_uri":"https://login.criterion.com/activate","verification_uri_complete":"https://login.criterion.com/activate?user_code=FIXTURE","expires_in":600,"interval":5}"#,
    )
}

#[tokio::test]
async fn a_held_link_reply_is_stale_after_a_later_command_replaces_its_grant() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Store(Arc::new(Mutex::new(None)), events.clone());
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![link(), link()])), events),
        Clock(Arc::new(AtomicU64::new(0))),
        store,
        tokio::runtime::Handle::current(),
    );
    let held = owner.start_link();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::Linking { .. })));
    owner.start_link().await.unwrap();
    assert_eq!(held.await.err(), Some(Error::Stale));
}

#[tokio::test]
async fn a_held_link_reply_cannot_publish_expired_activation_instructions() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let time = Arc::new(AtomicU64::new(0));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![link()])), events.clone()),
        Clock(time.clone()),
        Store(Arc::new(Mutex::new(None)), events),
        tokio::runtime::Handle::current(),
    );
    let held = owner.start_link();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::Linking { .. })));
    time.store(600, Ordering::SeqCst);
    assert_eq!(held.await.err(), Some(Error::Stale));
}

#[tokio::test]
async fn clock_invalidation_rejects_held_success_and_cannot_confirm_revocation() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let time = Arc::new(AtomicU64::new(10));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token()])), events.clone()),
        Clock(time.clone()),
        Store(Arc::new(Mutex::new(Some(saved()))), events),
        tokio::runtime::Handle::current(),
    );
    let held = owner.restore();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::SignedIn { .. })));
    time.store(0, Ordering::SeqCst);
    assert_eq!(held.await, Err(Error::Stale));
    assert_eq!(owner.status(), Ok(Status::ReauthenticationRequired));
    assert_eq!(owner.logout().await, Err(Error::RevocationUnconfirmed));
}

#[tokio::test]
async fn a_held_refresh_rejects_direct_clock_regression_before_publication() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let time = Arc::new(AtomicU64::new(10));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token(), token()])), events.clone()),
        Clock(time.clone()),
        Store(Arc::new(Mutex::new(Some(saved()))), events),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    let held = owner.refresh();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::SignedIn { .. })));
    time.store(0, Ordering::SeqCst);
    assert_eq!(held.await, Err(Error::Stale));
    assert_eq!(owner.status(), Ok(Status::ReauthenticationRequired));
    assert_eq!(owner.logout().await, Err(Error::RevocationUnconfirmed));
}

#[tokio::test]
async fn unadmitted_issued_poll_response_cannot_claim_confirmed_revocation() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let time = Arc::new(AtomicU64::new(0));
    let gate = Gate::new();
    let issuer = ControlledIssuer {
        plans: Arc::new(Mutex::new(std::collections::VecDeque::from([
            Plan {
                response: Ok(link()),
                gate: None,
            },
            Plan {
                response: Ok(token()),
                gate: Some(gate.clone()),
            },
        ]))),
        events: events.clone(),
        revoked: Arc::new(Mutex::new(None)),
    };
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        issuer,
        Clock(time.clone()),
        Store(Arc::new(Mutex::new(None)), events),
        tokio::runtime::Handle::current(),
    );
    owner.start_link().await.unwrap();
    time.store(5, Ordering::SeqCst);
    let poll = owner.poll_once();
    gate.entered.notified().await;
    time.store(0, Ordering::SeqCst);
    gate.release.notify_one();
    assert_eq!(poll.await, Err(Error::ClockRegression));
    assert_eq!(owner.logout().await, Err(Error::RevocationUnconfirmed));
}

#[tokio::test]
async fn issued_stop_survives_dropping_its_public_owner() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token()])), events),
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    let stop = owner.graceful_stop();
    drop(owner);
    stop.await.unwrap();
    assert_eq!(
        store.take().await.unwrap().unwrap().refresh_token.expose(),
        "fixture-current"
    );
}

#[tokio::test]
async fn a_panicking_token_borrow_cannot_claim_confirmed_revocation() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token()])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        Store(Arc::new(Mutex::new(Some(saved()))), events),
        tokio::runtime::Handle::current(),
    );
    let held = owner.restore();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::SignedIn { .. })));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || owner.with_access_token::<()>(|_| panic!("fixture callback"))
        ))
        .is_err()
    );
    assert_eq!(held.await, Err(Error::Stale));
    assert_eq!(owner.logout().await, Err(Error::RevocationUnconfirmed));
}

#[derive(Clone)]
struct ScriptedClock(Arc<Mutex<std::collections::VecDeque<Duration>>>);
impl MonotonicClock for ScriptedClock {
    fn now(&self) -> Duration {
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Duration::from_secs(10))
    }
}

#[tokio::test]
async fn token_borrow_rejects_its_result_if_post_borrow_observation_invalidates_it() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let clock = ScriptedClock(Arc::new(Mutex::new(std::collections::VecDeque::new())));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token()])), events.clone()),
        clock.clone(),
        Store(Arc::new(Mutex::new(Some(saved()))), events),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    clock
        .0
        .lock()
        .unwrap()
        .extend([Duration::from_secs(10), Duration::ZERO]);
    assert_eq!(
        owner.with_access_token(|_| "fixture header"),
        Err(Error::Stale)
    );
    assert_eq!(owner.logout().await, Err(Error::RevocationUnconfirmed));
}

#[tokio::test]
async fn failed_admission_of_a_consumed_checkpoint_leaves_revocation_unconfirmed() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let time = Arc::new(AtomicU64::new(10));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![])), events.clone()),
        Clock(time.clone()),
        Store(Arc::new(Mutex::new(Some(saved()))), events),
        tokio::runtime::Handle::current(),
    );
    assert_eq!(owner.status(), Ok(Status::SignedOut));
    time.store(0, Ordering::SeqCst);
    assert_eq!(owner.restore().await, Err(Error::ClockRegression));
    assert_eq!(owner.logout().await, Err(Error::RevocationUnconfirmed));
}

#[tokio::test]
async fn a_held_pending_poll_cannot_publish_after_grant_expiry() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let time = Arc::new(AtomicU64::new(0));
    let pending = Response {
        status: 400,
        body: SecretBody::new(br#"{"error":"authorization_pending"}"#.to_vec()),
    };
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![link(), pending])), events.clone()),
        Clock(time.clone()),
        Store(Arc::new(Mutex::new(None)), events),
        tokio::runtime::Handle::current(),
    );
    owner.start_link().await.unwrap();
    time.store(5, Ordering::SeqCst);
    let held = owner.poll_once();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::Linking { .. })));
    time.store(600, Ordering::SeqCst);
    assert_eq!(held.await, Err(Error::Stale));
    assert_eq!(owner.status(), Ok(Status::Expired));
    owner.logout().await.unwrap();
}

#[tokio::test]
async fn a_held_authorized_poll_cannot_publish_after_access_expiry() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let time = Arc::new(AtomicU64::new(0));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(
            Arc::new(Mutex::new(vec![link(), token(), json("{}")])),
            events.clone(),
        ),
        Clock(time.clone()),
        Store(Arc::new(Mutex::new(None)), events),
        tokio::runtime::Handle::current(),
    );
    owner.start_link().await.unwrap();
    time.store(5, Ordering::SeqCst);
    let held = owner.poll_once();
    for _ in 0..100 {
        if owner.status().is_ok() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(owner.status(), Ok(Status::SignedIn { .. })));
    time.store(3605, Ordering::SeqCst);
    assert_eq!(held.await, Err(Error::Stale));
    assert_eq!(owner.status(), Ok(Status::RefreshRequired));
    owner.logout().await.unwrap();
}

#[tokio::test]
async fn dropped_rotation_reply_is_joined_before_stop_saves_current_checkpoint() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let gate = Gate::new();
    let mut store = Store(Arc::new(Mutex::new(Some(saved()))), events.clone());
    let issuer = ControlledIssuer {
        plans: Arc::new(Mutex::new(std::collections::VecDeque::from([
            Plan {
                response: Ok(token()),
                gate: None,
            },
            Plan {
                response: Ok(json(
                    r#"{"access_token":"fixture-next","refresh_token":"fixture-next-refresh","expires_in":3600}"#,
                )),
                gate: Some(gate.clone()),
            },
        ]))),
        events: events.clone(),
        revoked: Arc::new(Mutex::new(None)),
    };
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        issuer,
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    let reply = owner.refresh();
    gate.entered.notified().await;
    drop(reply);
    let stop = owner.graceful_stop();
    assert!(store.0.lock().unwrap().is_none());
    assert_eq!(*events.lock().unwrap(), ["take", "token", "clear", "token"]);
    gate.release.notify_one();
    stop.await.unwrap();
    assert_eq!(
        store.take().await.unwrap().unwrap().refresh_token.expose(),
        "fixture-next-refresh"
    );
}

#[tokio::test]
async fn unknown_save_blocks_access_and_reopening_even_if_a_record_was_written() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = FaultStore::new(Store(Arc::new(Mutex::new(Some(saved()))), events.clone()));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token()])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    *store.fault.lock().unwrap() = Some("save");
    assert_eq!(owner.graceful_stop().await, Err(Error::StorageUnconfirmed));
    assert_eq!(
        owner.with_access_token(|_| ()),
        Err(Error::StorageUnconfirmed)
    );
    assert!(store.inner.0.lock().unwrap().is_some());
    drop(owner);
    let reopened = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store,
        tokio::runtime::Handle::current(),
    );
    assert_eq!(reopened.restore().await, Err(Error::StorageUnconfirmed));
    assert_eq!(*events.lock().unwrap(), ["take", "token", "save"]);
}

#[test]
fn runtime_interruption_retains_storage_uncertainty_and_requires_backend_recovery() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let gate = Gate::new();
    let mut store = FaultStore::new(Store(Arc::new(Mutex::new(Some(saved()))), events.clone()));
    store.hold = Some(("take", gate.clone()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        runtime.handle().clone(),
    );
    let reply = owner.restore();
    runtime.block_on(gate.entered.notified());
    drop(reply);
    drop(runtime);
    assert_eq!(owner.status(), Err(Error::StorageUnconfirmed));
    assert!(store.inner.0.lock().unwrap().is_some());
    assert!(store.recovery_required.load(Ordering::SeqCst));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let reopened = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store,
        runtime.handle().clone(),
    );
    assert_eq!(
        runtime.block_on(reopened.restore()),
        Err(Error::StorageUnconfirmed)
    );
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn interrupted_stop_reports_storage_uncertainty_after_public_owner_drops() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let gate = Gate::new();
    let mut store = FaultStore::new(Store(Arc::new(Mutex::new(Some(saved()))), events.clone()));
    store.hold = Some(("save", gate.clone()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token()])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        runtime.handle().clone(),
    );
    runtime.block_on(owner.restore()).unwrap();
    let stop = owner.graceful_stop();
    runtime.block_on(gate.entered.notified());
    drop(owner);
    drop(runtime);
    let observer = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert_eq!(observer.block_on(stop), Err(Error::StorageUnconfirmed));
    assert!(store.recovery_required.load(Ordering::SeqCst));
    assert!(store.inner.0.lock().unwrap().is_none());
    assert_eq!(*events.lock().unwrap(), ["take", "token"]);
}

#[tokio::test]
async fn invalidation_joins_a_held_removal_before_clearing_and_erasing_credentials() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let gate = Gate::new();
    let mut store = FaultStore::new(Store(Arc::new(Mutex::new(Some(saved()))), events.clone()));
    store.hold = Some(("clear", gate.clone()));
    let owner = PersistentSession::with_transport(
        Configuration::production(),
        Issuer(Arc::new(Mutex::new(vec![token(), token()])), events.clone()),
        Clock(Arc::new(AtomicU64::new(0))),
        store.clone(),
        tokio::runtime::Handle::current(),
    );
    owner.restore().await.unwrap();
    let refresh = owner.refresh();
    gate.entered.notified().await;
    let invalidation = owner.invalidate();
    assert_eq!(owner.with_access_token(|_| ()), Err(Error::Disposed));
    assert_eq!(*events.lock().unwrap(), ["take", "token"]);
    gate.release.notify_one();
    assert_eq!(refresh.await, Err(Error::Stale));
    gate.entered.notified().await;
    gate.release.notify_one();
    invalidation.await.unwrap();
    assert!(store.inner.0.lock().unwrap().is_none());
    assert_eq!(owner.with_access_token(|_| ()), Err(Error::Disposed));
    assert_eq!(
        *events.lock().unwrap(),
        ["take", "token", "clear", "token", "clear"]
    );
}
