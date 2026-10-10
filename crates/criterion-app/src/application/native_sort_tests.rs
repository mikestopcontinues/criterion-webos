// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual Application input/paint journey; HTTP bodies are explicitly synthetic.
use super::*;

fn sort_fixture() -> Fixture {
    sort_fixture_with_positions(false)
}
pub(super) fn sort_fixture_with_positions(positions: bool) -> Fixture {
    let mut steps = vec![
        list(),
        detail("Listed01", None),
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
    ];
    if positions {
        steps.insert(
            0,
            Step {
                kind: Kind::ContinueWatching,
                gate: None,
            },
        );
    }
    let mut fixture = Fixture::new(positions, steps, 5);
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"collection","mediaid":"Listed01","title":"Synthetic sortable collection","is_first_tab_sortable":true,"featured":{"title":"Supplied Feature","playlist":[{"contentType":"film","mediaid":"Feature1","title":"Feature unchanged"}]},"playlists":[{"type":"GENERIC_PLAYLIST","title":"First supplied tab","playlistId":"first","playlist":[{"contentType":"film","mediaid":"Related1","title":"beta","release_date":"2000-12-31","duration":90.5},{"contentType":"film","mediaid":"Related2","title":"Alpha","release_date":"2000-01-01","duration":120}]},{"type":"GENERIC_PLAYLIST","title":"Tail unchanged","playlistId":"tail","playlist":[{"contentType":"film","mediaid":"Tail0001","title":"Tail card"}]}]}"#.to_vec());
    if positions {
        *fixture.script.continue_body.lock().unwrap() = Some(br#"{"playlist":[{"contentType":"film","mediaid":"Related2","title":"Saved synthetic Alpha"}],"positions":[{"media_id":"Related2","pos":25,"dur":100}]}"#.to_vec());
        fixture.wait(|fixture| fixture.saved().len() == 1);
    }
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == if positions { 4 } else { 3 }
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    fixture
}

fn apply_title(fixture: &mut Fixture) {
    first_tab(fixture);
    fixture.key(40, 13);
    fixture.key(81, 1_073_741_905);
    fixture.key(40, 13);
    fixture.key(79, 1_073_741_903);
    fixture.key(40, 13);
    fixture.pump();
    assert_eq!(ids(fixture), ["Related2", "Related1"]);
}

#[test]
fn actual_sorted_open_warm_back_background_and_logout_keep_anonymous_order_and_retire_progress() {
    let mut fixture = sort_fixture_with_positions(true);
    apply_title(&mut fixture);
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert_eq!(view.rails[0].cards[0].saved_fraction, Some(0.25))
        });
    fixture.script.steps.lock().unwrap().extend([
        detail("Related1", None),
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
    ]);
    *fixture.script.detail_body.lock().unwrap() = None;
    fixture.key(81, 1_073_741_905);
    fixture.key(79, 1_073_741_903); // Sorted column1 is Related1, the parent transport's admitted child ID.
    fixture.key(40, 13);
    fixture.wait(|fixture| fixture.native_ready("Related1"));
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == 6
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    fixture.key(41, 27);
    fixture.wait(|fixture| {
        fixture.native_ready("Listed01")
            && fixture.script.calls.lock().unwrap().len() == 7
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    assert_eq!(
        ids(&fixture),
        ["Related2", "Related1"],
        "warm Back restores its retained anonymous sorted projection"
    );
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert!(
                view.rails[0]
                    .cards
                    .iter()
                    .all(|card| card.saved_fraction.is_none())
            )
        });
    fixture.key(82, 1_073_741_906);
    assert_eq!(fixture.app.ui.focus(), Focus::DetailTab(0));
    fixture.key(40, 13);
    fixture.app.background();
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert!(!view.detail.as_ref().unwrap().sort.unwrap().visible)
        });
    fixture.app.foreground(fixture.runtime.handle());
    fixture.pump();
    assert_eq!(fixture.app.ui.focus(), Focus::DetailTab(0));
    fixture.key(40, 13);
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| fixture.issuer.revokes.load(Ordering::SeqCst) == 1);
    fixture.pump();
    assert_eq!(ids(&fixture), ["Related2", "Related1"]);
    assert_eq!(fixture.app.ui.focus(), Focus::DetailTab(0));
    assert!(!texts(&fixture).iter().any(|text| text == "Sort by"));
    let calls = fixture.script.calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        8,
        "foreground legitimately refreshes the existing membership visit"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, Kind::NativeDetail(_)))
            .count(),
        2,
        "local lifecycle must not reload anonymous metadata"
    );
}

#[test]
fn actual_controller_rejects_wrong_root_hidden_apply_and_background_sort_intents() {
    let mut fixture = sort_fixture();
    let root = criterion_provider::MediaId::new("Listed01").unwrap();
    for (id, action) in [
        (
            criterion_provider::MediaId::new("Related1").unwrap(),
            criterion_ui::DetailSortAction::Open,
        ),
        (root.clone(), criterion_ui::DetailSortAction::Apply),
    ] {
        assert!(matches!(
            fixture.app.controller.command(
                Command::DetailSort { root: id, action },
                Page::Detail,
                fixture.runtime.handle()
            ),
            Effect::None
        ));
        assert!(
            !fixture
                .app
                .controller
                .view
                .native_sort_view()
                .unwrap()
                .visible
        );
    }
    fixture.app.background();
    fixture.app.command(
        Command::DetailSort {
            root,
            action: criterion_ui::DetailSortAction::Open,
        },
        fixture.runtime.handle(),
    );
    assert!(
        !fixture
            .app
            .controller
            .view
            .native_sort_view()
            .unwrap()
            .visible
    );
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
}

fn texts(fixture: &Fixture) -> Vec<String> {
    fixture
        .app
        .output
        .as_ref()
        .unwrap()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn repeated_first_tab_select_opens_actual_painted_sort_sheet_without_an_account_read() {
    let mut fixture = sort_fixture();
    for _ in 0..3 {
        fixture.key(81, 1_073_741_905);
    }
    assert_eq!(fixture.app.ui.focus(), Focus::DetailTab(0));
    fixture.key(40, 13);
    fixture.pump();
    assert!(
        texts(&fixture).iter().any(|text| text == "Sort by"),
        "the actual Application must paint the sort sheet after repeating the selected first tab"
    );
    assert_eq!(
        fixture.script.calls.lock().unwrap().len(),
        3,
        "local sorting must issue no account read"
    );
}

fn first_tab(fixture: &mut Fixture) {
    for _ in 0..3 {
        fixture.key(81, 1_073_741_905);
    }
    assert_eq!(fixture.app.ui.focus(), Focus::DetailTab(0));
}
fn ids(fixture: &Fixture) -> Vec<String> {
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            view.rails[0]
                .cards
                .iter()
                .map(|card| card.key.media_id().unwrap().as_str().to_owned())
                .collect()
        })
}
#[test]
fn actual_apply_paints_sorted_cards_dismiss_reopens_applied_and_default_restores_source() {
    let mut fixture = sort_fixture();
    first_tab(&mut fixture);
    fixture.key(40, 13);
    fixture.pump();
    fixture.key(81, 1_073_741_905); // Title
    fixture.key(40, 13);
    assert_eq!(
        ids(&fixture),
        ["Related1", "Related2"],
        "a draft cannot reorder the supplied cards"
    );
    fixture.key(79, 1_073_741_903); // Apply
    fixture.key(40, 13);
    fixture.pump();
    assert_eq!(ids(&fixture), ["Related2", "Related1"]);
    assert_eq!(fixture.app.ui.focus(), Focus::DetailTab(0));
    assert!(!texts(&fixture).iter().any(|text| text == "Sort by"));
    fixture.key(81, 1_073_741_905);
    fixture.pump();
    assert!(
        texts(&fixture).iter().any(|text| text == "Alpha"),
        "the actual resulting frame must paint the reordered card"
    );
    let bounds = |caption: &str| {
        fixture
            .app
            .output
            .as_ref()
            .unwrap()
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == caption => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()))
                }
                _ => None,
            })
            .expect("both actual sorted card captions must paint")
    };
    let alpha = bounds("Alpha");
    let beta = bounds("beta");
    let screen = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1920.0, 1080.0));
    assert!(
        alpha.left() < beta.left(),
        "actual painted positions must follow the sorted sequence"
    );
    assert!(
        screen.contains(alpha.min)
            && screen.contains(alpha.max)
            && screen.contains(beta.min)
            && screen.contains(beta.max)
    );
    fixture.key(82, 1_073_741_906);
    fixture.key(40, 13);
    fixture.key(40, 13); // Toggle Title to descending, still pending
    fixture.key(41, 27); // Dismiss
    fixture.pump();
    assert_eq!(ids(&fixture), ["Related2", "Related1"]);
    fixture.key(40, 13); // Reopen copies applied ascending
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let sort = view.detail.as_ref().unwrap().sort.unwrap();
            assert_eq!(sort.pending, sort.applied);
            assert_eq!(
                sort.pending.direction,
                criterion_ui::DetailSortDirection::Ascending
            );
            let feature = view.detail.as_ref().unwrap().featured.as_ref().unwrap();
            assert_eq!(
                feature.cards[0].key.media_id().unwrap().as_str(),
                "Feature1"
            );
            assert_eq!(view.rails[1].title, "Tail unchanged");
        });
    fixture.key(82, 1_073_741_906); // Default
    fixture.key(40, 13);
    fixture.key(79, 1_073_741_903);
    fixture.key(40, 13);
    fixture.pump();
    assert_eq!(ids(&fixture), ["Related1", "Related2"]);
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    let effect = fixture.app.controller.command(
        Command::DetailSort {
            root: criterion_provider::MediaId::new("Listed01").unwrap(),
            action: criterion_ui::DetailSortAction::Open,
        },
        Page::Detail,
        fixture.runtime.handle(),
    );
    assert!(
        matches!(effect, Effect::None),
        "the actual local consumer has no remote effect"
    );
}
