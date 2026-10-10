// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual Application input and paint; all account/provider data is synthetic.
use super::*;

fn film() -> Fixture {
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
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|f| {
        f.native_ready("Listed01")
            && f.script.active.load(Ordering::SeqCst) == 0
            && f.script.calls.lock().unwrap().len() == 3
    });
    fixture
}

fn output(f: &mut Fixture) -> egui::FullOutput {
    let mut result = f.app.take_output().unwrap();
    // This CPU fixture has no GL texture consumer.
    result.textures_delta.clear();
    result
}

fn text(output: &egui::FullOutput, expected: &str) -> Vec<egui::Rect> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(value) if value.galley.job.text == expected => {
                let bounds = shape.shape.visual_bounding_rect();
                assert!(shape.clip_rect.contains_rect(bounds));
                Some(bounds)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn actual_primary_attempt_keeps_ready_detail_actions_and_back_with_visible_unavailable_feedback() {
    let mut f = film();
    assert_eq!(f.app.ui.focus(), Focus::DetailAction(0));
    output(&mut f);
    f.key(40, 13);
    f.pump();
    assert!(
        f.native_ready("Listed01"),
        "unavailable playback must preserve the Ready native Detail"
    );
    let frame = output(&mut f);
    let notices = text(&frame, "Playback unavailable");
    assert!(!notices.is_empty());
    let notice_region = egui::Rect::from_min_max(egui::pos2(150.0, 20.0), egui::pos2(1770.0, 60.0));
    assert!(
        notices
            .iter()
            .all(|bounds| notice_region.contains_rect(*bounds))
    );
    assert!(!text(&frame, "Native Listed01").is_empty());
    assert!(!text(&frame, "WATCH NOW").is_empty());
    assert!(text(&frame, "Unable to load films — try again").is_empty());
    assert_eq!(f.app.ui.focus(), Focus::DetailAction(0));
    assert!(f.script.violation.lock().unwrap().is_none());
    assert_eq!(
        *f.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds
        ]
    );
    f.key(41, 27);
    f.pump();
    assert_eq!(f.app.ui.page(), Page::MyList);
    assert_eq!(f.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    assert!(text(&output(&mut f), "Playback unavailable").is_empty());
}

fn series() -> Fixture {
    let mut f = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: None,
            },
            detail("Listed01", None),
            Step {
                kind: Kind::MyListIds,
                gate: None,
            },
        ],
        5,
    );
    *f.script.continue_body.lock().unwrap() = Some(br#"{"playlist":[{"contentType":"episode","mediaid":"Epis0001","title":"Synthetic clicked episode","series_id":"Listed01"}],"positions":[{"media_id":"Listed01","pos":0,"dur":100,"commentary_track":"root-commentary-outside-media-id"},{"media_id":"Epis0001","pos":95,"dur":100,"series_id":"Listed01"},{"media_id":"Epis0003","pos":20,"dur":100,"commentary_track":"child-commentary-must-not-win"}]}"#.to_vec());
    *f.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"series","mediaid":"Listed01","title":"Synthetic Series","playlists":[{"type":"seasons","title":"Episodes","playlist":[{"season_number":20,"season_title":"First supplied season","episodes":[{"mediaid":"Epis0001","title":"First episode"},{"mediaid":"Epis0002","title":"Second episode"}]},{"season_number":3,"season_title":"Second supplied season","episodes":[{"mediaid":"Epis0003","title":"Resume episode","series_id":"Other001","series_title":"Unrelated DTO parent"}]}]}]}"#.to_vec());
    f.wait(|f| f.saved().len() == 1);
    f.key(81, 1_073_741_905);
    f.key(81, 1_073_741_905);
    f.key(40, 13);
    f.wait(|f| {
        f.native_ready("Listed01")
            && f.script.calls.lock().unwrap().len() == 3
            && f.script.active.load(Ordering::SeqCst) == 0
    });
    f
}

#[test]
fn actual_series_primary_uses_selected_episode_start_and_independent_root_commentary() {
    let mut f = series();
    f.key(40, 13);
    let attempt = f.app.native_attempt.as_ref().unwrap();
    assert_eq!(attempt.selection.selected.as_str(), "Epis0003");
    assert_eq!(attempt.selection.root.as_str(), "Listed01");
    let parent = attempt.selection.parent_series.as_ref().unwrap();
    assert_eq!(
        (parent.0.as_str(), parent.1.as_str()),
        ("Listed01", "Synthetic Series")
    );
    assert_eq!(attempt.start_ms, Some(20_000));
    assert_eq!(
        attempt.commentary.as_deref(),
        Some("root-commentary-outside-media-id")
    );
    assert_eq!(
        attempt.selection.trigger,
        crate::controller::NativePlayTrigger::Primary
    );
    assert!(f.script.violation.lock().unwrap().is_none());
    assert_eq!(
        *f.script.calls.lock().unwrap(),
        [
            Kind::ContinueWatching,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds
        ]
    );
}

#[test]
fn admitted_current_auto_play_attempts_once_without_an_account_playback_request() {
    let mut f = series();
    let attempt = f
        .app
        .native_attempt
        .as_ref()
        .expect("current autoPlay must reach the actual unavailable consumer");
    assert_eq!(
        attempt.selection.trigger,
        crate::controller::NativePlayTrigger::DetailAutoPlay
    );
    assert_eq!(attempt.selection.selected.as_str(), "Epis0003");
    assert_eq!(attempt.start_ms, Some(20_000));
    assert_eq!(attempt.count, 1);
    for _ in 0..8 {
        f.pump();
    }
    assert_eq!(f.app.native_attempt.as_ref().unwrap().count, 1);
    assert!(f.native_ready("Listed01"));
    assert!(!text(&output(&mut f), "Playback unavailable").is_empty());
    assert!(f.script.violation.lock().unwrap().is_none());
    assert_eq!(
        *f.script.calls.lock().unwrap(),
        [
            Kind::ContinueWatching,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds
        ]
    );
}

fn click(f: &mut Fixture, x: i32, y: i32) {
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
        f.app.event(
            Event::PointerMoved { x, y },
            surface,
            &f.runtime,
            f.clock.now(),
        );
        f.app.event(
            Event::PointerButton {
                button: 1,
                pressed,
                x,
                y,
            },
            surface,
            &f.runtime,
            f.clock.now(),
        );
    }
    f.app.consume(&f.runtime, f.clock.now());
    f.pump();
}

#[test]
fn pointer_and_information_primary_share_current_native_context_without_obscuring_actions() {
    let mut f = film();
    let artwork: Vec<_> = f
        .app
        .controller
        .view
        .artwork_bindings()
        .iter()
        .map(|binding| (binding.key.clone(), binding.source.clone()))
        .collect();
    click(&mut f, 380, 660);
    assert_eq!(f.app.native_attempt.as_ref().unwrap().count, 1);
    assert_eq!(
        f.app.native_attempt.as_ref().unwrap().selection.trigger,
        crate::controller::NativePlayTrigger::Primary
    );
    click(&mut f, 670, 660);
    assert_eq!(f.app.ui.focus(), Focus::InformationPrimary);
    click(&mut f, 960, 943);
    assert_eq!(f.app.native_attempt.as_ref().unwrap().count, 2);
    assert!(f.native_ready("Listed01"));
    assert_eq!(
        f.app
            .controller
            .view
            .artwork_bindings()
            .iter()
            .map(|binding| (binding.key.clone(), binding.source.clone()))
            .collect::<Vec<_>>(),
        artwork
    );
    let frame = output(&mut f);
    assert!(!text(&frame, "Playback unavailable").is_empty());
    assert!(!text(&frame, "WATCH NOW").is_empty());
    assert!(text(&frame, "Unable to load films — try again").is_empty());
    assert_eq!(f.script.calls.lock().unwrap().len(), 3);
}

#[test]
fn collection_episode_at_the_exact_address_plays_without_a_primary_or_invented_parent() {
    let mut f = super::native_featured_tests::featured_fixture(false);
    assert!(
        f.app
            .controller
            .view
            .with_view(LoginView::SignedIn, |view| view
                .detail
                .as_ref()
                .unwrap()
                .primary_playback_target
                .is_none())
    );
    for _ in 0..4 {
        f.key(81, 1_073_741_905);
    }
    assert_eq!(f.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    f.key(40, 13);
    let attempt = f.app.native_attempt.as_ref().unwrap();
    assert_eq!(attempt.selection.selected.as_str(), "Related1");
    assert_eq!(attempt.selection.root.as_str(), "Listed01");
    assert_eq!(
        attempt.selection.trigger,
        crate::controller::NativePlayTrigger::EpisodeCard(Focus::Card { row: 0, column: 0 })
    );
    assert!(attempt.selection.parent_series.is_none());
    assert!(attempt.commentary.is_none());
    assert_eq!(attempt.start_ms, Some(0));
    assert!(f.native_ready("Listed01"));
    // The same ID at its Feature address is Film/Open, not this Episode/Play.
    *f.script.detail_body.lock().unwrap() = None;
    f.script.steps.lock().unwrap().extend([
        detail("Related1", None),
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
    ]);
    f.key(82, 1_073_741_906);
    f.key(82, 1_073_741_906);
    assert_eq!(f.app.ui.focus(), Focus::FeaturedCard(0));
    f.key(40, 13);
    f.wait(|f| {
        f.native_ready("Related1")
            && f.script.active.load(Ordering::SeqCst) == 0
            && f.script.calls.lock().unwrap().len() == 5
    });
    assert!(f.app.native_attempt.is_none());
    assert!(text(&output(&mut f), "Playback unavailable").is_empty());
    f.script.steps.lock().unwrap().push_back(Step {
        kind: Kind::MyListIds,
        gate: None,
    });
    f.key(41, 27);
    f.wait(|f| {
        f.native_ready("Listed01")
            && f.script.calls.lock().unwrap().len() == 6
            && f.script.active.load(Ordering::SeqCst) == 0
    });
    assert_eq!(f.app.ui.focus(), Focus::FeaturedCard(0));
    assert!(f.app.native_attempt.is_none());
    assert!(
        text(&output(&mut f), "Playback unavailable").is_empty(),
        "feedback is not part of cached Detail history"
    );
}

#[test]
fn actual_series_episode_card_uses_its_own_id_start_and_no_root_commentary() {
    let mut f = series();
    for _ in 0..3 {
        f.key(81, 1_073_741_905);
    }
    assert_eq!(f.app.ui.focus(), Focus::DetailSeason(1));
    f.key(80, 1_073_741_904);
    f.key(81, 1_073_741_905);
    f.key(79, 1_073_741_903);
    assert_eq!(f.app.ui.focus(), Focus::Card { row: 0, column: 1 });
    f.key(40, 13);
    let attempt = f.app.native_attempt.as_ref().unwrap();
    assert_eq!(attempt.selection.selected.as_str(), "Epis0002");
    assert_eq!(
        attempt.selection.parent_series.as_ref().unwrap().0.as_str(),
        "Listed01"
    );
    assert_eq!(
        attempt.selection.trigger,
        crate::controller::NativePlayTrigger::EpisodeCard(Focus::Card { row: 0, column: 1 })
    );
    assert_eq!(
        attempt.start_ms,
        Some(0),
        "saved Epis0003 is not this selected Episode"
    );
    assert!(
        attempt.commentary.is_none(),
        "root commentary belongs to the primary/autoPlay caller"
    );
    assert_eq!(attempt.count, 2);
    assert!(f.native_ready("Listed01"));
    assert_eq!(f.script.calls.lock().unwrap().len(), 3);
}

#[test]
fn standalone_episode_primary_does_not_repurpose_optional_dto_parent_fields() {
    let mut f = Fixture::new(
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
    *f.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"episode","mediaid":"Listed01","title":"Standalone Episode","series_id":"Other001","series_title":"Unproved player parent"}"#.to_vec());
    f.open_list();
    select_listed(&mut f);
    f.wait(|f| {
        f.native_ready("Listed01")
            && f.script.calls.lock().unwrap().len() == 3
            && f.script.active.load(Ordering::SeqCst) == 0
    });
    f.key(40, 13);
    let attempt = f.app.native_attempt.as_ref().unwrap();
    assert_eq!(attempt.selection.selected.as_str(), "Listed01");
    assert!(attempt.selection.parent_series.is_none());
    assert_eq!(attempt.start_ms, Some(0));
    assert!(f.native_ready("Listed01"));
}

#[test]
fn unavailable_notice_retires_on_background_exit_finish_and_identical_token_relink() {
    for transition in ["background", "exit", "finish", "relink"] {
        let mut f = film();
        f.key(40, 13);
        assert!(f.app.native_attempt.is_some());
        let epoch = f.app.account_epoch;
        match transition {
            "background" => {
                f.app.background();
                f.script.steps.lock().unwrap().push_back(Step {
                    kind: Kind::MyListIds,
                    gate: None,
                });
                f.app.foreground(f.runtime.handle());
                f.wait(|f| {
                    f.script.calls.lock().unwrap().len() == 4
                        && f.script.active.load(Ordering::SeqCst) == 0
                });
            }
            "exit" => f.app.exit(),
            "finish" => {
                assert!(f.app.finish(&f.runtime));
            }
            "relink" => {
                f.app.command(Command::Logout, f.runtime.handle());
                f.wait(|f| matches!(f.app.authentication.view(), LoginView::SignedOut));
                f.app.command(Command::Authenticate, f.runtime.handle());
                f.wait(|f| matches!(f.app.authentication.view(), LoginView::Awaiting { .. }));
                f.clock.0.store(10, Ordering::SeqCst);
                f.wait(|f| f.app.authentication.signed_in());
                f.script.steps.lock().unwrap().push_back(Step {
                    kind: Kind::MyListIds,
                    gate: None,
                });
                f.app
                    .command(Command::Restore(Page::Detail), f.runtime.handle());
                f.wait(|f| {
                    f.script.calls.lock().unwrap().len() == 4
                        && f.script.active.load(Ordering::SeqCst) == 0
                });
                assert_ne!(f.app.account_epoch, epoch);
                assert_eq!(f.issuer.tokens.load(Ordering::SeqCst), 2);
            }
            _ => unreachable!(),
        }
        assert!(f.app.native_play_notice.is_none(), "{transition}");
        assert!(f.app.native_attempt.is_none(), "{transition}");
        assert_eq!(
            f.script.calls.lock().unwrap().len(),
            if matches!(transition, "background" | "relink") {
                4
            } else {
                3
            },
            "{transition}"
        );
    }
}

#[test]
fn unsigned_primary_activates_then_cancel_restores_detail_without_replaying() {
    let mut f = film();
    f.app.command(Command::Logout, f.runtime.handle());
    f.wait(|f| matches!(f.app.authentication.view(), LoginView::SignedOut));
    f.key(40, 13);
    f.wait(|f| matches!(f.app.authentication.view(), LoginView::Awaiting { .. }));
    assert_eq!(f.app.ui.page(), Page::Login);
    assert!(f.app.native_attempt.is_none());
    f.key(41, 27);
    f.wait(|f| {
        f.app.ui.page() == Page::Detail
            && matches!(f.app.authentication.view(), LoginView::SignedOut)
    });
    assert!(f.native_ready("Listed01"));
    assert_eq!(f.app.ui.focus(), Focus::DetailAction(0));
    assert!(text(&output(&mut f), "Playback unavailable").is_empty());
    assert_eq!(f.script.calls.lock().unwrap().len(), 3);
}

fn pending_series(gate: Arc<Gate>) -> Fixture {
    let mut f = Fixture::new(
        true,
        vec![
            Step {
                kind: Kind::ContinueWatching,
                gate: None,
            },
            detail("Listed01", Some(gate.clone())),
        ],
        5,
    );
    *f.script.continue_body.lock().unwrap() = Some(br#"{"playlist":[{"contentType":"episode","mediaid":"Epis0001","title":"Pending clicked Episode","series_id":"Listed01"}],"positions":[{"media_id":"Epis0001","pos":20,"dur":100}]}"#.to_vec());
    *f.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"series","mediaid":"Listed01","title":"Pending Series","playlists":[{"type":"seasons","title":"Episodes","playlist":[{"season_number":1,"season_title":"First","episodes":[{"mediaid":"Epis0001","title":"Episode"}]}]}]}"#.to_vec());
    f.wait(|f| f.saved().len() == 1);
    f.key(81, 1_073_741_905);
    f.key(81, 1_073_741_905);
    f.key(40, 13);
    f.wait(|_| gate.entered.load(Ordering::SeqCst));
    f
}

#[test]
fn forged_play_during_native_loading_cannot_fall_through_to_public_load_error() {
    let gate = Arc::new(Gate::default());
    let mut f = pending_series(gate.clone());
    assert_eq!(status(&f), LoadState::Loading);
    f.app.command(
        Command::Play(criterion_provider::MediaId::new("Listed01").unwrap()),
        f.runtime.handle(),
    );
    assert_eq!(
        status(&f),
        LoadState::Loading,
        "a native action has no admitted current context yet"
    );
    f.app.background();
    f.wait(|_| gate.retired.load(Ordering::SeqCst) == 1);
}

#[test]
fn departed_background_logged_out_and_expired_auto_play_reads_cannot_publish_an_attempt() {
    for transition in ["departure", "background", "logout", "deadline"] {
        let gate = Arc::new(Gate::default());
        let mut f = pending_series(gate.clone());
        match transition {
            "departure" => f.key(41, 27),
            "background" => f.app.background(),
            "logout" => f.app.command(Command::Logout, f.runtime.handle()),
            "deadline" => {
                f.clock.0.store(65, Ordering::SeqCst);
                f.pump();
            }
            _ => unreachable!(),
        }
        f.wait(|_| gate.retired.load(Ordering::SeqCst) == 1);
        gate.release.notify_one();
        for _ in 0..8 {
            f.pump();
        }
        assert!(f.app.native_attempt.is_none(), "{transition}");
        assert!(f.app.native_play_notice.is_none(), "{transition}");
        assert_eq!(f.script.calls.lock().unwrap().len(), 2, "{transition}");
        assert_eq!(f.script.active.load(Ordering::SeqCst), 0, "{transition}");
        assert!(f.script.violation.lock().unwrap().is_none(), "{transition}");
    }
}

#[test]
fn backgrounded_pending_auto_play_reloads_metadata_without_replaying_the_retired_intent() {
    let gate = Arc::new(Gate::default());
    let mut f = pending_series(gate.clone());
    f.app.background();
    f.wait(|_| gate.retired.load(Ordering::SeqCst) == 1);
    f.script.steps.lock().unwrap().extend([
        detail("Listed01", None),
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
    ]);
    f.app.foreground(f.runtime.handle());
    f.wait(|f| {
        f.native_ready("Listed01")
            && f.script.calls.lock().unwrap().len() == 4
            && f.script.active.load(Ordering::SeqCst) == 0
    });
    assert!(f.app.native_attempt.is_none());
    assert!(text(&output(&mut f), "Playback unavailable").is_empty());
    f.key(40, 13);
    let attempt = f.app.native_attempt.as_ref().unwrap();
    assert_eq!(
        attempt.selection.trigger,
        crate::controller::NativePlayTrigger::Primary
    );
    assert_eq!(attempt.selection.selected.as_str(), "Epis0001");
    assert_eq!(
        attempt.start_ms,
        Some(0),
        "background retired private saved positions"
    );
    assert_eq!(attempt.count, 1);
    assert!(f.native_ready("Listed01"));
    assert_eq!(f.script.calls.lock().unwrap().len(), 4);
}

#[test]
fn saved_start_literal_boundaries_use_seconds_and_refuse_milliseconds_overflow() {
    let id = criterion_provider::MediaId::new("Listed01").unwrap();
    assert_eq!(crate::controller::saved_start_ms(None), Some(0));
    for (pos, dur, expected) in [
        (0, 100, Some(0)),
        (-1, 100, Some(0)),
        (1, 0, Some(0)),
        (1, -1, Some(0)),
        (94, 100, Some(94_000)),
        (95, 100, Some(0)),
        (100, 100, Some(0)),
        (283, 299, Some(283_000)),
        (285, 299, Some(0)),
        (293, 300, Some(293_000)),
        (294, 300, Some(0)),
        (979, 1000, Some(979_000)),
        (980, 1000, Some(0)),
        (i64::MAX / 2, i64::MAX, None),
    ] {
        let position = criterion_account::Position {
            media_id: id.clone(),
            pos,
            dur,
            series_id: None,
            series_title: None,
            commentary_track: None,
        };
        assert_eq!(
            crate::controller::saved_start_ms(Some(&position)),
            expected,
            "pos={pos}, dur={dur}"
        );
    }
}
