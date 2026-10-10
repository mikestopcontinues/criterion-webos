// SPDX-License-Identifier: GPL-3.0-or-later
//! Root-only live read admission. Private roots/projections exist only in bounded memory.
use super::*;
use crate::{controller::MembershipScope, presentation::NativeActivation};
use criterion_account::Region;
use criterion_provider::MediaId;
use criterion_ui::{Focus, ListMembership, Target};

const LEFT: (u32, i32) = (80, 1_073_741_904);
const RIGHT: (u32, i32) = (79, 1_073_741_903);
const DOWN: (u32, i32) = (81, 1_073_741_905);
const SELECT: (u32, i32) = (40, 13);
const BACK: (u32, i32) = (41, 27);

// No Debug or body/header retention. Only Membership mode creates this guard;
// arming requires an independently observed ordinary current Watch List root.
#[derive(Default)]
pub(super) struct MembershipTrace {
    selected: Option<MediaId>,
    region: Option<Region>,
    detail_started: bool,
    pub(super) detail_returned: Option<bool>,
    ids_started: bool,
    pub(super) ids_returned: Option<bool>,
    retired: bool,
}
impl MembershipTrace {
    fn arm(&mut self, root: MediaId) -> Result<(), &'static str> {
        if self.selected.is_some() || self.detail_started || self.ids_started || self.retired {
            return Err("membership one-root admission");
        }
        self.selected = Some(root);
        Ok(())
    }
    pub(super) fn admit_detail(&mut self, region: Region, root: &MediaId) -> bool {
        if self.retired || self.detail_started || self.selected.as_ref() != Some(root) {
            return false;
        }
        self.region = Some(region);
        self.detail_started = true;
        true
    }
    pub(super) fn admit_ids(&mut self, region: Region) -> bool {
        if self.retired
            || self.selected.is_none()
            || !self.detail_started
            || self.detail_returned != Some(true)
            || self.ids_started
            || self.region != Some(region)
        {
            return false;
        }
        self.ids_started = true;
        true
    }
    fn completed(&self) -> bool {
        self.detail_started
            && self.detail_returned == Some(true)
            && self.ids_started
            && self.ids_returned == Some(true)
    }
    fn retire(&mut self) {
        self.selected = None;
        self.region = None;
        self.retired = true;
    }
}

pub(super) struct PaintObserver {
    root: MediaId,
    epoch: u64,
    scope: Option<MembershipScope>,
    pub(super) painted: bool,
}
impl PaintObserver {
    pub(super) fn prepare(
        &mut self,
        app: &SubscriberApp,
        output: &egui::FullOutput,
    ) -> Result<bool, &'static str> {
        if !app.authentication.access_ready() || app.account_epoch != Some(self.epoch) {
            return Err("membership current account authority");
        }
        if app.ui.page() == Page::MyList && self.painted {
            retired_frame(app, output)?;
            return Ok(false);
        }
        if app.ui.page() != Page::Detail {
            return Err("membership native Detail authority");
        }
        let ready = app
            .controller
            .view
            .with_view(app.authentication.view(), |view| {
                if matches!(view.status, LoadState::Error | LoadState::Offline) {
                    return Err("membership native Detail admission");
                }
                Ok(view.status == LoadState::Ready && view.detail.is_some())
            })?;
        if !ready {
            return Ok(false);
        }
        let scope = app
            .controller
            .membership_scope(self.epoch)
            .ok_or("membership exact root visit")?;
        if scope.root != self.root
            || self
                .scope
                .as_ref()
                .is_some_and(|previous| *previous != scope)
            || !app.controller.membership_owns(&scope)
            || app.native_detail_pending.is_some()
            || app.native_detail_generation.is_some()
        {
            return Err("membership exact current root projection");
        }
        self.scope = Some(scope);
        match app.membership_view() {
            ListMembership::Pending => Ok(false),
            ListMembership::Known { present: true } => {
                let primary = app
                    .controller
                    .view
                    .with_view(app.authentication.view(), |view| {
                        view.detail
                            .as_ref()
                            .is_some_and(|detail| detail.primary_playback_target.is_some())
                    });
                let left = if primary { 732.0 } else { 252.0 };
                let top = 716.0 - app.ui.scroll_y();
                let area =
                    egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(340.0, 32.0));
                let screen =
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
                if output
                    .shapes
                    .iter()
                    .filter(|shape| {
                        matches!(&shape.shape, egui::Shape::Text(text)
                        if text.galley.job.text == "IN MY LIST" && !text.galley.elided
                            && area.contains_rect(shape.shape.visual_bounding_rect())
                            && screen.contains_rect(shape.shape.visual_bounding_rect())
                            && shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect()))
                    })
                    .count()
                    != 1
                {
                    return Err("membership complete current painted caption");
                }
                Ok(true)
            }
            _ => Err("membership independently listed root unavailable"),
        }
    }
}

pub(super) fn retired_frame(
    app: &SubscriberApp,
    output: &egui::FullOutput,
) -> Result<(), &'static str> {
    if app.list_membership.is_some()
        || !matches!(
            app.membership_view(),
            ListMembership::Unavailable | ListMembership::SignedOut
        )
        || output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text)
                if matches!(text.galley.job.text.as_str(),
                    "IN MY LIST" | "NOT IN MY LIST" | "CHECKING MY LIST" | "MY LIST UNAVAILABLE"))
        })
    {
        return Err("membership departed private frame retirement");
    }
    Ok(())
}

fn exact_window(before: &PrivateWindow, after: &PrivateWindow) -> bool {
    before.selected == after.selected
        && before.first == after.first
        && before.tail == after.tail
        && before.cards == after.cards
}

pub(super) fn verify_membership_trace(trace: &SharedTrace) -> Result<(), &'static str> {
    let trace = trace.lock().map_err(|_| "private request trace")?;
    let membership = trace.membership.as_ref().ok_or("membership mode trace")?;
    if trace.refused
        || !membership.completed()
        || !membership.retired
        || membership.selected.is_some()
        || membership.region.is_some()
    {
        return Err("one Detail and one membership read without repeat");
    }
    Ok(())
}

pub(super) fn admit_membership(
    app: &mut SubscriberApp,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    journey: &mut Journey,
    trace: &SharedTrace,
) -> Result<(), &'static str> {
    let deadline = journey.stage(Duration::from_secs(75));
    // Eligibility comes from the production projection, independently of the
    // IDs response: an ordinary native row must open its exact root sans autoplay.
    let (global, root) = app
        .controller
        .view
        .with_view(app.authentication.view(), |view| {
            let first = view.catalog.ok_or("membership native shelf window")?.first;
            view.cards
                .iter()
                .take(180)
                .enumerate()
                .find_map(|(index, card)| {
                    let Target::Native(root) = card.key else {
                        return None;
                    };
                    matches!(app.controller.view.native_activation(card.key),
                Some(NativeActivation::Detail { id, auto_play: false }) if id == *root)
                    .then(|| (first + index, root.clone()))
                })
                .ok_or("membership scope unadmitted: no eligible current native root")
        })?;
    if matches!(app.ui.focus(), Focus::MyListGroup(_)) {
        key(app, window, painter, runtime, journey, DOWN)?;
    }
    let wanted = Focus::Card {
        row: global / 4,
        column: global % 4,
    };
    for _ in 0..48 {
        if app.ui.focus() == wanted {
            break;
        }
        let Focus::Card { row, column } = app.ui.focus() else {
            return Err("membership native card focus");
        };
        let remote = if row < global / 4 {
            DOWN
        } else if row == global / 4 && column < global % 4 {
            RIGHT
        } else if row == global / 4 && column > global % 4 {
            LEFT
        } else {
            return Err("membership bounded forward card navigation");
        };
        key(app, window, painter, runtime, journey, remote)?;
        if Instant::now() >= deadline {
            return Err("membership native navigation deadline");
        }
    }
    if app.ui.focus() != wanted {
        return Err("membership native card navigation bound");
    }
    wait_shelf(app, window, painter, runtime, journey, deadline)?;
    let before = private_window(app)?;
    let index = global
        .checked_sub(before.first)
        .ok_or("membership native selected window")?;
    if before
        .cards
        .get(index)
        .is_none_or(|card| card.0 != Target::Native(root.clone()))
    {
        return Err("membership exact selected current root");
    }
    let focus = app.ui.focus();
    let scroll = app.ui.scroll_y();
    let epoch = app
        .account_epoch
        .ok_or("membership selected account epoch")?;
    trace
        .lock()
        .map_err(|_| "private request trace")?
        .membership
        .as_mut()
        .ok_or("membership mode trace")?
        .arm(root.clone())?;
    journey.membership = Some(PaintObserver {
        root,
        epoch,
        scope: None,
        painted: false,
    });
    key(app, window, painter, runtime, journey, SELECT)?;
    while !journey
        .membership
        .as_ref()
        .is_some_and(|observer| observer.painted)
    {
        if Instant::now() >= deadline {
            return Err("membership current paint deadline");
        }
        frame(app, window, painter, runtime, journey)?;
        std::thread::sleep(Duration::from_millis(16));
    }
    for _ in 0..16 {
        frame(app, window, painter, runtime, journey)?;
        if Instant::now() >= deadline {
            return Err("membership settled read deadline");
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    if !trace
        .lock()
        .map_err(|_| "private request trace")?
        .membership
        .as_ref()
        .is_some_and(MembershipTrace::completed)
    {
        return Err("membership completed one-read census");
    }
    key(app, window, painter, runtime, journey, BACK)?;
    if app.ui.page() != Page::MyList
        || app.ui.focus() != focus
        || app.ui.scroll_y() != scroll
        || !exact_window(&before, &private_window(app)?)
        || app.account_epoch != Some(epoch)
        || app.list_membership.is_some()
    {
        return Err("membership exact warm shelf Back restoration");
    }
    // Drop all selected-root and current-visit observers before Account can
    // render; only coarse completed census flags survive for joined cleanup.
    journey.membership = None;
    journey.membership_retired = true;
    trace
        .lock()
        .map_err(|_| "private request trace")?
        .membership
        .as_mut()
        .ok_or("membership mode trace")?
        .retire();
    drop(before);
    verify_membership_trace(trace)?;
    for _ in 0..3 {
        match app.ui.focus() {
            Focus::Card { column: 0, .. } => break,
            Focus::Card { .. } => key(app, window, painter, runtime, journey, LEFT)?,
            _ => return Err("membership shelf logout origin"),
        }
    }
    if !matches!(app.ui.focus(), Focus::Card { column: 0, .. }) {
        return Err("membership bounded logout origin");
    }
    verify_trace(trace, journey)?;
    println!("subscriber admission: current native root membership painted; exact Back admitted");
    Ok(())
}

#[test]
#[ignore = "actual subscriber/provider and serialized SDL/GLES; private external mount; root executor only"]
fn native_subscriber_detail_membership_back_and_logout_end_to_end() {
    subscriber_test(AdmissionMode::Membership);
}

#[test]
fn membership_guard_requires_exact_single_root_success_region_and_retirement() {
    let root = MediaId::new("Synth001").unwrap();
    let other = MediaId::new("Synth002").unwrap();
    let mut trace = MembershipTrace::default();
    assert!(!trace.admit_detail(Region::Us, &root));
    assert!(!trace.admit_ids(Region::Us));
    trace.arm(root.clone()).unwrap();
    assert!(!trace.admit_detail(Region::Us, &other));
    assert!(!trace.admit_ids(Region::Us));
    assert!(trace.admit_detail(Region::Us, &root));
    assert!(!trace.admit_detail(Region::Us, &root));
    assert!(!trace.admit_ids(Region::Us));
    trace.detail_returned = Some(false);
    assert!(!trace.admit_ids(Region::Us));
    trace.detail_returned = Some(true);
    assert!(!trace.admit_ids(Region::Ca));
    assert!(trace.admit_ids(Region::Us));
    assert!(!trace.admit_ids(Region::Us));
    trace.ids_returned = Some(true);
    assert!(trace.completed());
    trace.retire();
    assert!(trace.selected.is_none() && trace.region.is_none());
    assert!(!trace.admit_detail(Region::Us, &root));
    assert!(!trace.admit_ids(Region::Us));
    assert!(trace.arm(root).is_err());
}

#[test]
fn anonymous_detail_passthrough_is_mode_gated_exact_and_single() {
    use criterion_account::{AccountClient, Request, Response, Transport};
    use criterion_session::SecretBody;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    // Fully synthetic source. No issuer, subscriber credential or real transport.
    struct Receiver(Arc<AtomicUsize>);
    impl Transport for Receiver {
        async fn send(&self, request: Request) -> Result<Response, criterion_account::Error> {
            let body = match request {
                Request::Bootstrap => br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.as_slice(),
                Request::Detail { region, media_id, authorization } => {
                    assert!(region == Region::Us && media_id.as_str() == "Synth001");
                    assert!(authorization.header().is_sensitive());
                    assert!(authorization.header().as_bytes() == b"Bearer synthetic-bootstrap");
                    self.0.fetch_add(1, Ordering::SeqCst);
                    br#"{"contentType":"film","mediaid":"Synth001","title":"Synthetic ordinary Film"}"#.as_slice()
                }
                Request::Subscriber { .. } => panic!("synthetic anonymous transport only"),
            };
            Ok(Response {
                status: 200,
                body: SecretBody::new(body.to_vec()),
            })
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let trace = Arc::new(Mutex::new(ReadTrace::default()));
    let client = AccountClient::with_transport(TracedAccount {
        inner: Receiver(calls.clone()),
        trace: trace.clone(),
    });
    runtime.block_on(client.bootstrap()).unwrap();
    let root = MediaId::new("Synth001").unwrap();
    assert!(runtime.block_on(client.detail(&root)).is_err());
    assert!(calls.load(Ordering::SeqCst) == 0 && trace.lock().unwrap().refused);
    {
        let mut trace = trace.lock().unwrap();
        trace.refused = false;
        let mut membership = MembershipTrace::default();
        membership.arm(root.clone()).unwrap();
        trace.membership = Some(membership);
    }
    assert!(
        runtime
            .block_on(client.detail(&MediaId::new("Synth002").unwrap()))
            .is_err()
    );
    assert!(calls.load(Ordering::SeqCst) == 0 && trace.lock().unwrap().refused);
    assert!(runtime.block_on(client.detail(&root)).is_ok());
    assert!(calls.load(Ordering::SeqCst) == 1);
    assert!(
        trace
            .lock()
            .unwrap()
            .membership
            .as_ref()
            .unwrap()
            .detail_returned
            == Some(true)
    );
    assert!(runtime.block_on(client.detail(&root)).is_err());
    assert!(calls.load(Ordering::SeqCst) == 1);
}
