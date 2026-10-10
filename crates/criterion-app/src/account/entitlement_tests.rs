// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic entitlement requests through the actual shared Accounts owner.
use super::*;
use criterion_account::{Region, Request, Response, SecretBody, SubscriberTarget};
use criterion_provider::MediaId;
use criterion_session::{Configuration, Endpoint, Status};
use std::collections::VecDeque;
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Clone, Default)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Clone, Default)]
struct Issuer {
    calls: Arc<Mutex<Vec<Endpoint>>>,
    refresh: Option<Arc<Refresh>>,
}
#[derive(Default)]
struct Refresh {
    entered: AtomicBool,
    release: Notify,
}
impl criterion_session::Transport for Issuer {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        self.calls.lock().unwrap().push(request.endpoint);
        if request.endpoint == Endpoint::Token
            && request
                .body
                .expose()
                .windows(b"grant_type=refresh_token".len())
                .any(|part| part == b"grant_type=refresh_token")
            && let Some(gate) = &self.refresh
        {
            gate.entered.store(true, Ordering::SeqCst);
            gate.release.notified().await;
            return Ok(criterion_session::Response {
                status: 400,
                body: SecretBody::new(br#"{"error":"invalid_grant"}"#.to_vec()),
            });
        }
        let body = match request.endpoint {
            Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
            Endpoint::Token => br#"{"access_token":"synthetic-same-access","refresh_token":"synthetic-refresh","expires_in":10}"#.to_vec(),
            Endpoint::Revoke => b"{}".to_vec(),
        };
        Ok(criterion_session::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Observation {
    Bootstrap,
    Subscriber(SubscriberTarget),
    Detail { region: Region, media_id: MediaId },
}
struct Step {
    target: Observation,
    body: Result<Vec<u8>, Error>,
    gate: Option<Arc<Gate>>,
}
impl Step {
    fn bootstrap(region: Region) -> Self {
        let country = match region {
            Region::Us => "US",
            Region::Ca => "CA",
        };
        Self {
            target: Observation::Bootstrap,
            body: Ok(format!(r#"{{"country":"{country}","token":"synthetic-bootstrap","baseUrl":{{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}}}"#).into_bytes()),
            gate: None,
        }
    }
    fn entitlement(
        region: Region,
        captured_unix_time_ms: i64,
        granted: bool,
        customer: i32,
    ) -> Self {
        Self {
            target: Observation::Subscriber(SubscriberTarget::Entitlement {
                region,
                captured_unix_time_ms,
            }),
            body: Ok(
                format!(r#"{{"accessGranted":{granted},"customerId":{customer}}}"#).into_bytes(),
            ),
            gate: None,
        }
    }
    fn read(region: Region, request: &ReadRequest) -> Self {
        let (target, body) = match request {
            ReadRequest::Playback(_) => panic!("fixture does not admit Playback"),
            ReadRequest::Entitlement {
                captured_unix_time_ms,
            } => {
                return Self::entitlement(region, *captured_unix_time_ms, true, 42);
            }
            ReadRequest::NativeDetail { media_id } => (
                Observation::Detail {
                    region,
                    media_id: media_id.clone(),
                },
                format!(
                    r#"{{"contentType":"film","mediaid":"{}","title":"Synthetic native film"}}"#,
                    media_id.as_str()
                )
                .into_bytes(),
            ),
            ReadRequest::ContinueWatching => (
                Observation::Subscriber(SubscriberTarget::ContinueWatching(region)),
                br#"{"playlist":[],"positions":[]}"#.to_vec(),
            ),
            ReadRequest::WatchList(request) => (
                Observation::Subscriber(SubscriberTarget::WatchList {
                    region,
                    request: request.clone(),
                }),
                br#"{"paging":{"page_limit":50},"type_counts":{},"playlist":[]}"#.to_vec(),
            ),
        };
        Self {
            target,
            body: Ok(body),
            gate: None,
        }
    }
}
#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    retiring: AtomicBool,
    retired: AtomicUsize,
    release: Notify,
    drop_fence: (Mutex<bool>, Condvar),
}
impl Gate {
    fn open(&self) {
        *self.drop_fence.0.lock().unwrap() = true;
        self.drop_fence.1.notify_all();
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Trace {
    Started(Observation),
    Retired(Observation),
}
struct Flight {
    middleware: Middleware,
    target: Observation,
    gate: Option<Arc<Gate>>,
}
impl Drop for Flight {
    fn drop(&mut self) {
        if let Some(gate) = &self.gate {
            gate.retiring.store(true, Ordering::SeqCst);
            drop(
                gate.drop_fence
                    .1
                    .wait_while(gate.drop_fence.0.lock().unwrap(), |open| !*open)
                    .unwrap(),
            );
            gate.retired.fetch_add(1, Ordering::SeqCst);
        }
        self.middleware
            .trace
            .lock()
            .unwrap()
            .push(Trace::Retired(self.target.clone()));
        self.middleware.active.fetch_sub(1, Ordering::SeqCst);
    }
}
#[derive(Clone)]
struct Middleware {
    calls: Arc<Mutex<Vec<Observation>>>,
    steps: Arc<Mutex<VecDeque<Step>>>,
    violation: Arc<Mutex<Option<&'static str>>>,
    trace: Arc<Mutex<Vec<Trace>>>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}
impl Middleware {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            calls: Arc::default(),
            steps: Arc::new(Mutex::new(steps.into())),
            violation: Arc::default(),
            trace: Arc::default(),
            active: Arc::default(),
            max_active: Arc::default(),
        }
    }
    fn refuse(&self, message: &'static str) -> Error {
        self.violation.lock().unwrap().get_or_insert(message);
        Error::InvalidRequest
    }
    fn complete(&self) {
        assert_eq!(
            *self.violation.lock().unwrap(),
            None,
            "all synthetic requests must be admitted by the closed script"
        );
        assert!(
            self.steps.lock().unwrap().is_empty(),
            "every expected read must execute"
        );
        assert_eq!(
            self.active.load(Ordering::SeqCst),
            0,
            "all transports retire before owner disposal returns"
        );
        assert!(
            self.max_active.load(Ordering::SeqCst) <= 1,
            "one native account transport at a time"
        );
    }
}
impl criterion_account::Transport for Middleware {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        let target = match request {
            Request::Bootstrap => Observation::Bootstrap,
            Request::Subscriber {
                target,
                credentials,
            } => {
                if credentials.bootstrap().as_bytes() != b"Bearer synthetic-bootstrap"
                    || credentials.subscriber().as_bytes() != b"synthetic-same-access"
                    || !credentials.bootstrap().is_sensitive()
                    || !credentials.subscriber().is_sensitive()
                {
                    return Err(
                        self.refuse("distinct private sensitive header capabilities changed")
                    );
                }
                Observation::Subscriber(target)
            }
            Request::Detail {
                region,
                media_id,
                authorization,
            } => {
                if authorization.header().as_bytes() != b"Bearer synthetic-bootstrap"
                    || !authorization.header().is_sensitive()
                {
                    return Err(self.refuse("anonymous detail authorization changed"));
                }
                Observation::Detail { region, media_id }
            }
        };
        self.calls.lock().unwrap().push(target.clone());
        let Some(step) = self.steps.lock().unwrap().pop_front() else {
            return Err(self.refuse("unscripted account read"));
        };
        if target != step.target {
            return Err(
                self.refuse("exact native request kind/region/captured timestamp/ID changed")
            );
        }
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        self.trace
            .lock()
            .unwrap()
            .push(Trace::Started(target.clone()));
        let _flight = Flight {
            middleware: self.clone(),
            target,
            gate: step.gate.clone(),
        };
        if active != 1 {
            return Err(self.refuse("new read started before predecessor retirement"));
        }
        if let Some(gate) = &step.gate {
            gate.entered.store(true, Ordering::SeqCst);
            gate.release.notified().await;
        }
        Ok(Response {
            status: 200,
            body: SecretBody::new(step.body?),
        })
    }
}
fn entitlement(captured_unix_time_ms: i64) -> ReadRequest {
    ReadRequest::Entitlement {
        captured_unix_time_ms,
    }
}
struct Fixture {
    owner: Accounts<Middleware, Issuer, Clock>,
    runtime: Runtime,
    session: Arc<Session<Issuer, Clock>>,
    account: Arc<AccountClient<Middleware>>,
    clock: Clock,
    issuer: Issuer,
    middleware: Middleware,
    gates: Vec<Arc<Gate>>,
}
impl Fixture {
    fn new(steps: Vec<Step>, signed_in: bool) -> Self {
        Self::with_issuer(steps, signed_in, Issuer::default())
    }
    fn with_issuer(steps: Vec<Step>, signed_in: bool, issuer: Issuer) -> Self {
        let held = steps.iter().any(|s| s.gate.is_some());
        let runtime = if held {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
        } else {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        };
        let clock = Clock::default();
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            issuer.clone(),
            clock.clone(),
        ));
        if signed_in {
            runtime.block_on(session.start_link()).unwrap();
            clock.0.store(5, Ordering::SeqCst);
            runtime.block_on(session.poll_once()).unwrap();
        }
        let gates = steps.iter().filter_map(|s| s.gate.clone()).collect();
        let middleware = Middleware::new(steps);
        let account = Arc::new(AccountClient::with_transport(middleware.clone()));
        Self {
            owner: Accounts::from_parts(account.clone(), session.clone()),
            runtime,
            session,
            account,
            clock,
            issuer,
            middleware,
            gates,
        }
    }
    fn pump(&self) {
        self.runtime.block_on(async {
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
        });
    }
    fn result(&mut self, epoch: u64) -> Result<LoadedAccount, Error> {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            self.pump();
            if let Some(result) = self.owner.poll(&self.runtime, true, epoch) {
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "bounded actual owner read must complete"
            );
            std::thread::yield_now();
        }
    }
    fn wait(&self, predicate: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !predicate() {
            self.pump();
            assert!(
                std::time::Instant::now() < deadline,
                "bounded actual worker phase must settle"
            );
            std::thread::yield_now();
        }
    }
    fn quiet(&mut self, active: bool, epoch: u64) {
        for _ in 0..8 {
            self.pump();
            assert!(self.owner.poll(&self.runtime, active, epoch).is_none());
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(refresh) = &self.issuer.refresh {
            refresh.release.notify_one();
        }
        for gate in &self.gates {
            gate.open();
        }
        self.owner.dispose(&self.runtime);
        if !std::thread::panicking() {
            self.middleware.complete();
        }
    }
}

#[test]
fn entitlement_preserves_exact_caller_timestamp_native_result_and_request_identity() {
    let mut fixture = Fixture::new(
        vec![
            Step::bootstrap(Region::Ca),
            Step::entitlement(Region::Ca, -1700000000123, false, -2147483648),
        ],
        true,
    );
    let request = entitlement(-1700000000123);
    let generation = fixture
        .owner
        .request(fixture.runtime.handle(), 7, request.clone())
        .unwrap();
    assert!(
        fixture.middleware.calls.lock().unwrap().is_empty(),
        "fresh owner waits for foreground polling"
    );
    let loaded = fixture.result(7).unwrap();
    assert_eq!(
        (loaded.generation(), loaded.session_generation()),
        (generation, 7)
    );
    assert!(loaded.matches_request(&request));
    assert!(!loaded.matches_request(&entitlement(-1700000000122)));
    assert!(!loaded.matches_request(&ReadRequest::ContinueWatching));
    let debug = format!("{loaded:?}");
    assert!(
        !debug.contains("-2147483648")
            && !debug.contains("synthetic-same-access")
            && !debug.contains("synthetic-bootstrap")
    );
    assert!(
        !debug.contains("-1700000000123"),
        "entitlement diagnostics omit the captured request value"
    );
    let Loaded::Entitlement {
        captured_unix_time_ms,
        entitlement,
    } = loaded.data
    else {
        panic!("exact native entitlement payload");
    };
    assert_eq!(captured_unix_time_ms, -1700000000123);
    assert!(!entitlement.access_granted);
    assert_eq!(entitlement.customer_id(), i32::MIN);
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Subscriber(SubscriberTarget::Entitlement {
                region: Region::Ca,
                captured_unix_time_ms: -1700000000123
            })
        ]
    );
    assert_eq!(
        *fixture.issuer.calls.lock().unwrap(),
        [Endpoint::DeviceCode, Endpoint::Token]
    );
}

#[test]
fn entitlement_uses_exact_us_ca_signed_extremes_without_recapturing_or_caching() {
    for (region, captured, customer) in [
        (Region::Us, i64::MIN, i32::MAX),
        (Region::Ca, i64::MAX, -7),
        (Region::Us, 0, 0),
    ] {
        let mut fixture = Fixture::new(
            vec![
                Step::bootstrap(region),
                Step::entitlement(region, captured, false, customer),
                Step::entitlement(region, captured, true, customer),
            ],
            true,
        );
        let request = entitlement(captured);
        assert!(request.requires_subscriber());
        for granted in [false, true] {
            let generation = fixture
                .owner
                .request(fixture.runtime.handle(), 3, request.clone())
                .unwrap();
            let loaded = fixture.result(3).unwrap();
            assert_eq!(loaded.generation(), generation);
            assert!(loaded.matches_request(&request));
            let Loaded::Entitlement {
                captured_unix_time_ms,
                entitlement,
            } = loaded.data
            else {
                panic!("exact entitlement response");
            };
            assert_eq!(captured_unix_time_ms, captured);
            assert_eq!(entitlement.access_granted, granted);
            assert_eq!(entitlement.customer_id(), customer);
        }
        assert_eq!(
            fixture.middleware.calls.lock().unwrap().len(),
            3,
            "explicit repeated captures fetch again through one admitted bootstrap"
        );
        assert_eq!(
            *fixture.issuer.calls.lock().unwrap(),
            [Endpoint::DeviceCode, Endpoint::Token]
        );
    }
}

fn detail(id: &str) -> ReadRequest {
    ReadRequest::NativeDetail {
        media_id: MediaId::new(id).unwrap(),
    }
}

#[test]
fn unsigned_and_expired_entitlement_refuse_before_bootstrap_without_refreshing() {
    for signed_in in [false, true] {
        let mut fixture = Fixture::new(
            vec![
                Step::bootstrap(Region::Us),
                Step::read(Region::Us, &detail("Native01")),
            ],
            signed_in,
        );
        if signed_in {
            fixture.clock.0.store(15, Ordering::SeqCst);
        }
        let expected = if signed_in {
            criterion_session::Error::Expired
        } else {
            criterion_session::Error::NoSession
        };
        assert_eq!(
            fixture
                .owner
                .request(fixture.runtime.handle(), 9, entitlement(1700000000123)),
            Err(Error::Session(expected))
        );
        fixture.quiet(true, 9);
        assert!(fixture.middleware.calls.lock().unwrap().is_empty());
        fixture
            .owner
            .request(fixture.runtime.handle(), 9, detail("Native01"))
            .unwrap();
        assert!(
            fixture
                .result(9)
                .unwrap()
                .matches_request(&detail("Native01")),
            "anonymous metadata retains its separate credential contract"
        );
        assert_eq!(
            fixture.issuer.calls.lock().unwrap().len(),
            if signed_in { 2 } else { 0 },
            "Accounts never initiates issuer activation/refresh"
        );
    }
}

#[test]
fn entitlement_and_other_native_reads_join_held_predecessors_and_publish_only_latest() {
    for (first, intermediate, last) in [
        (
            entitlement(1700000000123),
            ReadRequest::WatchList(WatchListRequest::default()),
            detail("Native01"),
        ),
        (
            detail("Native02"),
            ReadRequest::ContinueWatching,
            entitlement(-1700000000456),
        ),
        (
            entitlement(i64::MIN),
            ReadRequest::ContinueWatching,
            entitlement(i64::MAX),
        ),
    ] {
        let gate = Arc::new(Gate::default());
        let mut held = Step::read(Region::Ca, &first);
        held.gate = Some(gate.clone());
        let first_target = held.target.clone();
        let latest = Step::read(Region::Ca, &last);
        let latest_target = latest.target.clone();
        let mut fixture = Fixture::new(vec![Step::bootstrap(Region::Ca), held, latest], true);
        let original = fixture
            .owner
            .request(fixture.runtime.handle(), 11, first.clone())
            .unwrap();
        assert!(fixture.owner.poll(&fixture.runtime, true, 11).is_none());
        fixture.wait(|| gate.entered.load(Ordering::SeqCst));
        let discarded = fixture
            .owner
            .request(fixture.runtime.handle(), 11, intermediate.clone())
            .unwrap();
        let generation = fixture
            .owner
            .request(fixture.runtime.handle(), 11, last.clone())
            .unwrap();
        assert!(original < discarded && discarded < generation);
        fixture.wait(|| gate.retiring.load(Ordering::SeqCst));
        fixture.quiet(true, 11);
        assert_eq!(
            fixture.middleware.calls.lock().unwrap().len(),
            2,
            "pending replacements cannot contact middleware before predecessor joins"
        );
        assert_eq!(
            gate.retired.load(Ordering::SeqCst),
            0,
            "held transport is still inside retirement"
        );
        gate.open();
        let loaded = fixture.result(11).unwrap();
        assert_eq!(
            (loaded.generation(), loaded.session_generation()),
            (generation, 11)
        );
        assert!(loaded.matches_request(&last));
        assert!(!loaded.matches_request(&first) && !loaded.matches_request(&intermediate));
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                first_target.clone(),
                latest_target.clone()
            ]
        );
        assert_eq!(
            *fixture.middleware.trace.lock().unwrap(),
            [
                Trace::Started(Observation::Bootstrap),
                Trace::Retired(Observation::Bootstrap),
                Trace::Started(first_target.clone()),
                Trace::Retired(first_target),
                Trace::Started(latest_target.clone()),
                Trace::Retired(latest_target)
            ]
        );
        assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.middleware.max_active.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn background_retires_entitlement_and_queued_successor_before_foreground_explicit_capture() {
    let gate = Arc::new(Gate::default());
    let mut old = Step::entitlement(Region::Us, 1700000000123, true, 17);
    old.gate = Some(gate.clone());
    let mut fixture = Fixture::new(
        vec![
            Step::bootstrap(Region::Us),
            old,
            Step::entitlement(Region::Us, 1700000000999, false, 19),
        ],
        true,
    );
    fixture
        .owner
        .request(fixture.runtime.handle(), 12, entitlement(1700000000123))
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 12).is_none());
    fixture.wait(|| gate.entered.load(Ordering::SeqCst));
    fixture
        .owner
        .request(fixture.runtime.handle(), 12, detail("Native01"))
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, false, 12).is_none());
    fixture.wait(|| gate.retiring.load(Ordering::SeqCst));
    gate.open();
    fixture.quiet(false, 12);
    assert_eq!(
        fixture.middleware.calls.lock().unwrap().len(),
        2,
        "background clears both old read and already queued native successor"
    );
    let generation = fixture
        .owner
        .request(fixture.runtime.handle(), 12, entitlement(1700000000999))
        .unwrap();
    fixture.quiet(false, 12);
    assert_eq!(
        fixture.middleware.calls.lock().unwrap().len(),
        2,
        "latest background intent waits for foreground"
    );
    let loaded = fixture.result(12).unwrap();
    assert_eq!(loaded.generation(), generation);
    assert!(loaded.matches_request(&entitlement(1700000000999)));
    let Loaded::Entitlement { entitlement, .. } = loaded.data else {
        panic!("new explicit capture");
    };
    assert!(!entitlement.access_granted);
    assert_eq!(entitlement.customer_id(), 19);
    assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 3);
}

#[test]
fn same_token_relink_root_epoch_discards_completed_entitlement_and_refuses_old_epoch() {
    let mut fixture = Fixture::new(
        vec![
            Step::bootstrap(Region::Ca),
            Step::entitlement(Region::Ca, 1700000000123, true, 17),
            Step::entitlement(Region::Ca, 1700000000123, false, 19),
        ],
        true,
    );
    assert!(fixture.owner.poll(&fixture.runtime, true, 40).is_none());
    fixture
        .owner
        .request(fixture.runtime.handle(), 40, entitlement(1700000000123))
        .unwrap();
    fixture.pump();
    assert_eq!(fixture.middleware.active.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture.middleware.calls.lock().unwrap().len(),
        2,
        "the old exact response actually completed before root relink"
    );
    fixture.runtime.block_on(fixture.session.logout()).unwrap();
    fixture
        .runtime
        .block_on(fixture.session.start_link())
        .unwrap();
    fixture.clock.0.store(10, Ordering::SeqCst);
    fixture
        .runtime
        .block_on(fixture.session.poll_once())
        .unwrap();
    assert!(matches!(fixture.session.status(), Status::SignedIn { .. }));
    assert!(
        fixture.owner.poll(&fixture.runtime, true, 41).is_none(),
        "root epoch retires completed result despite identical credential bytes"
    );
    fixture.quiet(true, 41);
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 40, entitlement(1700000000123)),
        Err(Error::Stale)
    );
    let generation = fixture
        .owner
        .request(fixture.runtime.handle(), 41, entitlement(1700000000123))
        .unwrap();
    let loaded = fixture.result(41).unwrap();
    assert_eq!(
        (loaded.generation(), loaded.session_generation()),
        (generation, 41)
    );
    let Loaded::Entitlement { entitlement, .. } = loaded.data else {
        panic!("current root epoch only");
    };
    assert!(!entitlement.access_granted);
    assert_eq!(entitlement.customer_id(), 19);
    assert_eq!(
        fixture.middleware.calls.lock().unwrap().len(),
        3,
        "bootstrap is reused but departed entitlement data is not cached"
    );
}

#[test]
fn entitlement_deadline_schema_and_stale_failures_require_explicit_new_intent() {
    for (body, expected) in [
        (Err(Error::Deadline), Error::Deadline),
        (
            Ok(br#"{"accessGranted":true,"customerId":2147483648}"#.to_vec()),
            Error::InvalidResponse,
        ),
        (
            Ok(br#"{"accessGranted":"true","customerId":17}"#.to_vec()),
            Error::InvalidResponse,
        ),
    ] {
        let mut failed = Step::entitlement(Region::Ca, 1700000000123, true, 17);
        failed.body = body;
        let mut fixture = Fixture::new(
            vec![
                Step::bootstrap(Region::Ca),
                failed,
                Step::entitlement(Region::Ca, 1700000000555, false, 19),
            ],
            true,
        );
        fixture
            .owner
            .request(fixture.runtime.handle(), 50, entitlement(1700000000123))
            .unwrap();
        assert!(matches!(fixture.result(50), Err(error) if error == expected));
        fixture.quiet(true, 50);
        assert_eq!(
            fixture.middleware.calls.lock().unwrap().len(),
            2,
            "ordinary polling never retries failed entitlement"
        );
        fixture
            .owner
            .request(fixture.runtime.handle(), 50, entitlement(1700000000555))
            .unwrap();
        assert!(
            fixture
                .result(50)
                .unwrap()
                .matches_request(&entitlement(1700000000555))
        );
        assert_eq!(
            fixture.middleware.calls.lock().unwrap().len(),
            3,
            "explicit new capture reuses admitted bootstrap"
        );
    }
    let gate = Arc::new(Gate::default());
    gate.open();
    let mut held = Step::entitlement(Region::Us, 1700000000123, true, 17);
    held.gate = Some(gate.clone());
    let mut fixture = Fixture::new(
        vec![
            Step::bootstrap(Region::Us),
            held,
            Step::entitlement(Region::Us, 1700000000555, false, 19),
        ],
        true,
    );
    fixture
        .owner
        .request(fixture.runtime.handle(), 50, entitlement(1700000000123))
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 50).is_none());
    fixture.wait(|| gate.entered.load(Ordering::SeqCst));
    fixture.account.cancel();
    gate.release.notify_one();
    assert!(matches!(fixture.result(50), Err(Error::Stale)));
    fixture.quiet(true, 50);
    assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 2);
    fixture
        .owner
        .request(fixture.runtime.handle(), 50, entitlement(1700000000555))
        .unwrap();
    assert!(
        fixture
            .result(50)
            .unwrap()
            .matches_request(&entitlement(1700000000555))
    );
}

#[test]
fn dispose_joins_held_entitlement_discards_native_successor_and_wipes_account_only() {
    for queued_successor in [false, true] {
        let gate = Arc::new(Gate::default());
        let mut held = Step::entitlement(Region::Ca, 1700000000123, true, 17);
        held.gate = Some(gate.clone());
        let mut fixture = Fixture::new(vec![Step::bootstrap(Region::Ca), held], true);
        fixture
            .owner
            .request(fixture.runtime.handle(), 60, entitlement(1700000000123))
            .unwrap();
        assert!(fixture.owner.poll(&fixture.runtime, true, 60).is_none());
        fixture.wait(|| gate.entered.load(Ordering::SeqCst));
        if queued_successor {
            fixture
                .owner
                .request(fixture.runtime.handle(), 60, detail("Native01"))
                .unwrap();
        }
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        let (observed_pending, retired_at_return) = std::thread::scope(|scope| {
            let owner = &mut fixture.owner;
            let runtime = &fixture.runtime;
            let gate_at_return = gate.clone();
            let disposal = scope.spawn(move || {
                started_tx.send(()).unwrap();
                owner.dispose(runtime);
                completed_tx
                    .send(gate_at_return.retired.load(Ordering::SeqCst))
                    .unwrap();
            });
            let started = started_rx.recv_timeout(Duration::from_secs(2)).is_ok();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !gate.retiring.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                std::thread::yield_now();
            }
            let entered_retirement = gate.retiring.load(Ordering::SeqCst);
            let completion_while_closed = completed_rx.recv_timeout(Duration::from_millis(100));
            let observed_pending = started
                && entered_retirement
                && matches!(
                    completion_while_closed,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                );
            // Open before assertions so a failing no-join oracle cannot strand the thread.
            gate.open();
            let retired_at_return = match completion_while_closed {
                Ok(retired) => retired,
                Err(_) => completed_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            };
            disposal.join().unwrap();
            (observed_pending, retired_at_return)
        });
        assert!(
            observed_pending,
            "dispose cannot return while the actual transport retirement fence remains closed"
        );
        assert_eq!(
            retired_at_return, 1,
            "transport must retire before the disposal API returns"
        );
        assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.account.region(), Err(Error::Disposed));
        assert!(
            fixture.session.with_access_token(|_| ()).is_ok(),
            "read owner disposal never logs out separately owned Session"
        );
        assert_eq!(
            fixture
                .owner
                .request(fixture.runtime.handle(), 60, entitlement(1700000000777)),
            Err(Error::Disposed)
        );
        assert_eq!(
            fixture
                .owner
                .request(fixture.runtime.handle(), 60, detail("Native01")),
            Err(Error::Disposed)
        );
        fixture.quiet(true, 60);
        assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 2);
    }
}

#[test]
fn exhausted_generation_retires_last_entitlement_and_never_wraps_or_starts_successors() {
    let gate = Arc::new(Gate::default());
    let mut held = Step::entitlement(Region::Us, i64::MIN, true, 17);
    held.gate = Some(gate.clone());
    let mut fixture = Fixture::new(vec![Step::bootstrap(Region::Us), held], true);
    // Seed the otherwise unreachable overflow boundary; all decisions use request/poll.
    fixture.owner.generation = u64::MAX - 1;
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), u64::MAX, entitlement(i64::MIN)),
        Ok(u64::MAX)
    );
    assert!(
        fixture
            .owner
            .poll(&fixture.runtime, true, u64::MAX)
            .is_none()
    );
    fixture.wait(|| gate.entered.load(Ordering::SeqCst));
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), u64::MAX, entitlement(i64::MAX)),
        Err(Error::Unavailable)
    );
    fixture.wait(|| gate.retiring.load(Ordering::SeqCst));
    gate.open();
    fixture.wait(|| gate.retired.load(Ordering::SeqCst) == 1);
    fixture.quiet(true, u64::MAX);
    for request in [
        entitlement(i64::MAX),
        detail("Native01"),
        ReadRequest::WatchList(WatchListRequest::default()),
        ReadRequest::ContinueWatching,
    ] {
        assert_eq!(
            fixture
                .owner
                .request(fixture.runtime.handle(), u64::MAX, request),
            Err(Error::Unavailable)
        );
    }
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 0, entitlement(0)),
        Err(Error::Stale),
        "root epoch high-water value never wraps"
    );
    fixture.quiet(true, u64::MAX);
    assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 2);
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
}

#[test]
fn completed_entitlement_cannot_publish_after_expiry_or_logout_and_never_retries() {
    for expire in [false, true] {
        let mut fixture = Fixture::new(
            vec![
                Step::bootstrap(Region::Us),
                Step::entitlement(Region::Us, 1700000000123, true, 17),
            ],
            true,
        );
        assert!(fixture.owner.poll(&fixture.runtime, true, 70).is_none());
        fixture
            .owner
            .request(fixture.runtime.handle(), 70, entitlement(1700000000123))
            .unwrap();
        fixture.pump();
        assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 2);
        assert_eq!(
            fixture.middleware.active.load(Ordering::SeqCst),
            0,
            "actual native decode completes before credential invalidation"
        );
        if expire {
            fixture.clock.0.store(15, Ordering::SeqCst);
        } else {
            fixture.runtime.block_on(fixture.session.logout()).unwrap();
        }
        assert!(
            matches!(
                fixture.owner.poll(&fixture.runtime, true, 70),
                Some(Err(Error::Session(_)))
            ),
            "departed credentials refuse completed entitlement data"
        );
        fixture.quiet(true, 70);
        assert!(matches!(
            fixture
                .owner
                .request(fixture.runtime.handle(), 71, entitlement(1700000000555)),
            Err(Error::Session(_))
        ));
        fixture.quiet(true, 71);
        assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 2);
        assert_eq!(
            fixture
                .owner
                .request(fixture.runtime.handle(), 70, detail("Native01")),
            Err(Error::Stale),
            "failed subscriber preflight still advances root epoch high-water value"
        );
        assert_eq!(
            fixture.issuer.calls.lock().unwrap().len(),
            if expire { 2 } else { 3 },
            "ordinary polls issue no refresh/revoke/relink"
        );
    }
}

#[test]
fn held_refresh_keeps_original_expiry_and_failed_refresh_requires_explicit_relink() {
    let read_gate = Arc::new(Gate::default());
    read_gate.open();
    let refresh = Arc::new(Refresh::default());
    let issuer = Issuer {
        refresh: Some(refresh.clone()),
        ..Issuer::default()
    };
    let mut held = Step::entitlement(Region::Ca, 1700000000123, true, 17);
    held.gate = Some(read_gate.clone());
    let mut fixture = Fixture::with_issuer(
        vec![
            Step::bootstrap(Region::Ca),
            held,
            Step::entitlement(Region::Ca, 1700000000555, false, 19),
        ],
        true,
        issuer,
    );
    fixture
        .owner
        .request(fixture.runtime.handle(), 80, entitlement(1700000000123))
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 80).is_none());
    fixture.wait(|| read_gate.entered.load(Ordering::SeqCst));
    fixture.clock.0.store(14, Ordering::SeqCst);
    let session = fixture.session.clone();
    let refresh_job = fixture
        .runtime
        .spawn(async move { session.refresh().await });
    fixture.wait(|| refresh.entered.load(Ordering::SeqCst));
    assert!(
        fixture.owner.poll(&fixture.runtime, true, 80).is_none(),
        "held refresh retains original still-valid access before its expiry"
    );
    assert_eq!(read_gate.retired.load(Ordering::SeqCst), 0);
    fixture.clock.0.store(15, Ordering::SeqCst);
    assert!(
        matches!(
            fixture.owner.poll(&fixture.runtime, true, 80),
            Some(Err(Error::Session(criterion_session::Error::Expired)))
        ),
        "refresh does not extend original lease before success"
    );
    fixture.wait(|| read_gate.retired.load(Ordering::SeqCst) == 1);
    fixture.quiet(true, 80);
    assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 2);
    refresh.release.notify_one();
    assert_eq!(
        fixture.runtime.block_on(refresh_job).unwrap(),
        Err(criterion_session::Error::ReauthenticationRequired)
    );
    assert_eq!(fixture.session.status(), Status::ReauthenticationRequired);
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 81, entitlement(1700000000555)),
        Err(Error::Session(criterion_session::Error::NoSession))
    );
    fixture.quiet(true, 81);
    assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 2);
    fixture
        .runtime
        .block_on(fixture.session.start_link())
        .unwrap();
    fixture.clock.0.store(20, Ordering::SeqCst);
    fixture
        .runtime
        .block_on(fixture.session.poll_once())
        .unwrap();
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 80, entitlement(1700000000555)),
        Err(Error::Stale)
    );
    let generation = fixture
        .owner
        .request(fixture.runtime.handle(), 81, entitlement(1700000000555))
        .unwrap();
    let loaded = fixture.result(81).unwrap();
    assert_eq!(
        (loaded.generation(), loaded.session_generation()),
        (generation, 81)
    );
    assert!(loaded.matches_request(&entitlement(1700000000555)));
    assert_eq!(
        fixture.middleware.calls.lock().unwrap().len(),
        3,
        "explicit relink/capture reuses bootstrap without retrying departed entitlement"
    );
    assert_eq!(
        *fixture.issuer.calls.lock().unwrap(),
        [
            Endpoint::DeviceCode,
            Endpoint::Token,
            Endpoint::Token,
            Endpoint::DeviceCode,
            Endpoint::Token
        ]
    );
}
