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
