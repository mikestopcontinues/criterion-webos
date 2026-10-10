// SPDX-License-Identifier: GPL-3.0-or-later
//! Anonymous Detail and private reads share the real account worker lifetime.
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
struct Clock {
    now: Arc<AtomicU64>,
    reads: Arc<AtomicUsize>,
}
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Duration::from_secs(self.now.load(Ordering::SeqCst))
    }
}
#[derive(Clone, Default)]
struct Issuer {
    requests: Arc<Mutex<Vec<Endpoint>>>,
    grant: bool,
}
impl criterion_session::Transport for Issuer {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        self.requests.lock().unwrap().push(request.endpoint);
        if !self.grant {
            return Err(criterion_session::Error::Unavailable);
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
    Detail { region: Region, media_id: MediaId },
    Subscriber(SubscriberTarget),
}
struct Step {
    target: Observation,
    body: Result<Vec<u8>, Error>,
    gate: Option<Arc<Gate>>,
}
impl Step {
    fn read(request: &ReadRequest) -> Self {
        let (target, body) = match request {
            ReadRequest::Playback(_) => panic!("fixture does not admit Playback"),
            ReadRequest::NativeDetail { media_id } => (
                Observation::Detail { region: Region::Ca, media_id: media_id.clone() },
                format!(r#"{{"contentType":"film","mediaid":"{}","title":"Synthetic native film","description":"Synthetic native description","duration":90.5}}"#, media_id.as_str()).into_bytes(),
            ),
            ReadRequest::ContinueWatching => (
                Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
                br#"{"playlist":[{"contentType":"film","mediaid":"Contin01","title":"Synthetic continued film"}],"positions":[{"media_id":"Contin01","pos":-1,"dur":7200}]}"#.to_vec(),
            ),
            ReadRequest::WatchList(request) => (
                Observation::Subscriber(SubscriberTarget::WatchList { region: Region::Ca, request: request.clone() }),
                br#"{"paging":{"page_limit":50},"type_counts":{"film":1},"playlist":[{"contentType":"film","mediaid":"Watch001","title":"Synthetic private film"}]}"#.to_vec(),
            ),
            ReadRequest::Entitlement { .. } => {
                panic!("metadata/shelf fixture does not admit Entitlement")
            }
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
    wait: Notify,
    drop_fence: (Mutex<bool>, Condvar),
}
impl Gate {
    fn open(&self) {
        *self.drop_fence.0.lock().unwrap() = true;
        self.drop_fence.1.notify_all();
    }
}
struct OpenOnDrop(Arc<Gate>);
impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.open();
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
        Self::new(vec![Step::read(&detail("Native01"))])
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
    fn refuse(&self, message: &'static str) -> Error {
        self.violation.lock().unwrap().get_or_insert(message);
        Error::InvalidRequest
    }
    fn assert_complete(&self) {
        assert_eq!(
            *self.violation.lock().unwrap(),
            None,
            "synthetic transport must validate every request"
        );
        assert!(
            self.steps.lock().unwrap().is_empty(),
            "exact expected read script must complete"
        );
        assert_eq!(
            self.active.load(Ordering::SeqCst),
            0,
            "no held account transport survives disposal"
        );
    }
}
impl criterion_account::Transport for Middleware {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        let target = match request {
            Request::Bootstrap => {
                self.calls.lock().unwrap().push(Observation::Bootstrap);
                return Ok(Response { status: 200, body: SecretBody::new(br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()) });
            }
            Request::Detail {
                region,
                media_id,
                authorization,
            } => {
                if authorization.header().as_bytes() != b"Bearer synthetic-bootstrap"
                    || !authorization.header().is_sensitive()
                {
                    return Err(self.refuse("anonymous bootstrap authorization changed"));
                }
                Observation::Detail { region, media_id }
            }
            Request::Subscriber {
                target,
                credentials,
            } => {
                if credentials.bootstrap().as_bytes() != b"Bearer synthetic-bootstrap"
                    || credentials.subscriber().as_bytes() != b"synthetic-same-access"
                    || !credentials.bootstrap().is_sensitive()
                    || !credentials.subscriber().is_sensitive()
                {
                    return Err(self.refuse("mandatory subscriber capabilities changed"));
                }
                Observation::Subscriber(target)
            }
        };
        self.calls.lock().unwrap().push(target.clone());
        let Some(step) = self.steps.lock().unwrap().pop_front() else {
            return Err(self.refuse("unexpected additional native read"));
        };
        if step.target != target {
            return Err(self.refuse("unexpected native method/region/identity/filter/cursor"));
        }
        if self
            .active
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(self.refuse("replacement contacted middleware before predecessor joined"));
        }
        self.trace
            .lock()
            .unwrap()
            .push(Trace::Started(target.clone()));
        let _flight = Flight {
            middleware: self.clone(),
            target,
            gate: step.gate.clone(),
        };
        if let Some(gate) = &step.gate {
            gate.entered.store(true, Ordering::SeqCst);
            gate.wait.notified().await;
        }
        Ok(Response {
            status: 200,
            body: SecretBody::new(step.body?),
        })
    }
}
fn detail(id: &str) -> ReadRequest {
    ReadRequest::NativeDetail {
        media_id: MediaId::new(id).unwrap(),
    }
}
type Owner = Accounts<Middleware, Issuer, Clock>;
struct Fixture {
    owner: Owner,
    runtime: Runtime,
    account: Arc<AccountClient<Middleware>>,
    session: Arc<Session<Issuer, Clock>>,
    middleware: Middleware,
    issuer: Issuer,
    clock: Clock,
    gates: Vec<Arc<Gate>>,
}
impl Fixture {
    fn new(steps: Vec<Step>, signed_in: bool, held: bool) -> Self {
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
        let issuer = Issuer {
            grant: signed_in,
            ..Issuer::default()
        };
        let clock = Clock::default();
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            issuer.clone(),
            clock.clone(),
        ));
        if signed_in {
            runtime.block_on(session.start_link()).unwrap();
            clock.now.store(5, Ordering::SeqCst);
            runtime.block_on(session.poll_once()).unwrap();
        }
        clock.reads.store(0, Ordering::SeqCst);
        let gates = steps.iter().filter_map(|step| step.gate.clone()).collect();
        let middleware = Middleware::new(steps);
        let account = Arc::new(AccountClient::with_transport(middleware.clone()));
        Self {
            owner: Accounts::from_parts(account.clone(), session.clone()),
            runtime,
            account,
            session,
            middleware,
            issuer,
            clock,
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
    fn wait(&self, predicate: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !predicate() {
            self.pump();
            assert!(
                std::time::Instant::now() < deadline,
                "bounded synthetic worker phase must settle"
            );
            std::thread::yield_now();
        }
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
                "bounded synthetic read must settle"
            );
            std::thread::yield_now();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
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
fn signed_out_native_detail_uses_bootstrap_only_without_session_access() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let clock = Clock::default();
    let issuer = Issuer::default();
    let session = Arc::new(Session::with_transport(
        Configuration::production(),
        issuer.clone(),
        clock.clone(),
    ));
    assert_eq!(session.status(), Status::SignedOut);
    clock.reads.store(0, Ordering::SeqCst);
    let middleware = Middleware::default();
    let account = Arc::new(AccountClient::with_transport(middleware.clone()));
    let mut owner = Accounts::from_parts(account.clone(), session.clone());
    let request = ReadRequest::NativeDetail {
        media_id: MediaId::new("Native01").unwrap(),
    };
    assert!(!request.requires_subscriber());
    let generation = owner.request(runtime.handle(), 0, request.clone()).unwrap();
    assert!(
        middleware.calls.lock().unwrap().is_empty(),
        "fresh owner waits for explicit foreground"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let loaded = loop {
        if let Some(result) = owner.poll(&runtime, true, 0) {
            break result.unwrap();
        }
        runtime.block_on(async { tokio::task::yield_now().await });
        assert!(
            std::time::Instant::now() < deadline,
            "synthetic anonymous read settles"
        );
    };
    assert_eq!(loaded.generation(), generation);
    assert_eq!(loaded.session_generation(), 0);
    assert!(loaded.matches_request(&request));
    assert!(!loaded.matches_request(&ReadRequest::NativeDetail {
        media_id: MediaId::new("Native02").unwrap()
    }));
    assert!(!loaded.matches_request(&ReadRequest::ContinueWatching));
    assert!(!loaded.matches_request(&ReadRequest::WatchList(WatchListRequest::default())));
    assert!(
        !format!("{loaded:?}").contains("Synthetic") && !format!("{loaded:?}").contains("Native01")
    );
    let Loaded::NativeDetail(detail) = loaded.data else {
        panic!("exact native payload")
    };
    assert_eq!(detail.media.id.as_str(), "Native01");
    assert_eq!(detail.media.title, "Synthetic native film");
    assert_eq!(detail.media.duration, Some(90.5));
    assert_eq!(
        detail.metadata.description.as_deref(),
        Some("Synthetic native description")
    );
    owner.dispose(&runtime);
    assert_eq!(account.region(), Err(Error::Disposed));
    assert_eq!(
        clock.reads.load(Ordering::SeqCst),
        0,
        "anonymous request/poll/issue/dispose never touches Session"
    );
    assert!(issuer.requests.lock().unwrap().is_empty());
    assert_eq!(*middleware.violation.lock().unwrap(), None);
    assert_eq!(
        *middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Detail {
                region: Region::Ca,
                media_id: MediaId::new("Native01").unwrap()
            }
        ]
    );
    assert_eq!(session.status(), Status::SignedOut);
}

#[test]
fn detail_watch_list_and_continue_watching_share_one_bootstrap_and_exact_payloads() {
    let requests = [
        detail("Native01"),
        ReadRequest::ContinueWatching,
        ReadRequest::WatchList(WatchListRequest {
            filter: criterion_account::WatchListFilter::OriginalFranchise,
            cursor: Some(criterion_provider::PageCursor::new("synthetic-observed/+% é").unwrap()),
        }),
        detail("Native02"),
    ];
    let mut fixture = Fixture::new(requests.iter().map(Step::read).collect(), true, false);
    let mut previous = 0;
    for request in &requests {
        let generation = fixture
            .owner
            .request(fixture.runtime.handle(), 7, request.clone())
            .unwrap();
        let loaded = fixture.result(7).unwrap();
        assert!(generation > previous);
        previous = generation;
        assert_eq!(
            (loaded.generation(), loaded.session_generation()),
            (generation, 7)
        );
        assert!(loaded.matches_request(request));
        assert_eq!(
            loaded.matches_request(&detail("Native01")),
            *request == detail("Native01")
        );
        match &loaded.data {
            Loaded::NativeDetail(data) => {
                let ReadRequest::NativeDetail { media_id } = request else {
                    panic!("exact typed Detail request")
                };
                assert_eq!(&data.media.id, media_id);
                assert_eq!(data.media.duration, Some(90.5));
            }
            Loaded::ContinueWatching(data) => {
                assert_eq!(data.playlist[0].id.as_str(), "Contin01");
                assert_eq!((data.positions[0].pos, data.positions[0].dur), (-1, 7200));
            }
            Loaded::WatchList {
                request: actual,
                page,
            } => {
                let ReadRequest::WatchList(expected) = request else {
                    panic!("exact typed WatchList request")
                };
                assert_eq!(actual, expected);
                assert_eq!(page.playlist[0].id.as_str(), "Watch001");
            }
            Loaded::Playback { .. } => panic!("fixture does not admit Playback"),
            Loaded::Entitlement { .. } => {
                panic!("metadata/shelf fixture does not admit Entitlement")
            }
        }
    }
    let calls = fixture.middleware.calls.lock().unwrap();
    assert_eq!(
        *calls,
        [
            Observation::Bootstrap,
            Observation::Detail {
                region: Region::Ca,
                media_id: MediaId::new("Native01").unwrap()
            },
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
            Observation::Subscriber(SubscriberTarget::WatchList {
                region: Region::Ca,
                request: WatchListRequest {
                    filter: criterion_account::WatchListFilter::OriginalFranchise,
                    cursor: Some(
                        criterion_provider::PageCursor::new("synthetic-observed/+% é").unwrap()
                    ),
                }
            }),
            Observation::Detail {
                region: Region::Ca,
                media_id: MediaId::new("Native02").unwrap()
            },
        ]
    );
}

#[test]
fn cross_kind_replacement_joins_detail_and_private_reads_before_latest_contact() {
    for (first, intermediate, last) in [
        (
            detail("Native01"),
            detail("Native03"),
            ReadRequest::ContinueWatching,
        ),
        (
            ReadRequest::ContinueWatching,
            ReadRequest::WatchList(WatchListRequest::default()),
            detail("Native02"),
        ),
        (
            ReadRequest::WatchList(WatchListRequest::default()),
            ReadRequest::ContinueWatching,
            detail("Native02"),
        ),
    ] {
        let gate = Arc::new(Gate::default());
        let mut old = Step::read(&first);
        old.gate = Some(gate.clone());
        let old_target = old.target.clone();
        let latest_target = Step::read(&last).target;
        let mut fixture = Fixture::new(vec![old, Step::read(&last)], true, true);
        fixture
            .owner
            .request(fixture.runtime.handle(), 20, first)
            .unwrap();
        assert!(fixture.owner.poll(&fixture.runtime, true, 20).is_none());
        fixture.wait(|| gate.entered.load(Ordering::SeqCst));
        fixture
            .owner
            .request(fixture.runtime.handle(), 20, intermediate)
            .unwrap();
        let generation = fixture
            .owner
            .request(fixture.runtime.handle(), 20, last.clone())
            .unwrap();
        fixture.wait(|| gate.retiring.load(Ordering::SeqCst));
        for _ in 0..4 {
            assert!(fixture.owner.poll(&fixture.runtime, true, 20).is_none());
            fixture.pump();
            assert_eq!(
                *fixture.middleware.calls.lock().unwrap(),
                [Observation::Bootstrap, old_target.clone()]
            );
        }
        assert_eq!(gate.retired.load(Ordering::SeqCst), 0);
        gate.open();
        let loaded = fixture.result(20).unwrap();
        assert_eq!(
            (loaded.generation(), loaded.session_generation()),
            (generation, 20)
        );
        assert!(loaded.matches_request(&last));
        assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                old_target.clone(),
                latest_target.clone()
            ]
        );
        assert_eq!(
            *fixture.middleware.trace.lock().unwrap(),
            [
                Trace::Started(old_target.clone()),
                Trace::Retired(old_target),
                Trace::Started(latest_target.clone()),
                Trace::Retired(latest_target),
            ]
        );
        assert!(
            fixture.owner.poll(&fixture.runtime, true, 20).is_none(),
            "only the latest typed payload can publish"
        );
    }
}

#[test]
fn background_discards_held_detail_and_queued_private_successor_without_contact() {
    let gate = Arc::new(Gate::default());
    let first = detail("Native01");
    let last = detail("Native02");
    let mut old = Step::read(&first);
    old.gate = Some(gate.clone());
    let old_target = old.target.clone();
    let latest_target = Step::read(&last).target;
    let mut fixture = Fixture::new(vec![old, Step::read(&last)], true, true);
    fixture
        .owner
        .request(fixture.runtime.handle(), 5, first)
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 5).is_none());
    fixture.wait(|| gate.entered.load(Ordering::SeqCst));
    fixture
        .owner
        .request(fixture.runtime.handle(), 5, ReadRequest::ContinueWatching)
        .unwrap();
    fixture.owner.background();
    fixture.wait(|| gate.retiring.load(Ordering::SeqCst));
    gate.open();
    fixture.wait(|| gate.retired.load(Ordering::SeqCst) == 1);
    for _ in 0..4 {
        assert!(fixture.owner.poll(&fixture.runtime, false, 5).is_none());
        fixture.pump();
    }
    fixture.clock.reads.store(0, Ordering::SeqCst);
    let generation = fixture
        .owner
        .request(fixture.runtime.handle(), 5, last.clone())
        .unwrap();
    for _ in 0..4 {
        fixture.pump();
        assert!(fixture.owner.poll(&fixture.runtime, false, 5).is_none());
    }
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [Observation::Bootstrap, old_target.clone()],
        "inactive latest intent never contacts middleware"
    );
    let loaded = fixture.result(5).unwrap();
    assert!(loaded.matches_request(&last));
    assert_eq!(loaded.generation(), generation);
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [Observation::Bootstrap, old_target, latest_target]
    );
    assert_eq!(
        fixture.clock.reads.load(Ordering::SeqCst),
        0,
        "anonymous resumption never accesses the signed-in Session"
    );
    assert_eq!(
        *fixture.issuer.requests.lock().unwrap(),
        [Endpoint::DeviceCode, Endpoint::Token]
    );
}

#[test]
fn logout_same_token_relink_and_root_epoch_retire_finished_detail_and_private_intent() {
    let mut fixture = Fixture::new(
        vec![
            Step::read(&detail("Native01")),
            Step::read(&ReadRequest::ContinueWatching),
            Step::read(&detail("Native02")),
        ],
        true,
        false,
    );
    fixture
        .owner
        .request(fixture.runtime.handle(), 7, detail("Native01"))
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 7).is_none());
    fixture.pump();
    assert_eq!(
        fixture.middleware.trace.lock().unwrap().len(),
        2,
        "the current-thread public task must finish before epoch retirement"
    );
    fixture.runtime.block_on(fixture.session.logout()).unwrap();
    assert!(
        fixture.owner.poll(&fixture.runtime, true, 8).is_none(),
        "departed completed Detail cannot publish"
    );
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 8, ReadRequest::ContinueWatching),
        Err(Error::Session(criterion_session::Error::NoSession))
    );
    fixture
        .runtime
        .block_on(fixture.session.start_link())
        .unwrap();
    fixture.clock.now.store(10, Ordering::SeqCst);
    fixture
        .runtime
        .block_on(fixture.session.poll_once())
        .unwrap();
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 7, detail("Native03")),
        Err(Error::Stale),
        "identical new token cannot authorize an old root epoch"
    );
    let private_generation = fixture
        .owner
        .request(fixture.runtime.handle(), 9, ReadRequest::ContinueWatching)
        .unwrap();
    let private = fixture.result(9).unwrap();
    assert_eq!(
        (private.generation(), private.session_generation()),
        (private_generation, 9)
    );
    assert!(private.matches_request(&ReadRequest::ContinueWatching));
    fixture.clock.reads.store(0, Ordering::SeqCst);
    let public_generation = fixture
        .owner
        .request(fixture.runtime.handle(), 9, detail("Native02"))
        .unwrap();
    let public = fixture.result(9).unwrap();
    assert_eq!(
        (public.generation(), public.session_generation()),
        (public_generation, 9)
    );
    assert!(public.matches_request(&detail("Native02")));
    assert_eq!(fixture.clock.reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Detail {
                region: Region::Ca,
                media_id: MediaId::new("Native01").unwrap()
            },
            Observation::Subscriber(SubscriberTarget::ContinueWatching(Region::Ca)),
            Observation::Detail {
                region: Region::Ca,
                media_id: MediaId::new("Native02").unwrap()
            },
        ]
    );
    assert_eq!(
        *fixture.issuer.requests.lock().unwrap(),
        [
            Endpoint::DeviceCode,
            Endpoint::Token,
            Endpoint::Revoke,
            Endpoint::DeviceCode,
            Endpoint::Token
        ]
    );
}

#[test]
fn expired_private_preflight_refuses_but_anonymous_detail_does_not_refresh_session() {
    let mut fixture = Fixture::new(vec![Step::read(&detail("Native01"))], true, false);
    fixture.clock.now.store(15, Ordering::SeqCst);
    for request in [
        ReadRequest::ContinueWatching,
        ReadRequest::WatchList(WatchListRequest::default()),
    ] {
        assert!(request.requires_subscriber());
        assert_eq!(
            fixture.owner.request(fixture.runtime.handle(), 3, request),
            Err(Error::Session(criterion_session::Error::Expired))
        );
    }
    assert!(
        fixture.middleware.calls.lock().unwrap().is_empty(),
        "private refusal precedes bootstrap/middleware contact"
    );
    fixture.clock.reads.store(0, Ordering::SeqCst);
    fixture
        .owner
        .request(fixture.runtime.handle(), 3, detail("Native01"))
        .unwrap();
    let public = fixture.result(3).unwrap();
    assert!(public.matches_request(&detail("Native01")));
    assert_eq!(fixture.clock.reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        *fixture.issuer.requests.lock().unwrap(),
        [Endpoint::DeviceCode, Endpoint::Token],
        "anonymous detail never starts or borrows a refresh"
    );
}

#[test]
fn detail_error_or_mismatched_wire_identity_never_publishes_or_retries_implicitly() {
    for (body, expected) in [
        (Err(Error::Deadline), Error::Deadline),
        (
            Ok(
                br#"{"contentType":"film","mediaid":"Native02","title":"Synthetic wrong root"}"#
                    .to_vec(),
            ),
            Error::InvalidResponse,
        ),
    ] {
        let mut failed = Step::read(&detail("Native01"));
        failed.body = body;
        let mut fixture = Fixture::new(vec![failed, Step::read(&detail("Native03"))], false, false);
        fixture
            .owner
            .request(fixture.runtime.handle(), 0, detail("Native01"))
            .unwrap();
        assert!(matches!(fixture.result(0), Err(error) if error == expected));
        for _ in 0..4 {
            fixture.pump();
            assert!(fixture.owner.poll(&fixture.runtime, true, 0).is_none());
        }
        assert_eq!(
            *fixture.middleware.calls.lock().unwrap(),
            [
                Observation::Bootstrap,
                Observation::Detail {
                    region: Region::Ca,
                    media_id: MediaId::new("Native01").unwrap()
                }
            ]
        );
        fixture
            .owner
            .request(fixture.runtime.handle(), 0, detail("Native03"))
            .unwrap();
        assert!(
            fixture
                .result(0)
                .unwrap()
                .matches_request(&detail("Native03")),
            "only explicit new intent makes another read"
        );
        assert_eq!(fixture.clock.reads.load(Ordering::SeqCst), 0);
        assert!(fixture.issuer.requests.lock().unwrap().is_empty());
        assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 3);
    }
}

#[test]
fn dispose_joins_held_detail_before_wiping_client_and_discards_private_successor() {
    let gate = Arc::new(Gate::default());
    let mut old = Step::read(&detail("Native01"));
    old.gate = Some(gate.clone());
    let mut fixture = Fixture::new(vec![old], true, true);
    fixture
        .owner
        .request(fixture.runtime.handle(), 6, detail("Native01"))
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 6).is_none());
    fixture.wait(|| gate.entered.load(Ordering::SeqCst));
    fixture
        .owner
        .request(fixture.runtime.handle(), 6, ReadRequest::ContinueWatching)
        .unwrap();
    let done = AtomicBool::new(false);
    let account = fixture.account.clone();
    let owner = &mut fixture.owner;
    let runtime = &fixture.runtime;
    std::thread::scope(|scope| {
        let task = scope.spawn(|| {
            owner.dispose(runtime);
            done.store(true, Ordering::SeqCst);
        });
        // Release the blocking destructor before scope joins even on assertion unwind.
        let _release = OpenOnDrop(gate.clone());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !gate.retiring.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "disposal must retire held transport"
            );
            std::thread::yield_now();
        }
        let waiting_for_join = !done.load(Ordering::SeqCst);
        let region_before_join = account.region();
        let retired_before_join = gate.retired.load(Ordering::SeqCst);
        gate.open();
        task.join().unwrap();
        assert!(
            waiting_for_join,
            "explicit disposal cannot finish before transport destructor joins"
        );
        assert_eq!(
            region_before_join,
            Ok(Region::Ca),
            "client wipe follows worker settlement"
        );
        assert_eq!(retired_before_join, 0);
    });
    assert!(done.load(Ordering::SeqCst));
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.account.region(), Err(Error::Disposed));
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 7, detail("Native02")),
        Err(Error::Disposed)
    );
    assert!(fixture.owner.poll(&fixture.runtime, true, 7).is_none());
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            Observation::Bootstrap,
            Observation::Detail {
                region: Region::Ca,
                media_id: MediaId::new("Native01").unwrap()
            }
        ]
    );
    assert_eq!(
        *fixture.issuer.requests.lock().unwrap(),
        [Endpoint::DeviceCode, Endpoint::Token],
        "account disposal owns no issuer revoke"
    );
    assert_eq!(
        fixture.session.status(),
        Status::SignedIn {
            expires_at: Duration::from_secs(15)
        }
    );
}
