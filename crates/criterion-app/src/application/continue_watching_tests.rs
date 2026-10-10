// SPDX-License-Identifier: GPL-3.0-or-later
//! CPU application journeys through real public, Session and account owners.
//! Responses and credentials are synthetic; no provider or device is contacted.
use super::*;
use criterion_account::{
    AccountClient, Region, Request, SecretBody, SubscriberTarget, WatchListRequest,
};
use criterion_platform::Size;
use criterion_session::{Configuration, Endpoint, Session};
use criterion_ui::{LoginView, Page};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};
use tokio::sync::Notify;

#[derive(Clone, Default)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Clone, Default)]
struct Issuer {
    tokens: Arc<AtomicUsize>,
    revokes: Arc<AtomicUsize>,
    hold_refresh: Arc<AtomicBool>,
    release: Arc<Notify>,
}
impl Transport for Issuer {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        let body=match request.endpoint {
            Endpoint::DeviceCode=>br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
            Endpoint::Token=>{
                let index=self.tokens.fetch_add(1,Ordering::SeqCst);
                if index>0&&self.hold_refresh.load(Ordering::SeqCst) {self.release.notified().await;}
                br#"{"access_token":"synthetic-subscriber","refresh_token":"synthetic-refresh","expires_in":3600}"#.to_vec()
            },
            Endpoint::Revoke=>{self.revokes.fetch_add(1,Ordering::SeqCst);b"{}".to_vec()},
        };
        Ok(criterion_session::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
struct Public {
    supplied: bool,
    requests: Arc<Mutex<Vec<String>>>,
}
impl RequestTransport for Public {
    async fn get(
        &self,
        request: criterion_provider::Request,
    ) -> Result<criterion_provider::Response, criterion_provider::Error> {
        self.requests
            .lock()
            .unwrap()
            .push(request.url.path().to_owned());
        if request.url.path() != "/" {
            return Err(criterion_provider::Error::Unavailable);
        }
        let mut blocks = vec![
            serde_json::json!({"type":20,"id":1,"header":"Public movies","playlistType":"playlist","imageJWLabel":"default_16x9","imageAspectRatio":56.25,"galleryPageNum":4,"galleryWrap":0,"playlist":[{"mediaid":"Public01","title":"Synthetic public film","contentType":"film","duration":90,"deeplink":"/films/Public01/public-film"}]}),
        ];
        if self.supplied {
            blocks.push(serde_json::json!({"type":20,"id":2,"header":"Supplied saved films","playlistType":"continueWatching","imageJWLabel":"default_16x9","imageAspectRatio":56.25,"galleryPageNum":4,"galleryWrap":0}));
        }
        let stream = format!(
            "baf:I[37,[],\"LanderStoryBlocks\"]\nace:{}\n",
            serde_json::json!(["$","$Lbaf",null,{"blocks":blocks}])
        );
        Ok(criterion_provider::Response {
            status: 200,
            content_type: "text/html".into(),
            body: format!(
                "<html><script>self.__next_f.push({})</script></html>",
                serde_json::json!([1, stream])
            )
            .into_bytes(),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    MyListIds,
    ContinueWatching,
    WatchList,
    NativeDetail(&'static str),
    AddWatchList,
    RemoveWatchList,
}
#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    retired: AtomicUsize,
    release: Notify,
}
struct Step {
    kind: Kind,
    gate: Option<Arc<Gate>>,
}
#[derive(Clone)]
struct Middleware {
    steps: Arc<Mutex<VecDeque<Step>>>,
    calls: Arc<Mutex<Vec<Kind>>>,
    bootstrap: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    trace: Arc<Mutex<Vec<(Kind, bool)>>>,
    violation: Arc<Mutex<Option<&'static str>>>,
    detail_response_id: Arc<Mutex<Option<&'static str>>>,
    watch_list_body: Arc<Mutex<Option<Vec<u8>>>>,
    native_kind: Arc<Mutex<&'static str>>,
    continue_body: Arc<Mutex<Option<Vec<u8>>>>,
    detail_body: Arc<Mutex<Option<Vec<u8>>>>,
    ids_result: Arc<Mutex<Result<Vec<u8>, criterion_account::Error>>>,
    write_result: Arc<Mutex<Result<Vec<u8>, criterion_account::Error>>>,
    write_panics: Arc<AtomicBool>,
}
impl Middleware {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: Arc::new(Mutex::new(steps.into())),
            calls: Arc::default(),
            bootstrap: Arc::default(),
            active: Arc::default(),
            maximum: Arc::default(),
            trace: Arc::default(),
            violation: Arc::default(),
            detail_response_id: Arc::default(),
            watch_list_body: Arc::default(),
            native_kind: Arc::new(Mutex::new("film")),
            continue_body: Arc::default(),
            detail_body: Arc::default(),
            ids_result: Arc::new(Mutex::new(Ok(
                br#"{"watchlist":["Listed01"],"positions":[]}"#.to_vec(),
            ))),
            write_result: Arc::new(Mutex::new(Ok(br#"{"sync":false}"#.to_vec()))),
            write_panics: Arc::default(),
        }
    }
    fn refuse(&self, reason: &'static str) -> criterion_account::Error {
        self.violation.lock().unwrap().get_or_insert(reason);
        criterion_account::Error::InvalidResponse
    }
}
struct Flight {
    script: Middleware,
    kind: Kind,
    gate: Option<Arc<Gate>>,
}
impl Drop for Flight {
    fn drop(&mut self) {
        self.script.active.fetch_sub(1, Ordering::SeqCst);
        self.script.trace.lock().unwrap().push((self.kind, false));
        if let Some(gate) = &self.gate {
            gate.retired.fetch_add(1, Ordering::SeqCst);
        }
    }
}
impl criterion_account::Transport for Middleware {
    async fn send(
        &self,
        request: Request,
    ) -> Result<criterion_account::Response, criterion_account::Error> {
        let kind = match request {
            Request::Bootstrap => {
                self.bootstrap.fetch_add(1, Ordering::SeqCst);
                return Ok(criterion_account::Response {status:200,body:SecretBody::new(br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec())});
            }
            Request::Subscriber {
                target,
                credentials,
            } => {
                if credentials.bootstrap().to_str().ok() != Some("Bearer synthetic-bootstrap")
                    || !credentials.bootstrap().is_sensitive()
                    || credentials.subscriber().to_str().ok() != Some("synthetic-subscriber")
                    || !credentials.subscriber().is_sensitive()
                {
                    return Err(self.refuse("subscriber read has incorrect header capabilities"));
                }
                match target {
                    SubscriberTarget::MyListIds(Region::Ca) => Kind::MyListIds,
                    SubscriberTarget::ContinueWatching(Region::Ca) => Kind::ContinueWatching,
                    SubscriberTarget::AddWatchList {
                        region: Region::Ca,
                        media_id,
                        content_type,
                    } if media_id.as_str() == "Listed01"
                        && matches!(
                            (*self.native_kind.lock().unwrap(), content_type),
                            ("film", criterion_account::WatchListContentType::Film)
                                | ("series", criterion_account::WatchListContentType::Series)
                                | (
                                    "collection",
                                    criterion_account::WatchListContentType::Collection
                                )
                                | ("episode", criterion_account::WatchListContentType::Episode)
                                | (
                                    "supplement",
                                    criterion_account::WatchListContentType::Supplement
                                )
                                | (
                                    "category",
                                    criterion_account::WatchListContentType::Category
                                )
                                | (
                                    "franchise",
                                    criterion_account::WatchListContentType::Franchise
                                )
                                | (
                                    "original",
                                    criterion_account::WatchListContentType::Original
                                )
                        ) =>
                    {
                        Kind::AddWatchList
                    }
                    SubscriberTarget::RemoveWatchList {
                        region: Region::Ca,
                        media_id,
                    } if media_id.as_str() == "Listed01" => Kind::RemoveWatchList,
                    SubscriberTarget::WatchList {
                        region: Region::Ca,
                        request,
                    } if request == WatchListRequest::default() => Kind::WatchList,
                    _ => {
                        return Err(
                            self.refuse("unexpected subscriber operation, region or request")
                        );
                    }
                }
            }
            Request::Detail {
                region,
                media_id,
                authorization,
            } => {
                if region != Region::Ca
                    || authorization.header().to_str().ok() != Some("Bearer synthetic-bootstrap")
                    || !authorization.header().is_sensitive()
                {
                    return Err(
                        self.refuse("anonymous Detail has incorrect regional bootstrap capability")
                    );
                }
                match media_id.as_str() {
                    "Listed01" => Kind::NativeDetail("Listed01"),
                    "Related1" => Kind::NativeDetail("Related1"),
                    _ => return Err(self.refuse("unexpected native selected ID")),
                }
            }
        };
        self.calls.lock().unwrap().push(kind);
        let Some(step) = self.steps.lock().unwrap().pop_front() else {
            return Err(self.refuse("unscripted account read"));
        };
        if step.kind != kind {
            return Err(self.refuse("account reads departed from source order"));
        }
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        self.trace.lock().unwrap().push((kind, true));
        let _flight = Flight {
            script: self.clone(),
            kind,
            gate: step.gate.clone(),
        };
        if let Some(gate) = &step.gate {
            gate.entered.store(true, Ordering::SeqCst);
            gate.release.notified().await;
        }
        let body=match kind {
            Kind::AddWatchList | Kind::RemoveWatchList=> {
                assert!(!self.write_panics.load(Ordering::SeqCst), "synthetic write transport panic");
                self.write_result.lock().unwrap().clone()?
            },
            Kind::MyListIds=>self.ids_result.lock().unwrap().clone()?,
            Kind::ContinueWatching=>self.continue_body.lock().unwrap().clone().unwrap_or_else(||br#"{"playlist":[{"mediaid":"Private1","title":"Synthetic saved film","contentType":"film","duration":90.5},{"mediaid":"Private2","title":"Synthetic completed film","contentType":"film"}],"positions":[{"media_id":"Private1","pos":98,"dur":100},{"media_id":"Private2","pos":120,"dur":100}]}"#.to_vec()),
            Kind::WatchList=>self.watch_list_body.lock().unwrap().clone().unwrap_or_else(||br#"{"paging":{"page_limit":50},"type_counts":{"film":1},"playlist":[{"mediaid":"Listed01","title":"Synthetic listed film","contentType":"film"}]}"#.to_vec()),
            Kind::NativeDetail(requested) => {
                if let Some(body) = self.detail_body.lock().unwrap().clone() {
                    return Ok(criterion_account::Response { status: 200, body: SecretBody::new(body) });
                }
                let id = self.detail_response_id.lock().unwrap().unwrap_or(requested);
                serde_json::to_vec(&serde_json::json!({
                    "contentType":*self.native_kind.lock().unwrap(), "mediaid":id, "title":format!("Native {id}"),
                    "duration":90.5, "description_long":"Native long description",
                    "description_medium":"Native medium description", "description":"Native short description",
                    "director":["Synthetic native director"], "starring":["Synthetic native actor"],
                    "country":["CA"], "language":["English"],
                    "playlists":[{"type":"GENERIC_PLAYLIST", "title":"Related", "playlistId":"synthetic-related", "key":"playlist_related",
                        "playlist":[{"contentType":"film", "mediaid":if requested=="Listed01" {"Related1"}else{"Listed01"},
                            "title":"Synthetic related film"}]}]
                })).unwrap()
            }
        };
        Ok(criterion_account::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
type App = Application<Public, Issuer, Clock, Middleware, Clock>;
struct Fixture {
    app: App,
    runtime: Runtime,
    clock: Clock,
    issuer: Issuer,
    script: Middleware,
    public_requests: Arc<Mutex<Vec<String>>>,
}
impl Fixture {
    fn new(supplied: bool, steps: Vec<Step>, token_age: u64) -> Self {
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
        runtime.block_on(session.start_link()).unwrap();
        clock.0.store(5, Ordering::SeqCst);
        runtime.block_on(session.poll_once()).unwrap();
        clock.0.store(token_age, Ordering::SeqCst);
        let script = Middleware::new(steps);
        let public_requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let surface = Surface {
            window: Size {
                width: 1920,
                height: 1080,
            },
            drawable: Size {
                width: 1920,
                height: 1080,
            },
        };
        let app = Application::with_parts(
            surface,
            Controller::with_clock(
                Catalog::with_transport(Public {
                    supplied,
                    requests: public_requests.clone(),
                }),
                runtime.handle(),
                clock.clone(),
            ),
            Authentication::with_session(session.clone(), clock.clone()),
            Accounts::from_parts(
                Arc::new(AccountClient::with_transport(script.clone())),
                session,
            ),
            Artwork::offline(),
        );
        Self {
            app,
            runtime,
            clock,
            issuer,
            script,
            public_requests,
        }
    }
    fn key(&mut self, scancode: u32, keycode: i32) {
        let surface = Surface {
            window: Size {
                width: 1920,
                height: 1080,
            },
            drawable: Size {
                width: 1920,
                height: 1080,
            },
        };
        for pressed in [true, false] {
            self.app.event(
                Event::Key(criterion_platform::KeyEvent {
                    scancode,
                    keycode,
                    pressed,
                    repeat: false,
                }),
                surface,
                &self.runtime,
                self.clock.now(),
            );
        }
    }
    fn open_list(&mut self) {
        for (scancode, keycode) in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
        ] {
            self.key(scancode, keycode);
        }
        assert_eq!(self.app.ui.page(), Page::MyList);
        self.wait(|fixture| {
            fixture
                .app
                .controller
                .view
                .with_view(fixture.app.authentication.view(), |view| {
                    view.cards
                        .iter()
                        .any(|card| card.title == "Synthetic listed film")
                })
        });
    }
    fn native_ready(&self, id: &str) -> bool {
        self.app
            .controller
            .view
            .with_view(self.app.authentication.view(), |view| {
                view.status == criterion_ui::LoadState::Ready
                    && view.detail.as_ref().is_some_and(|detail| {
                        detail
                            .card
                            .key
                            .media_id()
                            .is_some_and(|selected| selected.as_str() == id)
                    })
            })
    }
    fn pump(&mut self) {
        self.app.poll(&self.runtime, true);
        self.app.consume(&self.runtime, self.clock.now());
        self.runtime.block_on(tokio::task::yield_now());
    }
    #[track_caller]
    fn wait(&mut self, ready: impl Fn(&Self) -> bool) {
        for _ in 0..128 {
            self.pump();
            if ready(self) {
                return;
            }
        }
        panic!("synthetic application did not reach its expected boundary");
    }
    fn saved(&self) -> Vec<(String, Option<f32>)> {
        self.app
            .controller
            .view
            .with_view(LoginView::SignedIn, |view| {
                view.rails
                    .iter()
                    .filter(|rail| rail.title == "Supplied saved films")
                    .flat_map(|rail| {
                        rail.cards
                            .iter()
                            .map(|card| (card.title.to_owned(), card.saved_fraction))
                    })
                    .collect()
            })
    }
    fn public_visible(&self) -> bool {
        self.app
            .controller
            .view
            .with_view(LoginView::SignedIn, |view| {
                view.rails
                    .iter()
                    .flat_map(|rail| rail.cards.iter())
                    .any(|card| card.title == "Synthetic public film")
            })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.issuer.release.notify_one();
        self.app.background();
        let finished = self.app.finish(&self.runtime);
        if !std::thread::panicking() {
            assert!(finished);
            assert_eq!(*self.script.violation.lock().unwrap(), None);
            assert!(self.script.steps.lock().unwrap().is_empty());
            assert_eq!(self.script.active.load(Ordering::SeqCst), 0);
        }
    }
}
#[test]
fn supplied_home_hydrates_progress_and_logout_scrubs_current_and_retained_views() {
    let mut fixture = Fixture::new(
        true,
        vec![Step {
            kind: Kind::ContinueWatching,
            gate: None,
        }],
        5,
    );
    fixture.wait(|fixture| fixture.saved().len() == 2);
    assert!(fixture.public_visible());
    assert_eq!(
        fixture.saved(),
        [
            ("Synthetic saved film".into(), Some(0.98)),
            ("Synthetic completed film".into(), Some(1.0))
        ]
    );
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    fixture.pump();
    assert!(fixture.app.output.as_ref().is_some_and(|output|output.shapes.iter().any(|shape|
        matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains("Synthetic saved film")))),
        "privacy erasure is checked after actual private glyphs were painted");
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    fixture
        .app
        .command(Command::Navigate(Page::Search), fixture.runtime.handle());
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    if let Some(mut output) = fixture.app.take_output() {
        let shapes_erased = output.shapes.is_empty();
        output.textures_delta.clear();
        assert!(shapes_erased, "logout retires unpainted private shapes");
    }
    fixture
        .app
        .command(Command::Restore(Page::Home), fixture.runtime.handle());
    fixture.wait(|fixture| fixture.issuer.revokes.load(Ordering::SeqCst) == 1);
    assert!(fixture.saved().is_empty());
    assert!(fixture.public_visible());
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::ContinueWatching]
    );
}
#[test]
fn leaving_pending_gallery_joins_it_before_my_list_uses_same_worker() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: Some(gate.clone()),
            },
            Step {
                kind: Kind::WatchList,
                gate: None,
            },
        ],
        5,
    );
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    fixture
        .app
        .command(Command::Navigate(Page::MyList), fixture.runtime.handle());
    fixture.wait(|fixture| {
        fixture
            .app
            .controller
            .view
            .with_view(LoginView::SignedIn, |view| {
                view.cards
                    .iter()
                    .any(|card| card.title == "Synthetic listed film")
            })
    });
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.script.trace.lock().unwrap(),
        [
            (Kind::ContinueWatching, true),
            (Kind::ContinueWatching, false),
            (Kind::WatchList, true),
            (Kind::WatchList, false)
        ]
    );
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture
        .app
        .command(Command::Restore(Page::Home), fixture.runtime.handle());
    fixture.wait(|fixture| fixture.issuer.revokes.load(Ordering::SeqCst) == 1);
    assert!(fixture.saved().is_empty());
    assert!(fixture.public_visible());
}
#[test]
fn refresh_wait_does_not_extend_the_gallery_deadline_or_retry_failure() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        true,
        vec![Step {
            kind: Kind::ContinueWatching,
            gate: Some(gate.clone()),
        }],
        3580,
    );
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    fixture.issuer.hold_refresh.store(true, Ordering::SeqCst);
    fixture.clock.0.store(3605, Ordering::SeqCst);
    fixture.wait(|fixture| {
        fixture.issuer.tokens.load(Ordering::SeqCst) == 2
            && gate.retired.load(Ordering::SeqCst) == 1
    });
    fixture.clock.0.store(3639, Ordering::SeqCst);
    fixture.pump();
    assert!(!fixture.app.controller.continue_watching_expired());
    fixture.clock.0.store(3640, Ordering::SeqCst);
    fixture.pump();
    fixture.issuer.release.notify_one();
    fixture.wait(|fixture| fixture.app.authentication.access_ready());
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(fixture.saved().is_empty());
    assert!(fixture.public_visible());
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::ContinueWatching]
    );
}
#[test]
fn signed_in_home_without_supplied_gallery_never_fetches_account_data() {
    let mut fixture = Fixture::new(false, vec![], 5);
    fixture.wait(Fixture::public_visible);
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(fixture.script.calls.lock().unwrap().is_empty());
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 0);
}

#[test]
fn successful_predeadline_refresh_renews_the_same_gallery_demand_on_the_shared_worker() {
    let gate = Arc::new(Gate::default());
    let renewed = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: Some(gate.clone()),
            },
            Step {
                kind: Kind::ContinueWatching,
                gate: Some(renewed.clone()),
            },
        ],
        3580,
    );
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    let original = fixture
        .app
        .continue_watching_generation
        .as_ref()
        .unwrap()
        .read;
    fixture.issuer.hold_refresh.store(true, Ordering::SeqCst);
    fixture.clock.0.store(3605, Ordering::SeqCst);
    fixture.wait(|fixture| {
        fixture.issuer.tokens.load(Ordering::SeqCst) == 2
            && gate.retired.load(Ordering::SeqCst) == 1
    });
    fixture.clock.0.store(3620, Ordering::SeqCst);
    fixture.issuer.release.notify_one();
    fixture.wait(|_| renewed.entered.load(Ordering::SeqCst));
    assert_eq!(
        fixture
            .app
            .continue_watching_generation
            .as_ref()
            .unwrap()
            .read,
        original,
        "credential refresh preserves the original logical demand"
    );
    renewed.release.notify_one();
    fixture.wait(|fixture| fixture.saved().len() == 2);
    assert_eq!(fixture.saved()[0].1, Some(0.98));
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::ContinueWatching, Kind::ContinueWatching]
    );
    assert!(fixture.public_visible());
}

#[test]
fn same_token_relink_uses_a_new_root_epoch_and_never_restores_old_private_rows() {
    let first = Arc::new(Gate::default());
    let second = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: Some(first.clone()),
            },
            Step {
                kind: Kind::ContinueWatching,
                gate: Some(second.clone()),
            },
        ],
        5,
    );
    fixture.wait(|_| first.entered.load(Ordering::SeqCst));
    let departed = fixture
        .app
        .continue_watching_generation
        .as_ref()
        .unwrap()
        .read;
    first.release.notify_one();
    fixture.wait(|fixture| fixture.saved().len() == 2);
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| {
        fixture.issuer.revokes.load(Ordering::SeqCst) == 1
            && matches!(fixture.app.authentication.view(), LoginView::SignedOut)
    });
    assert!(fixture.saved().is_empty());
    fixture
        .app
        .command(Command::Authenticate, fixture.runtime.handle());
    fixture.wait(|fixture| {
        matches!(
            fixture.app.authentication.view(),
            LoginView::Awaiting { .. }
        )
    });
    fixture.clock.0.store(10, Ordering::SeqCst);
    fixture.wait(|fixture| fixture.app.authentication.signed_in());
    assert_eq!(
        fixture.issuer.tokens.load(Ordering::SeqCst),
        2,
        "fixture reissues identical token bytes"
    );
    fixture
        .app
        .command(Command::Navigate(Page::Home), fixture.runtime.handle());
    fixture.wait(|_| second.entered.load(Ordering::SeqCst));
    let current = fixture
        .app
        .continue_watching_generation
        .as_ref()
        .unwrap()
        .read;
    assert_ne!(current.epoch, departed.epoch);
    assert!(fixture.saved().is_empty());
    assert!(!fixture.app.controller.continue_watching_owns(&departed));
    second.release.notify_one();
    fixture.wait(|fixture| fixture.saved().len() == 2);
    assert!(fixture.public_visible());
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
}

#[path = "native_detail_tests.rs"]
mod native_detail_tests;
