// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual application input and rendering with explicitly synthetic HTTP data.
use super::*;

#[test]
fn unsupported_feature_live_refuses_actual_input_and_forged_card_activation() {
    let mut fixture = Fixture::new(
        false,
        vec![
            list(),
            detail("Listed01", None),
            Step {
                kind: Kind::MyListIds,
                gate: None,
            },
        ],
        5,
    );
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"collection","mediaid":"Listed01","title":"Synthetic Live Feature Collection","featured":{"playlist":[{"contentType":"live","mediaid":"Related1","title":"Synthetic unsupported Live"}]}}"#.to_vec());
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == 3
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    fixture.key(40, 13);
    assert_eq!(fixture.app.ui.page(), Page::Detail);
    assert_eq!(fixture.app.ui.focus(), Focus::FeaturedCard(0));
    assert!(fixture.native_ready("Listed01"));
    let effect = fixture.app.controller.command(
        Command::ActivateCard {
            target: criterion_ui::Target::Native(
                criterion_provider::MediaId::new("Related1").unwrap(),
            ),
            focus: Focus::FeaturedCard(0),
        },
        Page::Detail,
        fixture.runtime.handle(),
    );
    assert!(matches!(effect, Effect::None));
    assert!(fixture.native_ready("Listed01"));
    fixture.key(41, 27);
    assert_eq!(fixture.app.ui.page(), Page::MyList);
}

pub(super) fn featured_fixture(positions: bool) -> Fixture {
    let mut steps = Vec::new();
    if positions {
        steps.push(Step {
            kind: Kind::ContinueWatching,
            gate: None,
        });
    }
    steps.extend([
        list(),
        detail("Listed01", None),
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
    ]);
    let mut fixture = Fixture::new(positions, steps, 5);
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"collection","mediaid":"Listed01","title":"Synthetic Featured Collection","description":"Synthetic collection description","featured":{"title":"Synthetic supplied Feature heading","playlist":[{"contentType":"film","mediaid":"Related1","title":"Synthetic Feature Film","duration":90.5}]},"playlists":[{"type":"GENERIC_PLAYLIST","title":"Synthetic ordinary tab","playlistId":"synthetic-ordinary","playlist":[{"contentType":"episode","mediaid":"Related1","title":"Synthetic ordinary Episode","duration":90.5}]}]}"#.to_vec());
    if positions {
        *fixture.script.continue_body.lock().unwrap() = Some(br#"{"playlist":[{"contentType":"film","mediaid":"Related1","title":"Synthetic saved Feature Film","duration":90.5}],"positions":[{"media_id":"Related1","pos":20,"dur":100}]}"#.to_vec());
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

pub(super) fn assert_feature_projection(fixture: &Fixture, fraction: Option<f32>) {
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.card.title, "Synthetic Featured Collection");
            let featured = detail.featured.as_ref().unwrap();
            assert_eq!(featured.title, Some("Synthetic supplied Feature heading"));
            assert_eq!(featured.cards.len(), 1);
            assert_eq!(
                featured.cards[0].key.media_id().unwrap().as_str(),
                "Related1"
            );
            assert_eq!(featured.cards[0].title, "Synthetic Feature Film");
            assert_eq!(featured.cards[0].action, criterion_ui::CardAction::Open);
            assert_eq!(featured.cards[0].saved_fraction, fraction);
            assert_eq!(featured.cards[0].duration_label, Some("1 min"));
            assert_eq!(view.rails.len(), 1);
            assert_eq!(view.rails[0].title, "Synthetic ordinary tab");
            assert_eq!(view.rails[0].cards.len(), 1);
            assert_eq!(view.rails[0].cards[0].title, "Synthetic ordinary Episode");
            assert_eq!(
                view.rails[0].cards[0].action,
                criterion_ui::CardAction::Play
            );
            assert_eq!(view.rails[0].cards[0].saved_fraction, fraction);
            assert_eq!(detail.selected_playlist, Some(0));
        });
}

#[test]
fn supplied_feature_heading_is_rendered_without_becoming_a_playlist_tab() {
    let mut fixture = featured_fixture(false);
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    fixture.pump();
    assert!(
        fixture
            .app
            .output
            .as_ref()
            .unwrap()
            .shapes
            .iter()
            .any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text)
                if text.galley.job.text == "Synthetic supplied Feature heading")
            }),
        "the actual Collection frame must render its supplied Feature heading"
    );
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert_eq!(view.rails.len(), 1);
            assert_eq!(view.rails[0].title, "Synthetic ordinary tab");
            assert_eq!(view.detail.as_ref().unwrap().selected_playlist, Some(0));
        });
}

#[test]
fn feature_film_and_tab_episode_with_same_id_keep_their_selected_actions() {
    let mut fixture = featured_fixture(false);
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == 3
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    let target =
        criterion_ui::Target::Native(criterion_provider::MediaId::new("Related1").unwrap());
    let effect = fixture.app.controller.command(
        Command::ActivateCard {
            target: target.clone(),
            focus: Focus::Card { row: 0, column: 0 },
        },
        Page::Detail,
        fixture.runtime.handle(),
    );
    assert!(matches!(effect, Effect::Play(id) if id.as_str() == "Related1"));
    assert!(
        fixture.native_ready("Listed01"),
        "Episode Play leaves its origin unchanged"
    );
    for focus in [
        Focus::DetailAction(1),
        Focus::FeaturedCard(1),
        Focus::Card { row: 1, column: 0 },
    ] {
        assert!(matches!(
            fixture.app.controller.command(
                Command::ActivateCard {
                    target: target.clone(),
                    focus
                },
                Page::Detail,
                fixture.runtime.handle()
            ),
            Effect::None
        ));
        assert!(fixture.native_ready("Listed01"));
    }
    assert!(matches!(
        fixture.app.controller.command(
            Command::ActivateCard {
                target: criterion_ui::Target::Native(
                    criterion_provider::MediaId::new("Absent01").unwrap()
                ),
                focus: Focus::FeaturedCard(0),
            },
            Page::Detail,
            fixture.runtime.handle()
        ),
        Effect::None
    ));
    *fixture.script.detail_body.lock().unwrap() = None;
    fixture.script.steps.lock().unwrap().extend([
        detail("Related1", None),
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
    ]);
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    assert_eq!(fixture.app.ui.focus(), Focus::FeaturedCard(0));
    fixture.key(40, 13);
    fixture.wait(|fixture| fixture.native_ready("Related1"));
    assert!(
        matches!(
            fixture.app.controller.command(
                Command::ActivateCard {
                    target: target.clone(),
                    focus: Focus::FeaturedCard(0)
                },
                Page::Detail,
                fixture.runtime.handle()
            ),
            Effect::None
        ),
        "a departed Feature address cannot activate the new Detail"
    );
    assert!(fixture.native_ready("Related1"));
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == 5
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            Kind::NativeDetail("Related1"),
            Kind::MyListIds
        ]
    );
    fixture.script.steps.lock().unwrap().push_back(Step {
        kind: Kind::MyListIds,
        gate: None,
    });
    fixture.key(41, 27);
    assert!(fixture.native_ready("Listed01"));
    assert_eq!(fixture.app.ui.focus(), Focus::FeaturedCard(0));
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == 6
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    fixture.key(41, 27);
    assert_eq!(
        fixture.app.ui.page(),
        Page::MyList,
        "card Play adds no hidden controller history"
    );
    assert!(fixture.script.violation.lock().unwrap().is_none());
}

#[test]
fn feature_progress_is_retired_on_departure_and_warm_back_preserves_public_cards() {
    let mut fixture = featured_fixture(true);
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == 4
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    *fixture.script.detail_body.lock().unwrap() = None;
    fixture.script.steps.lock().unwrap().extend([
        detail("Related1", None),
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
    ]);
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    fixture.key(40, 13);
    fixture.wait(|fixture| fixture.native_ready("Related1"));
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == 6
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    fixture.script.steps.lock().unwrap().push_back(Step {
        kind: Kind::MyListIds,
        gate: None,
    });
    fixture.key(41, 27);
    assert!(fixture.native_ready("Listed01"));
    fixture.wait(|fixture| {
        fixture.script.calls.lock().unwrap().len() == 7
            && fixture.script.active.load(Ordering::SeqCst) == 0
    });
    assert_eq!(fixture.app.ui.focus(), Focus::FeaturedCard(0));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let featured = view.detail.as_ref().unwrap().featured.as_ref().unwrap();
            assert_eq!(featured.cards[0].title, "Synthetic Feature Film");
            assert_eq!(featured.cards[0].saved_fraction, None);
            assert_eq!(view.rails[0].cards[0].saved_fraction, None);
        });
    assert!(
        fixture.app.positions.is_some(),
        "account snapshot survives ordinary metadata navigation"
    );
}

#[test]
fn background_finish_exit_and_same_token_relink_do_not_retain_feature_progress() {
    for transition in ["background", "finish", "exit", "relink"] {
        let mut fixture = featured_fixture(true);
        match transition {
            "background" => fixture.app.background(),
            "finish" => assert!(fixture.app.finish(&fixture.runtime)),
            "exit" => fixture.app.exit(),
            "relink" => {
                fixture
                    .app
                    .command(Command::Logout, fixture.runtime.handle());
                fixture.wait(|fixture| {
                    matches!(fixture.app.authentication.view(), LoginView::SignedOut)
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
                fixture
                    .app
                    .command(Command::Restore(Page::Detail), fixture.runtime.handle());
                assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 2);
            }
            _ => unreachable!(),
        }
        assert!(fixture.app.positions.is_none(), "{transition}");
        fixture
            .app
            .controller
            .view
            .with_view(fixture.app.authentication.view(), |view| {
                let featured = view.detail.as_ref().unwrap().featured.as_ref().unwrap();
                assert_eq!(featured.cards[0].title, "Synthetic Feature Film");
                assert_eq!(featured.cards[0].saved_fraction, None);
                assert_eq!(view.rails[0].cards[0].saved_fraction, None);
            });
        if transition != "relink" {
            assert!(fixture.app.output.as_ref().unwrap().shapes.is_empty());
        }
    }
}

#[test]
fn logout_clears_feature_progress_but_keeps_public_feature_and_tab_identity() {
    let mut fixture = featured_fixture(true);
    assert_feature_projection(&fixture, Some(0.2));
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let featured = view.detail.as_ref().unwrap().featured.as_ref().unwrap();
            assert_eq!(featured.title, Some("Synthetic supplied Feature heading"));
            assert_eq!(featured.cards[0].saved_fraction, Some(0.2));
            assert_eq!(featured.cards[0].duration_label, Some("1 min"));
            assert_eq!(view.rails[0].cards[0].saved_fraction, Some(0.2));
        });
    fixture.key(81, 1_073_741_905);
    fixture.key(81, 1_073_741_905);
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| fixture.issuer.revokes.load(Ordering::SeqCst) == 1);
    assert!(fixture.app.positions.is_none());
    assert!(fixture.native_ready("Listed01"));
    assert_feature_projection(&fixture, None);
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let featured = view.detail.as_ref().unwrap().featured.as_ref().unwrap();
            assert_eq!(featured.title, Some("Synthetic supplied Feature heading"));
            assert_eq!(featured.cards[0].title, "Synthetic Feature Film");
            assert_eq!(featured.cards[0].saved_fraction, None);
            assert_eq!(view.rails[0].title, "Synthetic ordinary tab");
            assert_eq!(view.rails[0].cards[0].saved_fraction, None);
        });
    assert_eq!(fixture.app.ui.focus(), Focus::FeaturedCard(0));
}
