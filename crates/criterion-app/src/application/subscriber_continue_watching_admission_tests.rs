// SPDX-License-Identifier: GPL-3.0-or-later
//! Root-only real Continue Watching admission. Private observations stay in memory.
use super::*;
use criterion_account::{Region, Response};
use criterion_provider::MediaId;
use criterion_ui::{Focus, Target};

const LEFT: (u32, i32) = (80, 1_073_741_904);
const RIGHT: (u32, i32) = (79, 1_073_741_903);
const DOWN: (u32, i32) = (81, 1_073_741_905);
const SELECT: (u32, i32) = (40, 13);
const BACK: (u32, i32) = (41, 27);

// IDs/fractions are the only private oracle values retained, never Debug/logged.
struct ExpectedRow {
    id: MediaId,
    fraction: Option<f32>,
}

#[derive(Default)]
pub(super) struct ContinueWatchingTrace {
    armed: bool,
    bootstrap_started: bool,
    bootstrap_returned: Option<bool>,
    bootstrap_status: Option<u16>,
    bootstrap_error: Option<&'static str>,
    bootstrap_phase: Option<&'static str>,
    region: Option<Region>,
    read_started: bool,
    read_returned: Option<bool>,
    read_status: Option<u16>,
    read_error: Option<&'static str>,
    read_phase: Option<&'static str>,
    expected: Option<Vec<ExpectedRow>>,
    retired: bool,
    admitted: bool,
}
impl ContinueWatchingTrace {
    pub(super) fn admit_bootstrap(&mut self) -> bool {
        if !self.armed || self.bootstrap_started || self.retired {
            return false;
        }
        self.bootstrap_started = true;
        true
    }
    pub(super) fn admit_read(&mut self, region: Region) -> bool {
        if !self.armed
            || self.retired
            || self.read_started
            || self.bootstrap_returned != Some(true)
            || self.region != Some(region)
        {
            return false;
        }
        self.read_started = true;
        true
    }
    pub(super) fn complete_bootstrap(
        &mut self,
        result: &Result<Response, criterion_account::Error>,
    ) {
        self.bootstrap_returned = Some(false);
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                let (category, status) = coarse_account_error(error);
                self.bootstrap_status = status;
                self.bootstrap_error = Some(category);
                self.bootstrap_phase = Some(if status.is_some() {
                    "Continue Watching bootstrap HTTP status"
                } else {
                    "Continue Watching bootstrap transport error"
                });
                return;
            }
        };
        self.bootstrap_status = Some(response.status);
        if response.status != 200 {
            self.bootstrap_phase = Some("Continue Watching bootstrap HTTP status");
            return;
        }
        if response.body.expose().len() > 65_536 {
            self.bootstrap_phase = Some("Continue Watching bootstrap body bound");
            return;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(response.body.expose()) else {
            self.bootstrap_phase = Some("Continue Watching bootstrap JSON");
            return;
        };
        self.region = match value.get("country").and_then(serde_json::Value::as_str) {
            Some("US" | "us") => Some(Region::Us),
            Some("CA" | "ca") => Some(Region::Ca),
            _ => None,
        };
        self.bootstrap_returned = Some(self.region.is_some());
        if self.region.is_none() {
            self.bootstrap_phase = Some("Continue Watching bootstrap region");
        }
    }
    pub(super) fn complete_read(&mut self, result: &Result<Response, criterion_account::Error>) {
        self.read_returned = Some(false);
        match result {
            Err(error) => {
                let (category, status) = coarse_account_error(error);
                self.read_status = status;
                self.read_error = Some(category);
                self.read_phase = Some(if status.is_some() {
                    "Continue Watching read HTTP status"
                } else {
                    "Continue Watching read transport error"
                });
            }
            Ok(response) => {
                self.read_status = Some(response.status);
                if response.status != 200 {
                    self.read_phase = Some("Continue Watching read HTTP status");
                    return;
                }
                match expected_rows(response) {
                    Ok(rows) => {
                        self.expected = Some(rows);
                        self.read_returned = Some(true);
                    }
                    // Every expected_rows failure origin is a fixed source
                    // literal; parser errors and external values never cross.
                    Err(phase) => self.read_phase = Some(phase),
                }
            }
        }
    }
    fn completed(&self) -> bool {
        self.armed
            && self.bootstrap_started
            && self.bootstrap_returned == Some(true)
            && self.read_started
            && self.read_returned == Some(true)
            && self.expected.is_some()
    }
    fn retire(&mut self) {
        self.admitted |= self.completed();
        self.armed = false;
        self.region = None;
        self.expected = None;
        self.retired = true;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct HomeScope {
    epoch: u64,
    visit: u64,
}

// Only coarse ownership crosses a frame; private cards are borrowed during inspection.
#[derive(Clone, Copy)]
struct HomeAuthority {
    scope: Option<HomeScope>,
    home: bool,
    foreground: bool,
    access_ready: bool,
    supplied_slot: bool,
    positions_epoch: Option<u64>,
    account_idle: bool,
}
impl HomeAuthority {
    fn can_arm(self) -> bool {
        self.scope.is_some()
            && self.home
            && self.foreground
            && self.access_ready
            && self.supplied_slot
            && self.positions_epoch.is_none()
            && self.account_idle
    }
    fn current(self, scope: HomeScope) -> bool {
        self.scope == Some(scope) && self.home && self.foreground && self.access_ready
    }
    fn admitted(self, scope: HomeScope) -> bool {
        self.current(scope)
            && self.account_idle
            && self.positions_epoch == Some(scope.epoch)
            && !self.supplied_slot
    }
}
fn authority(app: &SubscriberApp) -> HomeAuthority {
    HomeAuthority {
        scope: app
            .account_epoch
            .zip(app.controller.membership_visit())
            .map(|(epoch, visit)| HomeScope { epoch, visit }),
        home: app.ui.page() == Page::Home,
        foreground: app.active && !app.exiting,
        access_ready: app.authentication.access_ready() && app.account_signed_in,
        supplied_slot: app
            .account_epoch
            .is_some_and(|epoch| app.controller.view.continue_watching_needs_read(epoch)),
        positions_epoch: app.positions.as_ref().map(|positions| positions.epoch),
        account_idle: app.shelf_pending.is_none()
            && app.shelf_generation.is_none()
            && app.continue_watching_pending.is_none()
            && app.continue_watching_generation.is_none()
            && app.native_detail_pending.is_none()
            && app.native_detail_generation.is_none()
            && app.list_membership.is_none(),
    }
}

pub(super) struct ContinueWatchingJourney {
    trace: SharedTrace,
    original_slot: bool,
    returning: bool,
    scope: Option<HomeScope>,
    read: Option<crate::controller::ContinueWatchingRead>,
    generation: Option<u64>,
    selected: Option<Focus>,
    pub(super) painted: bool,
    empty: bool,
}
impl ContinueWatchingJourney {
    pub(super) fn new(trace: SharedTrace) -> Self {
        Self {
            trace,
            original_slot: false,
            returning: false,
            scope: None,
            read: None,
            generation: None,
            selected: None,
            painted: false,
            empty: false,
        }
    }
    pub(super) fn observe(&mut self, app: &SubscriberApp) -> Result<(), &'static str> {
        if !self.returning {
            return Ok(());
        }
        let current = authority(app);
        if self.scope.is_none() && current.home {
            if !self.original_slot || !current.can_arm() {
                return Err("Continue Watching scope unadmitted: no current supplied Home slot");
            }
            let mut trace = self.trace.lock().map_err(|_| "private request trace")?;
            let watching = trace
                .continue_watching
                .as_mut()
                .ok_or("Continue Watching mode trace")?;
            if watching.armed
                || watching.bootstrap_started
                || watching.read_started
                || watching.retired
            {
                return Err("Continue Watching one foreground arm");
            }
            watching.armed = true;
            self.scope = current.scope;
        }
        let Some(scope) = self.scope else {
            return Ok(());
        };
        if !current.current(scope) {
            return Err("Continue Watching current Home authority");
        }
        if let Some(read) = app.continue_watching_pending.or_else(|| {
            app.continue_watching_generation
                .as_ref()
                .map(|issued| issued.read)
        }) {
            if read.epoch != scope.epoch
                || !app.controller.continue_watching_owns(&read)
                || self.read.is_some_and(|previous| previous != read)
            {
                return Err("Continue Watching exact foreground demand");
            }
            self.read = Some(read);
        }
        if let Some(issued) = &app.continue_watching_generation {
            if issued.generation == 0
                || self
                    .generation
                    .is_some_and(|previous| previous != issued.generation)
            {
                return Err("Continue Watching exact account read generation");
            }
            self.generation = Some(issued.generation);
        }
        Ok(())
    }
    pub(super) fn prepare(
        &mut self,
        app: &SubscriberApp,
        output: &egui::FullOutput,
    ) -> Result<bool, &'static str> {
        if !self.returning {
            return Ok(false);
        }
        let Some(scope) = self.scope else {
            return Ok(false);
        };
        if !authority(app).current(scope) {
            return Err("Continue Watching current paint authority");
        }
        let trace = self.trace.lock().map_err(|_| "private request trace")?;
        let watching = trace
            .continue_watching
            .as_ref()
            .ok_or("Continue Watching mode trace")?;
        if let Some(phase) = failure_phase(&trace) {
            return Err(phase);
        }
        let Some(expected) = &watching.expected else {
            return Ok(false);
        };
        if !authority(app).admitted(scope) {
            return Ok(false);
        }
        let selection = app
            .controller
            .view
            .with_view(app.authentication.view(), |view| {
                inspect_frame(&view, expected, app.ui.focus(), app.ui.scroll_y(), output)
            })?;
        if expected.is_empty() {
            self.empty = true;
            return Ok(true);
        }
        let Some(selected) = selection else {
            return Ok(false);
        };
        if self.selected.is_some_and(|previous| previous != selected) {
            return Err("Continue Watching stable selected native card");
        }
        self.selected = Some(selected);
        Ok(true)
    }
    pub(super) fn presented(&mut self, app: &SubscriberApp) -> Result<(), &'static str> {
        let scope = self.scope.ok_or("Continue Watching presented scope")?;
        if !authority(app).admitted(scope) || (!self.empty && self.selected != Some(app.ui.focus()))
        {
            return Err("Continue Watching current authority after paint and present");
        }
        self.painted = true;
        Ok(())
    }
    pub(super) fn retire(&mut self) -> Result<(), &'static str> {
        self.returning = false;
        self.scope = None;
        self.read = None;
        self.generation = None;
        self.selected = None;
        self.trace
            .lock()
            .map_err(|_| "private request trace")?
            .continue_watching
            .as_mut()
            .ok_or("Continue Watching mode trace")?
            .retire();
        Ok(())
    }
}

fn failure_phase(trace: &ReadTrace) -> Option<&'static str> {
    let watching = trace.continue_watching.as_ref()?;
    if trace.refused {
        Some("Continue Watching request refused before transport")
    } else {
        watching.bootstrap_phase.or(watching.read_phase)
    }
}

// Match categories only, never format the external Error or its nested value.
fn coarse_account_error(error: &criterion_account::Error) -> (&'static str, Option<u16>) {
    use criterion_account::Error;
    let category = match error {
        Error::Unavailable => "unavailable",
        Error::InvalidRequest => "invalid_request",
        Error::InvalidResponse => "invalid_response",
        Error::HttpStatus(status) => return ("http_status", Some(*status)),
        Error::ResponseTooLarge => "response_too_large",
        Error::Deadline => "deadline",
        Error::Busy => "busy",
        Error::Stale => "stale",
        Error::Disposed => "disposed",
        Error::NoBootstrap => "no_bootstrap",
        Error::UnsupportedRegion => "unsupported_region",
        Error::ReconciliationRequired => "reconciliation_required",
        Error::Session(_) => "session",
    };
    (category, None)
}

pub(super) fn report_diagnostic(
    trace: &SharedTrace,
    attempt_ok: bool,
    cleanup_acknowledged: bool,
    text_cleanup_ok: bool,
    observation_retired: bool,
) {
    let Ok(trace) = trace.lock() else {
        println!("subscriber admission: Continue Watching diagnostics trace unavailable");
        return;
    };
    let Some(watching) = &trace.continue_watching else {
        return;
    };
    // This single line survives trace erasure only after application/worker
    // disposal. It contains booleans, known HTTP codes and fixed categories.
    println!(
        "subscriber admission: Continue Watching diagnostics refused={} bootstrap_started={} bootstrap_result_seen={} bootstrap_oracle_admitted={} bootstrap_status={} bootstrap_error={} bootstrap_phase={} read_started={} read_result_seen={} read_oracle_admitted={} read_status={} read_error={} read_phase={} attempt_ok={} cleanup_acknowledged={} text_cleanup_ok={} observation_retired={}",
        trace.refused,
        watching.bootstrap_started,
        watching.bootstrap_returned.is_some(),
        watching.bootstrap_returned == Some(true),
        watching
            .bootstrap_status
            .map_or_else(|| "none".into(), |status| status.to_string()),
        watching.bootstrap_error.unwrap_or("none"),
        watching.bootstrap_phase.unwrap_or("none"),
        watching.read_started,
        watching.read_returned.is_some(),
        watching.read_returned == Some(true),
        watching
            .read_status
            .map_or_else(|| "none".into(), |status| status.to_string()),
        watching.read_error.unwrap_or("none"),
        watching.read_phase.unwrap_or("none"),
        attempt_ok,
        cleanup_acknowledged,
        text_cleanup_ok,
        observation_retired,
    );
}

fn inspect_frame(
    view: &criterion_ui::ViewData<'_>,
    expected: &[ExpectedRow],
    focus: Focus,
    scroll: f32,
    output: &egui::FullOutput,
) -> Result<Option<Focus>, &'static str> {
    if !matches!(view.status, LoadState::Ready | LoadState::Empty) || output.shapes.is_empty() {
        return Err("Continue Watching settled fresh Home frame");
    }
    let mut native_rails = 0;
    for rail in view.rails {
        if !rail
            .cards
            .iter()
            .any(|card| matches!(card.key, Target::Native(_)))
        {
            continue;
        }
        native_rails += 1;
        if rail.cards.len() != expected.len()
            || rail.cards.iter().zip(expected).any(|(card, row)| {
                !matches!(card.key,Target::Native(id) if *id == row.id)
                    || card.saved_fraction != row.fraction
            })
        {
            return Err("Continue Watching exact supplied native order and fractions");
        }
    }
    if expected.is_empty() {
        if native_rails != 0
            || view
                .rails
                .iter()
                .flat_map(|rail| rail.cards)
                .any(|card| card.saved_fraction.is_some())
        {
            return Err("Continue Watching honest empty private projection");
        }
        return Ok(None);
    }
    if native_rails == 0 {
        return Err("Continue Watching supplied native rail missing");
    }
    let Focus::Card { row, column: 0 } = focus else {
        return Ok(None);
    };
    let Some(card) = view.rails.get(row).and_then(|rail| rail.cards.first()) else {
        return Ok(None);
    };
    if !matches!(card.key,Target::Native(id) if *id == expected[0].id) {
        return Ok(None);
    }
    let y = 896.0 + row as f32 * 397.0 - scroll;
    let image = egui::Rect::from_min_size(egui::pos2(150.0, y + 60.0), egui::vec2(378.0, 212.625));
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    if !screen.contains_rect(image.expand(8.0)) {
        return Err("Continue Watching selected card viewport");
    }
    let caption: String = if card.title.chars().count() > 28 {
        card.title
            .chars()
            .take(27)
            .chain(std::iter::once('…'))
            .collect()
    } else {
        card.title.to_owned()
    };
    let caption: String = caption
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let caption_area = egui::Rect::from_min_size(
        egui::pos2(image.left(), image.bottom() + 8.0),
        egui::vec2(346.0, 45.0),
    );
    if !output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text)
        if text.galley.job.text == caption && caption_area.contains_rect(shape.shape.visual_bounding_rect())
            && shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect())))
        || !output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(rect)
            if rect.rect == image.expand(8.0) && rect.stroke.width == 8.0
                && rect.stroke.color == egui::Color32::from_rgb(181,138,22))) {
        // Application renders before applying the latest input batch. A fresh
        // next frame, rather than a departed output, must witness the selection.
        return Ok(None);
    }
    let track = egui::Rect::from_min_max(
        egui::pos2(image.left(), image.bottom() - 3.0),
        image.right_bottom(),
    );
    let white = egui::Color32::from_rgb(239, 239, 239);
    if let Some(fraction) = expected[0].fraction.filter(|fraction| *fraction > 0.0) {
        let fill = egui::Rect::from_min_size(track.min, egui::vec2(378.0 * fraction, 3.0));
        if !output
            .shapes
            .iter()
            .any(|shape| matches!(&shape.shape,egui::Shape::Rect(rect) if rect.rect==track))
            || !output.shapes.iter().any(|shape| {
                matches!(&shape.shape,egui::Shape::Rect(rect)
                if rect.rect==fill && rect.fill==white && shape.clip_rect.contains_rect(fill))
            })
        {
            return Err("Continue Watching supplied selected fraction painted");
        }
    } else if output.shapes.iter().any(|shape| {
        matches!(&shape.shape,egui::Shape::Rect(rect)
        if rect.rect.height()==3.0 && track.contains_rect(rect.rect) && rect.fill==white)
    }) {
        return Err("Continue Watching absent or zero fraction painted honestly");
    }
    Ok(Some(focus))
}

pub(super) fn verify_trace(trace: &SharedTrace) -> Result<(), &'static str> {
    let trace = trace.lock().map_err(|_| "private request trace")?;
    let watching = trace
        .continue_watching
        .as_ref()
        .ok_or("Continue Watching mode trace")?;
    if trace.refused
        || !trace.reads.is_empty()
        || trace.membership.is_some()
        || !(watching.completed()
            || (watching.retired
                && watching.admitted
                && !watching.armed
                && watching.region.is_none()
                && watching.expected.is_none()))
    {
        return Err("Continue Watching exact one-read census and retirement");
    }
    Ok(())
}
pub(super) fn verify_cleanup(app: &SubscriberApp) -> Result<(), &'static str> {
    if app.positions.is_some()
        || app.continue_watching_pending.is_some()
        || app.continue_watching_generation.is_some()
        || app.list_membership.is_some()
        || app.native_detail_pending.is_some()
        || app.native_detail_generation.is_some()
    {
        return Err("Continue Watching private owner disposal");
    }
    Ok(())
}

pub(super) fn settle_original_home(
    app: &mut SubscriberApp,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    journey: &mut Journey,
) -> Result<(), &'static str> {
    let deadline = journey.stage(Duration::from_secs(45));
    loop {
        frame(app, window, painter, runtime, journey)?;
        if app.ui.page() != Page::Home || app.authentication.signed_in() {
            return Err("Continue Watching original anonymous Home");
        }
        let status = app
            .controller
            .view
            .with_view(app.authentication.view(), |view| view.status);
        if matches!(status, LoadState::Ready | LoadState::Empty) {
            break;
        }
        if matches!(status, LoadState::Error | LoadState::Offline) || Instant::now() >= deadline {
            return Err("Continue Watching original Home admission");
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    if app.controller.view.estimated_bytes() > 8 * 1024 * 1024 {
        return Err("Continue Watching original Home exceeds warm history bound");
    }
    let epoch = app
        .account_epoch
        .ok_or("Continue Watching original epoch")?;
    journey
        .continue_watching
        .as_mut()
        .ok_or("Continue Watching observer")?
        .original_slot = app.controller.view.continue_watching_needs_read(epoch);
    Ok(())
}

pub(super) fn admit_home(
    app: &mut SubscriberApp,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    journey: &mut Journey,
) -> Result<(), &'static str> {
    let deadline = journey.stage(Duration::from_secs(75));
    journey
        .continue_watching
        .as_mut()
        .ok_or("Continue Watching observer")?
        .returning = true;
    key(app, window, painter, runtime, journey, BACK)?;
    if app.ui.page() != Page::Home {
        return Err("Continue Watching exact Home Back");
    }
    key(app, window, painter, runtime, journey, RIGHT)?;
    loop {
        frame(app, window, painter, runtime, journey)?;
        if authority(app)
            .scope
            .is_some_and(|scope| authority(app).admitted(scope))
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err("Continue Watching current read deadline");
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let native_row = app
        .controller
        .view
        .with_view(app.authentication.view(), |view| {
            view.rails.iter().position(|rail| {
                rail.cards
                    .first()
                    .is_some_and(|card| matches!(card.key, Target::Native(_)))
            })
        });
    if let Some(row) = native_row {
        let wanted = Focus::Card { row, column: 0 };
        for _ in 0..128 {
            if app.ui.focus() == wanted {
                break;
            }
            let before = app.ui.focus();
            key(app, window, painter, runtime, journey, DOWN)?;
            if app.ui.focus() == before || Instant::now() >= deadline {
                return Err("Continue Watching supplied card navigation unadmitted");
            }
        }
        if app.ui.focus() != wanted {
            return Err("Continue Watching bounded supplied card navigation");
        }
    }
    while !journey
        .continue_watching
        .as_ref()
        .is_some_and(|watching| watching.painted)
    {
        frame(app, window, painter, runtime, journey)?;
        if Instant::now() >= deadline {
            return Err("Continue Watching fresh current paint deadline");
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    for _ in 0..16 {
        frame(app, window, painter, runtime, journey)?;
        if Instant::now() >= deadline {
            return Err("Continue Watching steady frame deadline");
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let watching = journey
        .continue_watching
        .as_mut()
        .ok_or("Continue Watching observer")?;
    verify_trace(&watching.trace)?;
    println!(
        "subscriber admission: Continue Watching current Home painted; bounded steady frames admitted"
    );
    watching.retire()
}

pub(super) fn return_to_account(
    app: &mut SubscriberApp,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    journey: &mut Journey,
) -> Result<(), &'static str> {
    for remote in [LEFT, DOWN, DOWN, DOWN, DOWN, SELECT] {
        key(app, window, painter, runtime, journey, remote)?;
    }
    Ok(())
}

#[test]
#[ignore = "actual subscriber/provider and serialized SDL/GLES; private external mount; root executor only"]
fn native_subscriber_continue_watching_home_and_logout_end_to_end() {
    subscriber_test(AdmissionMode::ContinueWatching);
}

fn synthetic_saved_projection() -> crate::presentation::Presentation {
    use criterion_account::{ContinueWatching, MediaKind, MediaSummary, Position};
    let id = MediaId::new("Synth001").unwrap();
    let data = ContinueWatching {
        playlist: vec![MediaSummary {
            id: id.clone(),
            title: "Synthetic saved Film".into(),
            kind: MediaKind::Film,
            duration: None,
            release_date: None,
            series_id: None,
            series_title: None,
        }],
        positions: vec![Position {
            media_id: id,
            pos: 25,
            dur: 100,
            commentary_track: None,
            series_id: None,
            series_title: None,
        }],
    };
    synthetic_projection(data)
}
fn synthetic_projection(
    data: criterion_account::ContinueWatching,
) -> crate::presentation::Presentation {
    use criterion_provider::{
        DiscoveryBlock, DiscoveryPage, GalleryLayout, GalleryPresentation, ImageLabel, RailSource,
    };
    let (shelf, _) = crate::continue_watching::ContinueWatchingShelf::from_admitted(data).unwrap();
    let mut view = crate::presentation::Presentation::discovery(DiscoveryPage {
        blocks: vec![DiscoveryBlock::Rail {
            id: 1,
            header: Some("Supplied synthetic saved rail".into()),
            cta: None,
            target: None,
            opens_new_window: false,
            source: RailSource::ContinueWatching,
            cards: vec![],
            image_label: ImageLabel::Landscape,
            presentation: GalleryPresentation {
                aspect_ratio_percent: 56.25,
                cards_per_view: 4,
                layout: GalleryLayout::Rail,
                variant: 0,
            },
        }],
    });
    view.mark_continue_watching_pending(7);
    assert!(view.admit_continue_watching(7, &shelf) == Ok(true));
    view
}

#[test]
fn current_selection_waits_for_a_fresh_shape_frame_after_actual_ui_input() {
    let view = synthetic_saved_projection();
    let expected = [ExpectedRow {
        id: MediaId::new("Synth001").unwrap(),
        fraction: Some(0.25),
    }];
    let mut ui = criterion_ui::AppUi::new();
    let mut output = view.with_view(LoginView::SignedIn, |data| {
        ui.render(egui::RawInput::default(), &data).output
    });
    view.with_view(LoginView::SignedIn, |data| {
        ui.handle(criterion_ui::Action::Down, &data)
    });
    let result = view.with_view(LoginView::SignedIn, |data| {
        inspect_frame(&data, &expected, ui.focus(), ui.scroll_y(), &output)
    });
    output.textures_delta.clear();
    assert!(matches!(result, Ok(None)));
}

#[test]
fn fresh_native_ui_frame_proves_exact_supplied_selected_fraction_without_pixels() {
    let view = synthetic_saved_projection();
    let expected = [ExpectedRow {
        id: MediaId::new("Synth001").unwrap(),
        fraction: Some(0.25),
    }];
    let mut ui = criterion_ui::AppUi::new();
    view.with_view(LoginView::SignedIn, |data| {
        ui.handle(criterion_ui::Action::Down, &data)
    });
    let mut output = view.with_view(LoginView::SignedIn, |data| {
        ui.render(egui::RawInput::default(), &data).output
    });
    let result = view.with_view(LoginView::SignedIn, |data| {
        inspect_frame(&data, &expected, ui.focus(), ui.scroll_y(), &output)
    });
    assert!(matches!(
        result,
        Ok(Some(Focus::Card { row: 0, column: 0 }))
    ));
    let wrong = [ExpectedRow {
        id: MediaId::new("Synth001").unwrap(),
        fraction: Some(0.5),
    }];
    let rejected = view.with_view(LoginView::SignedIn, |data| {
        inspect_frame(&data, &wrong, ui.focus(), ui.scroll_y(), &output)
    });
    output.textures_delta.clear();
    assert!(rejected.is_err());
}

#[test]
fn home_arm_and_post_present_admission_require_current_epoch_visit_and_foreground() {
    let scope = HomeScope {
        epoch: 7,
        visit: 19,
    };
    let base = HomeAuthority {
        scope: Some(scope),
        home: true,
        foreground: true,
        access_ready: true,
        supplied_slot: true,
        positions_epoch: None,
        account_idle: true,
    };
    assert!(base.can_arm());
    assert!(
        !HomeAuthority {
            supplied_slot: false,
            ..base
        }
        .can_arm()
    );
    assert!(
        !HomeAuthority {
            foreground: false,
            ..base
        }
        .can_arm()
    );
    assert!(
        !HomeAuthority {
            access_ready: false,
            ..base
        }
        .can_arm()
    );
    assert!(
        !HomeAuthority {
            home: false,
            ..base
        }
        .can_arm()
    );
    assert!(
        !HomeAuthority {
            scope: None,
            ..base
        }
        .can_arm()
    );
    let settled = HomeAuthority {
        supplied_slot: false,
        positions_epoch: Some(7),
        ..base
    };
    assert!(settled.admitted(scope));
    assert!(
        !HomeAuthority {
            positions_epoch: None,
            ..settled
        }
        .admitted(scope)
    );
    assert!(
        !HomeAuthority {
            positions_epoch: Some(6),
            ..settled
        }
        .admitted(scope)
    );
    assert!(
        !HomeAuthority {
            scope: Some(HomeScope {
                epoch: 7,
                visit: 20
            }),
            ..settled
        }
        .admitted(scope)
    );
    assert!(
        !HomeAuthority {
            foreground: false,
            ..settled
        }
        .admitted(scope)
    );
    assert!(
        !HomeAuthority {
            account_idle: false,
            ..settled
        }
        .admitted(scope)
    );
}

#[test]
fn continue_watching_oracle_preserves_first_playlist_order_last_positions_and_literal_fractions() {
    let response=Response { status:200,body:criterion_session::SecretBody::new(br#"{"playlist":[{"mediaid":"Synth001","contentType":"film"},{"mediaid":"Synth002","contentType":"episode"},{"mediaid":"Synth001","contentType":"supplement"},{"mediaid":"Synth003","contentType":"original"},{"mediaid":"Synth004","contentType":"supplement"},{"mediaid":"Synth005","contentType":"film"},{"mediaid":"Synth006","contentType":"collection"}],"positions":[{"media_id":"Synth001","pos":25,"dur":100},{"media_id":"Synth001","pos":75,"dur":100},{"media_id":"Synth002","pos":-5,"dur":100},{"media_id":"Synth003","pos":25,"dur":100},{"media_id":"Synth004","pos":25,"dur":0},{"media_id":"Synth005","pos":120,"dur":100}]}"#.to_vec()) };
    let rows = expected_rows(&response).unwrap();
    assert!(rows.len() == 6);
    assert!(rows.iter().map(|row| row.id.as_str()).eq([
        "Synth001", "Synth002", "Synth003", "Synth004", "Synth005", "Synth006"
    ]));
    assert!(rows.iter().map(|row| row.fraction).eq([
        Some(0.75),
        Some(0.0),
        None,
        None,
        Some(1.0),
        None
    ]));
}

#[test]
fn continue_watching_oracle_accepts_exact_512_bound_and_rejects_513_or_non_success() {
    for count in [512, 513] {
        let playlist: Vec<_> = (0..count)
            .map(|index| serde_json::json!({"mediaid":format!("F{index:07}"),"contentType":"film"}))
            .collect();
        let response = Response {
            status: 200,
            body: criterion_session::SecretBody::new(
                serde_json::to_vec(&serde_json::json!({"playlist":playlist,"positions":[]}))
                    .unwrap(),
            ),
        };
        assert!(expected_rows(&response).is_ok() == (count == 512));
        let positions: Vec<_> = (0..count)
            .map(|_| serde_json::json!({"media_id":"Synth001","pos":1,"dur":4}))
            .collect();
        let response = Response {
            status: 200,
            body: criterion_session::SecretBody::new(
                serde_json::to_vec(&serde_json::json!({"playlist":[],"positions":positions}))
                    .unwrap(),
            ),
        };
        assert!(expected_rows(&response).is_ok() == (count == 512));
    }
    let response = Response {
        status: 500,
        body: criterion_session::SecretBody::new(br#"{"playlist":[],"positions":[]}"#.to_vec()),
    };
    assert!(expected_rows(&response).is_err());
}

#[test]
fn continue_watching_honest_empty_requires_success_and_refuses_a_nonempty_projection() {
    let response = Response {
        status: 200,
        body: criterion_session::SecretBody::new(br#"{"playlist":[],"positions":[]}"#.to_vec()),
    };
    let empty = expected_rows(&response).unwrap();
    assert!(empty.is_empty());
    let view = synthetic_saved_projection();
    let mut ui = criterion_ui::AppUi::new();
    let mut output = view.with_view(LoginView::SignedIn, |data| {
        ui.render(egui::RawInput::default(), &data).output
    });
    let result = view.with_view(LoginView::SignedIn, |data| {
        inspect_frame(&data, &empty, ui.focus(), ui.scroll_y(), &output)
    });
    output.textures_delta.clear();
    assert!(result.is_err());
    let trace = ContinueWatchingTrace {
        armed: true,
        bootstrap_started: true,
        bootstrap_returned: Some(true),
        read_started: true,
        read_returned: Some(false),
        expected: Some(vec![]),
        ..ContinueWatchingTrace::default()
    };
    assert!(!trace.completed());
    let view = synthetic_projection(criterion_account::ContinueWatching {
        playlist: vec![],
        positions: vec![],
    });
    let mut output = view.with_view(LoginView::SignedIn, |data| {
        ui.render(egui::RawInput::default(), &data).output
    });
    let result = view.with_view(LoginView::SignedIn, |data| {
        inspect_frame(&data, &empty, ui.focus(), ui.scroll_y(), &output)
    });
    output.textures_delta.clear();
    assert!(matches!(result, Ok(None)));
}

#[test]
fn continue_watching_guard_consumes_failed_attempts_and_erases_oracle_on_retirement() {
    let mut trace = ContinueWatchingTrace {
        armed: true,
        ..ContinueWatchingTrace::default()
    };
    assert!(trace.admit_bootstrap());
    trace.complete_bootstrap(&Err(criterion_account::Error::Unavailable));
    assert!(!trace.admit_bootstrap() && !trace.admit_read(Region::Us));
    let mut trace = ContinueWatchingTrace {
        armed: true,
        bootstrap_started: true,
        bootstrap_returned: Some(true),
        region: Some(Region::Us),
        ..ContinueWatchingTrace::default()
    };
    assert!(!trace.admit_read(Region::Ca));
    assert!(trace.admit_read(Region::Us));
    trace.complete_read(&Err(criterion_account::Error::Deadline));
    assert!(!trace.admit_read(Region::Us) && !trace.completed());
    let mut trace = ContinueWatchingTrace {
        armed: true,
        bootstrap_started: true,
        bootstrap_returned: Some(true),
        region: Some(Region::Us),
        ..ContinueWatchingTrace::default()
    };
    assert!(trace.admit_read(Region::Us));
    let response = Response {
        status: 200,
        body: criterion_session::SecretBody::new(br#"{"playlist":[],"positions":[]}"#.to_vec()),
    };
    trace.complete_read(&Ok(response));
    assert!(trace.completed());
    assert!(!trace.admit_read(Region::Us) && !trace.admit_bootstrap());
    trace.retire();
    assert!(trace.admitted && trace.expected.is_none() && trace.region.is_none());
    assert!(!trace.admit_bootstrap() && !trace.admit_read(Region::Us));
}

#[test]
fn continue_watching_transport_denies_every_other_subscriber_target_with_zero_receiver_calls() {
    use criterion_account::{
        DrmPolicy, NativePlaybackRequest, Request, SubscriberTarget, Transport,
        WatchListContentType,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Receiver(Arc<AtomicUsize>);
    impl Transport for Receiver {
        async fn send(&self, _: Request) -> Result<Response, criterion_account::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(criterion_account::Error::Unavailable)
        }
    }
    let id = MediaId::new("Synth001").unwrap();
    let targets = [
        SubscriberTarget::MyListIds(Region::Us),
        SubscriberTarget::Entitlement {
            region: Region::Us,
            captured_unix_time_ms: 0,
        },
        SubscriberTarget::Playback {
            region: Region::Us,
            request: NativePlaybackRequest {
                media_id: id.clone(),
                drm_policy: DrmPolicy::Low,
            },
        },
        SubscriberTarget::AddWatchList {
            region: Region::Us,
            media_id: id.clone(),
            content_type: WatchListContentType::Film,
        },
        SubscriberTarget::RemoveWatchList {
            region: Region::Us,
            media_id: id,
        },
        SubscriberTarget::WatchList {
            region: Region::Us,
            request: criterion_account::WatchListRequest {
                filter: criterion_account::WatchListFilter::Collection,
                cursor: Some(criterion_provider::PageCursor::new("synthetic-cursor").unwrap()),
            },
        },
    ];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let trace = Arc::new(Mutex::new(ReadTrace {
        continue_watching: Some(ContinueWatchingTrace {
            armed: true,
            bootstrap_started: true,
            bootstrap_returned: Some(true),
            region: Some(Region::Us),
            ..ContinueWatchingTrace::default()
        }),
        ..ReadTrace::default()
    }));
    let transport = TracedAccount {
        inner: Receiver(calls.clone()),
        trace: trace.clone(),
    };
    for target in targets {
        let result = runtime.block_on(transport.send(Request::Subscriber {
            target,
            credentials: synthetic_credentials(),
        }));
        assert!(matches!(
            result,
            Err(criterion_account::Error::InvalidRequest)
        ));
    }
    assert!(calls.load(Ordering::SeqCst) == 0 && trace.lock().unwrap().refused);
}

fn expected_rows(response: &Response) -> Result<Vec<ExpectedRow>, &'static str> {
    if response.status != 200 || response.body.expose().len() > 65_536 {
        return Err("Continue Watching bounded response oracle");
    }
    let value: serde_json::Value = serde_json::from_slice(response.body.expose())
        .map_err(|_| "Continue Watching response oracle")?;
    let playlist = value
        .get("playlist")
        .and_then(serde_json::Value::as_array)
        .ok_or("Continue Watching supplied playlist")?;
    let positions = value
        .get("positions")
        .and_then(serde_json::Value::as_array)
        .ok_or("Continue Watching supplied positions")?;
    if playlist.len() > 512 || positions.len() > 512 {
        return Err("Continue Watching oracle row bound");
    }
    let mut rows: Vec<ExpectedRow> = Vec::with_capacity(playlist.len());
    for media in playlist {
        let id = media
            .get("mediaid")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| MediaId::new(id).ok())
            .ok_or("Continue Watching oracle identity")?;
        let kind = media
            .get("contentType")
            .and_then(serde_json::Value::as_str)
            .ok_or("Continue Watching oracle kind")?;
        if !matches!(
            kind,
            "film"
                | "supplement"
                | "episode"
                | "original"
                | "series"
                | "category"
                | "collection"
                | "franchise"
                | "live"
        ) {
            return Err("Continue Watching oracle kind");
        }
        if rows.iter().any(|row| row.id == id) {
            continue;
        }
        let fraction = if matches!(kind, "film" | "supplement" | "episode") {
            match positions.iter().rev().find(|row| {
                row.get("media_id").and_then(serde_json::Value::as_str) == Some(id.as_str())
            }) {
                Some(position) => {
                    let pos = position
                        .get("pos")
                        .and_then(serde_json::Value::as_i64)
                        .ok_or("Continue Watching oracle position")?;
                    let dur = position
                        .get("dur")
                        .and_then(serde_json::Value::as_i64)
                        .ok_or("Continue Watching oracle duration")?;
                    (dur > 0).then(|| ((pos as f32) / (dur as f32)).clamp(0.0, 1.0))
                }
                None => None,
            }
        } else {
            None
        };
        rows.push(ExpectedRow { id, fraction });
    }
    Ok(rows)
}

#[test]
fn continue_watching_guard_refuses_bootstrap_until_current_home_arm() {
    use criterion_account::{Request, Response, Transport};
    use criterion_session::SecretBody;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Receiver(Arc<AtomicUsize>);
    impl Transport for Receiver {
        async fn send(&self, _: Request) -> Result<Response, criterion_account::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Response {
                status: 200,
                body: SecretBody::new(b"{}".to_vec()),
            })
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = TracedAccount {
        inner: Receiver(calls.clone()),
        trace: Arc::new(Mutex::new(ReadTrace {
            continue_watching: Some(ContinueWatchingTrace::default()),
            ..ReadTrace::default()
        })),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let result = runtime.block_on(transport.send(Request::Bootstrap));
    assert!(matches!(
        result,
        Err(criterion_account::Error::InvalidRequest)
    ));
    assert!(calls.load(Ordering::SeqCst) == 0);
}

fn synthetic_credentials() -> criterion_account::Credentials {
    use criterion_session::{Endpoint, SecretBody};
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    #[derive(Clone)]
    struct Clock(Arc<AtomicU64>);
    impl MonotonicClock for Clock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }
    struct Issuer;
    impl criterion_session::Transport for Issuer {
        async fn post(
            &self,
            request: criterion_session::Request,
        ) -> Result<criterion_session::Response, criterion_session::Error> {
            let body = match request.endpoint {
                Endpoint::DeviceCode => br#"{"device_code":"synthetic","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.as_slice(),
                Endpoint::Token => br#"{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","expires_in":3600}"#.as_slice(),
                Endpoint::Revoke => b"{}".as_slice(),
            };
            Ok(criterion_session::Response {
                status: 200,
                body: SecretBody::new(body.to_vec()),
            })
        }
    }
    struct Bootstrap;
    impl criterion_account::Transport for Bootstrap {
        async fn send(
            &self,
            _: criterion_account::Request,
        ) -> Result<criterion_account::Response, criterion_account::Error> {
            Ok(criterion_account::Response { status:200, body:SecretBody::new(br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()) })
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let time = Arc::new(AtomicU64::new(0));
    let session = criterion_session::Session::with_transport(
        criterion_session::Configuration::production(),
        Issuer,
        Clock(time.clone()),
    );
    runtime.block_on(session.start_link()).unwrap();
    time.store(5, Ordering::SeqCst);
    runtime.block_on(session.poll_once()).unwrap();
    let account = criterion_account::AccountClient::with_transport(Bootstrap);
    runtime.block_on(account.bootstrap()).unwrap();
    account.credentials(&session).unwrap()
}

#[test]
fn bootstrap_country_oracle_matches_production_client_exact_cases_before_one_cw_read() {
    use criterion_account::{AccountClient, Request, SubscriberTarget, Transport};
    use criterion_session::{Endpoint, SecretBody};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    };
    #[derive(Clone)]
    struct Clock(Arc<AtomicU64>);
    impl MonotonicClock for Clock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }
    struct Issuer;
    impl criterion_session::Transport for Issuer {
        async fn post(
            &self,
            request: criterion_session::Request,
        ) -> Result<criterion_session::Response, criterion_session::Error> {
            let body = match request.endpoint {
                Endpoint::DeviceCode => br#"{"device_code":"synthetic","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.as_slice(),
                Endpoint::Token => br#"{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","expires_in":3600}"#.as_slice(),
                Endpoint::Revoke => b"{}".as_slice(),
            };
            Ok(criterion_session::Response {
                status: 200,
                body: SecretBody::new(body.to_vec()),
            })
        }
    }
    struct Receiver {
        country: &'static str,
        region: Option<Region>,
        calls: Arc<AtomicUsize>,
    }
    impl Transport for Receiver {
        async fn send(&self, request: Request) -> Result<Response, criterion_account::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let body = match request {
                Request::Bootstrap => serde_json::to_vec(&serde_json::json!({
                    "country": self.country,
                    "token": "synthetic-bootstrap",
                    "baseUrl": {
                        "us": "https://mw.criterion.com/api/us",
                        "ca": "https://mw.criterion.com/api/ca"
                    }
                }))
                .unwrap(),
                Request::Subscriber {
                    target: SubscriberTarget::ContinueWatching(region),
                    ..
                } if self.region == Some(region) => br#"{"playlist":[],"positions":[]}"#.to_vec(),
                _ => return Err(criterion_account::Error::InvalidRequest),
            };
            Ok(Response {
                status: 200,
                body: SecretBody::new(body),
            })
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let time = Arc::new(AtomicU64::new(0));
    let session = criterion_session::Session::with_transport(
        criterion_session::Configuration::production(),
        Issuer,
        Clock(time.clone()),
    );
    assert!(runtime.block_on(session.start_link()).is_ok());
    time.store(5, Ordering::SeqCst);
    assert!(runtime.block_on(session.poll_once()).is_ok());

    let mut supported_witnesses = Vec::new();
    for (country, region) in [
        ("US", Region::Us),
        ("us", Region::Us),
        ("CA", Region::Ca),
        ("ca", Region::Ca),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let trace = Arc::new(Mutex::new(ReadTrace {
            continue_watching: Some(ContinueWatchingTrace {
                armed: true,
                ..ContinueWatchingTrace::default()
            }),
            ..ReadTrace::default()
        }));
        let client = AccountClient::with_transport(TracedAccount {
            inner: Receiver {
                country,
                region: Some(region),
                calls: calls.clone(),
            },
            trace: trace.clone(),
        });
        assert!(runtime.block_on(client.bootstrap()) == Ok(region));
        let read_ok = runtime.block_on(client.continue_watching(&session)).is_ok();
        let guarded = trace.lock().unwrap();
        let watching = guarded.continue_watching.as_ref().unwrap();
        supported_witnesses.push(
            read_ok
                && calls.load(Ordering::SeqCst) == 2
                && !guarded.refused
                && watching.bootstrap_started
                && watching.bootstrap_returned == Some(true)
                && watching.region == Some(region)
                && watching.read_started
                && watching.read_returned == Some(true)
                && watching.completed()
                && failure_phase(&guarded).is_none(),
        );
    }
    for country in ["Us", "uS", "Ca", "cA", "ZZ"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let trace = Arc::new(Mutex::new(ReadTrace {
            continue_watching: Some(ContinueWatchingTrace {
                armed: true,
                ..ContinueWatchingTrace::default()
            }),
            ..ReadTrace::default()
        }));
        let client = AccountClient::with_transport(TracedAccount {
            inner: Receiver {
                country,
                region: None,
                calls: calls.clone(),
            },
            trace: trace.clone(),
        });
        assert!(matches!(
            runtime.block_on(client.bootstrap()),
            Err(criterion_account::Error::UnsupportedRegion)
        ));
        assert!(matches!(
            runtime.block_on(client.continue_watching(&session)),
            Err(criterion_account::Error::NoBootstrap)
        ));
        let guarded = trace.lock().unwrap();
        let watching = guarded.continue_watching.as_ref().unwrap();
        assert!(calls.load(Ordering::SeqCst) == 1 && !guarded.refused);
        assert!(watching.bootstrap_started && watching.bootstrap_returned == Some(false));
        assert!(watching.region.is_none() && !watching.read_started);
        assert!(watching.read_returned.is_none() && !watching.completed());
        assert!(failure_phase(&guarded) == Some("Continue Watching bootstrap region"));
    }
    assert!(
        supported_witnesses == [true, true, true, true],
        "four supported Bootstrap literals must admit exactly one production CW read"
    );
}

#[test]
fn continue_watching_guard_refuses_even_default_watch_list_before_contact() {
    use criterion_account::{Region, Request, Response, SubscriberTarget, Transport};
    use criterion_session::SecretBody;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Receiver(Arc<AtomicUsize>);
    impl Transport for Receiver {
        async fn send(&self, _: Request) -> Result<Response, criterion_account::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Response {
                status: 200,
                body: SecretBody::new(b"{}".to_vec()),
            })
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let trace = Arc::new(Mutex::new(ReadTrace {
        continue_watching: Some(ContinueWatchingTrace::default()),
        ..ReadTrace::default()
    }));
    let transport = TracedAccount {
        inner: Receiver(calls.clone()),
        trace,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let result = runtime.block_on(transport.send(Request::Subscriber {
        target: SubscriberTarget::WatchList {
            region: Region::Us,
            request: criterion_account::WatchListRequest::default(),
        },
        credentials: synthetic_credentials(),
    }));
    assert!(matches!(
        result,
        Err(criterion_account::Error::InvalidRequest)
    ));
    assert!(calls.load(Ordering::SeqCst) == 0);
}

#[test]
fn continue_watching_guard_admits_one_bootstrap_then_one_exact_read() {
    use criterion_account::{Region, Request, Response, SubscriberTarget, Transport};
    use criterion_session::SecretBody;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Receiver(Arc<AtomicUsize>);
    impl Transport for Receiver {
        async fn send(&self, request: Request) -> Result<Response, criterion_account::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let body = if matches!(request, Request::Bootstrap) {
                br#"{"country":"US"}"#.as_slice()
            } else {
                br#"{"playlist":[],"positions":[]}"#.as_slice()
            };
            Ok(Response {
                status: 200,
                body: SecretBody::new(body.to_vec()),
            })
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let trace = Arc::new(Mutex::new(ReadTrace {
        continue_watching: Some(ContinueWatchingTrace {
            armed: true,
            ..ContinueWatchingTrace::default()
        }),
        ..ReadTrace::default()
    }));
    let transport = TracedAccount {
        inner: Receiver(calls.clone()),
        trace: trace.clone(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(runtime.block_on(transport.send(Request::Bootstrap)).is_ok());
    let result = runtime.block_on(transport.send(Request::Subscriber {
        target: SubscriberTarget::ContinueWatching(Region::Us),
        credentials: synthetic_credentials(),
    }));
    assert!(result.is_ok());
    assert!(calls.load(Ordering::SeqCst) == 2);
    let duplicate = runtime.block_on(transport.send(Request::Subscriber {
        target: SubscriberTarget::ContinueWatching(Region::Us),
        credentials: synthetic_credentials(),
    }));
    assert!(matches!(
        duplicate,
        Err(criterion_account::Error::InvalidRequest)
    ));
    assert!(matches!(
        runtime.block_on(transport.send(Request::Bootstrap)),
        Err(criterion_account::Error::InvalidRequest)
    ));
    trace
        .lock()
        .unwrap()
        .continue_watching
        .as_mut()
        .unwrap()
        .retire();
    let retired = runtime.block_on(transport.send(Request::Subscriber {
        target: SubscriberTarget::ContinueWatching(Region::Us),
        credentials: synthetic_credentials(),
    }));
    assert!(matches!(
        retired,
        Err(criterion_account::Error::InvalidRequest)
    ));
    assert!(calls.load(Ordering::SeqCst) == 2);
}

#[test]
fn continue_watching_transport_refuses_real_anonymous_detail_adapter_before_contact() {
    use criterion_account::{AccountClient, Request, Transport};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Receiver(Arc<AtomicUsize>);
    impl Transport for Receiver {
        async fn send(&self, _: Request) -> Result<Response, criterion_account::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Response { status:200,body:criterion_session::SecretBody::new(br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()) })
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let trace = Arc::new(Mutex::new(ReadTrace {
        continue_watching: Some(ContinueWatchingTrace {
            armed: true,
            ..ContinueWatchingTrace::default()
        }),
        ..ReadTrace::default()
    }));
    let client = AccountClient::with_transport(TracedAccount {
        inner: Receiver(calls.clone()),
        trace: trace.clone(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(runtime.block_on(client.bootstrap()).is_ok());
    assert!(matches!(
        runtime.block_on(client.detail(&MediaId::new("Synth001").unwrap())),
        Err(criterion_account::Error::InvalidRequest)
    ));
    assert!(calls.load(Ordering::SeqCst) == 1 && trace.lock().unwrap().refused);
}

#[test]
fn actual_application_login_back_arms_only_supplied_home_before_one_read_and_logout() {
    use criterion_account::{AccountClient, Request, SubscriberTarget};
    use criterion_platform::{KeyEvent, Size};
    use criterion_session::{Endpoint, SecretBody};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    };
    #[derive(Clone)]
    struct Clock(Arc<AtomicU64>);
    impl MonotonicClock for Clock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }
    struct Issuer(Arc<AtomicUsize>);
    impl Transport for Issuer {
        async fn post(
            &self,
            request: criterion_session::Request,
        ) -> Result<criterion_session::Response, criterion_session::Error> {
            let body = match request.endpoint {
                Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.as_slice(),
                Endpoint::Token => br#"{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","expires_in":3600}"#.as_slice(),
                Endpoint::Revoke => {
                    self.0.fetch_add(1, Ordering::SeqCst);
                    b"{}".as_slice()
                }
            };
            Ok(criterion_session::Response {
                status: 200,
                body: SecretBody::new(body.to_vec()),
            })
        }
    }
    struct Public {
        supplied: bool,
        calls: Arc<AtomicUsize>,
    }
    impl RequestTransport for Public {
        async fn get(
            &self,
            request: criterion_provider::Request,
        ) -> Result<criterion_provider::Response, criterion_provider::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if request.url.path() != "/" {
                return Err(criterion_provider::Error::Unavailable);
            }
            let block = if self.supplied {
                serde_json::json!({"type":20,"id":1,"header":"Synthetic saved films","playlistType":"continueWatching","imageJWLabel":"default_16x9","imageAspectRatio":56.25,"galleryPageNum":4,"galleryWrap":0})
            } else {
                serde_json::json!({"type":20,"id":1,"header":"Synthetic public films","playlistType":"playlist","imageJWLabel":"default_16x9","imageAspectRatio":56.25,"galleryPageNum":4,"galleryWrap":0,"playlist":[{"mediaid":"Public01","title":"Synthetic public film","contentType":"film","duration":90,"deeplink":"/films/Public01/public-film"}]})
            };
            let stream = format!(
                "baf:I[37,[],\"LanderStoryBlocks\"]\nace:{}\n",
                serde_json::json!(["$", "$Lbaf", null, {"blocks":[block]}])
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
    struct Middleware(Arc<AtomicUsize>);
    impl criterion_account::Transport for Middleware {
        async fn send(&self, request: Request) -> Result<Response, criterion_account::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let body = match request {
                Request::Bootstrap => br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.as_slice(),
                Request::Subscriber {
                    target: SubscriberTarget::ContinueWatching(Region::Us),
                    ..
                } => br#"{"playlist":[{"mediaid":"Synth001","title":"Synthetic saved film","contentType":"film","duration":90}],"positions":[{"media_id":"Synth001","pos":25,"dur":100}]}"#.as_slice(),
                _ => return Err(criterion_account::Error::InvalidRequest),
            };
            Ok(Response {
                status: 200,
                body: SecretBody::new(body.to_vec()),
            })
        }
    }
    type App = Application<Public, Issuer, Clock, TracedAccount<Middleware>, Clock>;
    fn surface() -> Surface {
        let size = Size {
            width: 1920,
            height: 1080,
        };
        Surface {
            window: size,
            drawable: size,
        }
    }
    fn discard(app: &mut App) {
        if let Some(mut output) = app.output.take() {
            output.textures_delta.clear();
        }
    }
    fn input(app: &mut App, runtime: &Runtime, key: (u32, i32)) {
        for pressed in [true, false] {
            app.event(
                Event::Key(KeyEvent {
                    scancode: key.0,
                    keycode: key.1,
                    pressed,
                    repeat: false,
                }),
                surface(),
                runtime,
                Duration::ZERO,
            );
        }
        discard(app);
    }
    fn pump(app: &mut App, runtime: &Runtime) {
        app.poll(runtime, true);
        app.consume(runtime, Duration::ZERO);
        discard(app);
        runtime.block_on(tokio::task::yield_now());
    }
    for supplied in [true, false] {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let time = Arc::new(AtomicU64::new(0));
        let clock = Clock(time.clone());
        let revokes = Arc::new(AtomicUsize::new(0));
        let public_calls = Arc::new(AtomicUsize::new(0));
        let native_calls = Arc::new(AtomicUsize::new(0));
        let trace = Arc::new(Mutex::new(ReadTrace {
            continue_watching: Some(ContinueWatchingTrace::default()),
            ..ReadTrace::default()
        }));
        let session = Arc::new(criterion_session::Session::with_transport(
            criterion_session::Configuration::production(),
            Issuer(revokes.clone()),
            clock.clone(),
        ));
        let mut app = Application::with_parts(
            surface(),
            Controller::with_clock(
                Catalog::with_transport(Public {
                    supplied,
                    calls: public_calls.clone(),
                }),
                runtime.handle(),
                clock.clone(),
            ),
            Authentication::with_session(session.clone(), clock),
            Accounts::from_parts(
                Arc::new(AccountClient::with_transport(TracedAccount {
                    inner: Middleware(native_calls.clone()),
                    trace: trace.clone(),
                })),
                session,
            ),
            Artwork::offline(),
        );
        for _ in 0..128 {
            pump(&mut app, &runtime);
            if matches!(
                app.controller.view.status(),
                LoadState::Ready | LoadState::Empty
            ) {
                break;
            }
        }
        assert!(matches!(
            app.controller.view.status(),
            LoadState::Ready | LoadState::Empty
        ));
        assert!(
            app.controller
                .view
                .continue_watching_needs_read(app.account_epoch.unwrap())
                == supplied
        );
        for key in [LEFT, DOWN, DOWN, DOWN, SELECT] {
            input(&mut app, &runtime, key);
        }
        for _ in 0..128 {
            pump(&mut app, &runtime);
            if matches!(app.authentication.view(), LoginView::Awaiting { .. }) {
                break;
            }
        }
        assert!(
            app.ui.page() == Page::Login
                && matches!(app.authentication.view(), LoginView::Awaiting { .. })
        );
        time.store(5, Ordering::SeqCst);
        for _ in 0..128 {
            pump(&mut app, &runtime);
            if app.authentication.access_ready() {
                break;
            }
        }
        assert!(app.authentication.access_ready() && native_calls.load(Ordering::SeqCst) == 0);
        input(&mut app, &runtime, BACK);
        let current = HomeAuthority {
            scope: app
                .account_epoch
                .zip(app.controller.membership_visit())
                .map(|(epoch, visit)| HomeScope { epoch, visit }),
            home: app.ui.page() == Page::Home,
            foreground: app.active && !app.exiting,
            access_ready: app.authentication.access_ready() && app.account_signed_in,
            supplied_slot: app
                .controller
                .view
                .continue_watching_needs_read(app.account_epoch.unwrap()),
            positions_epoch: app.positions.as_ref().map(|positions| positions.epoch),
            account_idle: app.shelf_pending.is_none()
                && app.shelf_generation.is_none()
                && app.continue_watching_pending.is_none()
                && app.continue_watching_generation.is_none()
                && app.native_detail_pending.is_none()
                && app.native_detail_generation.is_none()
                && app.list_membership.is_none(),
        };
        assert!(current.can_arm() == supplied && native_calls.load(Ordering::SeqCst) == 0);
        if current.can_arm() {
            trace
                .lock()
                .unwrap()
                .continue_watching
                .as_mut()
                .unwrap()
                .armed = true;
        }
        input(&mut app, &runtime, RIGHT);
        for _ in 0..128 {
            pump(&mut app, &runtime);
            if app.positions.is_some() {
                break;
            }
        }
        if supplied {
            assert!(
                app.positions
                    .as_ref()
                    .is_some_and(|positions| Some(positions.epoch) == app.account_epoch)
            );
            input(&mut app, &runtime, DOWN);
            app.consume(&runtime, Duration::ZERO);
            let mut output = app.output.take().unwrap();
            let selected = app
                .controller
                .view
                .with_view(app.authentication.view(), |view| {
                    let guarded = trace.lock().unwrap();
                    inspect_frame(
                        &view,
                        guarded
                            .continue_watching
                            .as_ref()
                            .unwrap()
                            .expected
                            .as_ref()
                            .unwrap(),
                        app.ui.focus(),
                        app.ui.scroll_y(),
                        &output,
                    )
                });
            output.textures_delta.clear();
            assert!(matches!(
                selected,
                Ok(Some(Focus::Card { row: 0, column: 0 }))
            ));
        } else {
            assert!(app.positions.is_none());
        }
        for _ in 0..16 {
            pump(&mut app, &runtime);
        }
        assert!(native_calls.load(Ordering::SeqCst) == if supplied { 2 } else { 0 });
        trace
            .lock()
            .unwrap()
            .continue_watching
            .as_mut()
            .unwrap()
            .retire();
        for key in [LEFT, DOWN, DOWN, DOWN, DOWN, SELECT, SELECT] {
            input(&mut app, &runtime, key);
        }
        for _ in 0..128 {
            pump(&mut app, &runtime);
            if matches!(app.authentication.view(), LoginView::SignedOut) {
                break;
            }
        }
        assert!(
            matches!(app.authentication.view(), LoginView::SignedOut)
                && revokes.load(Ordering::SeqCst) == 1
        );
        assert!(app.positions.is_none() && app.finish(&runtime));
        assert!(native_calls.load(Ordering::SeqCst) == if supplied { 2 } else { 0 });
        assert!(public_calls.load(Ordering::SeqCst) == 1 && !trace.lock().unwrap().refused);
    }
}

#[test]
fn diagnostics_distinguish_actual_precontact_refusal_without_a_receiver_call() {
    use criterion_account::{Request, Transport};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Receiver(Arc<AtomicUsize>);
    impl Transport for Receiver {
        async fn send(&self, _: Request) -> Result<Response, criterion_account::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(criterion_account::Error::Unavailable)
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let trace = Arc::new(Mutex::new(ReadTrace {
        continue_watching: Some(ContinueWatchingTrace::default()),
        ..ReadTrace::default()
    }));
    let transport = TracedAccount {
        inner: Receiver(calls.clone()),
        trace: trace.clone(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(
        runtime
            .block_on(transport.send(Request::Bootstrap))
            .is_err()
    );
    let guarded = trace.lock().unwrap();
    assert!(calls.load(Ordering::SeqCst) == 0);
    assert!(failure_phase(&guarded) == Some("Continue Watching request refused before transport"));
    let watching = guarded.continue_watching.as_ref().unwrap();
    assert!(!watching.bootstrap_started && !watching.read_started);
}

#[test]
fn diagnostics_distinguish_actual_read_http_schema_and_transport_failure() {
    use criterion_account::{Request, SubscriberTarget, Transport};
    use criterion_session::SecretBody;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    #[derive(Clone, Copy)]
    enum Case {
        Http,
        StatusError,
        Json,
        Playlist,
        Positions,
        Bound,
        Transport,
        Session,
    }
    struct Receiver {
        case: Case,
        calls: Arc<AtomicUsize>,
    }
    impl Transport for Receiver {
        async fn send(&self, request: Request) -> Result<Response, criterion_account::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if matches!(request, Request::Bootstrap) {
                return Ok(Response {
                    status: 200,
                    body: SecretBody::new(br#"{"country":"US"}"#.to_vec()),
                });
            }
            let (status, body) = match self.case {
                Case::Http => (503, b"{}".as_slice()),
                Case::StatusError => return Err(criterion_account::Error::HttpStatus(401)),
                Case::Json => (200, b"not-json".as_slice()),
                Case::Playlist => (200, br#"{"positions":[]}"#.as_slice()),
                Case::Positions => (200, br#"{"playlist":[]}"#.as_slice()),
                Case::Bound => {
                    return Ok(Response {
                        status: 200,
                        body: SecretBody::new(vec![b' '; 65_537]),
                    });
                }
                Case::Transport => return Err(criterion_account::Error::Deadline),
                Case::Session => {
                    return Err(criterion_account::Error::Session(
                        criterion_session::Error::RevocationUnconfirmed,
                    ));
                }
            };
            Ok(Response {
                status,
                body: SecretBody::new(body.to_vec()),
            })
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    for (case, phase, status, error) in [
        (
            Case::Http,
            "Continue Watching read HTTP status",
            Some(503),
            None,
        ),
        (
            Case::StatusError,
            "Continue Watching read HTTP status",
            Some(401),
            Some("http_status"),
        ),
        (
            Case::Json,
            "Continue Watching response oracle",
            Some(200),
            None,
        ),
        (
            Case::Playlist,
            "Continue Watching supplied playlist",
            Some(200),
            None,
        ),
        (
            Case::Positions,
            "Continue Watching supplied positions",
            Some(200),
            None,
        ),
        (
            Case::Bound,
            "Continue Watching bounded response oracle",
            Some(200),
            None,
        ),
        (
            Case::Transport,
            "Continue Watching read transport error",
            None,
            Some("deadline"),
        ),
        (
            Case::Session,
            "Continue Watching read transport error",
            None,
            Some("session"),
        ),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let trace = Arc::new(Mutex::new(ReadTrace {
            continue_watching: Some(ContinueWatchingTrace {
                armed: true,
                ..ContinueWatchingTrace::default()
            }),
            ..ReadTrace::default()
        }));
        let transport = TracedAccount {
            inner: Receiver {
                case,
                calls: calls.clone(),
            },
            trace: trace.clone(),
        };
        assert!(runtime.block_on(transport.send(Request::Bootstrap)).is_ok());
        assert!(failure_phase(&trace.lock().unwrap()).is_none());
        let _result = runtime.block_on(transport.send(Request::Subscriber {
            target: SubscriberTarget::ContinueWatching(Region::Us),
            credentials: synthetic_credentials(),
        }));
        let mut guarded = trace.lock().unwrap();
        let watching = guarded.continue_watching.as_ref().unwrap();
        assert!(calls.load(Ordering::SeqCst) == 2 && !guarded.refused);
        assert!(
            watching.read_started
                && watching.read_returned == Some(false)
                && watching.expected.is_none()
        );
        assert!(watching.read_status == status && watching.read_error == error);
        assert!(failure_phase(&guarded) == Some(phase));
        guarded.continue_watching.as_mut().unwrap().retire();
        let watching = guarded.continue_watching.as_ref().unwrap();
        assert!(watching.retired && watching.region.is_none() && watching.expected.is_none());
        assert!(
            watching.read_started
                && watching.read_returned == Some(false)
                && watching.read_status == status
                && watching.read_error == error
        );
        assert!(failure_phase(&guarded) == Some(phase));
    }
}

#[test]
fn diagnostics_distinguish_actual_bootstrap_http_schema_and_transport_failure() {
    use criterion_account::{Request, Transport};
    use criterion_session::SecretBody;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    #[derive(Clone, Copy)]
    enum Case {
        Http,
        StatusError,
        Json,
        Region,
        Bound,
        Transport,
    }
    struct Receiver {
        case: Case,
        calls: Arc<AtomicUsize>,
    }
    impl Transport for Receiver {
        async fn send(&self, _: Request) -> Result<Response, criterion_account::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (status, body) = match self.case {
                Case::Http => (503, b"{}".as_slice()),
                Case::StatusError => return Err(criterion_account::Error::HttpStatus(401)),
                Case::Json => (200, b"not-json".as_slice()),
                Case::Region => (200, br#"{"country":"ZZ"}"#.as_slice()),
                Case::Bound => {
                    return Ok(Response {
                        status: 200,
                        body: SecretBody::new(vec![b' '; 65_537]),
                    });
                }
                Case::Transport => return Err(criterion_account::Error::Deadline),
            };
            Ok(Response {
                status,
                body: SecretBody::new(body.to_vec()),
            })
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    for (case, phase, status, error) in [
        (
            Case::Http,
            "Continue Watching bootstrap HTTP status",
            Some(503),
            None,
        ),
        (
            Case::StatusError,
            "Continue Watching bootstrap HTTP status",
            Some(401),
            Some("http_status"),
        ),
        (
            Case::Json,
            "Continue Watching bootstrap JSON",
            Some(200),
            None,
        ),
        (
            Case::Region,
            "Continue Watching bootstrap region",
            Some(200),
            None,
        ),
        (
            Case::Bound,
            "Continue Watching bootstrap body bound",
            Some(200),
            None,
        ),
        (
            Case::Transport,
            "Continue Watching bootstrap transport error",
            None,
            Some("deadline"),
        ),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let trace = Arc::new(Mutex::new(ReadTrace {
            continue_watching: Some(ContinueWatchingTrace {
                armed: true,
                ..ContinueWatchingTrace::default()
            }),
            ..ReadTrace::default()
        }));
        let transport = TracedAccount {
            inner: Receiver {
                case,
                calls: calls.clone(),
            },
            trace: trace.clone(),
        };
        let _result = runtime.block_on(transport.send(Request::Bootstrap));
        let mut guarded = trace.lock().unwrap();
        let watching = guarded.continue_watching.as_ref().unwrap();
        assert!(calls.load(Ordering::SeqCst) == 1 && !guarded.refused);
        assert!(
            watching.bootstrap_started
                && watching.bootstrap_returned == Some(false)
                && !watching.read_started
        );
        assert!(watching.bootstrap_status == status && watching.bootstrap_error == error);
        assert!(failure_phase(&guarded) == Some(phase));
        guarded.continue_watching.as_mut().unwrap().retire();
        let watching = guarded.continue_watching.as_ref().unwrap();
        assert!(watching.retired && watching.region.is_none() && watching.expected.is_none());
        assert!(
            watching.bootstrap_started
                && watching.bootstrap_returned == Some(false)
                && watching.bootstrap_status == status
                && watching.bootstrap_error == error
        );
        assert!(failure_phase(&guarded) == Some(phase));
    }
}
