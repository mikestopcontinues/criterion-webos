// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic playback reads through the actual joined Accounts owner.
use super::*;
use criterion_account::{DrmPolicy, Region, Request, Response, SecretBody, SubscriberTarget};
use criterion_provider::MediaId;
use criterion_session::{Configuration, Endpoint};
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

const INIT: &[u8] = br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
const PLAYBACK: &[u8] = br#"{"playlist":[{"contentType":"episode","mediaid":"Resp0001","title":"Synthetic","sources":[{"type":"application/dash+xml","file":"private-dash","drm":{"widevine":{"url":"private-license"}}}]}]}"#;
fn playback(id: &str, policy: DrmPolicy) -> ReadRequest {
    ReadRequest::Playback(NativePlaybackRequest {
        media_id: MediaId::new(id).unwrap(),
        drm_policy: policy,
    })
}
fn target(id: &str, policy: DrmPolicy) -> SubscriberTarget {
    SubscriberTarget::Playback {
        region: Region::Ca,
        request: NativePlaybackRequest {
            media_id: MediaId::new(id).unwrap(),
            drm_policy: policy,
        },
    }
}
#[derive(Clone, Default)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Clone, Default)]
struct Issuer(Arc<Mutex<Vec<Endpoint>>>);
impl criterion_session::Transport for Issuer {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        self.0.lock().unwrap().push(request.endpoint);
        let body = match request.endpoint {
            Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
            Endpoint::Token => br#"{"access_token":"synthetic-same-token","refresh_token":"synthetic-refresh","expires_in":10}"#.to_vec(),
            Endpoint::Revoke => b"{}".to_vec(),
        };
        Ok(criterion_session::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    retiring: AtomicBool,
    retired: AtomicUsize,
    release: Notify,
    fence: (Mutex<bool>, Condvar),
}
impl Gate {
    fn open(&self) {
        *self.fence.0.lock().unwrap() = true;
        self.fence.1.notify_all();
        self.release.notify_one();
    }
}
#[derive(Clone)]
struct Middleware {
    calls: Arc<Mutex<Vec<SubscriberTarget>>>,
    bootstrap: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    gate: Option<Arc<Gate>>,
    result: Result<(u16, Vec<u8>), Error>,
}
struct Flight {
    owner: Middleware,
    gate: Option<Arc<Gate>>,
}
impl Drop for Flight {
    fn drop(&mut self) {
        if let Some(gate) = &self.gate {
            gate.retiring.store(true, Ordering::SeqCst);
            drop(
                gate.fence
                    .1
                    .wait_while(gate.fence.0.lock().unwrap(), |open| !*open)
                    .unwrap(),
            );
            gate.retired.fetch_add(1, Ordering::SeqCst);
        }
        self.owner.active.fetch_sub(1, Ordering::SeqCst);
    }
}
impl criterion_account::Transport for Middleware {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        match request {
            Request::Bootstrap => {
                self.bootstrap.fetch_add(1, Ordering::SeqCst);
                Ok(Response {
                    status: 200,
                    body: SecretBody::new(INIT.to_vec()),
                })
            }
            Request::Subscriber {
                target: request_target,
                credentials,
            } => {
                let SubscriberTarget::Playback {
                    region: Region::Ca,
                    request,
                } = &request_target
                else {
                    panic!("script admits only native CA Playback")
                };
                assert!(matches!(request.media_id.as_str(), "Chosen01" | "Chosen02"));
                assert_eq!(
                    credentials.bootstrap().as_bytes(),
                    b"Bearer synthetic-bootstrap"
                );
                assert_eq!(credentials.subscriber().as_bytes(), b"synthetic-same-token");
                assert!(
                    credentials.bootstrap().is_sensitive()
                        && credentials.subscriber().is_sensitive()
                );
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.maximum.fetch_max(active, Ordering::SeqCst);
                assert_eq!(active, 1, "predecessor must retire before successor sends");
                self.calls.lock().unwrap().push(request_target.clone());
                let gate = (request.media_id.as_str() == "Chosen01")
                    .then(|| self.gate.clone())
                    .flatten();
                let _flight = Flight {
                    owner: self.clone(),
                    gate: gate.clone(),
                };
                if let Some(gate) = &gate {
                    gate.entered.store(true, Ordering::SeqCst);
                    gate.release.notified().await;
                }
                let (status, body) = self.result.clone()?;
                Ok(Response {
                    status,
                    body: SecretBody::new(body),
                })
            }
            Request::Detail { .. } => panic!("playback fixture does not admit Detail"),
        }
    }
}
struct Fixture {
    owner: Accounts<Middleware, Issuer, Clock>,
    runtime: Runtime,
    session: Arc<Session<Issuer, Clock>>,
    clock: Clock,
    issuer: Issuer,
    middleware: Middleware,
}
impl Fixture {
    fn new(
        signed_in: bool,
        gate: Option<Arc<Gate>>,
        result: Result<(u16, Vec<u8>), Error>,
    ) -> Self {
        let runtime = if gate.is_some() {
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
        let middleware = Middleware {
            calls: Arc::default(),
            bootstrap: Arc::default(),
            active: Arc::default(),
            maximum: Arc::default(),
            gate,
            result,
        };
        let issuer = Issuer::default();
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
        let owner = Accounts::from_parts(
            Arc::new(AccountClient::with_transport(middleware.clone())),
            session.clone(),
        );
        Self {
            owner,
            runtime,
            session,
            clock,
            issuer,
            middleware,
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
        let deadline = Instant::now() + Duration::from_secs(2);
        while !predicate() {
            self.pump();
            assert!(Instant::now() < deadline, "worker phase deadline");
            std::thread::yield_now();
        }
    }
    fn result(&mut self, epoch: u64) -> Result<LoadedAccount, Error> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            self.pump();
            if let Some(result) = self.owner.poll(&self.runtime, true, epoch) {
                return result;
            }
            assert!(Instant::now() < deadline, "read publication deadline");
            std::thread::yield_now();
        }
    }
    fn quiet(&mut self, active: bool, epoch: u64) {
        for _ in 0..16 {
            self.pump();
            assert!(self.owner.poll(&self.runtime, active, epoch).is_none());
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(gate) = &self.middleware.gate {
            gate.open();
        }
        self.owner.dispose(&self.runtime);
        if !std::thread::panicking() {
            assert_eq!(self.middleware.active.load(Ordering::SeqCst), 0);
            assert!(self.middleware.maximum.load(Ordering::SeqCst) <= 1);
        }
    }
}

#[test]
fn native_playback_worker_keeps_requested_stream_separate_from_returned_media() {
    let mut fixture = Fixture::new(true, None, Ok((200, PLAYBACK.to_vec())));
    let request = playback("Chosen01", DrmPolicy::High);
    let generation = fixture
        .owner
        .request(fixture.runtime.handle(), 7, request.clone())
        .unwrap();
    assert_eq!(fixture.middleware.bootstrap.load(Ordering::SeqCst), 0);
    assert!(fixture.middleware.calls.lock().unwrap().is_empty());
    let loaded = fixture.result(7).unwrap();
    assert_eq!(
        (loaded.generation(), loaded.session_generation()),
        (generation, 7)
    );
    assert!(loaded.matches_request(&request));
    assert!(!loaded.matches_request(&playback("Resp0001", DrmPolicy::High)));
    assert!(!loaded.matches_request(&playback("Chosen01", DrmPolicy::Medium)));
    for diagnostic in [format!("{request:?}"), format!("{loaded:?}")] {
        for private in [
            "Chosen01",
            "Resp0001",
            "private-dash",
            "private-license",
            "synthetic-same-token",
        ] {
            assert!(!diagnostic.contains(private));
        }
    }
    let Loaded::Playback {
        request,
        selection: NativePlaybackSelection::Selected(selected),
    } = loaded.data
    else {
        panic!("selected playback")
    };
    assert_eq!(request.media_id.as_str(), "Chosen01");
    assert_eq!(selected.media.id.as_str(), "Resp0001");
    assert_eq!(selected.dash_file(), "private-dash");
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [target("Chosen01", DrmPolicy::High)]
    );
    assert_eq!(fixture.middleware.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.issuer.0.lock().unwrap(),
        [Endpoint::DeviceCode, Endpoint::Token]
    );
}

#[test]
fn native_playback_worker_refuses_unsigned_and_defers_only_latest_background_intent() {
    let request = playback("Chosen01", DrmPolicy::High);
    let mut unsigned = Fixture::new(false, None, Ok((200, PLAYBACK.to_vec())));
    assert_eq!(
        unsigned
            .owner
            .request(unsigned.runtime.handle(), 7, request.clone())
            .err(),
        Some(Error::Session(criterion_session::Error::NoSession))
    );
    unsigned.quiet(true, 7);
    assert_eq!(unsigned.middleware.bootstrap.load(Ordering::SeqCst), 0);
    assert!(unsigned.middleware.calls.lock().unwrap().is_empty());
    assert!(unsigned.issuer.0.lock().unwrap().is_empty());

    let mut fixture = Fixture::new(true, None, Ok((200, PLAYBACK.to_vec())));
    fixture.owner.background();
    fixture
        .owner
        .request(fixture.runtime.handle(), 7, request)
        .unwrap();
    let newest = playback("Chosen02", DrmPolicy::Medium);
    fixture
        .owner
        .request(fixture.runtime.handle(), 7, newest.clone())
        .unwrap();
    fixture.quiet(false, 7);
    assert_eq!(fixture.middleware.bootstrap.load(Ordering::SeqCst), 0);
    assert!(fixture.middleware.calls.lock().unwrap().is_empty());
    let loaded = fixture.result(7).unwrap();
    assert!(loaded.matches_request(&newest));
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [target("Chosen02", DrmPolicy::Medium)]
    );
    assert_eq!(
        fixture
            .owner
            .request(fixture.runtime.handle(), 6, newest)
            .err(),
        Some(Error::Stale)
    );
    fixture.quiet(true, 7);
    assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 1);
}

#[test]
fn native_playback_worker_waits_for_transport_retirement_before_latest_successor() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(true, Some(gate.clone()), Ok((200, PLAYBACK.to_vec())));
    let first = playback("Chosen01", DrmPolicy::High);
    fixture
        .owner
        .request(fixture.runtime.handle(), 7, first.clone())
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 7).is_none());
    fixture.wait(|| gate.entered.load(Ordering::SeqCst));
    let latest = playback("Chosen02", DrmPolicy::Low);
    fixture
        .owner
        .request(fixture.runtime.handle(), 7, latest.clone())
        .unwrap();
    fixture.wait(|| gate.retiring.load(Ordering::SeqCst));
    fixture.quiet(true, 7);
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [target("Chosen01", DrmPolicy::High)]
    );
    assert_eq!(gate.retired.load(Ordering::SeqCst), 0);
    gate.open();
    let loaded = fixture.result(7).unwrap();
    assert!(loaded.matches_request(&latest));
    assert!(!loaded.matches_request(&first));
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            target("Chosen01", DrmPolicy::High),
            target("Chosen02", DrmPolicy::Low)
        ]
    );
    assert_eq!(fixture.middleware.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.middleware.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
}

#[test]
fn native_playback_worker_same_token_relink_rejects_completed_old_epoch() {
    let mut fixture = Fixture::new(true, None, Ok((200, PLAYBACK.to_vec())));
    fixture
        .owner
        .request(
            fixture.runtime.handle(),
            7,
            playback("Chosen01", DrmPolicy::High),
        )
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 7).is_none());
    // Wait for actual owner completion but leave the result unpublished.
    fixture.wait(|| {
        fixture.middleware.calls.lock().unwrap().len() == 1
            && fixture.middleware.active.load(Ordering::SeqCst) == 0
    });
    fixture.pump();
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
    assert!(
        fixture
            .session
            .with_access_token(|token| token == "synthetic-same-token")
            .unwrap()
    );
    fixture.quiet(true, 8);
    assert_eq!(
        fixture
            .owner
            .request(
                fixture.runtime.handle(),
                7,
                playback("Chosen02", DrmPolicy::High)
            )
            .err(),
        Some(Error::Stale)
    );
    let newest = playback("Chosen02", DrmPolicy::High);
    fixture
        .owner
        .request(fixture.runtime.handle(), 8, newest.clone())
        .unwrap();
    let loaded = fixture.result(8).unwrap();
    assert_eq!(loaded.session_generation(), 8);
    assert!(loaded.matches_request(&newest));
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [
            target("Chosen01", DrmPolicy::High),
            target("Chosen02", DrmPolicy::High)
        ]
    );
    assert_eq!(
        *fixture.issuer.0.lock().unwrap(),
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
fn native_playback_worker_background_logout_and_expiry_retire_held_private_result() {
    for action in ["background", "logout", "expire"] {
        let gate = Arc::new(Gate::default());
        let mut fixture = Fixture::new(true, Some(gate.clone()), Ok((200, PLAYBACK.to_vec())));
        fixture
            .owner
            .request(
                fixture.runtime.handle(),
                7,
                playback("Chosen01", DrmPolicy::High),
            )
            .unwrap();
        assert!(fixture.owner.poll(&fixture.runtime, true, 7).is_none());
        fixture.wait(|| gate.entered.load(Ordering::SeqCst));
        match action {
            "background" => fixture.owner.background(),
            "logout" => {
                fixture.runtime.block_on(fixture.session.logout()).unwrap();
                assert!(matches!(
                    fixture.owner.poll(&fixture.runtime, true, 7),
                    Some(Err(Error::Session(criterion_session::Error::NoSession)))
                ));
            }
            "expire" => {
                fixture.clock.0.store(15, Ordering::SeqCst);
                assert!(matches!(
                    fixture.owner.poll(&fixture.runtime, true, 7),
                    Some(Err(Error::Session(criterion_session::Error::Expired)))
                ));
            }
            _ => unreachable!(),
        }
        fixture.wait(|| gate.retiring.load(Ordering::SeqCst));
        gate.open();
        fixture.wait(|| gate.retired.load(Ordering::SeqCst) == 1);
        fixture.quiet(action != "background", 7);
        assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 1);
        assert_eq!(
            fixture
                .issuer
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|endpoint| **endpoint == Endpoint::Token)
                .count(),
            1
        );
    }
}

#[test]
fn native_playback_worker_preserves_unselected_or_failed_read_without_retry_or_refresh() {
    for (response, expected) in [
        (Ok((200, br#"{"playlist":[]}"#.to_vec())), None),
        (Ok((200, br#"{"playlist":[{"contentType":"film","mediaid":"Resp0001","title":"Synthetic","sources":[]}] }"#.to_vec())), None),
        (Ok((200, br#"{"playlist":null}"#.to_vec())), Some(Error::InvalidResponse)),
        (Ok((403, PLAYBACK.to_vec())), Some(Error::HttpStatus(403))),
        (Err(Error::Deadline), Some(Error::Deadline)),
    ] {
        let mut fixture = Fixture::new(true, None, response);
        let request = playback("Chosen02", DrmPolicy::Highest);
        fixture.owner.request(fixture.runtime.handle(), 7, request.clone()).unwrap();
        let result = fixture.result(7);
        match expected {
            Some(error) => assert_eq!(result.unwrap_err(), error),
            None => {
                let loaded = result.unwrap();
                assert!(loaded.matches_request(&request));
                assert!(matches!(loaded.data, Loaded::Playback { selection: NativePlaybackSelection::EmptyPlaylist | NativePlaybackSelection::NoDash, .. }));
            }
        }
        fixture.quiet(true, 7);
        assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 1);
        assert_eq!(*fixture.issuer.0.lock().unwrap(), [Endpoint::DeviceCode, Endpoint::Token]);
    }
}

#[test]
fn native_playback_worker_disposal_waits_for_actual_transport_drop_and_refuses_new_work() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(true, Some(gate.clone()), Ok((200, PLAYBACK.to_vec())));
    fixture
        .owner
        .request(
            fixture.runtime.handle(),
            7,
            playback("Chosen01", DrmPolicy::High),
        )
        .unwrap();
    assert!(fixture.owner.poll(&fixture.runtime, true, 7).is_none());
    fixture.wait(|| gate.entered.load(Ordering::SeqCst));
    let (entered, before_open, retired_at_return) = std::thread::scope(|scope| {
        let owner = &mut fixture.owner;
        let runtime = &fixture.runtime;
        let at_return = gate.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = scope.spawn(move || {
            owner.dispose(runtime);
            tx.send(at_return.retired.load(Ordering::SeqCst)).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !gate.retiring.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let entered = gate.retiring.load(Ordering::SeqCst);
        let before_open = rx.try_recv();
        // Unblock cleanup before any assertion can unwind the scoped thread.
        gate.open();
        let retired = match before_open {
            Ok(value) => value,
            Err(_) => rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        };
        thread.join().unwrap();
        (entered, before_open, retired)
    });
    assert!(entered, "dispose must retire the issued worker");
    assert!(
        matches!(before_open, Err(std::sync::mpsc::TryRecvError::Empty)),
        "dispose must wait for the transport fence"
    );
    assert_eq!(retired_at_return, 1);
    assert_eq!(
        fixture
            .owner
            .request(
                fixture.runtime.handle(),
                7,
                playback("Chosen02", DrmPolicy::High)
            )
            .err(),
        Some(Error::Disposed)
    );
    assert!(fixture.owner.poll(&fixture.runtime, true, 7).is_none());
    assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 1);
    assert!(
        fixture.session.with_access_token(|_| ()).is_ok(),
        "read disposal does not revoke Session"
    );
}
