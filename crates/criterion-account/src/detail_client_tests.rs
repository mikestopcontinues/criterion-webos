//! Synthetic ownership fixtures exercise anonymous Detail and its shared lease.
use crate::*;
use criterion_provider::MediaId;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const INIT: &[u8] = br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
const FILM: &[u8] = br#"{"contentType":"film","mediaid":"Film0001","title":"Fixture"}"#;
fn id() -> MediaId {
    MediaId::new("Film0001").unwrap()
}
struct Held {
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    calls: Arc<AtomicUsize>,
}
impl Transport for Held {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match request {
            Request::Bootstrap => Ok(Response {
                status: 200,
                body: SecretBody::new(INIT.to_vec()),
            }),
            Request::Detail {
                region,
                media_id,
                authorization,
            } => {
                assert_eq!(region, Region::Us);
                assert_eq!(media_id, id());
                assert!(authorization.header().is_sensitive());
                assert_eq!(
                    authorization.header().as_bytes(),
                    b"Bearer synthetic-bootstrap"
                );
                self.started.notify_one();
                self.release.notified().await;
                Ok(Response {
                    status: 200,
                    body: SecretBody::new(FILM.to_vec()),
                })
            }
            Request::Subscriber { .. } => {
                panic!("anonymous fixture must not issue a subscriber request")
            }
        }
    }
}
fn held() -> (
    AccountClient<Held>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
    Arc<AtomicUsize>,
) {
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let client = AccountClient::with_transport(Held {
        started: started.clone(),
        release: release.clone(),
        calls: calls.clone(),
    });
    (client, started, release, calls)
}

#[tokio::test]
async fn anonymous_detail_requires_bootstrap_without_contact_or_session() {
    let (client, _, _, calls) = held();
    assert_eq!(client.detail(&id()).await.err(), Some(Error::NoBootstrap));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    client.dispose();
    assert_eq!(client.detail(&id()).await.err(), Some(Error::Disposed));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn public_detail_exclusively_owns_shared_read_lease_and_cancel_rejects_completion() {
    let (client, started, release, calls) = held();
    client.bootstrap().await.unwrap();
    let media_id = id();
    let mut pending = Box::pin(client.detail(&media_id));
    tokio::select! {
        result = &mut pending => panic!("held read completed: {result:?}"),
        () = started.notified() => {},
    }
    assert_eq!(client.bootstrap().await.err(), Some(Error::Busy));
    assert_eq!(client.detail(&id()).await.err(), Some(Error::Busy));
    client.cancel();
    release.notify_one();
    assert_eq!(pending.await.err(), Some(Error::Stale));
    assert_eq!(client.region(), Ok(Region::Us));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn public_detail_disposal_rejects_completion_and_erases_bootstrap() {
    let (client, started, release, _) = held();
    client.bootstrap().await.unwrap();
    let media_id = id();
    let mut pending = Box::pin(client.detail(&media_id));
    tokio::select! {
        result = &mut pending => panic!("held read completed: {result:?}"),
        () = started.notified() => {},
    }
    client.dispose();
    release.notify_one();
    assert_eq!(pending.await.err(), Some(Error::Disposed));
    assert_eq!(client.region(), Err(Error::Disposed));
}

#[tokio::test]
async fn dropped_public_detail_releases_read_lease_and_keeps_admitted_bootstrap() {
    let (client, started, _, _) = held();
    client.bootstrap().await.unwrap();
    let media_id = id();
    let mut pending = Box::pin(client.detail(&media_id));
    tokio::select! {
        result = &mut pending => panic!("held read completed: {result:?}"),
        () = started.notified() => {},
    }
    drop(pending);
    assert_eq!(client.region(), Ok(Region::Us));
    assert_eq!(client.bootstrap().await, Ok(Region::Us));
}

struct UnconfirmedWriteThenDetail {
    calls: Arc<AtomicUsize>,
}
impl Transport for UnconfirmedWriteThenDetail {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match request {
            Request::Bootstrap => Ok(Response {
                status: 200,
                body: SecretBody::new(INIT.to_vec()),
            }),
            Request::Subscriber {
                target: SubscriberTarget::AddWatchList { .. },
                ..
            } => Err(Error::Unavailable),
            Request::Detail { .. } => Ok(Response {
                status: 200,
                body: SecretBody::new(FILM.to_vec()),
            }),
            Request::Subscriber { .. } => panic!("unexpected subscriber route"),
        }
    }
}

#[tokio::test]
async fn public_detail_does_not_erase_an_unconfirmed_private_write() {
    let (session, _) = crate::native_tests::linked().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let client = AccountClient::with_transport(UnconfirmedWriteThenDetail {
        calls: calls.clone(),
    });
    client.bootstrap().await.unwrap();
    assert_eq!(
        client
            .add_watch_list(&session, &id(), WatchListContentType::Film)
            .await,
        Err(WriteFailure::Unconfirmed(Error::Unavailable))
    );
    assert_eq!(client.write_status(), WriteStatus::Unconfirmed);
    assert_eq!(client.detail(&id()).await.unwrap().media.id, id());
    assert_eq!(client.write_status(), WriteStatus::Unconfirmed);
    assert_eq!(
        client
            .add_watch_list(&session, &id(), WatchListContentType::Film)
            .await,
        Err(WriteFailure::NotIssued(Error::ReconciliationRequired))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
