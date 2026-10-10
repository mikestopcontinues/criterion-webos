// SPDX-License-Identifier: GPL-3.0-or-later
//! Real Application/input/worker composition with synthetic middleware bodies.
use super::*;

#[path = "list_write_tests.rs"]
mod list_write_tests;

fn ids(gate: Option<Arc<Gate>>) -> Step {
    Step {
        kind: Kind::MyListIds,
        gate,
    }
}
fn has_membership_caption(fixture: &Fixture, caption: &str) -> bool {
    fixture.app.output.as_ref().is_some_and(|output| {
        output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == caption)
        })
    })
}

fn ordinary_detail(steps: Vec<Step>) -> Fixture {
    let mut fixture = Fixture::new(false, steps, 5);
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    fixture
}

fn click_list(fixture: &mut Fixture) {
    let surface = Surface {
        window: criterion_platform::Size {
            width: 1920,
            height: 1080,
        },
        drawable: criterion_platform::Size {
            width: 1920,
            height: 1080,
        },
    };
    for pressed in [true, false] {
        fixture.app.event(
            Event::PointerMoved { x: 770, y: 660 },
            surface,
            &fixture.runtime,
            fixture.clock.now(),
        );
        fixture.app.event(
            Event::PointerButton {
                button: 1,
                pressed,
                x: 770,
                y: 660,
            },
            surface,
            &fixture.runtime,
            fixture.clock.now(),
        );
    }
    fixture.app.consume(&fixture.runtime, fixture.clock.now());
}

#[test]
fn native_series_membership_observes_root_and_discards_unrelated_ids_and_positions() {
    let mut fixture = Fixture::new(false, vec![list(), detail("Listed01", None), ids(None)], 5);
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"series","mediaid":"Listed01","title":"Synthetic membership Series","playlists":[{"type":"seasons","title":"Episodes","playlist":[{"season_number":1,"season_title":"First season","episodes":[{"mediaid":"Epis0001","title":"First Episode"}]}]}]}"#.to_vec());
    *fixture.script.ids_result.lock().unwrap() = Ok(br#"{"watchlist":["Other001","Listed01"],"positions":[{"media_id":"Epis0001","pos":90,"dur":100,"series_id":"Other002","series_title":"Unrelated private Series"}]}"#.to_vec());
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.card.key.media_id().unwrap().as_str(), "Listed01");
            assert_eq!(detail.primary_playback_target.unwrap().as_str(), "Epis0001");
            assert_eq!(detail.primary_action, "WATCH FIRST EPISODE");
        });
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    assert!(
        fixture.app.positions.is_none(),
        "IDs positions are discarded"
    );
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds
        ]
    );
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.primary_action, "WATCH FIRST EPISODE");
            assert!(view.rails[0].cards[0].saved_fraction.is_none());
            assert_eq!(detail.membership, criterion_ui::ListMembership::Unavailable);
        });
}

#[test]
fn ordinary_native_kinds_read_membership_but_live_never_issues_the_ids_read() {
    for kind in [
        "film",
        "series",
        "collection",
        "episode",
        "supplement",
        "category",
        "franchise",
        "original",
        "live",
    ] {
        let mut steps = vec![list(), detail("Listed01", None)];
        if kind != "live" {
            steps.push(ids(None));
        }
        let mut fixture = Fixture::new(false, steps, 5);
        *fixture.script.native_kind.lock().unwrap() = kind;
        fixture.open_list();
        select_listed(&mut fixture);
        fixture.wait(|fixture| fixture.native_ready("Listed01"));
        if kind == "live" {
            fixture.wait(|fixture| has_membership_caption(fixture, "MY LIST UNAVAILABLE"));
            for _ in 0..8 {
                fixture.pump();
            }
            assert_eq!(fixture.script.calls.lock().unwrap().len(), 2);
        } else {
            fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
            assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
        }
    }
}

#[test]
fn empty_and_unrelated_ids_are_known_absence_and_signed_in_controls_refuse_every_write() {
    for body in [
        br#"{"watchlist":[],"positions":[]}"#.as_slice(),
        br#"{"watchlist":["Other001"],"positions":[]}"#.as_slice(),
    ] {
        let mut fixture = ordinary_detail(vec![list(), detail("Listed01", None), ids(None)]);
        *fixture.script.ids_result.lock().unwrap() = Ok(body.to_vec());
        fixture.wait(|fixture| has_membership_caption(fixture, "NOT IN MY LIST"));
        fixture.key(79, 1_073_741_903);
        fixture.key(79, 1_073_741_903);
        assert_eq!(fixture.app.ui.focus(), criterion_ui::Focus::DetailAction(2));
        fixture.key(40, 13);
        click_list(&mut fixture);
        fixture.app.command(
            Command::ToggleList {
                root: criterion_provider::MediaId::new("Listed01").unwrap(),
                from_visit: fixture.app.controller.membership_visit(),
                expected_present: Some(false),
            },
            fixture.runtime.handle(),
        );
        fixture.pump();
        assert!(has_membership_caption(&fixture, "NOT IN MY LIST"));
        assert_eq!(
            status(&fixture),
            LoadState::Ready,
            "write refusal preserves admitted Detail"
        );
        assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
        assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn invalid_or_failed_ids_are_visible_unavailable_and_never_retried_as_absence() {
    for result in [
        Ok(br#"{"watchlist":[]}"#.to_vec()),
        Err(criterion_account::Error::Deadline),
        Err(criterion_account::Error::HttpStatus(403)),
    ] {
        let mut fixture = ordinary_detail(vec![list(), detail("Listed01", None), ids(None)]);
        *fixture.script.ids_result.lock().unwrap() = result;
        fixture.wait(|fixture| has_membership_caption(fixture, "MY LIST UNAVAILABLE"));
        for _ in 0..8 {
            fixture.pump();
        }
        assert_eq!(status(&fixture), LoadState::Ready);
        assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
        assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn issued_read_expires_at_original_deadline_and_joins_without_publishing_or_replaying() {
    let gate = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(Some(gate.clone())),
    ]);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    assert!(has_membership_caption(&fixture, "CHECKING MY LIST"));
    fixture.clock.0.store(64, Ordering::SeqCst);
    fixture.pump();
    assert!(has_membership_caption(&fixture, "CHECKING MY LIST"));
    fixture.clock.0.store(65, Ordering::SeqCst);
    fixture.wait(|fixture| {
        has_membership_caption(fixture, "MY LIST UNAVAILABLE")
            && gate.retired.load(Ordering::SeqCst) == 1
    });
    gate.release.notify_one();
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(!has_membership_caption(&fixture, "IN MY LIST"));
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.active.load(Ordering::SeqCst), 0);
}

#[test]
fn waiting_for_credentials_uses_one_original_budget_and_expiry_does_not_replay_after_refresh() {
    // No IDs step is admitted: the logical demand must expire before credentials settle.
    let mut fixture = ordinary_detail(vec![list(), detail("Listed01", None)]);
    fixture.issuer.hold_refresh.store(true, Ordering::SeqCst);
    fixture.clock.0.store(3605, Ordering::SeqCst);
    fixture.wait(|fixture| fixture.issuer.tokens.load(Ordering::SeqCst) == 2);
    assert!(has_membership_caption(&fixture, "CHECKING MY LIST"));
    assert!(!fixture.app.authentication.access_ready());
    fixture.clock.0.store(3664, Ordering::SeqCst);
    fixture.pump();
    assert!(has_membership_caption(&fixture, "CHECKING MY LIST"));
    fixture.clock.0.store(3665, Ordering::SeqCst);
    fixture.pump();
    assert!(has_membership_caption(&fixture, "MY LIST UNAVAILABLE"));
    fixture.issuer.release.notify_one();
    fixture.wait(|fixture| fixture.app.authentication.access_ready());
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 2);
    assert!(has_membership_caption(&fixture, "MY LIST UNAVAILABLE"));
    assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 2);
}

#[test]
fn same_epoch_cached_back_reads_again_only_after_the_departed_issued_read_is_joined() {
    let gate = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(Some(gate.clone())),
        ids(None),
    ]);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    let original_epoch = fixture.app.account_epoch;
    fixture
        .app
        .command(Command::Navigate(Page::Search), fixture.runtime.handle());
    assert!(fixture.app.list_membership.is_none());
    assert!(
        fixture
            .app
            .output
            .as_ref()
            .is_none_or(|output| output.shapes.is_empty())
    );
    *fixture.script.ids_result.lock().unwrap() = Ok(br#"{"watchlist":[],"positions":[]}"#.to_vec());
    fixture
        .app
        .command(Command::Restore(Page::Detail), fixture.runtime.handle());
    assert!(
        fixture.native_ready("Listed01"),
        "Back uses cached anonymous metadata"
    );
    fixture.wait(|fixture| has_membership_caption(fixture, "NOT IN MY LIST"));
    gate.release.notify_one();
    assert_eq!(fixture.app.account_epoch, original_epoch);
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            Kind::MyListIds
        ]
    );
    assert_eq!(
        *fixture.script.trace.lock().unwrap(),
        [
            (Kind::WatchList, true),
            (Kind::WatchList, false),
            (Kind::NativeDetail("Listed01"), true),
            (Kind::NativeDetail("Listed01"), false),
            (Kind::MyListIds, true),
            (Kind::MyListIds, false),
            (Kind::MyListIds, true),
            (Kind::MyListIds, false),
        ]
    );
}

#[test]
fn background_retirement_joins_before_a_fresh_foreground_membership_read() {
    let gate = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(Some(gate.clone())),
        ids(None),
    ]);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    fixture.app.background();
    assert!(fixture.app.list_membership.is_none());
    assert!(
        fixture
            .app
            .output
            .as_ref()
            .is_none_or(|output| output.shapes.is_empty())
    );
    for _ in 0..4 {
        fixture.app.poll(&fixture.runtime, false);
    }
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    fixture.app.foreground(fixture.runtime.handle());
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
    gate.release.notify_one();
}

#[test]
fn logout_and_same_token_relink_retire_private_membership_and_signed_out_activation_never_replays_a_write()
 {
    let mut fixture = ordinary_detail(vec![list(), detail("Listed01", None), ids(None), ids(None)]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let prior_epoch = fixture.app.account_epoch;
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    assert!(fixture.app.list_membership.is_none());
    assert!(
        fixture
            .app
            .output
            .as_ref()
            .is_none_or(|output| output.shapes.is_empty())
    );
    fixture.wait(|fixture| matches!(fixture.app.authentication.view(), LoginView::SignedOut));
    assert!(has_membership_caption(&fixture, "MY LIST"));
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    fixture.key(79, 1_073_741_903);
    fixture.key(79, 1_073_741_903);
    assert_eq!(fixture.app.ui.focus(), criterion_ui::Focus::DetailAction(2));
    fixture.key(40, 13);
    assert_eq!(fixture.app.ui.page(), Page::Login);
    fixture.wait(|fixture| {
        matches!(
            fixture.app.authentication.view(),
            LoginView::Awaiting { .. }
        )
    });
    assert_eq!(fixture.app.ui.focus(), criterion_ui::Focus::LoginCancel);
    fixture.key(40, 13);
    assert_eq!(fixture.app.ui.page(), Page::Detail);
    assert_eq!(fixture.app.ui.focus(), criterion_ui::Focus::DetailAction(2));
    assert!(fixture.native_ready("Listed01"));
    for _ in 0..4 {
        fixture.pump();
    }
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    assert!(has_membership_caption(&fixture, "MY LIST"));

    // The same synthetic token bytes are admitted by a new root epoch.
    fixture.key(40, 13);
    fixture.wait(|fixture| {
        matches!(
            fixture.app.authentication.view(),
            LoginView::Awaiting { .. }
        )
    });
    fixture.clock.0.store(10, Ordering::SeqCst);
    fixture.wait(|fixture| fixture.app.authentication.signed_in());
    assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 2);
    assert_ne!(fixture.app.account_epoch, prior_epoch);
    assert!(
        fixture.app.list_membership.is_none(),
        "no private result survives activation"
    );
    *fixture.script.ids_result.lock().unwrap() = Ok(br#"{"watchlist":[],"positions":[]}"#.to_vec());
    fixture.key(41, 27);
    assert_eq!(fixture.app.ui.page(), Page::Detail);
    fixture.wait(|fixture| has_membership_caption(fixture, "NOT IN MY LIST"));
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
    assert_eq!(fixture.issuer.revokes.load(Ordering::SeqCst), 1);
    assert_eq!(status(&fixture), LoadState::Ready);
}

#[test]
fn disposal_joins_issued_membership_without_authorizing_contact_or_private_publication() {
    let gate = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(Some(gate.clone())),
    ]);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    assert!(fixture.app.finish(&fixture.runtime));
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.active.load(Ordering::SeqCst), 0);
    assert!(fixture.app.list_membership.is_none());
    assert!(
        fixture
            .app
            .output
            .as_ref()
            .is_none_or(|output| output.shapes.is_empty())
    );
    fixture.app.foreground(fixture.runtime.handle());
    for _ in 0..4 {
        fixture.pump();
    }
    assert!(has_membership_caption(&fixture, "MY LIST UNAVAILABLE"));
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    gate.release.notify_one();
}

#[test]
fn logout_retires_an_issued_read_before_same_token_relink_returns_to_the_same_root() {
    let gate = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(Some(gate.clone())),
        ids(None),
    ]);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    let prior_epoch = fixture.app.account_epoch;
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| {
        matches!(fixture.app.authentication.view(), LoginView::SignedOut)
            && gate.retired.load(Ordering::SeqCst) == 1
    });
    assert!(fixture.app.list_membership.is_none());
    assert!(!has_membership_caption(&fixture, "IN MY LIST"));
    fixture.key(79, 1_073_741_903);
    fixture.key(79, 1_073_741_903);
    fixture.key(40, 13);
    fixture.wait(|fixture| {
        matches!(
            fixture.app.authentication.view(),
            LoginView::Awaiting { .. }
        )
    });
    fixture.clock.0.store(10, Ordering::SeqCst);
    fixture.wait(|fixture| fixture.app.authentication.signed_in());
    assert_ne!(fixture.app.account_epoch, prior_epoch);
    assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 2);
    *fixture.script.ids_result.lock().unwrap() = Ok(br#"{"watchlist":[],"positions":[]}"#.to_vec());
    fixture.key(41, 27);
    fixture.wait(|fixture| has_membership_caption(fixture, "NOT IN MY LIST"));
    gate.release.notify_one();
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(!has_membership_caption(&fixture, "IN MY LIST"));
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(gate.retired.load(Ordering::SeqCst), 1);
}

#[test]
fn mismatched_native_response_identity_cannot_become_a_membership_root_or_a_primary_child_alias() {
    let mut fixture = Fixture::new(false, vec![list(), detail("Listed01", None)], 5);
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"series","mediaid":"Related1","title":"Different returned root","playlists":[{"type":"seasons","title":"Episodes","playlist":[{"season_number":1,"season_title":"First","episodes":[{"mediaid":"Epis0001","title":"Different primary child"}]}]}]}"#.to_vec());
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| status(fixture) == LoadState::Error);
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(!fixture.native_ready("Listed01"));
    assert!(!fixture.native_ready("Related1"));
    assert!(fixture.app.list_membership.is_none());
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::WatchList, Kind::NativeDetail("Listed01")]
    );
    assert!(!has_membership_caption(&fixture, "IN MY LIST"));
}
