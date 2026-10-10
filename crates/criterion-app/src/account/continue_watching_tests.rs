// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic native requests through the real shared Accounts/AccountClient/Session owners.
use super::*;
use criterion_account::{Region, Request, Response, SecretBody, SubscriberTarget};
use criterion_provider::MediaId;
use criterion_session::{Configuration, Endpoint};
use std::collections::VecDeque;
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;
#[derive(Clone, Debug, PartialEq, Eq)]
enum Observation {
    Bootstrap,
    Subscriber(SubscriberTarget),
    Detail,
}

#[derive(Clone)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Default)]
struct Refresh {
    entered: AtomicBool,
    release: Notify,
    fail: bool,
}
#[derive(Clone, Default)]
struct Issuer {
    requests: Arc<Mutex<Vec<Endpoint>>>,
    refresh: Option<Arc<Refresh>>,
}
impl criterion_session::Transport for Issuer {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        self.requests.lock().unwrap().push(request.endpoint);
        let refreshing = request.endpoint == Endpoint::Token
            && request
                .body
                .expose()
                .windows(b"grant_type=refresh_token".len())
                .any(|part| part == b"grant_type=refresh_token");
        if refreshing && let Some(gate) = &self.refresh {
            gate.entered.store(true, Ordering::SeqCst);
            gate.release.notified().await;
            if gate.fail {
                return Ok(criterion_session::Response {
                    status: 400,
                    body: SecretBody::new(br#"{"error":"invalid_grant"}"#.to_vec()),
                });
            }
        }
        let body=match request.endpoint {
            Endpoint::DeviceCode=>br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
            Endpoint::Token if refreshing=>br#"{"access_token":"synthetic-rotated-access","refresh_token":"synthetic-rotated-refresh","expires_in":3600}"#.to_vec(),
            Endpoint::Token=>br#"{"access_token":"synthetic-same-access","refresh_token":"synthetic-refresh","expires_in":10}"#.to_vec(),
            Endpoint::Revoke=>b"{}".to_vec(),
        };
        Ok(criterion_session::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
const CONTINUE: &[u8] = br#"{"playlist":[{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic continued film","duration":90.5,"release_date":"2000-02-29"}],"positions":[{"media_id":"AbCd1234","pos":-1,"dur":7200}]}"#;

const WATCH: &[u8] = br#"{"paging":{"page_limit":50},"type_counts":{"film":1},"playlist":[{"contentType":"film","mediaid":"Watch001","title":"Synthetic Watch List film"}]}"#;
struct Step {
    target: Observation,
    response: Result<Vec<u8>, Error>,
    gate: Option<Arc<Gate>>,
    refreshed: bool,
}
impl Step {
    fn page(request: ReadRequest) -> Self {
        match request {
            ReadRequest::WatchList(request) => Self {
                target: Observation::Subscriber(SubscriberTarget::WatchList {
                    region: Region::Ca,
                    request,
                }),
                response: Ok(WATCH.to_vec()),
                gate: None,
                refreshed: false,
            },
            ReadRequest::ContinueWatching => Self {
                target: Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
                response: Ok(CONTINUE.to_vec()),
                gate: None,
                refreshed: false,
            },
            ReadRequest::NativeDetail { .. } => {
                panic!("private read fixture does not admit NativeDetail")
            }
            ReadRequest::Entitlement { .. } => {
                panic!("private shelf fixture does not admit Entitlement")
            }
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
    violation: Arc<Mutex<Option<&'static str>>>,
    steps: Arc<Mutex<VecDeque<Step>>>,
    trace: Arc<Mutex<Vec<Trace>>>,
    active: Arc<AtomicUsize>,
}
impl Default for Middleware {
    fn default() -> Self {
        Self::new(vec![Step::page(ReadRequest::ContinueWatching)])
    }
}
impl Middleware {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            calls: Arc::default(),
            violation: Arc::default(),
            steps: Arc::new(Mutex::new(steps.into())),
            trace: Arc::default(),
            active: Arc::default(),
        }
    }
    fn violation(&self, message: &'static str) -> Error {
        *self.violation.lock().unwrap() = Some(message);
        Error::InvalidRequest
    }
    fn assert_complete(&self) {
        assert_eq!(
            *self.violation.lock().unwrap(),
            None,
            "synthetic transport fixture violation"
        );
        assert!(
            self.steps.lock().unwrap().is_empty(),
            "complete native method script must be exercised"
        );
    }
}
impl criterion_account::Transport for Middleware {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        let (target, headers) = match request {
            Request::Bootstrap => {
                self.calls.lock().unwrap().push(Observation::Bootstrap);
                return Ok(Response {status:200,body:SecretBody::new(br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec())});
            }
            Request::Subscriber {
                target,
                credentials,
            } => (Observation::Subscriber(target), credentials),
            Request::Detail { .. } => {
                self.calls.lock().unwrap().push(Observation::Detail);
                return Err(self.violation("unexpected native Detail read"));
            }
        };
        self.calls.lock().unwrap().push(target.clone());
        let Some(step) = self.steps.lock().unwrap().pop_front() else {
            return Err(self.violation("unexpected additional native method"));
        };
        if step.target != target {
            return Err(self.violation("unexpected native method/region/filter/cursor"));
        }
        let subscriber = if step.refreshed {
            b"synthetic-rotated-access".as_slice()
        } else {
            b"synthetic-same-access".as_slice()
        };
        if headers.bootstrap().as_bytes() != b"Bearer synthetic-bootstrap"
            || headers.subscriber().as_bytes() != subscriber
            || !headers.bootstrap().is_sensitive()
            || !headers.subscriber().is_sensitive()
        {
            return Err(self.violation("private header roles or credential generation changed"));
        }
        self.trace
            .lock()
            .unwrap()
            .push(Trace::Started(target.clone()));
        let active = self.active.fetch_add(1, Ordering::SeqCst);
        let _flight = Flight {
            middleware: self.clone(),
            target,
            gate: step.gate.clone(),
        };
        if active != 0 {
            return Err(self.violation("overlapping native account read"));
        }
        if let Some(gate) = step.gate {
            gate.entered.store(true, Ordering::SeqCst);
            gate.release.notified().await;
        }
        Ok(Response {
            status: 200,
            body: SecretBody::new(step.response?),
        })
    }
}
struct Fixture {
    owner: Accounts<Middleware, Issuer, Clock>,
    runtime: Runtime,
    session: Arc<Session<Issuer, Clock>>,
    time: Arc<AtomicU64>,
    middleware: Middleware,
    gates: Vec<Arc<Gate>>,
    issuer: Issuer,
}
impl Fixture {
    fn new(steps: Vec<Step>) -> Self {
        Self::with_issuer(steps, Issuer::default())
    }
    // Immediate transport responses and synchronous native DTO admission run
    // through job completion in this runtime's pump. This establishes the
    // completed-but-unpublished boundary without relying on Transport drop.
    fn current_thread(steps: Vec<Step>) -> Self {
        Self::with_runtime(
            steps,
            Issuer::default(),
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
        )
    }
    fn with_issuer(steps: Vec<Step>, issuer: Issuer) -> Self {
        Self::with_runtime(
            steps,
            issuer,
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap(),
        )
    }
    fn with_runtime(steps: Vec<Step>, issuer: Issuer, runtime: Runtime) -> Self {
        let time = Arc::new(AtomicU64::new(0));
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            issuer.clone(),
            Clock(time.clone()),
        ));
        runtime.block_on(session.start_link()).unwrap();
        time.store(5, Ordering::SeqCst);
        runtime.block_on(session.poll_once()).unwrap();
        let gates = steps.iter().filter_map(|step| step.gate.clone()).collect();
        let middleware = Middleware::new(steps);
        Self {
            owner: Accounts::from_parts(
                Arc::new(AccountClient::with_transport(middleware.clone())),
                session.clone(),
            ),
            runtime,
            session,
            time,
            middleware,
            gates,
            issuer,
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
                "synthetic account job must settle"
            );
            std::thread::yield_now();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(gate) = &self.issuer.refresh {
            gate.release.notify_one();
        }
        for gate in &self.gates {
            gate.open();
        }
        self.owner.dispose(&self.runtime);
        if !std::thread::panicking() {
            self.middleware.assert_complete();
        }
    }
}

#[test]
fn continue_watching_uses_shared_owner_and_lends_exact_native_payload() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let time = Arc::new(AtomicU64::new(0));
    let session = Arc::new(Session::with_transport(
        Configuration::production(),
        Issuer::default(),
        Clock(time.clone()),
    ));
    runtime.block_on(session.start_link()).unwrap();
    time.store(5, Ordering::SeqCst);
    runtime.block_on(session.poll_once()).unwrap();
    let middleware = Middleware::default();
    let mut owner = Accounts::from_parts(
        Arc::new(AccountClient::with_transport(middleware.clone())),
        session,
    );
    let generation = owner
        .request(runtime.handle(), 7, ReadRequest::ContinueWatching)
        .unwrap();
    runtime.block_on(async {
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    });
    assert!(
        middleware.calls.lock().unwrap().is_empty(),
        "queued intent waits for foreground"
    );
    let mut completed = None;
    for _ in 0..32 {
        if let Some(result) = owner.poll(&runtime, true, 7) {
            completed = Some(result.unwrap());
            break;
        }
        runtime.block_on(async {
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
        });
    }
    let loaded = completed.expect("native Continue Watching must complete");
    assert_eq!(loaded.generation(), generation);
    assert_eq!(loaded.session_generation(), 7);
    assert!(loaded.matches_request(&ReadRequest::ContinueWatching));
    assert!(!loaded.matches_request(&ReadRequest::WatchList(WatchListRequest::default())));
    let Loaded::ContinueWatching(data) = loaded.data else {
        panic!("native method must retain its payload variant");
    };
    assert_eq!(data.playlist.len(), 1);
    assert_eq!(data.playlist[0].id, MediaId::new("AbCd1234").unwrap());
    assert_eq!(data.playlist[0].title, "Synthetic continued film");
    assert_eq!(data.playlist[0].duration, Some(90.5));
    assert_eq!(
        data.playlist[0].release_date.unwrap().to_string(),
        "2000-02-29"
    );
    assert_eq!(
        data.positions[0].media_id,
        MediaId::new("AbCd1234").unwrap()
    );
    assert_eq!((data.positions[0].pos, data.positions[0].dur), (-1, 7200));
    owner.dispose(&runtime);
    assert_eq!(
        *middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
        ]
    );
    middleware.assert_complete();
}

#[test]
fn native_methods_share_one_bootstrap_and_retain_each_typed_result() {
    let first = ReadRequest::WatchList(WatchListRequest::default());
    let last = ReadRequest::WatchList(WatchListRequest {
        filter: criterion_account::WatchListFilter::OriginalFranchise,
        cursor: Some(criterion_provider::PageCursor::new("synthetic-observed/+% é").unwrap()),
    });
    let mut fixture = Fixture::new(vec![
        Step::page(first.clone()),
        Step::page(ReadRequest::ContinueWatching),
        Step::page(last.clone()),
    ]);
    let mut previous = 0;
    for request in [first.clone(), ReadRequest::ContinueWatching, last.clone()] {
        let generation = fixture
            .owner
            .request(fixture.runtime.handle(), 12, request.clone())
            .unwrap();
        let loaded = fixture.result(12).unwrap();
        assert!(generation > previous);
        previous = generation;
        assert_eq!(loaded.generation(), generation);
        assert_eq!(loaded.session_generation(), 12);
        assert!(loaded.matches_request(&request));
        match &loaded.data {
            Loaded::WatchList {
                request: actual,
                page,
            } => {
                assert!(matches!(request,ReadRequest::WatchList(ref expected) if expected==actual));
                assert_eq!(page.playlist[0].id, MediaId::new("Watch001").unwrap());
                assert!(!loaded.matches_request(&ReadRequest::ContinueWatching));
            }
            Loaded::ContinueWatching(data) => {
                assert_eq!(data.positions[0].pos, -1);
                assert!(!loaded.matches_request(&first));
            }
            Loaded::NativeDetail(_) => {
                panic!("private read fixture does not admit NativeDetail")
            }
            Loaded::Entitlement { .. } => {
                panic!("private shelf fixture does not admit Entitlement")
            }
        }
        assert!(!format!("{loaded:?}").contains("Synthetic"));
        assert!(!format!("{loaded:?}").contains("AbCd1234"));
        assert!(!format!("{loaded:?}").contains("synthetic-observed"));
    }
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Step::page(first).target,
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
            Step::page(last).target
        ]
    );
}

#[test]
fn cross_kind_replacement_joins_held_old_read_before_only_latest_method_enters() {
    let watch_first = ReadRequest::WatchList(WatchListRequest {
        filter: criterion_account::WatchListFilter::FilmSeries,
        cursor: None,
    });
    let watch_last = ReadRequest::WatchList(WatchListRequest {
        filter: criterion_account::WatchListFilter::Category,
        cursor: Some(criterion_provider::PageCursor::new("synthetic-latest/+%opaque").unwrap()),
    });
    for (first, intermediate, last) in [
        (
            watch_first,
            ReadRequest::ContinueWatching,
            ReadRequest::ContinueWatching,
        ),
        (
            ReadRequest::ContinueWatching,
            ReadRequest::WatchList(WatchListRequest::default()),
            watch_last,
        ),
    ] {
        let gate = Arc::new(Gate::default());
        let mut old = Step::page(first.clone());
        old.gate = Some(gate.clone());
        let first_target = old.target.clone();
        let last_target = Step::page(last.clone()).target;
        let mut fixture = Fixture::new(vec![old, Step::page(last.clone())]);
        fixture
            .owner
            .request(fixture.runtime.handle(), 20, first.clone())
            .unwrap();
        assert!(fixture.owner.poll(&fixture.runtime, true, 20).is_none());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !gate.entered.load(Ordering::SeqCst) {
            fixture.pump();
            assert!(
                std::time::Instant::now() < deadline,
                "old read actually enters"
            );
            std::thread::yield_now();
        }
        fixture
            .owner
            .request(fixture.runtime.handle(), 20, intermediate)
            .unwrap();
        let latest = fixture
            .owner
            .request(fixture.runtime.handle(), 20, last.clone())
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !gate.retiring.load(Ordering::SeqCst) {
            fixture.pump();
            assert!(
                std::time::Instant::now() < deadline,
                "old transport begins retirement"
            );
            std::thread::yield_now();
        }
        for _ in 0..4 {
            assert!(fixture.owner.poll(&fixture.runtime, true, 20).is_none());
            fixture.pump();
            assert_eq!(
                *fixture.middleware.calls.lock().unwrap(),
                [Observation::Bootstrap, first_target.clone()],
                "held retirement blocks successor contact"
            );
        }
        assert_eq!(gate.retired.load(Ordering::SeqCst), 0);
        gate.open();
        let loaded = fixture.result(20).unwrap();
        assert_eq!(loaded.generation(), latest);
        assert_eq!(loaded.session_generation(), 20);
        assert!(loaded.matches_request(&last));
        assert!(!loaded.matches_request(&first));
        assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                first_target.clone(),
                last_target.clone()
            ]
        );
        let trace = fixture.middleware.trace.lock().unwrap();
        let retired = trace
            .iter()
            .position(|event| event == &Trace::Retired(first_target.clone()))
            .unwrap();
        let started = trace
            .iter()
            .position(|event| event == &Trace::Started(last_target.clone()))
            .unwrap();
        assert!(
            retired < started,
            "old transport retirement must precede successor entry"
        );
        drop(trace);
        gate.release.notify_one();
        fixture.pump();
        assert!(fixture.owner.poll(&fixture.runtime, true, 20).is_none());
    }
}

#[test]
fn background_discards_continue_result_and_starts_only_explicit_latest_foreground_intent() {
    for held in [false, true] {
        let gate = Arc::new(Gate::default());
        gate.open();
        let mut first = Step::page(ReadRequest::ContinueWatching);
        if held {
            first.gate = Some(gate.clone());
        }
        let latest_request = ReadRequest::WatchList(WatchListRequest {
            filter: criterion_account::WatchListFilter::OriginalFranchise,
            cursor: Some(
                criterion_provider::PageCursor::new("synthetic-after-background/+%").unwrap(),
            ),
        });
        let latest_target = Step::page(latest_request.clone()).target;
        let steps = vec![first, Step::page(latest_request.clone())];
        let mut fixture = if held {
            Fixture::new(steps)
        } else {
            Fixture::current_thread(steps)
        };
        assert!(fixture.owner.poll(&fixture.runtime, true, 30).is_none());
        fixture
            .owner
            .request(fixture.runtime.handle(), 30, ReadRequest::ContinueWatching)
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            fixture.pump();
            let entered = fixture.middleware.calls.lock().unwrap().len() == 2;
            let finished = fixture
                .middleware
                .trace
                .lock()
                .unwrap()
                .contains(&Trace::Retired(Observation::Subscriber(
                    SubscriberTarget::ContinueWatching(Region::Ca),
                )));
            if entered && (held && gate.entered.load(Ordering::SeqCst) || !held && finished) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "old read reaches controlled departure boundary"
            );
            std::thread::yield_now();
        }
        fixture.owner.background();
        for _ in 0..4 {
            fixture.pump();
            assert!(fixture.owner.poll(&fixture.runtime, false, 30).is_none());
        }
        assert!(
            fixture.owner.poll(&fixture.runtime, true, 30).is_none(),
            "foreground alone cannot reload departed private payload"
        );
        fixture.owner.background();
        fixture
            .owner
            .request(fixture.runtime.handle(), 30, ReadRequest::ContinueWatching)
            .unwrap();
        let latest = fixture
            .owner
            .request(fixture.runtime.handle(), 30, latest_request.clone())
            .unwrap();
        for _ in 0..4 {
            fixture.pump();
            assert!(fixture.owner.poll(&fixture.runtime, false, 30).is_none());
            assert_eq!(
                fixture.middleware.calls.lock().unwrap().len(),
                2,
                "inactive latest intent has no contact"
            );
        }
        let loaded = fixture.result(30).unwrap();
        assert_eq!(loaded.generation(), latest);
        assert!(loaded.matches_request(&latest_request));
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
                latest_target
            ]
        );
        if held {
            assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
        }
    }
}

#[test]
fn same_token_relink_rejects_returned_unpublished_continue_epoch_and_preserves_new_read() {
    let mut new = Step::page(ReadRequest::ContinueWatching);
    new.response = Ok(String::from_utf8(CONTINUE.to_vec())
        .unwrap()
        .replace("Synthetic continued film", "Synthetic newer film")
        .into_bytes());
    let mut fixture = Fixture::current_thread(vec![Step::page(ReadRequest::ContinueWatching), new]);
    assert!(fixture.owner.poll(&fixture.runtime, true, 40).is_none());
    let old = fixture
        .owner
        .request(fixture.runtime.handle(), 40, ReadRequest::ContinueWatching)
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !fixture
        .middleware
        .trace
        .lock()
        .unwrap()
        .contains(&Trace::Retired(Observation::Subscriber(
            SubscriberTarget::ContinueWatching(Region::Ca),
        )))
    {
        fixture.pump();
        assert!(
            std::time::Instant::now() < deadline,
            "old transport returns before publication"
        );
        std::thread::yield_now();
    }
    fixture.runtime.block_on(fixture.session.logout()).unwrap();
    fixture
        .runtime
        .block_on(fixture.session.start_link())
        .unwrap();
    fixture.time.store(10, Ordering::SeqCst);
    fixture
        .runtime
        .block_on(fixture.session.poll_once())
        .unwrap();
    assert!(
        fixture.owner.poll(&fixture.runtime, true, 41).is_none(),
        "old private epoch cannot publish identical-token payload"
    );
    fixture.pump();
    assert!(fixture.owner.poll(&fixture.runtime, true, 41).is_none());
    let current = fixture
        .owner
        .request(fixture.runtime.handle(), 41, ReadRequest::ContinueWatching)
        .unwrap();
    assert!(current > old);
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 40, ReadRequest::ContinueWatching),
        Err(Error::Stale)
    );
    let loaded = fixture.result(41).unwrap();
    assert_eq!(loaded.generation(), current);
    assert_eq!(loaded.session_generation(), 41);
    let Loaded::ContinueWatching(data) = loaded.data else {
        panic!("current native payload");
    };
    assert_eq!(data.playlist[0].title, "Synthetic newer film");
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
        ]
    );
}

#[test]
fn expired_continue_read_retires_during_held_session_refresh_and_new_access_reuses_bootstrap() {
    let read_gate = Arc::new(Gate::default());
    read_gate.open();
    let refresh = Arc::new(Refresh::default());
    let issuer = Issuer {
        refresh: Some(refresh.clone()),
        ..Issuer::default()
    };
    let mut old = Step::page(ReadRequest::ContinueWatching);
    old.gate = Some(read_gate.clone());
    let mut next = Step::page(ReadRequest::ContinueWatching);
    next.refreshed = true;
    next.response = Ok(String::from_utf8(CONTINUE.to_vec())
        .unwrap()
        .replace("Synthetic continued film", "Synthetic refreshed film")
        .into_bytes());
    let mut fixture = Fixture::with_issuer(vec![old, next], issuer);
    fixture
        .owner
        .request(fixture.runtime.handle(), 50, ReadRequest::ContinueWatching)
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 50).is_none());
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !read_gate.entered.load(Ordering::SeqCst) {
        fixture.pump();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    fixture.time.store(15, Ordering::SeqCst);
    assert_eq!(
        fixture.session.status(),
        criterion_session::Status::RefreshRequired
    );
    let session = fixture.session.clone();
    let refresh_job = fixture
        .runtime
        .spawn(async move { session.refresh().await });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !refresh.entered.load(Ordering::SeqCst) {
        fixture.pump();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(matches!(
        fixture.owner.poll(&fixture.runtime, true, 50),
        Some(Err(Error::Session(_)))
    ));
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while read_gate.retired.load(Ordering::SeqCst) != 1 {
        fixture.pump();
        assert!(fixture.owner.poll(&fixture.runtime, true, 50).is_none());
        assert!(
            std::time::Instant::now() < deadline,
            "expired read retirement completes while refresh remains held"
        );
        std::thread::yield_now();
    }
    assert_eq!(read_gate.retired.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
        ],
        "no private retry while the issuer refresh is held"
    );
    refresh.release.notify_one();
    fixture.runtime.block_on(refresh_job).unwrap().unwrap();
    let generation = fixture
        .owner
        .request(fixture.runtime.handle(), 50, ReadRequest::ContinueWatching)
        .unwrap();
    let loaded = fixture.result(50).unwrap();
    assert_eq!(loaded.generation(), generation);
    assert_eq!(loaded.session_generation(), 50);
    let Loaded::ContinueWatching(data) = loaded.data else {
        panic!("renewed native payload");
    };
    assert_eq!(data.playlist[0].title, "Synthetic refreshed film");
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
        ]
    );
    assert_eq!(
        *fixture.issuer.requests.lock().unwrap(),
        [Endpoint::DeviceCode, Endpoint::Token, Endpoint::Token],
        "Accounts must not issue issuer requests or revoke during a read"
    );
}

#[test]
fn continue_errors_have_no_automatic_retry_and_explicit_new_demand_reuses_bootstrap() {
    for (response, expected) in [
        (Ok(b"[]".to_vec()), Error::InvalidResponse),
        (Err(Error::Deadline), Error::Deadline),
        (Err(Error::HttpStatus(503)), Error::HttpStatus(503)),
    ] {
        let mut first = Step::page(ReadRequest::ContinueWatching);
        first.response = response;
        let mut fixture = Fixture::new(vec![first, Step::page(ReadRequest::ContinueWatching)]);
        let old = fixture
            .owner
            .request(fixture.runtime.handle(), 60, ReadRequest::ContinueWatching)
            .unwrap();
        assert_eq!(fixture.result(60).unwrap_err(), expected);
        for _ in 0..8 {
            fixture.pump();
            assert!(fixture.owner.poll(&fixture.runtime, true, 60).is_none());
        }
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
            ]
        );
        let current = fixture
            .owner
            .request(fixture.runtime.handle(), 60, ReadRequest::ContinueWatching)
            .unwrap();
        assert!(current > old);
        let loaded = fixture.result(60).unwrap();
        assert_eq!(loaded.generation(), current);
        assert!(loaded.matches_request(&ReadRequest::ContinueWatching));
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
                Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
            ]
        );
    }
}

#[test]
fn expired_or_logged_out_continue_payload_cannot_publish_and_failed_epoch_stays_retired() {
    for expire in [false, true] {
        let mut fixture = Fixture::current_thread(vec![
            Step::page(ReadRequest::ContinueWatching),
            Step::page(ReadRequest::ContinueWatching),
        ]);
        assert!(fixture.owner.poll(&fixture.runtime, true, 70).is_none());
        fixture
            .owner
            .request(fixture.runtime.handle(), 70, ReadRequest::ContinueWatching)
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !fixture
            .middleware
            .trace
            .lock()
            .unwrap()
            .contains(&Trace::Retired(Observation::Subscriber(
                SubscriberTarget::ContinueWatching(Region::Ca),
            )))
        {
            fixture.pump();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        if expire {
            fixture.time.store(15, Ordering::SeqCst);
        } else {
            fixture.runtime.block_on(fixture.session.logout()).unwrap();
        }
        assert!(matches!(
            fixture.owner.poll(&fixture.runtime, true, 70),
            Some(Err(Error::Session(_)))
        ));
        assert!(matches!(
            fixture
                .owner
                .request(fixture.runtime.handle(), 71, ReadRequest::ContinueWatching),
            Err(Error::Session(_))
        ));
        for _ in 0..4 {
            fixture.pump();
            assert!(fixture.owner.poll(&fixture.runtime, true, 71).is_none());
        }
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
            ]
        );
        fixture.runtime.block_on(fixture.session.logout()).unwrap();
        fixture
            .runtime
            .block_on(fixture.session.start_link())
            .unwrap();
        fixture.time.fetch_add(5, Ordering::SeqCst);
        fixture
            .runtime
            .block_on(fixture.session.poll_once())
            .unwrap();
        assert_eq!(
            fixture
                .owner
                .request(fixture.runtime.handle(), 70, ReadRequest::ContinueWatching),
            Err(Error::Stale)
        );
        let generation = fixture
            .owner
            .request(fixture.runtime.handle(), 71, ReadRequest::ContinueWatching)
            .unwrap();
        let loaded = fixture.result(71).unwrap();
        assert_eq!(loaded.generation(), generation);
        assert_eq!(loaded.session_generation(), 71);
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
                Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
            ]
        );
    }
}

#[test]
fn dispose_joins_held_continue_read_discards_pending_watch_list_and_clears_client() {
    let gate = Arc::new(Gate::default());
    let mut old = Step::page(ReadRequest::ContinueWatching);
    old.gate = Some(gate.clone());
    let mut fixture = Fixture::new(vec![old]);
    fixture
        .owner
        .request(fixture.runtime.handle(), 80, ReadRequest::ContinueWatching)
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 80).is_none());
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !gate.entered.load(Ordering::SeqCst) {
        fixture.pump();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let latest = ReadRequest::WatchList(WatchListRequest {
        filter: criterion_account::WatchListFilter::Collection,
        cursor: Some(criterion_provider::PageCursor::new("synthetic-disposed/+%").unwrap()),
    });
    fixture
        .owner
        .request(fixture.runtime.handle(), 80, latest.clone())
        .unwrap();
    let account = fixture.owner.account.clone();
    let finished = AtomicBool::new(false);
    let reached_fence = std::thread::scope(|scope| {
        let owner = &mut fixture.owner;
        let runtime = &fixture.runtime;
        let finished = &finished;
        let disposal = scope.spawn(move || {
            owner.dispose(runtime);
            finished.store(true, Ordering::SeqCst);
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !gate.retiring.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        // Open before any assertion: a failing oracle must still release the
        // scoped disposal thread and let the fixture dispose its runtime.
        let reached_fence =
            gate.retiring.load(Ordering::SeqCst) && !finished.load(Ordering::SeqCst);
        gate.open();
        disposal.join().unwrap();
        reached_fence
    });
    assert!(
        reached_fence,
        "dispose must await actual read retirement before returning"
    );
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
    assert!(finished.load(Ordering::SeqCst));
    assert_eq!(account.region(), Err(Error::Disposed));
    assert!(
        fixture.session.with_access_token(|_| ()).is_ok(),
        "read disposal does not log out the separately owned session"
    );
    assert_eq!(
        fixture.owner.request(fixture.runtime.handle(), 80, latest),
        Err(Error::Disposed)
    );
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 80, ReadRequest::ContinueWatching),
        Err(Error::Disposed)
    );
    assert!(fixture.owner.poll(&fixture.runtime, true, 80).is_none());
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca))
        ]
    );
}
