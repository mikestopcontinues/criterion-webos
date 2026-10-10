//! Synthetic native entitlement fixtures, separate from licensed playback.
use crate::*;
use std::sync::{Arc, Mutex};

const INIT: &[u8] = br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
const CAPTURED_UNIX_MS: i64 = 1_791_590_123_456;

struct Fixture {
    body: Vec<u8>,
    status: u16,
    requests: Arc<Mutex<Vec<SubscriberTarget>>>,
}
impl Transport for Fixture {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        match request {
            Request::Bootstrap => Ok(Response {
                status: 200,
                body: SecretBody::new(INIT.to_vec()),
            }),
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
                Ok(Response {
                    status: self.status,
                    body: SecretBody::new(self.body.clone()),
                })
            }
            Request::Detail { .. } => panic!("entitlement cannot request Detail"),
        }
    }
}

#[tokio::test]
async fn native_entitlement_reads_required_fields_with_separate_sensitive_credentials() {
    let (session, _) = crate::native_tests::linked().await;
    let requests = Arc::default();
    let client = AccountClient::with_transport(Fixture {
        body: br#"{"accessGranted":true,"customerId":2147483647}"#.to_vec(),
        status: 200,
        requests: Arc::clone(&requests),
    });
    client.bootstrap().await.unwrap();
    let result = client
        .entitlement(&session, CAPTURED_UNIX_MS)
        .await
        .unwrap();
    assert!(result.access_granted);
    assert_eq!(result.customer_id(), 2_147_483_647);
    assert!(!format!("{result:?}").contains("2147483647"));
    assert_eq!(
        *requests.lock().unwrap(),
        [SubscriberTarget::Entitlement {
            region: Region::Ca,
            captured_unix_time_ms: CAPTURED_UNIX_MS,
        }]
    );
}

async fn fixture(
    body: Vec<u8>,
    status: u16,
) -> (AccountClient<Fixture>, Arc<Mutex<Vec<SubscriberTarget>>>) {
    let requests = Arc::default();
    let client = AccountClient::with_transport(Fixture {
        body,
        status,
        requests: Arc::clone(&requests),
    });
    client.bootstrap().await.unwrap();
    (client, requests)
}

#[tokio::test]
async fn native_entitlement_preserves_false_and_signed_customer_without_grant_policy() {
    let (session, _) = crate::native_tests::linked().await;
    let (client, _) = fixture(br#"{"accessGranted":false,"customerId":-2147483648,"offerId":"not-owned","grantType":"not-a-policy","expiresAt":0,"message":"private-not-retained","data":[{"unowned":"extension"}]}"#.to_vec(), 200).await;
    let result = client
        .entitlement(&session, CAPTURED_UNIX_MS)
        .await
        .unwrap();
    assert!(!result.access_granted);
    assert_eq!(result.customer_id(), -2_147_483_648);
    let diagnostic = format!("{result:?}");
    for private in [
        "-2147483648",
        "private-not-retained",
        "not-a-policy",
        "extension",
    ] {
        assert!(!diagnostic.contains(private));
    }
}

#[tokio::test]
async fn native_entitlement_required_object_schema_refuses_malformed_success_without_retry() {
    let (session, _) = crate::native_tests::linked().await;
    for body in [
        "{}",
        "null",
        "[]",
        "[true,7]",
        r#"{"accessGranted":true}"#,
        r#"{"customerId":7}"#,
        r#"{"accessGranted":null,"customerId":7}"#,
        r#"{"accessGranted":"true","customerId":7}"#,
        r#"{"accessGranted":1,"customerId":7}"#,
        r#"{"accessGranted":true,"customerId":null}"#,
        r#"{"accessGranted":true,"customerId":"7"}"#,
        r#"{"accessGranted":true,"customerId":7.0}"#,
        r#"{"accessGranted":true,"customerId":2147483648}"#,
        r#"{"accessGranted":true,"customerId":-2147483649}"#,
        r#"{"accessGranted":true,"accessGranted":false,"customerId":7}"#,
        r#"{"accessGranted":true,"customerId":7,"customerId":8}"#,
        r#"{"accessGranted":true,"customerId":7}{}"#,
    ] {
        let (client, requests) = fixture(body.as_bytes().to_vec(), 200).await;
        let error = client
            .entitlement(&session, CAPTURED_UNIX_MS)
            .await
            .unwrap_err();
        assert_eq!(error, Error::InvalidResponse);
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert_eq!(format!("{error}"), "InvalidResponse");
    }
}

#[tokio::test]
async fn native_entitlement_body_limit_is_inclusive_and_http_failure_is_not_a_grant() {
    let (session, _) = crate::native_tests::linked().await;
    let mut body = br#"{"accessGranted":true,"customerId":7}"#.to_vec();
    body.resize(65_536, b' ');
    let (client, _) = fixture(body.clone(), 200).await;
    assert!(
        client
            .entitlement(&session, CAPTURED_UNIX_MS)
            .await
            .unwrap()
            .access_granted
    );
    body.push(b' ');
    let (client, requests) = fixture(body, 200).await;
    assert_eq!(
        client.entitlement(&session, CAPTURED_UNIX_MS).await.err(),
        Some(Error::ResponseTooLarge)
    );
    assert_eq!(requests.lock().unwrap().len(), 1);
    for status in [204, 401, 403, 429, 503] {
        let (client, requests) =
            fixture(br#"{"accessGranted":true,"customerId":7}"#.to_vec(), status).await;
        assert_eq!(
            client.entitlement(&session, CAPTURED_UNIX_MS).await.err(),
            Some(Error::HttpStatus(status))
        );
        assert_eq!(requests.lock().unwrap().len(), 1);
    }
}

struct Held {
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    fail_write: bool,
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
                target: SubscriberTarget::Entitlement { .. },
                credentials,
            } => {
                assert!(credentials.bootstrap().is_sensitive());
                assert!(credentials.subscriber().is_sensitive());
                self.started.notify_one();
                self.release.notified().await;
                Ok(Response {
                    status: 200,
                    body: SecretBody::new(br#"{"accessGranted":true,"customerId":7}"#.to_vec()),
                })
            }
            Request::Subscriber {
                target: SubscriberTarget::AddWatchList { .. },
                ..
            } if self.fail_write => Err(Error::Unavailable),
            _ => panic!("unexpected request in held entitlement fixture"),
        }
    }
}
type HeldFixture = (
    AccountClient<Held>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
    Arc<std::sync::atomic::AtomicUsize>,
);
fn held(fail_write: bool) -> HeldFixture {
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    (
        AccountClient::with_transport(Held {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
            calls: Arc::clone(&calls),
            fail_write,
        }),
        started,
        release,
        calls,
    )
}

#[tokio::test]
async fn native_entitlement_refuses_missing_bootstrap_or_subscriber_before_contact() {
    let (session, _) = crate::native_tests::linked().await;
    let (client, _, _, calls) = held(false);
    assert_eq!(
        client.entitlement(&session, CAPTURED_UNIX_MS).await.err(),
        Some(Error::NoBootstrap)
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    client.bootstrap().await.unwrap();
    session.logout().await.unwrap();
    assert_eq!(
        client.entitlement(&session, CAPTURED_UNIX_MS).await.err(),
        Some(Error::Session(criterion_session::Error::NoSession))
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    client.dispose();
    assert_eq!(
        client.entitlement(&session, CAPTURED_UNIX_MS).await.err(),
        Some(Error::Disposed)
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn native_entitlement_exclusively_holds_shared_lease_and_cancel_retires_result() {
    let (session, _) = crate::native_tests::linked().await;
    let (client, started, release, calls) = held(false);
    client.bootstrap().await.unwrap();
    let mut pending = Box::pin(client.entitlement(&session, CAPTURED_UNIX_MS));
    tokio::select! { result = &mut pending => panic!("held read completed: {result:?}"), () = started.notified() => {} }
    assert_eq!(
        client
            .detail(&criterion_provider::MediaId::new("Film0001").unwrap())
            .await
            .err(),
        Some(Error::Busy)
    );
    assert_eq!(
        client.entitlement(&session, CAPTURED_UNIX_MS).await.err(),
        Some(Error::Busy)
    );
    assert_eq!(client.bootstrap().await.err(), Some(Error::Busy));
    client.cancel();
    release.notify_one();
    assert_eq!(pending.await.err(), Some(Error::Stale));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(client.region(), Ok(Region::Ca));
}

#[tokio::test]
async fn native_entitlement_disposal_retires_result_and_dropped_future_releases_lease() {
    let (session, _) = crate::native_tests::linked().await;
    let (client, started, release, _) = held(false);
    client.bootstrap().await.unwrap();
    let mut pending = Box::pin(client.entitlement(&session, CAPTURED_UNIX_MS));
    tokio::select! { result = &mut pending => panic!("held read completed: {result:?}"), () = started.notified() => {} }
    client.dispose();
    release.notify_one();
    assert_eq!(pending.await.err(), Some(Error::Disposed));
    assert_eq!(client.region(), Err(Error::Disposed));

    let (client, started, _, calls) = held(false);
    client.bootstrap().await.unwrap();
    let mut pending = Box::pin(client.entitlement(&session, CAPTURED_UNIX_MS));
    tokio::select! { result = &mut pending => panic!("held read completed: {result:?}"), () = started.notified() => {} }
    drop(pending);
    assert_eq!(client.region(), Ok(Region::Ca));
    assert_eq!(client.bootstrap().await, Ok(Region::Ca));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn native_entitlement_changed_or_expired_access_token_cannot_admit_awaited_success() {
    for expire in [false, true] {
        let (session, now) = crate::native_tests::linked().await;
        let (client, started, release, _) = held(false);
        client.bootstrap().await.unwrap();
        let mut pending = Box::pin(client.entitlement(&session, CAPTURED_UNIX_MS));
        tokio::select! { result = &mut pending => panic!("held read completed: {result:?}"), () = started.notified() => {} }
        if expire {
            now.store(3605, std::sync::atomic::Ordering::SeqCst);
        } else {
            session.refresh().await.unwrap();
        }
        release.notify_one();
        assert_eq!(pending.await.err(), Some(Error::Stale));
    }
}

#[tokio::test]
async fn native_entitlement_does_not_clear_an_unconfirmed_private_write() {
    let (session, _) = crate::native_tests::linked().await;
    let (client, _, release, calls) = held(true);
    client.bootstrap().await.unwrap();
    let id = criterion_provider::MediaId::new("Film0001").unwrap();
    assert_eq!(
        client
            .add_watch_list(&session, &id, WatchListContentType::Film)
            .await,
        Err(WriteFailure::Unconfirmed(Error::Unavailable))
    );
    release.notify_one();
    assert!(
        client
            .entitlement(&session, CAPTURED_UNIX_MS)
            .await
            .unwrap()
            .access_granted
    );
    assert_eq!(client.write_status(), WriteStatus::Unconfirmed);
    assert_eq!(
        client
            .add_watch_list(&session, &id, WatchListContentType::Film)
            .await,
        Err(WriteFailure::NotIssued(Error::ReconciliationRequired))
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
}
