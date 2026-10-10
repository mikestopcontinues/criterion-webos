// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic IDs reads through the same joined Accounts owner as native Detail.
use super::*;
use criterion_account::{Region, Request, Response, SecretBody, SubscriberTarget};
use criterion_provider::MediaId;
use criterion_session::{Configuration, Endpoint};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

const INIT: &[u8] = br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
const IDS: &[u8] = br#"{"watchlist":["Series01","Other001"],"positions":[{"media_id":"Episode1","pos":90,"dur":100,"series_id":"Else0001"}]}"#;

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
#[derive(Clone)]
struct Middleware {
    calls: Arc<Mutex<Vec<SubscriberTarget>>>,
    bootstraps: Arc<AtomicUsize>,
    result: Result<(u16, Vec<u8>), Error>,
}
impl criterion_account::Transport for Middleware {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        match request {
            Request::Bootstrap => {
                self.bootstraps.fetch_add(1, Ordering::SeqCst);
                Ok(Response {
                    status: 200,
                    body: SecretBody::new(INIT.to_vec()),
                })
            }
            Request::Subscriber {
                target,
                credentials,
            } => {
                assert_eq!(target, SubscriberTarget::MyListIds(Region::Ca));
                assert_eq!(
                    credentials.bootstrap().as_bytes(),
                    b"Bearer synthetic-bootstrap"
                );
                assert_eq!(credentials.subscriber().as_bytes(), b"synthetic-same-token");
                assert!(
                    credentials.bootstrap().is_sensitive()
                        && credentials.subscriber().is_sensitive()
                );
                self.calls.lock().unwrap().push(target);
                let (status, body) = self.result.clone()?;
                Ok(Response {
                    status,
                    body: SecretBody::new(body),
                })
            }
            Request::Detail { .. } => panic!("IDs fixture does not admit Detail"),
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
    fn new(signed_in: bool, result: Result<(u16, Vec<u8>), Error>) -> Self {
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
        if signed_in {
            runtime.block_on(session.start_link()).unwrap();
            clock.0.store(5, Ordering::SeqCst);
            runtime.block_on(session.poll_once()).unwrap();
        }
        let middleware = Middleware {
            calls: Arc::default(),
            bootstraps: Arc::default(),
            result,
        };
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
    fn result(&mut self, epoch: u64) -> Result<LoadedAccount, Error> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(result) = self.owner.poll(&self.runtime, true, epoch) {
                return result;
            }
            self.runtime.block_on(tokio::task::yield_now());
            assert!(Instant::now() < deadline, "IDs publication deadline");
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.owner.dispose(&self.runtime);
    }
}

#[test]
fn ids_worker_returns_the_bounded_typed_payload_with_epoch_and_sensitive_headers() {
    let mut fixture = Fixture::new(true, Ok((200, IDS.to_vec())));
    let generation = fixture
        .owner
        .request(fixture.runtime.handle(), 7, ReadRequest::MyListIds)
        .unwrap();
    assert!(fixture.middleware.calls.lock().unwrap().is_empty());
    let loaded = fixture.result(7).unwrap();
    assert_eq!(
        (loaded.generation(), loaded.session_generation()),
        (generation, 7)
    );
    assert!(loaded.matches_request(&ReadRequest::MyListIds));
    assert!(!loaded.matches_request(&ReadRequest::ContinueWatching));
    for private in ["Series01", "Episode1", "Else0001", "synthetic-same-token"] {
        assert!(!format!("{loaded:?}").contains(private));
    }
    let Loaded::MyListIds(ids) = loaded.data else {
        panic!("typed native IDs payload")
    };
    assert_eq!(ids.watchlist[0], MediaId::new("Series01").unwrap());
    assert_eq!(ids.watchlist[1], MediaId::new("Other001").unwrap());
    assert_eq!(ids.positions[0].media_id, MediaId::new("Episode1").unwrap());
    assert_eq!(ids.positions[0].pos, 90);
    assert_eq!(
        *fixture.middleware.calls.lock().unwrap(),
        [SubscriberTarget::MyListIds(Region::Ca)]
    );
    assert_eq!(fixture.middleware.bootstraps.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.issuer.0.lock().unwrap(),
        [Endpoint::DeviceCode, Endpoint::Token]
    );
}

#[test]
fn ids_worker_refuses_unsigned_or_expired_session_before_any_middleware_contact() {
    for signed_in in [false, true] {
        let mut fixture = Fixture::new(signed_in, Ok((200, IDS.to_vec())));
        if signed_in {
            fixture.clock.0.store(15, Ordering::SeqCst);
        }
        let expected = if signed_in {
            criterion_session::Error::Expired
        } else {
            criterion_session::Error::NoSession
        };
        assert_eq!(
            fixture.session.with_access_token(|_| ()).unwrap_err(),
            expected
        );
        assert_eq!(
            fixture
                .owner
                .request(fixture.runtime.handle(), 7, ReadRequest::MyListIds)
                .unwrap_err(),
            Error::Session(expected)
        );
        assert!(fixture.owner.poll(&fixture.runtime, true, 7).is_none());
        fixture.owner.dispose(&fixture.runtime);
        assert_eq!(fixture.middleware.bootstraps.load(Ordering::SeqCst), 0);
        assert!(fixture.middleware.calls.lock().unwrap().is_empty());
        assert_eq!(
            *fixture.issuer.0.lock().unwrap(),
            if signed_in {
                vec![Endpoint::DeviceCode, Endpoint::Token]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn ids_worker_keeps_malformed_oversized_or_failed_reads_distinct_from_an_empty_list() {
    let too_many = serde_json::to_vec(&serde_json::json!({
        "watchlist": vec!["Other001"; 513], "positions": []
    }))
    .unwrap();
    for (response, expected) in [
        (
            Ok((200, br#"{"watchlist":[],"positions":[]}"#.to_vec())),
            None,
        ),
        (
            Ok((200, br#"{"watchlist":[]}"#.to_vec())),
            Some(Error::InvalidResponse),
        ),
        (
            Ok((200, br#"{"watchlist":null,"positions":[]}"#.to_vec())),
            Some(Error::InvalidResponse),
        ),
        (Ok((200, too_many)), Some(Error::InvalidResponse)),
        (Ok((200, vec![b' '; 65_537])), Some(Error::ResponseTooLarge)),
        (Ok((403, IDS.to_vec())), Some(Error::HttpStatus(403))),
        (Err(Error::Deadline), Some(Error::Deadline)),
    ] {
        let mut fixture = Fixture::new(true, response);
        fixture
            .owner
            .request(fixture.runtime.handle(), 7, ReadRequest::MyListIds)
            .unwrap();
        match expected {
            Some(error) => assert_eq!(fixture.result(7).unwrap_err(), error),
            None => {
                let loaded = fixture.result(7).unwrap();
                let Loaded::MyListIds(ids) = loaded.data else {
                    panic!("typed empty IDs payload")
                };
                assert!(ids.watchlist.is_empty() && ids.positions.is_empty());
            }
        }
        fixture.owner.dispose(&fixture.runtime);
        assert_eq!(fixture.middleware.calls.lock().unwrap().len(), 1);
        assert_eq!(fixture.middleware.bootstraps.load(Ordering::SeqCst), 1);
        assert_eq!(
            *fixture.issuer.0.lock().unwrap(),
            [Endpoint::DeviceCode, Endpoint::Token]
        );
    }
}
