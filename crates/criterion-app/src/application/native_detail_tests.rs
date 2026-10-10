// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual application/input/worker composition; only HTTP bodies are synthetic.
use super::*;
use criterion_ui::{Focus, LoadState};

fn detail(id: &'static str, gate: Option<Arc<Gate>>) -> Step {
    Step {
        kind: Kind::NativeDetail(id),
        gate,
    }
}
fn list() -> Step {
    Step {
        kind: Kind::WatchList,
        gate: None,
    }
}
fn status(fixture: &Fixture) -> LoadState {
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| view.status)
}
fn select_listed(fixture: &mut Fixture) {
    if matches!(fixture.app.ui.focus(), Focus::MyListGroup(_)) {
        fixture.key(81, 1_073_741_905);
    }
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    fixture.key(40, 13);
    assert_eq!(fixture.app.ui.page(), Page::Detail);
}
fn open_related(fixture: &mut Fixture) {
    for _ in 0..3 {
        fixture.key(81, 1_073_741_905);
    }
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    fixture.key(40, 13);
}

fn open_saved_resume_series() -> Fixture {
    let mut fixture = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: None,
            },
            detail("Listed01", None),
        ],
        5,
    );
    *fixture.script.continue_body.lock().unwrap() = Some(br#"{"playlist":[{"contentType":"episode","mediaid":"Epis0001","title":"Synthetic clicked episode","series_id":"Listed01"}],"positions":[{"media_id":"Epis0001","pos":95,"dur":100,"series_id":"Listed01"},{"media_id":"Epis0003","pos":20,"dur":100}]}"#.to_vec());
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"series","mediaid":"Listed01","title":"Synthetic Series","playlists":[{"type":"seasons","title":"Episodes","playlist":[{"season_number":20,"season_title":"First supplied season","episodes":[{"mediaid":"Epis0001","title":"First episode"},{"mediaid":"Epis0002","title":"Second episode"}]},{"season_number":3,"season_title":"Second supplied season","episodes":[{"mediaid":"Epis0003","title":"Resume episode"}]}]}]}"#.to_vec());
    fixture.wait(|fixture| fixture.saved().len() == 1);
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 1, column: 0 });
    fixture.key(40, 13);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    fixture
}

#[test]
fn saved_episode_opens_series_and_selects_its_native_resume_child_then_logout_restores_public_default()
 {
    let mut fixture = open_saved_resume_series();
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.primary_playback_target.unwrap().as_str(), "Epis0003");
            assert_eq!(detail.primary_action, "RESUME SEASON 3, EPISODE 1");
            assert_eq!(detail.seasons.as_ref().unwrap().selected, 1);
            assert_eq!(view.rails[0].cards[0].saved_fraction, Some(0.2));
        });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::ContinueWatching, Kind::NativeDetail("Listed01")]
    );
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| fixture.issuer.revokes.load(Ordering::SeqCst) == 1);
    assert!(
        fixture.native_ready("Listed01"),
        "anonymous native metadata survives private retirement"
    );
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.primary_playback_target.unwrap().as_str(), "Epis0001");
            assert_eq!(detail.primary_action, "WATCH FIRST EPISODE");
            assert_eq!(detail.seasons.as_ref().unwrap().selected, 0);
            assert!(
                view.rails[0]
                    .cards
                    .iter()
                    .all(|card| card.saved_fraction.is_none())
            );
        });
}

#[test]
fn empty_first_season_disables_primary_but_displays_saved_later_season_then_retires_to_empty_default()
 {
    let mut fixture = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: None,
            },
            detail("Listed01", None),
        ],
        5,
    );
    // Literal admitted HTTP shape, not an observed account/catalog response.
    *fixture.script.continue_body.lock().unwrap() = Some(br#"{"playlist":[{"contentType":"episode","mediaid":"Ep000003","title":"Synthetic clicked Episode","series_id":"Listed01"}],"positions":[{"media_id":"Ep000003","pos":20,"dur":100}]}"#.to_vec());
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"series","mediaid":"Listed01","title":"Synthetic empty-first Series","playlists":[{"type":"seasons","title":"Episodes","playlist":[{"season_number":20,"season_title":"Synthetic empty season","episodes":[]},{"season_number":3,"season_title":"Synthetic saved season","episodes":[{"mediaid":"Ep000003","title":"Synthetic saved Episode"}]}]}]}"#.to_vec());
    fixture.wait(|fixture| fixture.saved().len() == 1);
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 1, column: 0 });
    fixture.key(40, 13);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.primary_playback_target, None);
            assert_eq!(detail.primary_action, "WATCH FIRST EPISODE");
            assert_eq!(detail.seasons.as_ref().unwrap().selected, 1);
            assert_eq!(view.rails[0].cards.len(), 1);
            assert_eq!(
                view.rails[0].cards[0].key.media_id().unwrap().as_str(),
                "Ep000003"
            );
            assert_eq!(view.rails[0].cards[0].saved_fraction, Some(0.2));
        });
    assert_eq!(fixture.app.ui.focus(), Focus::DetailAction(1));
    for _ in 0..4 {
        fixture.key(81, 1_073_741_905);
    }
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| fixture.issuer.revokes.load(Ordering::SeqCst) == 1);
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.primary_playback_target, None);
            assert_eq!(detail.primary_action, "WATCH FIRST EPISODE");
            assert_eq!(detail.seasons.as_ref().unwrap().selected, 0);
            assert!(view.rails[0].cards.is_empty());
        });
    assert!(fixture.app.positions.is_none());
    assert_eq!(fixture.app.ui.focus(), Focus::DetailSeason(0));
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::ContinueWatching, Kind::NativeDetail("Listed01")]
    );
}

#[test]
fn native_input_opens_exact_listed_detail_and_signed_out_related_detail_anonymously() {
    let mut fixture = Fixture::new(
        false,
        vec![list(), detail("Listed01", None), detail("Related1", None)],
        5,
    );
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.card.title, "Native Listed01");
            assert_eq!(detail.description, "Native long description");
            assert_eq!(detail.directors, "Synthetic native director");
            assert_eq!(detail.starring, Some("Synthetic native actor"));
            assert_eq!(detail.countries, Some("CA"));
            assert_eq!(detail.languages, Some("English"));
            assert_eq!(detail.primary_playback_target.unwrap().as_str(), "Listed01");
            assert_eq!(view.rails[0].title, "Related");
            assert_eq!(
                view.rails[0].cards[0].key.media_id().unwrap().as_str(),
                "Related1"
            );
        });
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| fixture.issuer.revokes.load(Ordering::SeqCst) == 1);
    assert!(!fixture.app.authentication.signed_in());
    assert!(
        fixture.native_ready("Listed01"),
        "anonymous metadata survives subscriber logout"
    );
    open_related(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Related1"));
    assert!(!fixture.app.authentication.signed_in());
    assert_eq!(
        fixture.issuer.tokens.load(Ordering::SeqCst),
        1,
        "metadata cannot initiate token refresh or activation"
    );
    assert_eq!(
        fixture.script.bootstrap.load(Ordering::SeqCst),
        1,
        "private and public reads reuse one native client lease"
    );
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::NativeDetail("Related1")
        ]
    );
    assert!(
        fixture
            .public_requests
            .lock()
            .unwrap()
            .iter()
            .all(|path| path == "/"),
        "native cards never use public website Detail"
    );
    fixture.key(41, 27);
    assert_eq!(fixture.app.ui.page(), Page::Detail);
    assert!(fixture.native_ready("Listed01"));
    fixture.key(41, 27);
    assert_eq!(
        fixture.app.ui.page(),
        Page::Home,
        "logout removes private My List from both histories"
    );
}

#[test]
fn back_joins_departed_native_detail_and_restores_the_exact_cached_shelf() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        false,
        vec![list(), detail("Listed01", Some(gate.clone()))],
        5,
    );
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    let departed = fixture
        .app
        .native_detail_generation
        .as_ref()
        .unwrap()
        .read
        .clone();
    fixture.key(41, 27);
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    fixture.wait(|_| gate.retired.load(Ordering::SeqCst) == 1);
    assert!(!fixture.app.controller.native_detail_owns(&departed));
    assert!(fixture.app.native_detail_generation.is_none());
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.script.trace.lock().unwrap(),
        [
            (Kind::WatchList, true),
            (Kind::WatchList, false),
            (Kind::NativeDetail("Listed01"), true),
            (Kind::NativeDetail("Listed01"), false)
        ]
    );
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    assert!(
        fixture
            .app
            .controller
            .view
            .with_view(fixture.app.authentication.view(), |view| view
                .cards
                .iter()
                .any(|card| card.title == "Synthetic listed film"))
    );
}

#[test]
fn departed_native_detail_joins_before_continue_watching_reuses_the_same_worker() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: None,
            },
            list(),
            detail("Listed01", Some(gate.clone())),
            Step {
                kind: Kind::ContinueWatching,
                gate: None,
            },
        ],
        5,
    );
    fixture.wait(|fixture| fixture.saved().len() == 2);
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    let departed = fixture
        .app
        .native_detail_generation
        .as_ref()
        .unwrap()
        .read
        .clone();
    fixture
        .app
        .command(Command::Navigate(Page::Home), fixture.runtime.handle());
    fixture.wait(|fixture| fixture.saved().len() == 2);
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
    assert!(!fixture.app.controller.native_detail_owns(&departed));
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.script.trace.lock().unwrap(),
        [
            (Kind::ContinueWatching, true),
            (Kind::ContinueWatching, false),
            (Kind::WatchList, true),
            (Kind::WatchList, false),
            (Kind::NativeDetail("Listed01"), true),
            (Kind::NativeDetail("Listed01"), false),
            (Kind::ContinueWatching, true),
            (Kind::ContinueWatching, false)
        ]
    );
}

#[test]
fn exact_native_deadline_cancels_without_retry_and_explicit_back_open_gets_fresh_intent() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        false,
        vec![
            list(),
            detail("Listed01", Some(gate.clone())),
            detail("Listed01", None),
        ],
        5,
    );
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    let departed = fixture
        .app
        .native_detail_generation
        .as_ref()
        .unwrap()
        .read
        .clone();
    fixture.clock.0.store(64, Ordering::SeqCst);
    fixture.pump();
    assert_eq!(status(&fixture), LoadState::Loading);
    assert_eq!(gate.retired.load(Ordering::SeqCst), 0);
    fixture.clock.0.store(65, Ordering::SeqCst);
    fixture.wait(|_| gate.retired.load(Ordering::SeqCst) == 1);
    assert_eq!(status(&fixture), LoadState::Error);
    assert!(!fixture.app.controller.native_detail_owns(&departed));
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::WatchList, Kind::NativeDetail("Listed01")]
    );
    fixture.key(41, 27);
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    assert!(!fixture.app.controller.native_detail_owns(&departed));
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::NativeDetail("Listed01")
        ]
    );
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
}

#[test]
fn wrong_native_identity_fails_atomically_and_never_retries_ordinary_polls() {
    let mut fixture = Fixture::new(false, vec![list(), detail("Listed01", None)], 5);
    fixture.open_list();
    *fixture.script.detail_response_id.lock().unwrap() = Some("Related1");
    select_listed(&mut fixture);
    fixture.wait(|fixture| status(fixture) == LoadState::Error);
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(!fixture.native_ready("Listed01"));
    assert!(!fixture.native_ready("Related1"));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert!(view.detail.is_none());
            assert!(view.rails.is_empty());
        });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::WatchList, Kind::NativeDetail("Listed01")]
    );
}

#[test]
fn background_retires_native_read_and_foreground_restarts_only_interrupted_metadata() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        false,
        vec![
            list(),
            detail("Listed01", Some(gate.clone())),
            detail("Listed01", None),
        ],
        5,
    );
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    let departed = fixture
        .app
        .native_detail_generation
        .as_ref()
        .unwrap()
        .read
        .clone();
    fixture.app.background();
    fixture.wait(|_| gate.retired.load(Ordering::SeqCst) == 1);
    assert!(!fixture.app.controller.native_detail_owns(&departed));
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::WatchList, Kind::NativeDetail("Listed01")]
    );
    fixture.app.foreground(fixture.runtime.handle());
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    fixture.app.background();
    fixture.app.foreground(fixture.runtime.handle());
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(fixture.native_ready("Listed01"));
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::NativeDetail("Listed01")
        ]
    );
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
}

#[test]
fn exhausted_root_epoch_joins_native_work_and_refuses_further_reads() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        false,
        vec![list(), detail("Listed01", Some(gate.clone()))],
        5,
    );
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    fixture.app.account_epoch = Some(u64::MAX);
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    assert_eq!(fixture.app.account_epoch, None);
    fixture.wait(|_| gate.retired.load(Ordering::SeqCst) == 1);
    fixture.wait(|fixture| fixture.issuer.revokes.load(Ordering::SeqCst) == 1);
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(fixture.app.native_detail_generation.is_none());
    assert_eq!(status(&fixture), LoadState::Error);
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::WatchList, Kind::NativeDetail("Listed01")]
    );
    assert_eq!(fixture.script.active.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
}

#[test]
fn native_episode_keeps_its_saved_key_while_requesting_the_exact_parent_series() {
    let gate = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        false,
        vec![list(), detail("Related1", Some(gate.clone()))],
        5,
    );
    *fixture.script.watch_list_body.lock().unwrap()=Some(br#"{"paging":{"page_limit":50},"type_counts":{"episode":1},"playlist":[{"mediaid":"Listed01","title":"Synthetic listed film","contentType":"episode","series_id":"Related1","series_title":"Synthetic parent series"}]}"#.to_vec());
    *fixture.script.native_kind.lock().unwrap() = "series";
    fixture.open_list();
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert_eq!(
                view.cards[0].key,
                &criterion_ui::Target::Native(
                    criterion_provider::MediaId::new("Listed01").unwrap()
                )
            );
        });
    select_listed(&mut fixture);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    let issued = &fixture.app.native_detail_generation.as_ref().unwrap().read;
    assert_eq!(issued.id.as_str(), "Related1");
    assert!(
        issued.auto_play,
        "native Episode navigation retains the proved autoplay intent only"
    );
    gate.release.notify_one();
    fixture.wait(|fixture| fixture.native_ready("Related1"));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.kind, criterion_ui::DetailKind::Series);
            assert!(
                detail.primary_playback_target.is_none(),
                "no supplied seasons means no invented first Episode"
            );
        });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::WatchList, Kind::NativeDetail("Related1")]
    );
    assert!(
        fixture
            .public_requests
            .lock()
            .unwrap()
            .iter()
            .all(|path| path == "/")
    );
}

#[test]
fn native_missing_parent_episode_and_live_refuse_without_detail_or_website_fallback() {
    for kind in ["episode", "live"] {
        let mut fixture = Fixture::new(false, vec![list()], 5);
        *fixture.script.watch_list_body.lock().unwrap()=Some(serde_json::to_vec(&serde_json::json!({
            "paging":{"page_limit":50},"type_counts":{kind:1},
            "playlist":[{"mediaid":"Listed01","title":"Synthetic listed film","contentType":kind}]
        })).unwrap());
        fixture.open_list();
        select_listed(&mut fixture);
        for _ in 0..8 {
            fixture.pump();
        }
        assert_eq!(status(&fixture), LoadState::Error);
        assert!(fixture.app.native_detail_generation.is_none());
        assert!(fixture.app.native_detail_pending.is_none());
        assert_eq!(*fixture.script.calls.lock().unwrap(), [Kind::WatchList]);
        assert!(
            fixture
                .public_requests
                .lock()
                .unwrap()
                .iter()
                .all(|path| path == "/")
        );
    }
}

fn boundary_series_fixture() -> Fixture {
    let fixture = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: None,
            },
            detail("Listed01", None),
        ],
        5,
    );
    *fixture.script.continue_body.lock().unwrap() = Some(br#"{"playlist":[{"contentType":"episode","mediaid":"Ep000001","title":"Synthetic clicked Episode","series_id":"Listed01"}],"positions":[{"media_id":"Ep000511","pos":20,"dur":100}]}"#.to_vec());
    // 511 children plus the root exactly meet the existing Native Detail bound.
    let episodes: Vec<_> = (2..=511).map(|i| serde_json::json!({"mediaid":format!("Ep{i:06}"),"title":if i == 511 { "Synthetic final Episode 511" } else { "Synthetic bounded Episode" }})).collect();
    *fixture.script.detail_body.lock().unwrap() = Some(serde_json::to_vec(&serde_json::json!({
        "contentType":"series","mediaid":"Listed01","title":"Synthetic boundary Series",
        "playlists":[{"type":"seasons","title":"Episodes","playlist":[
            {"season_number":1,"season_title":"First","episodes":[{"mediaid":"Ep000001","title":"First Episode"}]},
            {"season_number":-2147483648_i32,"season_title":"Supplied boundary number","episodes":episodes}
        ]}]
    })).unwrap());
    fixture
}

#[test]
fn widest_admitted_series_resume_caption_fits_the_actual_primary_button() {
    let mut fixture = boundary_series_fixture();
    fixture.wait(|fixture| fixture.saved().len() == 1);
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    fixture.key(40, 13);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    let caption = "RESUME SEASON -2147483648, EPISODE 510";
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert_eq!(view.detail.as_ref().unwrap().primary_action, caption)
        });
    let output = fixture.app.output.as_ref().unwrap();
    let bounds = output
        .shapes
        .iter()
        .rev()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == caption => {
                Some(shape.shape.visual_bounding_rect())
            }
            _ => None,
        })
        .expect("actual Application paints the complete supplied caption");
    let button = egui::Rect::from_min_size(egui::pos2(150.0, 620.0), egui::vec2(460.0, 80.0));
    assert!(
        button.contains_rect(bounds),
        "actual primary caption bounds {bounds:?} overflow primary button {button:?}"
    );
}

#[test]
fn departure_caches_only_anonymous_series_defaults_then_back_preserves_metadata() {
    let mut fixture = open_saved_resume_series();
    fixture
        .app
        .command(Command::Navigate(Page::Search), fixture.runtime.handle());
    fixture
        .app
        .command(Command::Restore(Page::Detail), fixture.runtime.handle());
    assert!(fixture.native_ready("Listed01"));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.card.title, "Synthetic Series");
            assert_eq!(detail.primary_playback_target.unwrap().as_str(), "Epis0001");
            assert_eq!(detail.primary_action, "WATCH FIRST EPISODE");
            assert_eq!(detail.seasons.as_ref().unwrap().selected, 0);
            assert!(
                view.rails[0]
                    .cards
                    .iter()
                    .all(|card| card.saved_fraction.is_none())
            );
        });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::ContinueWatching, Kind::NativeDetail("Listed01")]
    );
}

fn assert_public_series_default(fixture: &Fixture) {
    assert!(fixture.native_ready("Listed01"));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.card.title, "Synthetic Series");
            assert_eq!(detail.primary_playback_target.unwrap().as_str(), "Epis0001");
            assert_eq!(detail.primary_action, "WATCH FIRST EPISODE");
            assert_eq!(detail.seasons.as_ref().unwrap().selected, 0);
            assert!(
                view.rails[0]
                    .cards
                    .iter()
                    .all(|card| card.saved_fraction.is_none())
            );
        });
}

#[test]
fn background_finish_and_exit_erase_resume_snapshot_and_unpainted_private_labels() {
    for transition in ["background", "finish", "exit"] {
        let mut fixture = open_saved_resume_series();
        match transition {
            "background" => fixture.app.background(),
            "finish" => assert!(fixture.app.finish(&fixture.runtime)),
            "exit" => fixture.app.exit(),
            _ => unreachable!(),
        }
        assert_public_series_default(&fixture);
        assert!(
            fixture.app.positions.is_none(),
            "{transition} retires the owning private snapshot"
        );
        assert!(
            fixture.app.output.as_ref().unwrap().shapes.is_empty(),
            "{transition} erases queued private labels"
        );
    }
}

#[test]
fn same_token_relink_cannot_reapply_the_previous_series_resume_snapshot() {
    let mut fixture = open_saved_resume_series();
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| {
        fixture.issuer.revokes.load(Ordering::SeqCst) == 1
            && matches!(fixture.app.authentication.view(), LoginView::SignedOut)
    });
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
        "identical token bytes do not restore the old root intent"
    );
    fixture
        .app
        .command(Command::Restore(Page::Detail), fixture.runtime.handle());
    assert_public_series_default(&fixture);
    assert!(fixture.app.positions.is_none());
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::ContinueWatching, Kind::NativeDetail("Listed01")]
    );
}

#[test]
fn rendered_back_exit_cannot_republish_the_private_frame_after_retirement() {
    let mut fixture = Fixture::new(
        true,
        vec![Step {
            kind: Kind::ContinueWatching,
            gate: None,
        }],
        5,
    );
    fixture.wait(|fixture| fixture.saved().len() == 2);
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    assert!(fixture.app.output.as_ref().unwrap().shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("Synthetic saved film"))));
    fixture.key(41, 27);
    assert!(fixture.app.exiting());
    assert!(fixture.app.positions.is_none());
    assert!(
        fixture.app.output.as_ref().unwrap().shapes.is_empty(),
        "actual rendered Exit command cannot append its pre-retirement private frame"
    );
}

#[test]
fn cached_anonymous_series_reset_keeps_a_visible_canonical_card_focus() {
    let mut fixture = boundary_series_fixture();
    fixture.wait(|fixture| fixture.saved().len() == 1);
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    fixture.key(40, 13);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    for _ in 0..4 {
        fixture.key(81, 1_073_741_905);
    }
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    for _ in 0..509 {
        fixture.key(79, 1_073_741_903);
        if let Some(mut output) = fixture.app.take_output() {
            output.textures_delta.clear();
        }
    }
    assert_eq!(
        fixture.app.ui.focus(),
        Focus::Card {
            row: 0,
            column: 509
        }
    );
    fixture
        .app
        .command(Command::Navigate(Page::Search), fixture.runtime.handle());
    fixture
        .app
        .command(Command::Restore(Page::Detail), fixture.runtime.handle());
    if let Some(mut output) = fixture.app.take_output() {
        output.textures_delta.clear();
    }
    fixture.pump();
    assert!(fixture.native_ready("Listed01"));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert_eq!(
                view.detail
                    .as_ref()
                    .unwrap()
                    .seasons
                    .as_ref()
                    .unwrap()
                    .selected,
                0
            );
            assert_eq!(view.rails[0].cards.len(), 1);
            assert_eq!(view.rails[0].cards[0].title, "First Episode");
        });
    assert_eq!(
        fixture.app.ui.focus(),
        Focus::Card { row: 0, column: 0 },
        "cached anonymous defaults cannot keep a removed child index"
    );
    assert!(
        fixture
            .app
            .output
            .as_ref()
            .unwrap()
            .shapes
            .iter()
            .any(|shape| match &shape.shape {
                egui::Shape::Rect(rect) =>
                    rect.stroke.width == 8.0
                        && rect.stroke.color == egui::Color32::from_rgb(181, 138, 22)
                        && rect.rect.intersect(shape.clip_rect).is_positive(),
                _ => false,
            }),
        "actual resulting frame includes visible canonical card focus"
    );
}

#[path = "native_runtime_tests.rs"]
mod native_runtime_tests;
