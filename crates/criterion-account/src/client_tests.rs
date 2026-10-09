use crate::*;
struct Fixture;
impl Transport for Fixture {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        assert_eq!(request.target, Target::Bootstrap);
        assert!(request.credentials.is_none());
        Ok(Response { status: 200, body: SecretBody::new(br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()) })
    }
}
#[tokio::test]
async fn anonymous_bootstrap_admits_only_exact_production_bases_and_keeps_token_private() {
    let account = AccountClient::with_transport(Fixture);
    assert_eq!(account.bootstrap().await, Ok(Region::Us));
}

#[derive(Clone)]
struct Held {
    entered: std::sync::Arc<tokio::sync::Notify>,
    release: std::sync::Arc<tokio::sync::Notify>,
}
impl Transport for Held {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        self.entered.notify_one();
        self.release.notified().await;
        Fixture.send(request).await
    }
}
#[tokio::test]
async fn cancellation_disposal_and_dropped_future_reject_late_bootstrap_publication() {
    for action in [0, 1, 2] {
        let held = Held {
            entered: std::sync::Arc::new(tokio::sync::Notify::new()),
            release: std::sync::Arc::new(tokio::sync::Notify::new()),
        };
        let account = AccountClient::with_transport(held.clone());
        let mut pending = Box::pin(account.bootstrap());
        tokio::select! { value = &mut pending => panic!("bootstrap returned early: {value:?}"), () = held.entered.notified() => {} }
        assert_eq!(account.bootstrap().await, Err(Error::Busy));
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
            Err(if action == 1 {
                Error::Disposed
            } else {
                Error::NoBootstrap
            })
        );
    }
}
struct Body(String);
impl Transport for Body {
    async fn send(&self, _request: Request) -> Result<Response, Error> {
        Ok(Response {
            status: 200,
            body: SecretBody::new(self.0.as_bytes().to_vec()),
        })
    }
}
#[tokio::test]
async fn hostile_bootstrap_bases_codes_duplicate_fields_and_tokens_are_not_admitted() {
    for us in [
        "http://mw.criterion.com/api/us",
        "https://mw.criterion.com.evil.invalid/api/us",
        "https://name@mw.criterion.com/api/us",
        "https://mw.criterion.com:444/api/us",
        "https://mw.criterion.com/api/us/elsewhere",
        "https://mw.criterion.com/api/us?q=private",
        "https://mw.criterion.com/api/us#private",
    ] {
        let body = serde_json::to_string(&serde_json::json!({"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":us,"ca":CA_BASE}})).unwrap();
        let account = AccountClient::with_transport(Body(body));
        assert_eq!(account.bootstrap().await, Err(Error::InvalidResponse));
        assert_eq!(account.region(), Err(Error::NoBootstrap));
    }
    for token in [
        String::new(),
        "private\nheader".to_owned(),
        "private header".to_owned(),
        "private-é".to_owned(),
        "x".repeat(wire::MAX_TOKEN + 1),
    ] {
        let body = serde_json::to_string(&serde_json::json!({"country":"US","token":token,"baseUrl":{"us":US_BASE,"ca":CA_BASE}})).unwrap();
        assert_eq!(
            AccountClient::with_transport(Body(body)).bootstrap().await,
            Err(Error::InvalidResponse)
        );
    }
    let duplicate = format!(
        r#"{{"country":"US","token":"first","token":"second","baseUrl":{{"us":"{US_BASE}","ca":"{CA_BASE}"}}}}"#
    );
    assert_eq!(
        AccountClient::with_transport(Body(duplicate))
            .bootstrap()
            .await,
        Err(Error::InvalidResponse)
    );
    let unsupported = format!(
        r#"{{"country":"GB","token":"private","baseUrl":{{"us":"{US_BASE}","ca":"{CA_BASE}"}}}}"#
    );
    assert_eq!(
        AccountClient::with_transport(Body(unsupported))
            .bootstrap()
            .await,
        Err(Error::UnsupportedRegion)
    );
}
struct SessionFixture;
#[tokio::test]
async fn bootstrap_requires_objects_for_response_and_base_map() {
    for body in [
        format!(
            r#"{{"country":"US","token":"synthetic-bootstrap","baseUrl":["{US_BASE}","{CA_BASE}"]}}"#
        ),
        format!(r#"["US","synthetic-bootstrap",{{"us":"{US_BASE}","ca":"{CA_BASE}"}}]"#),
    ] {
        let account = AccountClient::with_transport(Body(body));
        assert_eq!(account.bootstrap().await, Err(Error::InvalidResponse));
        assert_eq!(account.region(), Err(Error::NoBootstrap));
    }
}
impl criterion_session::Transport for SessionFixture {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        let body = if request.endpoint == criterion_session::Endpoint::DeviceCode {
            br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec()
        } else {
            br#"{"access_token":"synthetic-subscriber","refresh_token":"synthetic-refresh","expires_in":3600}"#.to_vec()
        };
        Ok(criterion_session::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
struct Clock(std::sync::Arc<std::sync::atomic::AtomicU64>);
impl criterion_session::MonotonicClock for Clock {
    fn now(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}
#[tokio::test]
async fn native_headers_keep_bootstrap_bearer_and_raw_subscriber_separate_and_sensitive() {
    let now = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let session = criterion_session::Session::with_transport(
        criterion_session::Configuration::production(),
        SessionFixture,
        Clock(now.clone()),
    );
    let account = AccountClient::with_transport(Fixture);
    assert!(matches!(
        account.credentials(&session),
        Err(Error::NoBootstrap)
    ));
    account.bootstrap().await.unwrap();
    assert!(matches!(
        account.credentials(&session),
        Err(Error::Session(criterion_session::Error::NoSession))
    ));
    session.start_link().await.unwrap();
    now.store(5, std::sync::atomic::Ordering::SeqCst);
    session.poll_once().await.unwrap();
    let headers = account.credentials(&session).unwrap();
    assert_eq!(
        headers.bootstrap().as_bytes(),
        b"Bearer synthetic-bootstrap"
    );
    assert_eq!(headers.subscriber().as_bytes(), b"synthetic-subscriber");
    assert!(headers.bootstrap().is_sensitive());
    assert!(headers.subscriber().is_sensitive());
    assert!(
        !format!(
            "{headers:?} {:?} {:?}",
            headers.bootstrap(),
            headers.subscriber()
        )
        .contains("synthetic")
    );
    session.dispose();
    assert!(matches!(
        account.credentials(&session),
        Err(Error::Session(criterion_session::Error::Disposed))
    ));
}

struct ChangingBootstrap(std::sync::atomic::AtomicU64);
impl Transport for ChangingBootstrap {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            Fixture.send(request).await
        } else {
            Ok(Response {
                status: 200,
                body: SecretBody::new(b"{}".to_vec()),
            })
        }
    }
}
#[tokio::test]
async fn failed_rebootstrap_cannot_reuse_a_previously_admitted_region_or_token() {
    let account =
        AccountClient::with_transport(ChangingBootstrap(std::sync::atomic::AtomicU64::new(0)));
    assert_eq!(account.bootstrap().await, Ok(Region::Us));
    assert_eq!(account.bootstrap().await, Err(Error::InvalidResponse));
    assert_eq!(account.region(), Err(Error::NoBootstrap));
}
