use crate::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

const INIT: &[u8] = br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
const IDS: &str = r#"{"watchlist":["AbCd1234"],"positions":[{"media_id":"AbCd1234","pos":-1,"dur":7200,"commentary_track":null,"series_id":"Series01","series_title":"Synthetic series"}]}"#;
const CONTINUE: &str = r#"{"playlist":[{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic film","duration":90.5,"release_date":"2000-02-29"},{"contentType":"series","mediaid":"Series01","title":"Synthetic series"}],"positions":[]}"#;
struct NativeFixture {
    payload: String,
    requests: Arc<Mutex<Vec<Target>>>,
    status: u16,
}
impl Transport for NativeFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        self.requests.lock().unwrap().push(request.target);
        if request.target == Target::Bootstrap {
            assert!(request.credentials.is_none());
            return Ok(Response {
                status: 200,
                body: SecretBody::new(INIT.to_vec()),
            });
        }
        let credentials = request.credentials.unwrap();
        assert_eq!(
            credentials.bootstrap.as_bytes(),
            b"Bearer synthetic-bootstrap"
        );
        assert!(credentials.subscriber.is_sensitive());
        assert!(credentials.bootstrap.is_sensitive());
        Ok(Response {
            status: self.status,
            body: SecretBody::new(self.payload.as_bytes().to_vec()),
        })
    }
}
struct SessionFixture(AtomicU64);
impl criterion_session::Transport for SessionFixture {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        let body = match request.endpoint {
            criterion_session::Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
            criterion_session::Endpoint::Revoke => b"{}".to_vec(),
            _ => {
                let generation = self.0.fetch_add(1, Ordering::SeqCst);
                format!(r#"{{"access_token":"synthetic-subscriber-{generation}","refresh_token":"synthetic-refresh-{generation}","expires_in":3600}}"#).into_bytes()
            }
        };
        Ok(criterion_session::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
#[derive(Clone)]
struct Clock(Arc<AtomicU64>);
impl criterion_session::MonotonicClock for Clock {
    fn now(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
type FixtureSession = criterion_session::Session<SessionFixture, Clock>;
async fn linked() -> (FixtureSession, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(0));
    let session = criterion_session::Session::with_transport(
        criterion_session::Configuration::production(),
        SessionFixture(AtomicU64::new(0)),
        Clock(now.clone()),
    );
    session.start_link().await.unwrap();
    now.store(5, Ordering::SeqCst);
    session.poll_once().await.unwrap();
    (session, now)
}
async fn account(payload: &str) -> AccountClient<NativeFixture> {
    let account = AccountClient::with_transport(NativeFixture {
        payload: payload.into(),
        requests: Arc::default(),
        status: 200,
    });
    account.bootstrap().await.unwrap();
    account
}
#[tokio::test]
async fn native_ids_preserve_signed_positions_and_use_selected_region_with_dual_headers() {
    let (session, _) = linked().await;
    let requests: Arc<Mutex<Vec<Target>>> = Arc::default();
    let account = AccountClient::with_transport(NativeFixture {
        payload: IDS.into(),
        requests: requests.clone(),
        status: 200,
    });
    account.bootstrap().await.unwrap();
    let result = account.my_list_ids(&session).await.unwrap();
    assert_eq!(result.watchlist[0].as_str(), "AbCd1234");
    assert_eq!(result.positions[0].pos, -1);
    assert_eq!(result.positions[0].dur, 7200);
    assert_eq!(
        result.positions[0].series_id.as_ref().unwrap().as_str(),
        "Series01"
    );
    assert_eq!(
        *requests.lock().unwrap(),
        [Target::Bootstrap, Target::MyListIds(Region::Ca)]
    );
    let diagnostic = format!("{result:?} {:?}", result.positions[0]);
    assert!(
        !diagnostic.contains("AbCd")
            && !diagnostic.contains("Synthetic")
            && !diagnostic.contains("7200")
    );
}
#[tokio::test]
async fn native_continue_preserves_optional_fractional_duration_and_valid_calendar_date() {
    let (session, _) = linked().await;
    let result = account(CONTINUE)
        .await
        .continue_watching(&session)
        .await
        .unwrap();
    assert_eq!(result.playlist[0].duration, Some(90.5));
    assert_eq!(
        result.playlist[0].release_date,
        Some(time::Date::from_calendar_date(2000, time::Month::February, 29).unwrap())
    );
    assert_eq!(result.playlist[1].duration, None);
    assert_eq!(result.playlist[1].release_date, None);
    assert_eq!(result.playlist[1].kind, MediaKind::Series);
    assert!(!format!("{result:?} {:?}", result.playlist[0]).contains("Synthetic"));
}
#[tokio::test]
async fn native_polymorphism_uses_only_proven_subtype_fields_without_inventing_defaults() {
    let (session, _) = linked().await;
    for (kind, expected) in [
        ("category", MediaKind::Category),
        ("collection", MediaKind::Collection),
        ("series", MediaKind::Series),
        ("original", MediaKind::Original),
        ("episode", MediaKind::Episode),
        ("franchise", MediaKind::Franchise),
        ("live", MediaKind::Live),
        ("film", MediaKind::Film),
        ("supplement", MediaKind::Supplement),
    ] {
        let body = format!(
            r#"{{"playlist":[{{"contentType":"{kind}","mediaid":"AbCd1234","title":"Synthetic"}}],"positions":[]}}"#
        );
        let result = account(&body)
            .await
            .continue_watching(&session)
            .await
            .unwrap();
        assert_eq!(result.playlist[0].kind, expected);
        assert_eq!(result.playlist[0].duration, None);
    }
    let no_container_duration = r#"{"playlist":[{"contentType":"collection","mediaid":"AbCd1234","title":"Synthetic","duration":12.5,"release_date":"2000-01-01"}],"positions":[]}"#;
    let result = account(no_container_duration)
        .await
        .continue_watching(&session)
        .await
        .unwrap();
    assert_eq!(result.playlist[0].duration, None);
    assert_eq!(result.playlist[0].release_date, None);
}
#[tokio::test]
async fn native_response_required_types_and_bounds_are_enforced_without_payload_diagnostics() {
    let (session, _) = linked().await;
    for body in [
        r#"{}"#,
        r#"{"watchlist":[],"positions":null}"#,
        r#"{"watchlist":["bad/id"],"positions":[]}"#,
        r#"{"watchlist":[],"positions":[{"media_id":"AbCd1234","pos":1.5,"dur":2}]}"#,
        r#"{"watchlist":[],"positions":[{"media_id":"AbCd1234","pos":0}]}"#,
        r#"{"watchlist":[],"watchlist":[],"positions":[]}"#,
    ] {
        assert_eq!(
            account(body).await.my_list_ids(&session).await,
            Err(Error::InvalidResponse)
        );
    }
    for media in [
        r#"{"contentType":"unknown","mediaid":"AbCd1234","title":"Synthetic"}"#,
        r#"{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic","duration":null}"#,
        r#"{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic","duration":-1}"#,
        r#"{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic","duration":1e40}"#,
        r#"{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic","release_date":"1900-02-29"}"#,
        r#"{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic","release_date":"2000-02-29T00:00:00Z"}"#,
        r#"{"contentType":"film","mediaid":"AbCd1234","title":""}"#,
        r#"{"contentType":"film","mediaid":"AbCd1234","title":"bad\u0000title"}"#,
    ] {
        let body = format!(r#"{{"playlist":[{media}],"positions":[]}}"#);
        assert_eq!(
            account(&body).await.continue_watching(&session).await,
            Err(Error::InvalidResponse)
        );
    }
    let too_many =
        serde_json::json!({"watchlist": vec!["AbCd1234"; 513], "positions": []}).to_string();
    assert_eq!(
        account(&too_many).await.my_list_ids(&session).await,
        Err(Error::InvalidResponse)
    );
    let too_large = " ".repeat(65_537);
    assert_eq!(
        account(&too_large).await.my_list_ids(&session).await,
        Err(Error::ResponseTooLarge)
    );
    let too_long = serde_json::json!({"playlist":[{"contentType":"film","mediaid":"AbCd1234","title":"x".repeat(1025)}],"positions":[]}).to_string();
    assert_eq!(
        account(&too_long).await.continue_watching(&session).await,
        Err(Error::InvalidResponse)
    );
}
#[derive(Clone)]
struct HeldNative {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl Transport for HeldNative {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        if request.target != Target::Bootstrap {
            self.entered.notify_one();
            self.release.notified().await;
        }
        NativeFixture {
            payload: IDS.into(),
            requests: Arc::default(),
            status: 200,
        }
        .get(request)
        .await
    }
}
#[tokio::test]
async fn native_reads_reject_session_rotation_logout_disposal_and_expiry_after_await() {
    for action in 0..4 {
        let (session, now) = linked().await;
        let held = HeldNative {
            entered: Arc::default(),
            release: Arc::default(),
        };
        let account = AccountClient::with_transport(held.clone());
        account.bootstrap().await.unwrap();
        let mut pending = Box::pin(account.my_list_ids(&session));
        tokio::select! { value = &mut pending => panic!("read returned early: {value:?}"), () = held.entered.notified() => {} }
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
        assert_eq!(pending.await, Err(Error::Stale));
    }
}
#[tokio::test]
async fn native_read_capacity_cancel_disposal_and_future_drop_keep_bootstrap_owned() {
    for action in 0..3 {
        let (session, _) = linked().await;
        let held = HeldNative {
            entered: Arc::default(),
            release: Arc::default(),
        };
        let account = AccountClient::with_transport(held.clone());
        account.bootstrap().await.unwrap();
        let mut pending = Box::pin(account.my_list_ids(&session));
        tokio::select! { value = &mut pending => panic!("read returned early: {value:?}"), () = held.entered.notified() => {} }
        assert_eq!(account.continue_watching(&session).await, Err(Error::Busy));
        assert!(matches!(account.credentials(&session), Err(Error::Busy)));
        if action == 0 {
            account.cancel();
        } else if action == 1 {
            account.dispose();
        }
        held.release.notify_one();
        if action == 2 {
            drop(pending);
        } else {
            assert_eq!(
                pending.await,
                Err(if action == 1 {
                    Error::Disposed
                } else {
                    Error::Stale
                })
            );
        }
        assert_eq!(
            account.region(),
            if action == 1 {
                Err(Error::Disposed)
            } else {
                Ok(Region::Ca)
            }
        );
    }
}
#[tokio::test]
async fn native_reads_require_bootstrap_and_live_subscriber_and_preserve_status_failure() {
    let (session, _) = linked().await;
    let requests: Arc<Mutex<Vec<Target>>> = Arc::default();
    let account = AccountClient::with_transport(NativeFixture {
        payload: IDS.into(),
        requests: requests.clone(),
        status: 401,
    });
    assert_eq!(account.my_list_ids(&session).await, Err(Error::NoBootstrap));
    assert!(requests.lock().unwrap().is_empty());
    account.bootstrap().await.unwrap();
    assert_eq!(
        account.my_list_ids(&session).await,
        Err(Error::HttpStatus(401))
    );
    session.dispose();
    assert_eq!(
        account.my_list_ids(&session).await,
        Err(Error::Session(criterion_session::Error::Disposed))
    );
    assert_eq!(requests.lock().unwrap().len(), 2);
}

const WATCH: &str = r#"{"paging":{"page_limit":60,"next_pagination_key":"opaque-synthetic-cursor"},"type_counts":{"film":2,"opaque-future-type":1},"playlist":[{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic listed film","duration":95.25}]}"#;
#[tokio::test]
async fn native_watch_list_uses_default_route_and_preserves_required_paging_counts_cards() {
    let (session, _) = linked().await;
    let requests: Arc<Mutex<Vec<Target>>> = Arc::default();
    let client = AccountClient::with_transport(NativeFixture {
        payload: WATCH.into(),
        requests: requests.clone(),
        status: 200,
    });
    client.bootstrap().await.unwrap();
    let result = client.watch_list(&session).await.unwrap();
    assert_eq!(result.paging.page_limit, 60);
    assert_eq!(
        result.paging.next_pagination_key,
        Some(criterion_provider::PageCursor::new("opaque-synthetic-cursor").unwrap())
    );
    assert_eq!(result.type_counts.len(), 2);
    assert!(
        result
            .type_counts
            .iter()
            .any(|entry| entry.content_type == "opaque-future-type" && entry.count == 1)
    );
    assert_eq!(result.playlist[0].duration, Some(95.25));
    assert_eq!(
        *requests.lock().unwrap(),
        [Target::Bootstrap, Target::WatchList(Region::Ca)]
    );
    let diagnostic = format!("{result:?} {:?} {:?}", result.paging, result.type_counts[0]);
    assert!(!diagnostic.contains("opaque") && !diagnostic.contains("Synthetic"));
    let empty = account(
        r#"{"paging":{"page_limit":60,"next_pagination_key":null},"type_counts":{},"playlist":[]}"#,
    )
    .await
    .watch_list(&session)
    .await
    .unwrap();
    assert!(empty.playlist.is_empty() && empty.paging.next_pagination_key.is_none());
}
#[tokio::test]
async fn native_watch_list_rejects_missing_wrong_duplicate_and_oversized_metadata() {
    let (session, _) = linked().await;
    for body in [
        r#"{"paging":{"page_limit":60},"playlist":[]}"#,
        r#"{"paging":{},"type_counts":{},"playlist":[]}"#,
        r#"{"paging":{"page_limit":2147483648},"type_counts":{},"playlist":[]}"#,
        r#"{"paging":{"page_limit":60},"type_counts":{"film":1.5},"playlist":[]}"#,
        r#"{"paging":{"page_limit":60},"type_counts":{"film":null},"playlist":[]}"#,
        r#"{"paging":{"page_limit":60},"type_counts":{"film":1,"film":2},"playlist":[]}"#,
        r#"{"paging":{"page_limit":60,"next_pagination_key":""},"type_counts":{},"playlist":[]}"#,
    ] {
        assert_eq!(
            account(body).await.watch_list(&session).await,
            Err(Error::InvalidResponse)
        );
    }
    let counts: serde_json::Map<String, serde_json::Value> = (0..129)
        .map(|index| (format!("type{index}"), serde_json::json!(1)))
        .collect();
    let body = serde_json::json!({"paging":{"page_limit":60},"type_counts":counts,"playlist":[]})
        .to_string();
    assert_eq!(
        account(&body).await.watch_list(&session).await,
        Err(Error::InvalidResponse)
    );
    let body = serde_json::json!({"paging":{"page_limit":60,"next_pagination_key":"x".repeat(513)},"type_counts":{},"playlist":[]}).to_string();
    assert_eq!(
        account(&body).await.watch_list(&session).await,
        Err(Error::InvalidResponse)
    );
    let body = serde_json::json!({"paging":{"page_limit":60},"type_counts":{"x".repeat(65):1},"playlist":[]}).to_string();
    assert_eq!(
        account(&body).await.watch_list(&session).await,
        Err(Error::InvalidResponse)
    );
}
