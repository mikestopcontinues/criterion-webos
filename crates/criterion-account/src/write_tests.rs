use crate::native_tests::linked;
use crate::*;
use criterion_provider::MediaId;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
const INIT: &[u8] = br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
const IDS: &[u8] = br#"{"watchlist":["AbCd1234"],"positions":[]}"#;
fn id() -> MediaId {
    MediaId::new("AbCd1234").unwrap()
}
struct Fixture {
    body: Vec<u8>,
    status: u16,
    requests: Arc<Mutex<Vec<Target>>>,
}
impl Transport for Fixture {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        self.requests.lock().unwrap().push(request.target.clone());
        let body = match request.target {
            Target::Bootstrap => {
                assert!(request.credentials.is_none());
                INIT.to_vec()
            }
            Target::MyListIds(_) => IDS.to_vec(),
            _ => {
                let headers = request.credentials.unwrap();
                assert_eq!(headers.bootstrap.as_bytes(), b"Bearer synthetic-bootstrap");
                assert!(headers.bootstrap.is_sensitive() && headers.subscriber.is_sensitive());
                self.body.clone()
            }
        };
        Ok(Response {
            status: if matches!(request.target, Target::Bootstrap | Target::MyListIds(_)) {
                200
            } else {
                self.status
            },
            body: SecretBody::new(body),
        })
    }
}
async fn account(body: &[u8], status: u16) -> (AccountClient<Fixture>, Arc<Mutex<Vec<Target>>>) {
    let requests: Arc<Mutex<Vec<Target>>> = Arc::default();
    let account = AccountClient::with_transport(Fixture {
        body: body.to_vec(),
        status,
        requests: requests.clone(),
    });
    account.bootstrap().await.unwrap();
    (account, requests)
}
#[tokio::test]
async fn list_writes_use_explicit_native_types_and_return_receipts_without_applied_claims() {
    let (session, _) = linked().await;
    let (account, requests) = account(br#"{"sync":true}"#, 200).await;
    for content_type in [
        WatchListContentType::Film,
        WatchListContentType::Series,
        WatchListContentType::Collection,
        WatchListContentType::Episode,
        WatchListContentType::Supplement,
        WatchListContentType::Category,
        WatchListContentType::Franchise,
        WatchListContentType::Live,
        WatchListContentType::Original,
    ] {
        assert_eq!(
            account.add_watch_list(&session, &id(), content_type).await,
            Ok(SyncReceipt { sync: true })
        );
        assert_eq!(account.write_status(), WriteStatus::Ready);
        assert_eq!(
            requests.lock().unwrap().last(),
            Some(&Target::AddWatchList {
                region: Region::Ca,
                media_id: id(),
                content_type
            })
        );
    }
    assert_eq!(
        account.remove_watch_list(&session, &id()).await,
        Ok(SyncReceipt { sync: true })
    );
    assert_eq!(
        requests.lock().unwrap().last(),
        Some(&Target::RemoveWatchList {
            region: Region::Ca,
            media_id: id()
        })
    );
    let diagnostic = format!(
        "{:?} {:?}",
        requests.lock().unwrap().last().unwrap(),
        WriteFailure::Unconfirmed(Error::Deadline)
    );
    assert!(!diagnostic.contains("AbCd1234") && !diagnostic.contains("synthetic"));
}
#[tokio::test]
async fn omitted_and_false_sync_are_valid_receipts_while_null_wrong_and_duplicate_flags_are_unconfirmed()
 {
    let (session, _) = linked().await;
    for body in [b"{}".as_slice(), br#"{"sync":false}"#] {
        let (account, _) = account(body, 200).await;
        assert_eq!(
            account.remove_watch_list(&session, &id()).await,
            Ok(SyncReceipt { sync: false })
        );
        assert_eq!(account.write_status(), WriteStatus::Ready);
    }
    for body in [
        b"".as_slice(),
        br#"{"sync":null}"#,
        br#"{"sync":"true"}"#,
        br#"{"sync":true,"sync":false}"#,
        b"not-json",
    ] {
        let (account, _) = account(body, 200).await;
        assert_eq!(
            account.remove_watch_list(&session, &id()).await,
            Err(WriteFailure::Unconfirmed(Error::InvalidResponse))
        );
        assert_eq!(account.write_status(), WriteStatus::Unconfirmed);
    }
}
#[tokio::test]
async fn write_preflight_rejects_missing_bootstrap_or_disposed_session_without_contact() {
    let (session, _) = linked().await;
    let requests: Arc<Mutex<Vec<Target>>> = Arc::default();
    let account = AccountClient::with_transport(Fixture {
        body: b"{}".to_vec(),
        status: 200,
        requests: requests.clone(),
    });
    assert_eq!(
        account.remove_watch_list(&session, &id()).await,
        Err(WriteFailure::NotIssued(Error::NoBootstrap))
    );
    assert_eq!(account.write_status(), WriteStatus::Ready);
    assert!(requests.lock().unwrap().is_empty());
    account.bootstrap().await.unwrap();
    session.dispose();
    assert_eq!(
        account
            .add_watch_list(&session, &id(), WatchListContentType::Film)
            .await,
        Err(WriteFailure::NotIssued(Error::Session(
            criterion_session::Error::Disposed
        )))
    );
    assert_eq!(requests.lock().unwrap().len(), 1);
}
struct FailsOnce {
    error: Error,
    count: AtomicU64,
}
impl Transport for FailsOnce {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        if matches!(
            request.target,
            Target::AddWatchList { .. } | Target::RemoveWatchList { .. }
        ) && self.count.fetch_add(1, Ordering::SeqCst) == 0
        {
            return Err(self.error);
        }
        Fixture {
            body: b"{}".to_vec(),
            status: 200,
            requests: Arc::default(),
        }
        .send(request)
        .await
    }
}
#[tokio::test]
async fn definite_precontact_transport_failure_stays_not_issued_and_does_not_block_another_intent()
{
    let (session, _) = linked().await;
    for error in [Error::Busy, Error::InvalidRequest] {
        let account = AccountClient::with_transport(FailsOnce {
            error,
            count: AtomicU64::new(0),
        });
        account.bootstrap().await.unwrap();
        assert_eq!(
            account.remove_watch_list(&session, &id()).await,
            Err(WriteFailure::NotIssued(error))
        );
        assert_eq!(account.write_status(), WriteStatus::Ready);
        assert_eq!(
            account.remove_watch_list(&session, &id()).await,
            Ok(SyncReceipt { sync: false })
        );
    }
}
#[tokio::test]
async fn uncertain_delivery_remains_sticky_across_observed_membership_and_rebootstrap() {
    let (session, _) = linked().await;
    for error in [
        Error::Unavailable,
        Error::Deadline,
        Error::HttpStatus(503),
        Error::ResponseTooLarge,
    ] {
        let account = AccountClient::with_transport(FailsOnce {
            error,
            count: AtomicU64::new(0),
        });
        account.bootstrap().await.unwrap();
        assert_eq!(
            account
                .add_watch_list(&session, &id(), WatchListContentType::Film)
                .await,
            Err(WriteFailure::Unconfirmed(error))
        );
        assert_eq!(account.write_status(), WriteStatus::Unconfirmed);
        let observed = account.my_list_ids(&session).await.unwrap();
        assert_eq!(observed.watchlist, [id()]);
        account.bootstrap().await.unwrap();
        assert_eq!(account.write_status(), WriteStatus::Unconfirmed);
        assert_eq!(
            account.remove_watch_list(&session, &id()).await,
            Err(WriteFailure::NotIssued(Error::ReconciliationRequired))
        );
    }
    let (account, _) = account(b"{}", 429).await;
    assert_eq!(
        account.remove_watch_list(&session, &id()).await,
        Err(WriteFailure::Unconfirmed(Error::HttpStatus(429)))
    );
}
#[derive(Clone)]
struct Held {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl Transport for Held {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        if matches!(
            request.target,
            Target::AddWatchList { .. } | Target::RemoveWatchList { .. }
        ) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Fixture {
            body: br#"{"sync":true}"#.to_vec(),
            status: 200,
            requests: Arc::default(),
        }
        .send(request)
        .await
    }
}
#[tokio::test]
async fn issued_write_cancel_dispose_and_future_drop_leave_an_unconfirmed_fence() {
    for action in 0..3 {
        let (session, _) = linked().await;
        let held = Held {
            entered: Arc::default(),
            release: Arc::default(),
        };
        let account = AccountClient::with_transport(held.clone());
        account.bootstrap().await.unwrap();
        let media_id = id();
        let mut pending = Box::pin(account.remove_watch_list(&session, &media_id));
        tokio::select! { value = &mut pending => panic!("write returned early: {value:?}"), () = held.entered.notified() => {} }
        assert_eq!(account.write_status(), WriteStatus::Issued);
        assert_eq!(
            account
                .add_watch_list(&session, &id(), WatchListContentType::Film)
                .await,
            Err(WriteFailure::NotIssued(Error::Busy))
        );
        if action == 0 {
            account.cancel();
        } else if action == 1 {
            account.dispose();
        }
        if action == 2 {
            drop(pending);
        } else {
            held.release.notify_one();
            assert_eq!(
                pending.await,
                Err(WriteFailure::Unconfirmed(if action == 1 {
                    Error::Disposed
                } else {
                    Error::Stale
                }))
            );
        }
        assert_eq!(account.write_status(), WriteStatus::Unconfirmed);
        if action != 1 {
            assert_eq!(
                account.my_list_ids(&session).await.unwrap().watchlist,
                [id()]
            );
            assert_eq!(
                account.remove_watch_list(&session, &id()).await,
                Err(WriteFailure::NotIssued(Error::ReconciliationRequired))
            );
        }
    }
}
#[tokio::test]
async fn issued_write_rejects_late_receipts_after_session_rotation_logout_disposal_and_expiry() {
    for action in 0..4 {
        let (session, now) = linked().await;
        let held = Held {
            entered: Arc::default(),
            release: Arc::default(),
        };
        let account = AccountClient::with_transport(held.clone());
        account.bootstrap().await.unwrap();
        let media_id = id();
        let mut pending = Box::pin(account.remove_watch_list(&session, &media_id));
        tokio::select! { value = &mut pending => panic!("write returned early: {value:?}"), () = held.entered.notified() => {} }
        match action {
            0 => {
                session.refresh().await.unwrap();
            }
            1 => {
                session.logout().await.unwrap();
            }
            2 => session.dispose(),
            _ => now.store(3605, Ordering::SeqCst),
        }
        held.release.notify_one();
        assert_eq!(pending.await, Err(WriteFailure::Unconfirmed(Error::Stale)));
        assert_eq!(account.write_status(), WriteStatus::Unconfirmed);
    }
}
