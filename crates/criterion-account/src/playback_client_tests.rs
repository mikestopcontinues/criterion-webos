//! Synthetic request fixtures bind the signed native request contract, not rights.
use crate::*;
use criterion_provider::MediaId;
use std::sync::{Arc, Mutex};

const INIT: &[u8] = br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
// The response identity intentionally differs from the chosen request identity.
const PLAYBACK: &[u8] = br#"{"playlist":[{"contentType":"episode","mediaid":"Resp0001","title":"Synthetic","sources":[{"type":"application/dash+xml","file":"private-dash","drm":{"widevine":{"url":"private-license"}}}]}]}"#;
fn request() -> NativePlaybackRequest {
    NativePlaybackRequest {
        media_id: MediaId::new("Chosen01").unwrap(),
        drm_policy: DrmPolicy::High,
    }
}
struct Fixture {
    requests: Arc<Mutex<Vec<SubscriberTarget>>>,
}
impl Transport for Fixture {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        let body = match request {
            Request::Bootstrap => INIT,
            Request::Subscriber {
                target,
                credentials,
            } => {
                assert_eq!(
                    credentials.bootstrap().as_bytes(),
                    b"Bearer synthetic-bootstrap"
                );
                assert_eq!(
                    credentials.subscriber().as_bytes(),
                    b"synthetic-subscriber-0"
                );
                assert!(credentials.bootstrap().is_sensitive());
                assert!(credentials.subscriber().is_sensitive());
                self.requests.lock().unwrap().push(target);
                PLAYBACK
            }
            Request::Detail { .. } => panic!("playback fixture cannot serve Detail"),
        };
        Ok(Response {
            status: 200,
            body: SecretBody::new(body.to_vec()),
        })
    }
}

#[tokio::test]
async fn playback_read_preserves_explicit_request_and_separate_returned_identity() {
    let (session, _) = crate::native_tests::linked().await;
    let requests = Arc::default();
    let client = AccountClient::with_transport(Fixture {
        requests: Arc::clone(&requests),
    });
    client.bootstrap().await.unwrap();
    let selected = client.playback(&session, request()).await.unwrap();
    let NativePlaybackSelection::Selected(playback) = selected else {
        panic!("missing selected response")
    };
    assert_eq!(playback.media.id.as_str(), "Resp0001");
    assert_eq!(playback.dash_file(), "private-dash");
    assert_eq!(
        *requests.lock().unwrap(),
        [SubscriberTarget::Playback {
            region: Region::Ca,
            request: request()
        }]
    );
    for diagnostic in [
        format!("{:?}", request()),
        format!("{:?}", requests.lock().unwrap()),
        format!("{playback:?}"),
    ] {
        for private in [
            "Chosen01",
            "Resp0001",
            "private-dash",
            "private-license",
            "synthetic-subscriber",
            "synthetic-bootstrap",
        ] {
            assert!(!diagnostic.contains(private));
        }
    }
}

struct Script {
    response: Result<(u16, Vec<u8>), Error>,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}
impl Transport for Script {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        use std::sync::atomic::Ordering;
        self.calls.fetch_add(1, Ordering::SeqCst);
        match request {
            Request::Bootstrap => Ok(Response {
                status: 200,
                body: SecretBody::new(INIT.to_vec()),
            }),
            Request::Subscriber {
                target: SubscriberTarget::Playback { .. },
                ..
            } => {
                let (status, body) = self.response.clone()?;
                Ok(Response {
                    status,
                    body: SecretBody::new(body),
                })
            }
            Request::Subscriber {
                target: SubscriberTarget::AddWatchList { .. },
                ..
            } => Err(Error::Unavailable),
            _ => panic!("unexpected playback script request"),
        }
    }
}

#[tokio::test]
async fn playback_read_requires_admitted_bootstrap_and_current_subscriber_before_contact() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (session, _) = crate::native_tests::linked().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let client = AccountClient::with_transport(Script {
        response: Ok((200, PLAYBACK.to_vec())),
        calls: calls.clone(),
    });
    assert_eq!(
        client.playback(&session, request()).await.err(),
        Some(Error::NoBootstrap)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    client.bootstrap().await.unwrap();
    session.logout().await.unwrap();
    assert_eq!(
        client.playback(&session, request()).await.err(),
        Some(Error::Session(criterion_session::Error::NoSession))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    client.dispose();
    assert_eq!(
        client.playback(&session, request()).await.err(),
        Some(Error::Disposed)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn playback_read_preserves_unselected_results_and_returns_coarse_failures_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (session, _) = crate::native_tests::linked().await;
    let cases = [
        (Ok((200, br#"{"playlist":[]}"#.to_vec())), None, "EmptyPlaylist"),
        (Ok((200, br#"{"playlist":[{"contentType":"film","mediaid":"Resp0001","title":"Synthetic","sources":[]}] }"#.to_vec())), None, "NoDash"),
        (Ok((200, br#"{"playlist":null,"private":"private-failure-body"}"#.to_vec())), Some(Error::InvalidResponse), ""),
        (Ok((200, vec![b' '; 524_289])), Some(Error::ResponseTooLarge), ""),
        (Ok((403, PLAYBACK.to_vec())), Some(Error::HttpStatus(403)), ""),
        (Err(Error::Deadline), Some(Error::Deadline), ""),
    ];
    for (response, error, projection) in cases {
        let calls = Arc::new(AtomicUsize::new(0));
        let client = AccountClient::with_transport(Script {
            response,
            calls: calls.clone(),
        });
        client.bootstrap().await.unwrap();
        let result = client.playback(&session, request()).await;
        match error {
            Some(expected) => {
                let actual = result.unwrap_err();
                assert_eq!(actual, expected);
                assert!(!format!("{actual:?}").contains("private-failure-body"));
            }
            None => assert_eq!(format!("{:?}", result.unwrap()), projection),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "no preflight, refresh or replay"
        );
    }
}

struct Held {
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}
impl Transport for Held {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match request {
            Request::Bootstrap => Ok(Response {
                status: 200,
                body: SecretBody::new(INIT.to_vec()),
            }),
            Request::Subscriber {
                target: SubscriberTarget::Playback { .. },
                ..
            } => {
                self.started.notify_one();
                self.release.notified().await;
                Ok(Response {
                    status: 200,
                    body: SecretBody::new(PLAYBACK.to_vec()),
                })
            }
            _ => panic!("unexpected held playback request"),
        }
    }
}
type HeldFixture = (
    AccountClient<Held>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
    Arc<std::sync::atomic::AtomicUsize>,
);
fn held() -> HeldFixture {
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::default();
    let client = AccountClient::with_transport(Held {
        started: started.clone(),
        release: release.clone(),
        calls: Arc::clone(&calls),
    });
    (client, started, release, calls)
}

#[tokio::test]
async fn playback_read_uses_shared_lease_and_cancel_dispose_drop_retire_ownership() {
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    let (session, _) = crate::native_tests::linked().await;
    for action in ["cancel", "dispose", "drop"] {
        let (client, started, release, calls) = held();
        client.bootstrap().await.unwrap();
        let mut pending = Box::pin(client.playback(&session, request()));
        tokio::select! {
            result = &mut pending => panic!("held result: {result:?}"),
            signal = tokio::time::timeout(Duration::from_secs(1), started.notified()) => signal.unwrap(),
        }
        assert_eq!(
            client
                .detail(&MediaId::new("Chosen01").unwrap())
                .await
                .err(),
            Some(Error::Busy)
        );
        assert_eq!(
            client.entitlement(&session, 1).await.err(),
            Some(Error::Busy)
        );
        assert_eq!(
            client.playback(&session, request()).await.err(),
            Some(Error::Busy)
        );
        assert_eq!(client.bootstrap().await.err(), Some(Error::Busy));
        match action {
            "cancel" => {
                client.cancel();
                release.notify_one();
                assert_eq!(pending.await.err(), Some(Error::Stale));
                assert_eq!(client.region(), Ok(Region::Ca));
            }
            "dispose" => {
                client.dispose();
                release.notify_one();
                assert_eq!(pending.await.err(), Some(Error::Disposed));
                assert_eq!(client.region(), Err(Error::Disposed));
            }
            "drop" => {
                drop(pending);
                assert_eq!(client.region(), Ok(Region::Ca));
                client.bootstrap().await.unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            if action == "drop" { 3 } else { 2 }
        );
    }
}

#[tokio::test]
async fn playback_read_rejects_rotated_expired_or_removed_token_after_await() {
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    for action in ["rotate", "expire", "logout"] {
        let (session, now) = crate::native_tests::linked().await;
        let (client, started, release, calls) = held();
        client.bootstrap().await.unwrap();
        let mut pending = Box::pin(client.playback(&session, request()));
        tokio::select! {
            result = &mut pending => panic!("held result: {result:?}"),
            signal = tokio::time::timeout(Duration::from_secs(1), started.notified()) => signal.unwrap(),
        }
        match action {
            "rotate" => {
                session.refresh().await.unwrap();
            }
            "expire" => now.store(3605, Ordering::SeqCst),
            "logout" => {
                session.logout().await.unwrap();
            }
            _ => unreachable!(),
        }
        release.notify_one();
        assert_eq!(pending.await.err(), Some(Error::Stale));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn playback_read_does_not_clear_an_unconfirmed_write() {
    let (session, _) = crate::native_tests::linked().await;
    let client = AccountClient::with_transport(Script {
        response: Ok((200, PLAYBACK.to_vec())),
        calls: Arc::default(),
    });
    client.bootstrap().await.unwrap();
    assert_eq!(
        client
            .add_watch_list(
                &session,
                &MediaId::new("Chosen01").unwrap(),
                WatchListContentType::Episode
            )
            .await,
        Err(WriteFailure::Unconfirmed(Error::Unavailable))
    );
    assert!(matches!(
        client.playback(&session, request()).await.unwrap(),
        NativePlaybackSelection::Selected(_)
    ));
    assert_eq!(client.write_status(), WriteStatus::Unconfirmed);
}
