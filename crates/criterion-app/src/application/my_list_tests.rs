// SPDX-License-Identifier: GPL-3.0-or-later
//! CPU native-input journeys through real Application/Session/Account owners.
//! All credentials, rows, cursors and responses here are synthetic fixtures.
use super::*;
use criterion_account::{
    AccountClient, Region, SecretBody, Target as AccountTarget, WatchListFilter, WatchListRequest,
};
use criterion_platform::{KeyEvent, Size};
use criterion_provider::{MediaId, PageCursor};
use criterion_session::{Configuration, Endpoint, Session};
use criterion_ui::{CatalogTail, Focus, LoginView, MyListGroup, Page, Target};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};
use tokio::sync::Notify;

pub(super) const NEXT: &str = "synthetic-observed%+/=";
const GROUPS: [(MyListGroup, WatchListFilter); 6] = [
    (MyListGroup::All, WatchListFilter::All),
    (MyListGroup::FilmsAndSeries, WatchListFilter::FilmSeries),
    (MyListGroup::Collections, WatchListFilter::Collection),
    (
        MyListGroup::OriginalsAndFranchises,
        WatchListFilter::OriginalFranchise,
    ),
    (MyListGroup::Supplements, WatchListFilter::Supplement),
    (MyListGroup::Categories, WatchListFilter::Category),
];

#[derive(Clone)]
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
}
impl Transport for Issuer {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        let body = match request.endpoint {
            Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
            Endpoint::Token => {
                self.tokens.fetch_add(1, Ordering::SeqCst);
                br#"{"access_token":"synthetic-same-token","refresh_token":"synthetic-refresh","expires_in":3600}"#.to_vec()
            }
            Endpoint::Revoke => {
                self.revokes.fetch_add(1, Ordering::SeqCst);
                b"{}".to_vec()
            }
        };
        Ok(criterion_session::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}

#[derive(Clone, Default)]
struct Public(Arc<Mutex<Vec<MediaId>>>);
impl RequestTransport for Public {
    async fn get(
        &self,
        request: criterion_provider::Request,
    ) -> Result<criterion_provider::Response, criterion_provider::Error> {
        let Some(id) = request.url.path().strip_prefix("/api/media/") else {
            return Err(criterion_provider::Error::Unavailable);
        };
        self.0.lock().unwrap().push(MediaId::new(id).unwrap());
        Ok(criterion_provider::Response {
            status: 200,
            content_type: "application/json".into(),
            body: format!(r#"{{"mediaid":"{id}","title":"Synthetic selected detail","contentType":"film","duration":90}}"#).into_bytes(),
        })
    }
}

#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    retired: AtomicUsize,
    release: Notify,
}
struct Step {
    request: WatchListRequest,
    response: Result<Vec<u8>, criterion_account::Error>,
    gate: Option<Arc<Gate>>,
}
impl Step {
    fn page(filter: WatchListFilter, cursor: Option<&str>, body: Vec<u8>) -> Self {
        Self {
            request: requested(filter, cursor),
            response: Ok(body),
            gate: None,
        }
    }
    fn held(mut self, gate: Arc<Gate>) -> Self {
        self.gate = Some(gate);
        self
    }
}
#[derive(Clone)]
struct Script {
    steps: Arc<Mutex<VecDeque<Step>>>,
    calls: Arc<Mutex<Vec<WatchListRequest>>>,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    bootstrap: Arc<AtomicUsize>,
    violation: Arc<Mutex<Option<&'static str>>>,
    trace: Arc<Mutex<Vec<Trace>>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trace {
    Started(WatchListFilter),
    Retired(WatchListFilter),
}
struct Flight {
    active: Arc<AtomicUsize>,
    gate: Option<Arc<Gate>>,
    filter: WatchListFilter,
    trace: Arc<Mutex<Vec<Trace>>>,
}
impl Drop for Flight {
    fn drop(&mut self) {
        if let Some(gate) = &self.gate {
            gate.retired.fetch_add(1, Ordering::SeqCst);
        }
        self.trace.lock().unwrap().push(Trace::Retired(self.filter));
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Script {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: Arc::new(Mutex::new(steps.into())),
            calls: Arc::default(),
            active: Arc::default(),
            maximum: Arc::default(),
            bootstrap: Arc::default(),
            violation: Arc::default(),
            trace: Arc::default(),
        }
    }
    fn calls(&self) -> Vec<WatchListRequest> {
        self.calls.lock().unwrap().clone()
    }
    fn violation(&self, message: &'static str) -> criterion_account::Error {
        self.violation.lock().unwrap().get_or_insert(message);
        criterion_account::Error::InvalidResponse
    }
}
impl criterion_account::Transport for Script {
    async fn send(
        &self,
        request: criterion_account::Request,
    ) -> Result<criterion_account::Response, criterion_account::Error> {
        let intent = match request.target {
            AccountTarget::Bootstrap => {
                if request.credentials.is_some() {
                    return Err(self.violation("bootstrap received subscriber credentials"));
                }
                self.bootstrap.fetch_add(1, Ordering::SeqCst);
                return Ok(criterion_account::Response {
                    status: 200,
                    body: SecretBody::new(br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()),
                });
            }
            AccountTarget::WatchList { region, request } => {
                // Record attempted work before any fixture check. Worker panics
                // become product errors, so request-count proof cannot rely on them.
                self.calls.lock().unwrap().push(request.clone());
                self.trace
                    .lock()
                    .unwrap()
                    .push(Trace::Started(request.filter));
                if region != Region::Us {
                    return Err(self.violation("unexpected native request region"));
                }
                request
            }
            _ => return Err(self.violation("unexpected read-only account operation")),
        };
        let Some(credentials) = request.credentials else {
            return Err(self.violation("native request omitted subscriber credentials"));
        };
        if credentials.subscriber().to_str().ok() != Some("synthetic-same-token")
            || !credentials.subscriber().is_sensitive()
        {
            return Err(self.violation("native subscriber credentials differ from the fixture"));
        }
        let Some(step) = self.steps.lock().unwrap().pop_front() else {
            return Err(self.violation("unexpected native request beyond the script"));
        };
        if intent != step.request {
            return Err(self.violation("native filter or observed cursor differs from the script"));
        }
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        let _flight = Flight {
            active: self.active.clone(),
            gate: step.gate.clone(),
            filter: intent.filter,
            trace: self.trace.clone(),
        };
        if active != 1 {
            return Err(self.violation("replacement overlapped the departed read"));
        }
        if let Some(gate) = &step.gate {
            gate.entered.store(true, Ordering::SeqCst);
            gate.release.notified().await;
        }
        Ok(criterion_account::Response {
            status: 200,
            body: SecretBody::new(step.response?),
        })
    }
}

fn requested(filter: WatchListFilter, cursor: Option<&str>) -> WatchListRequest {
    WatchListRequest {
        filter,
        cursor: cursor.map(|value| PageCursor::new(value).unwrap()),
    }
}
fn id(index: usize) -> MediaId {
    MediaId::new(&format!("M{index:07}")).unwrap()
}
fn row(index: usize, prefix: &str, kind: &str) -> String {
    format!(
        r#"{{"mediaid":"M{index:07}","title":"{prefix}{index}","contentType":"{kind}","duration":90.5,"release_date":"2000-02-29"}}"#
    )
}
fn rows(first: usize, count: usize, prefix: &str, kind: &str) -> Vec<String> {
    (first..first + count)
        .map(|index| row(index, prefix, kind))
        .collect()
}
fn page(rows: Vec<String>, next: Option<&str>) -> Vec<u8> {
    let next = next.map_or("null".to_owned(), |value| format!("\"{value}\""));
    // Returned limit/counts intentionally disagree with row count and the fixed
    // native request policy. Continuation comes only from the supplied cursor.
    format!(r#"{{"paging":{{"page_limit":99,"next_pagination_key":{next}}},"type_counts":{{"film":500,"series":20,"collection":10,"original":3,"franchise":2,"supplement":7,"category":1}},"playlist":[{}]}}"#, rows.join(",")).into_bytes()
}

type App = Application<Public, Issuer, Clock, Script>;
pub(super) struct Fixture {
    app: App,
    runtime: Runtime,
    clock: Clock,
    issuer: Issuer,
    script: Script,
    public: Public,
}
#[derive(Debug, PartialEq)]
pub(super) struct Snapshot {
    pub(super) status: LoadState,
    pub(super) selected: MyListGroup,
    pub(super) choices: Vec<MyListGroup>,
    pub(super) first: usize,
    pub(super) tail: CatalogTail,
    pub(super) cards: Vec<(Target, String, String, u32)>,
    pub(super) focus: Focus,
    pub(super) scroll: f32,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.background();
        let disposed = self.app.finish(&self.runtime);
        if !std::thread::panicking() {
            assert!(disposed);
            assert_eq!(
                *self.script.violation.lock().unwrap(),
                None,
                "synthetic transport fixture failed"
            );
            assert!(
                self.script.steps.lock().unwrap().is_empty(),
                "the complete native request script must be exercised"
            );
        }
    }
}
fn surface() -> Surface {
    Surface {
        window: Size {
            width: 1920,
            height: 1080,
        },
        drawable: Size {
            width: 1920,
            height: 1080,
        },
    }
}
impl Fixture {
    fn new(steps: Vec<Step>) -> Self {
        let mut fixture = Self::with_steps(steps);
        // Settle the real initial public job; never overwrite a still-issued
        // Home read with a private projection or manufacture list publication.
        fixture.wait(|fixture| {
            fixture
                .app
                .controller
                .view
                .with_view(LoginView::SignedIn, |view| {
                    view.status == LoadState::Offline
                })
        });
        fixture
    }
    fn with_steps(steps: Vec<Step>) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let clock = Clock(Arc::new(AtomicU64::new(0)));
        let issuer = Issuer::default();
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            issuer.clone(),
            clock.clone(),
        ));
        runtime.block_on(session.start_link()).unwrap();
        clock.0.store(5, Ordering::SeqCst);
        runtime.block_on(session.poll_once()).unwrap();
        let script = Script::new(steps);
        let public = Public::default();
        Self {
            app: Application::with_parts(
                surface(),
                Controller::new(Catalog::with_transport(public.clone()), runtime.handle()),
                Authentication::with_session(session.clone(), clock.clone()),
                Accounts::from_parts(
                    Arc::new(AccountClient::with_transport(script.clone())),
                    session,
                ),
                Artwork::offline(),
            ),
            runtime,
            clock,
            issuer,
            script,
            public,
        }
    }
    fn clear_output(&mut self) {
        if let Some(mut output) = self.app.take_output() {
            output.textures_delta.clear();
        }
    }
    fn pump(&mut self) {
        self.app.poll(&self.runtime, true);
        self.app.consume(&self.runtime, self.clock.now());
        self.clear_output();
        self.runtime.block_on(tokio::task::yield_now());
    }
    fn wait(&mut self, predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            self.pump();
            assert!(
                Instant::now() < deadline,
                "synthetic application journey did not settle"
            );
            if predicate(self) {
                return;
            }
        }
    }
    fn key(&mut self, scancode: u32, keycode: i32) {
        for pressed in [true, false] {
            self.app.event(
                Event::Key(KeyEvent {
                    scancode,
                    keycode,
                    pressed,
                    repeat: false,
                }),
                surface(),
                &self.runtime,
                self.clock.now(),
            );
            self.clear_output();
        }
    }
    fn up(&mut self) {
        self.key(82, 1_073_741_906);
    }
    fn down(&mut self) {
        self.key(81, 1_073_741_905);
    }
    fn select(&mut self) {
        self.key(40, 13);
    }
    fn back(&mut self) {
        self.key(41, 27);
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
        self.ready(MyListGroup::All);
    }
    pub(super) fn snapshot(&self) -> Snapshot {
        self.app
            .controller
            .view
            .with_view(self.app.authentication.view(), |view| {
                let group = view.my_list.expect("native My List projection");
                let window = view.catalog.expect("native My List global window");
                Snapshot {
                    status: view.status,
                    selected: group.selected,
                    choices: group.choices.iter().map(|choice| choice.group).collect(),
                    first: window.first,
                    tail: window.tail,
                    cards: view
                        .cards
                        .iter()
                        .map(|card| {
                            (
                                card.key.clone(),
                                card.title.to_owned(),
                                card.year.to_owned(),
                                card.duration_seconds,
                            )
                        })
                        .collect(),
                    focus: self.app.ui.focus(),
                    scroll: self.app.ui.scroll_y(),
                }
            })
    }
    fn ready(&mut self, group: MyListGroup) {
        self.wait(|fixture| {
            fixture
                .app
                .controller
                .view
                .with_view(fixture.app.authentication.view(), |view| {
                    view.status == LoadState::Ready
                        && view.my_list.is_some_and(|list| list.selected == group)
                })
        });
    }
    fn choose(&mut self, group: MyListGroup) {
        while let Focus::Card { row, .. } = self.app.ui.focus() {
            self.up();
            if row == 0 {
                break;
            }
        }
        for _ in 0..6 {
            let Focus::MyListGroup(current) = self.app.ui.focus() else {
                panic!("native group header focus");
            };
            if current == group {
                self.select();
                return;
            }
            let current_index = GROUPS
                .iter()
                .position(|(value, _)| *value == current)
                .unwrap();
            let target_index = GROUPS
                .iter()
                .position(|(value, _)| *value == group)
                .unwrap();
            if current_index < target_index {
                self.key(79, 1_073_741_903);
            } else {
                self.key(80, 1_073_741_904);
            }
        }
        panic!("native group header traversal exceeded six choices");
    }
    fn card_zero(&mut self) {
        if matches!(self.app.ui.focus(), Focus::MyListGroup(_)) {
            self.down();
        }
        assert_eq!(self.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    }
}

#[cfg(feature = "sdl")]
pub(super) fn rendered_fixture() -> Fixture {
    // Unlike CPU construction, no frame is consumed/dropped before the live
    // renderer can upload the context's initial font texture delta.
    Fixture::with_steps(vec![
        Step::page(
            WatchListFilter::All,
            None,
            page(rows(0, 50, "Synthetic all ", "film"), None),
        ),
        Step::page(
            WatchListFilter::FilmSeries,
            None,
            page(rows(100, 50, "Synthetic grouped ", "film"), Some(NEXT)),
        ),
        Step {
            request: requested(WatchListFilter::FilmSeries, Some(NEXT)),
            response: Err(criterion_account::Error::Unavailable),
            gate: None,
        },
        Step::page(
            WatchListFilter::FilmSeries,
            Some(NEXT),
            page(rows(150, 10, "Synthetic grouped ", "film"), None),
        ),
    ])
}

#[cfg(feature = "sdl")]
impl Fixture {
    pub(super) fn render_poll(&mut self) {
        self.app.poll(&self.runtime, true);
        self.runtime.block_on(tokio::task::yield_now());
    }
    pub(super) fn render_event(&mut self, event: Event, surface: Surface, now: Duration) {
        self.app.event(event, surface, &self.runtime, now);
    }
    pub(super) fn render_consume(&mut self, now: Duration) {
        self.app.consume(&self.runtime, now);
    }
    pub(super) fn render_context(&self) -> &egui::Context {
        self.app.context()
    }
    pub(super) fn render_output(&mut self) -> Option<egui::FullOutput> {
        self.app.take_output()
    }
    pub(super) fn render_page(&self) -> Page {
        self.app.ui.page()
    }
    pub(super) fn render_focus(&self) -> Focus {
        self.app.ui.focus()
    }
    pub(super) fn render_status(&self) -> LoadState {
        self.app
            .controller
            .view
            .with_view(self.app.authentication.view(), |view| view.status)
    }
    pub(super) fn render_signed_in(&self) -> bool {
        self.app.authentication.signed_in()
    }
    pub(super) fn render_signed_out(&self) -> bool {
        matches!(self.app.authentication.view(), LoginView::SignedOut)
    }
    pub(super) fn render_detail(&self) -> Option<Target> {
        self.app
            .controller
            .view
            .with_view(self.app.authentication.view(), |view| {
                view.detail.as_ref().map(|detail| detail.card.key.clone())
            })
    }
    pub(super) fn render_calls(&self) -> Vec<WatchListRequest> {
        self.script.calls()
    }
    pub(super) fn render_revokes(&self) -> usize {
        self.issuer.revokes.load(Ordering::SeqCst)
    }
    pub(super) fn render_has_private_projection(&self) -> bool {
        self.app
            .controller
            .view
            .with_view(self.app.authentication.view(), |view| {
                view.my_list.is_some() || !view.cards.is_empty() || view.detail.is_some()
            })
    }
}

#[test]
fn synthetic_fixture_cannot_hide_unexpected_native_reads_as_product_errors() {
    let mut fixture = Fixture::new(Vec::new());
    let script = fixture.script.clone();
    let mut product_error_settled = false;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for (scancode, keycode) in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
        ] {
            fixture.key(scancode, keycode);
        }
        fixture.wait(|fixture| {
            fixture
                .app
                .controller
                .view
                .with_view(LoginView::SignedIn, |view| {
                    matches!(view.status, LoadState::Error | LoadState::Offline)
                })
        });
        product_error_settled = true;
        drop(fixture);
    }));
    assert!(
        product_error_settled,
        "the owner must settle its error before fixture disposal"
    );
    assert!(
        outcome.is_err(),
        "fixture disposal must expose unexpected native work"
    );
    assert_eq!(script.calls(), vec![requested(WatchListFilter::All, None)]);
}

#[test]
fn native_my_list_six_groups_issue_their_native_filter_and_warm_all_reuses_rows() {
    let kinds = [
        "film",
        "series",
        "collection",
        "franchise",
        "supplement",
        "category",
    ];
    let steps = GROUPS
        .iter()
        .enumerate()
        .map(|(index, (_, filter))| {
            Step::page(
                *filter,
                None,
                page(rows(index * 100, 1, "Group", kinds[index]), None),
            )
        })
        .collect();
    let mut fixture = Fixture::new(steps);
    fixture.open_list();
    let original = fixture.snapshot().cards;
    for (index, (group, _)) in GROUPS.iter().enumerate().skip(1) {
        fixture.choose(*group);
        fixture.ready(*group);
        let view = fixture.snapshot();
        assert_eq!(view.cards[0].0, Target::Media(id(index * 100)));
        assert_eq!(view.choices, GROUPS.map(|(group, _)| group));
        assert_eq!(view.tail, CatalogTail::End);
    }
    fixture.choose(MyListGroup::All);
    fixture.ready(MyListGroup::All);
    assert_eq!(fixture.snapshot().cards, original);
    assert_eq!(
        fixture.script.calls(),
        GROUPS.map(|(_, filter)| requested(filter, None))
    );
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
}

#[test]
fn native_my_list_continuation_preserves_older_first_keys_and_uses_observed_cursor() {
    let mut successor = vec![r#"{"mediaid":"M0000049","title":"Changed duplicate","contentType":"series","duration":999.25,"release_date":"2015-01-01"}"#.into()];
    successor.extend(rows(50, 5, "First", "film"));
    let mut fixture = Fixture::new(vec![
        Step::page(
            WatchListFilter::All,
            None,
            page(rows(0, 50, "First", "film"), Some(NEXT)),
        ),
        Step::page(WatchListFilter::All, Some(NEXT), page(successor, None)),
    ]);
    fixture.open_list();
    fixture.card_zero();
    for _ in 0..12 {
        fixture.down();
    }
    let pending = fixture.snapshot();
    assert_eq!(pending.status, LoadState::Ready);
    assert_eq!(pending.cards.len(), 50);
    assert_eq!(pending.tail, CatalogTail::Loading);
    fixture.wait(|fixture| fixture.snapshot().cards.len() == 55);
    let view = fixture.snapshot();
    assert_eq!(
        view.cards
            .iter()
            .map(|card| card.0.clone())
            .collect::<Vec<_>>(),
        (0..55)
            .map(|index| Target::Media(id(index)))
            .collect::<Vec<_>>()
    );
    assert_eq!(view.cards[49].1, "First49");
    assert_eq!(view.cards[49].2, "2000");
    assert!(
        view.cards.iter().all(|card| card.3 == 0),
        "native Float32 units are not converted into public seconds"
    );
    assert_eq!(view.tail, CatalogTail::End);
    assert_eq!(
        fixture.script.calls(),
        vec![
            requested(WatchListFilter::All, None),
            requested(WatchListFilter::All, Some(NEXT))
        ]
    );
}

#[test]
fn native_my_list_replacement_retires_held_group_before_latest_group_publication() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(vec![
        Step::page(
            WatchListFilter::All,
            None,
            page(rows(0, 1, "Initial", "film"), None),
        ),
        Step::page(
            WatchListFilter::FilmSeries,
            None,
            page(rows(100, 1, "Stale", "series"), None),
        )
        .held(gate.clone()),
        Step::page(
            WatchListFilter::Collection,
            None,
            page(rows(200, 1, "Latest", "collection"), None),
        ),
    ]);
    fixture.open_list();
    fixture.choose(MyListGroup::FilmsAndSeries);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    fixture.choose(MyListGroup::Collections);
    fixture.wait(|fixture| {
        let view = fixture.snapshot();
        assert!(view.cards.iter().all(|card| !card.1.starts_with("Stale")));
        view.status == LoadState::Ready && view.selected == MyListGroup::Collections
    });
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    let trace = fixture.script.trace.lock().unwrap().clone();
    let retired = trace
        .iter()
        .position(|event| *event == Trace::Retired(WatchListFilter::FilmSeries))
        .unwrap();
    let replacement = trace
        .iter()
        .position(|event| *event == Trace::Started(WatchListFilter::Collection))
        .unwrap();
    assert!(
        retired < replacement,
        "held transport retirement must precede successor entry"
    );
    assert_eq!(fixture.snapshot().cards[0].1, "Latest200");
    gate.release.notify_one();
    for _ in 0..8 {
        fixture.pump();
        assert_eq!(fixture.snapshot().cards[0].1, "Latest200");
    }
    assert_eq!(
        fixture.script.calls(),
        vec![
            requested(WatchListFilter::All, None),
            requested(WatchListFilter::FilmSeries, None),
            requested(WatchListFilter::Collection, None)
        ]
    );
}

#[test]
fn native_my_list_detail_back_preserves_selected_group_window_and_has_no_redundant_read() {
    let mut fixture = Fixture::new(vec![
        Step::page(
            WatchListFilter::All,
            None,
            page(rows(0, 2, "All", "film"), None),
        ),
        Step::page(
            WatchListFilter::FilmSeries,
            None,
            page(rows(100, 50, "Selected", "film"), Some(NEXT)),
        ),
        Step::page(
            WatchListFilter::FilmSeries,
            Some(NEXT),
            page(rows(150, 50, "Selected", "film"), None),
        ),
    ]);
    fixture.open_list();
    fixture.choose(MyListGroup::FilmsAndSeries);
    fixture.ready(MyListGroup::FilmsAndSeries);
    fixture.card_zero();
    for _ in 0..12 {
        fixture.down();
    }
    fixture.wait(|fixture| fixture.snapshot().cards.len() == 100);
    let saved = fixture.snapshot();
    assert_eq!(saved.focus, Focus::Card { row: 12, column: 0 });
    let selected = saved.cards[48].0.clone();
    let issued = fixture.script.calls();
    fixture.select();
    assert_eq!(fixture.app.ui.page(), Page::Detail);
    fixture.wait(|fixture| {
        fixture
            .app
            .controller
            .view
            .with_view(LoginView::SignedIn, |view| {
                view.status == LoadState::Ready
                    && view
                        .detail
                        .as_ref()
                        .is_some_and(|detail| detail.card.key == &selected)
            })
    });
    fixture.back();
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    assert_eq!(fixture.snapshot(), saved);
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(fixture.snapshot(), saved);
    assert_eq!(fixture.script.calls(), issued);
    assert_eq!(*fixture.public.0.lock().unwrap(), vec![id(148)]);
}

#[test]
fn native_my_list_logout_same_token_relink_discards_groups_cursors_and_private_history() {
    let mut fixture = Fixture::new(vec![
        Step::page(
            WatchListFilter::All,
            None,
            page(rows(0, 1, "OldAll", "film"), None),
        ),
        Step::page(
            WatchListFilter::Collection,
            None,
            page(rows(100, 50, "OldGroup", "collection"), Some(NEXT)),
        ),
        Step::page(
            WatchListFilter::All,
            None,
            page(rows(300, 1, "NewEpoch", "film"), None),
        ),
    ]);
    fixture.open_list();
    fixture.choose(MyListGroup::Collections);
    fixture.ready(MyListGroup::Collections);
    fixture.card_zero();
    let old_epoch = fixture.app.account_epoch;
    for (scancode, keycode) in [
        (80, 1_073_741_904),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (40, 13),
    ] {
        fixture.key(scancode, keycode);
    }
    assert_eq!(fixture.app.ui.page(), Page::Login);
    fixture.select();
    fixture.wait(|fixture| matches!(fixture.app.authentication.view(), LoginView::SignedOut));
    fixture
        .app
        .controller
        .view
        .with_view(LoginView::SignedOut, |view| {
            assert!(view.cards.is_empty() && view.my_list.is_none())
        });
    fixture.select();
    fixture.wait(|fixture| {
        matches!(
            fixture.app.authentication.view(),
            LoginView::Awaiting { .. }
        )
    });
    fixture.clock.0.store(10, Ordering::SeqCst);
    fixture.wait(|fixture| matches!(fixture.app.authentication.view(), LoginView::SignedIn));
    assert_ne!(fixture.app.account_epoch, old_epoch);
    assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.issuer.revokes.load(Ordering::SeqCst), 1);
    fixture.back();
    assert_eq!(fixture.app.ui.page(), Page::Home);
    fixture
        .app
        .controller
        .view
        .with_view(LoginView::SignedIn, |view| {
            assert!(view.cards.is_empty() && view.my_list.is_none())
        });
    fixture.open_list();
    let view = fixture.snapshot();
    assert_eq!(view.cards[0].1, "NewEpoch300");
    assert_eq!(view.first, 0);
    assert_eq!(view.selected, MyListGroup::All);
    assert_eq!(
        fixture.script.calls(),
        vec![
            requested(WatchListFilter::All, None),
            requested(WatchListFilter::Collection, None),
            requested(WatchListFilter::All, None)
        ]
    );
}

#[test]
fn native_my_list_tail_error_keeps_committed_rows_until_explicit_observed_cursor_retry() {
    let mut fixture = Fixture::new(vec![
        Step::page(
            WatchListFilter::All,
            None,
            page(rows(0, 50, "Committed", "film"), Some(NEXT)),
        ),
        Step {
            request: requested(WatchListFilter::All, Some(NEXT)),
            response: Err(criterion_account::Error::Unavailable),
            gate: None,
        },
        Step::page(
            WatchListFilter::All,
            Some(NEXT),
            page(rows(50, 10, "Committed", "film"), None),
        ),
    ]);
    fixture.open_list();
    fixture.card_zero();
    let committed = fixture.snapshot().cards;
    for _ in 0..12 {
        fixture.down();
    }
    fixture.wait(|fixture| fixture.snapshot().tail == CatalogTail::Error);
    assert_eq!(fixture.snapshot().status, LoadState::Ready);
    assert_eq!(fixture.snapshot().cards, committed);
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(
        fixture.script.calls().len(),
        2,
        "tail failure cannot retry speculatively"
    );
    fixture.down();
    assert_eq!(fixture.app.ui.focus(), Focus::CatalogRetry);
    fixture.select();
    fixture.wait(|fixture| fixture.snapshot().cards.len() == 60);
    assert_eq!(fixture.snapshot().focus, Focus::Card { row: 13, column: 0 });
    assert_eq!(fixture.snapshot().tail, CatalogTail::End);
    assert_eq!(fixture.snapshot().cards[..50], committed);
    assert_eq!(
        fixture.script.calls(),
        vec![
            requested(WatchListFilter::All, None),
            requested(WatchListFilter::All, Some(NEXT)),
            requested(WatchListFilter::All, Some(NEXT))
        ]
    );
}

#[test]
fn native_my_list_fresh_rail_return_preserves_latest_group_anchor_without_another_read() {
    let mut fixture = Fixture::new(vec![
        Step::page(
            WatchListFilter::All,
            None,
            page(rows(0, 2, "Initial all ", "film"), None),
        ),
        Step::page(
            WatchListFilter::Collection,
            None,
            page(rows(100, 50, "Warm collection ", "collection"), None),
        ),
    ]);
    let original_home_focus = fixture.app.ui.focus();
    fixture.open_list();
    fixture.choose(MyListGroup::Collections);
    fixture.ready(MyListGroup::Collections);
    fixture.card_zero();
    for _ in 0..4 {
        fixture.down();
    }
    let saved = fixture.snapshot();
    assert_eq!(saved.focus, Focus::Card { row: 4, column: 0 });
    let issued = fixture.script.calls();
    // Leave through a fresh rail visit, rather than Detail/Back or switching
    // groups while the controller still owns the current shelf state.
    for (scancode, keycode) in [
        (80, 1_073_741_904),
        (82, 1_073_741_906),
        (82, 1_073_741_906),
        (40, 13),
    ] {
        fixture.key(scancode, keycode);
    }
    assert_eq!(fixture.app.ui.page(), Page::Home);
    fixture.wait(|fixture| {
        fixture
            .app
            .controller
            .view
            .with_view(LoginView::SignedIn, |view| {
                view.status == LoadState::Offline
            })
    });
    let rail_home_focus = fixture.app.ui.focus();
    for (scancode, keycode) in [
        (80, 1_073_741_904),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (40, 13),
    ] {
        fixture.key(scancode, keycode);
    }
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    fixture.wait(|fixture| {
        fixture
            .app
            .controller
            .view
            .with_view(LoginView::SignedIn, |view| {
                view.status != LoadState::Loading
            })
    });
    let restored = fixture.snapshot();
    assert_eq!(
        restored.selected, saved.selected,
        "fresh rail entry must preserve the latest admitted group"
    );
    assert_eq!(restored, saved);
    assert_eq!(
        fixture.script.calls(),
        issued,
        "warm rail entry cannot issue another native read"
    );
    fixture.back();
    assert_eq!(fixture.app.ui.page(), Page::Home);
    assert_eq!(fixture.app.ui.focus(), rail_home_focus);
    fixture.back();
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    assert_eq!(
        fixture.snapshot(),
        saved,
        "earlier Back origin must remain exact"
    );
    fixture.back();
    assert_eq!(fixture.app.ui.page(), Page::Home);
    assert_eq!(fixture.app.ui.focus(), original_home_focus);
    assert_eq!(fixture.script.calls(), issued);
}
