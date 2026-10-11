// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual Detail input and account owner; every external response is synthetic.
use super::*;

struct Database(Arc<Mutex<(u64, bool)>>);
impl criterion_platform::write_fence::Db8Transport for Database {
    fn get(
        &mut self,
        _: std::time::Instant,
    ) -> Result<Vec<u8>, criterion_platform::write_fence::FenceError> {
        let (revision, issued) = *self.0.lock().unwrap();
        Ok(format!(r#"{{"returnValue":true,"results":[{{"_id":"crit.write.v1","_kind":"com.mikestopcontinues.criterion.unofficial.issuedwrite:1","_rev":{revision},"version":1,"possiblyIssued":{issued}}}]}}"#).into_bytes())
    }
    fn put(
        &mut self,
        revision: u64,
        issued: bool,
        _: std::time::Instant,
    ) -> Result<Vec<u8>, criterion_platform::write_fence::FenceError> {
        let mut record = self.0.lock().unwrap();
        if record.0 != revision {
            return Err(criterion_platform::write_fence::FenceError::Invalid);
        }
        *record = (revision + 1, issued);
        Ok(format!(
            r#"{{"returnValue":true,"results":[{{"id":"crit.write.v1","rev":{}}}]}}"#,
            record.0
        )
        .into_bytes())
    }
}

fn enable_writes(fixture: &mut Fixture) -> Arc<Mutex<(u64, bool)>> {
    let record = Arc::new(Mutex::new((7, false)));
    let fence =
        criterion_platform::write_fence::Db8WriteFence::with_transport(Database(record.clone()))
            .unwrap();
    fixture.app.accounts.use_write_fence(Arc::new(fence));
    fixture.wait(|fixture| fixture.app.accounts.write_ready());
    record
}

fn toggle(fixture: &mut Fixture) {
    fixture.key(79, 1_073_741_903);
    fixture.key(79, 1_073_741_903);
    assert_eq!(fixture.app.ui.focus(), criterion_ui::Focus::DetailAction(2));
    fixture.key(40, 13);
}

#[test]
fn current_known_absence_detail_toggle_issues_one_exact_native_add() {
    let write = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(None),
        Step {
            kind: Kind::AddWatchList,
            gate: Some(write.clone()),
        },
    ]);
    *fixture.script.ids_result.lock().unwrap() = Ok(br#"{"watchlist":[],"positions":[]}"#.to_vec());
    fixture.wait(|fixture| has_membership_caption(fixture, "NOT IN MY LIST"));
    let _record = enable_writes(&mut fixture);
    toggle(&mut fixture);
    for _ in 0..128 {
        fixture.pump();
        if fixture.script.calls.lock().unwrap().len() == 4 {
            break;
        }
    }
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            Kind::AddWatchList,
        ],
        "actual current Detail input must reach the native write transport once"
    );
    write.release.notify_one();
    assert!(fixture.app.finish(&fixture.runtime));
    assert_eq!(write.retired.load(Ordering::SeqCst), 1);
}

#[test]
fn held_write_logout_erases_now_but_finish_joins_ack_and_then_revokes_once() {
    let write = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(None),
        Step {
            kind: Kind::RemoveWatchList,
            gate: Some(write.clone()),
        },
    ]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let record = enable_writes(&mut fixture);
    toggle(&mut fixture);
    fixture.wait(|_| write.entered.load(Ordering::SeqCst));
    assert!(record.lock().unwrap().1);
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
    for _ in 0..8 {
        fixture.app.poll(&fixture.runtime, false);
    }
    assert_eq!(fixture.issuer.revokes.load(Ordering::SeqCst), 0);
    assert_eq!(
        write.retired.load(Ordering::SeqCst),
        0,
        "background cannot abort an issued write"
    );
    assert!(fixture.app.accounts.write_active());
    write.release.notify_one();
    assert!(
        fixture.app.finish(&fixture.runtime),
        "join then deferred explicit revoke must complete"
    );
    assert_eq!(write.retired.load(Ordering::SeqCst), 1);
    assert!(!record.lock().unwrap().1);
    assert_eq!(fixture.issuer.revokes.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            Kind::RemoveWatchList,
        ]
    );
}

#[test]
fn acknowledged_sync_flags_require_a_separate_ids_observation_and_never_mean_applied() {
    for receipt in [
        br#"{"sync":false}"#.as_slice(),
        br#"{"sync":true}"#.as_slice(),
    ] {
        let write = Arc::new(Gate::default());
        let observation = Arc::new(Gate::default());
        let mut fixture = ordinary_detail(vec![
            list(),
            detail("Listed01", None),
            ids(None),
            Step {
                kind: Kind::AddWatchList,
                gate: Some(write.clone()),
            },
            ids(Some(observation.clone())),
        ]);
        *fixture.script.ids_result.lock().unwrap() =
            Ok(br#"{"watchlist":[],"positions":[]}"#.to_vec());
        *fixture.script.write_result.lock().unwrap() = Ok(receipt.to_vec());
        fixture.wait(|fixture| has_membership_caption(fixture, "NOT IN MY LIST"));
        let record = enable_writes(&mut fixture);
        toggle(&mut fixture);
        fixture.wait(|_| write.entered.load(Ordering::SeqCst));
        assert!(has_membership_caption(&fixture, "UPDATING MY LIST"));
        assert!(
            record.lock().unwrap().1,
            "reservation precedes provider contact"
        );
        fixture.key(40, 13);
        click_list(&mut fixture);
        for _ in 0..8 {
            fixture.pump();
        }
        assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
        write.release.notify_one();
        fixture.wait(|_| observation.entered.load(Ordering::SeqCst));
        assert!(has_membership_caption(&fixture, "CHECKING MY LIST"));
        assert!(
            !record.lock().unwrap().1,
            "native ack earns durable completion"
        );
        observation.release.notify_one();
        fixture.wait(|fixture| has_membership_caption(fixture, "NOT IN MY LIST"));
        assert_eq!(
            *fixture.script.calls.lock().unwrap(),
            [
                Kind::WatchList,
                Kind::NativeDetail("Listed01"),
                Kind::MyListIds,
                Kind::AddWatchList,
                Kind::MyListIds,
            ]
        );
        assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn possibly_issued_write_dirties_retained_shelf_and_back_fetches_a_new_first_page() {
    let write = Arc::new(Gate::default());
    let reload = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        false,
        vec![
            list(),
            detail("Listed01", None),
            ids(None),
            Step {
                kind: Kind::RemoveWatchList,
                gate: Some(write.clone()),
            },
            ids(None),
            Step {
                kind: Kind::WatchList,
                gate: Some(reload.clone()),
            },
        ],
        5,
    );
    *fixture.script.watch_list_body.lock().unwrap() = Some(br#"{"paging":{"page_limit":50},"type_counts":{"film":2},"playlist":[{"mediaid":"Related1","title":"Synthetic old first row","contentType":"film"},{"mediaid":"Listed01","title":"Synthetic listed film","contentType":"film"}]}"#.to_vec());
    fixture.open_list();
    if matches!(fixture.app.ui.focus(), criterion_ui::Focus::MyListGroup(_)) {
        fixture.key(81, 1_073_741_905);
    }
    fixture.key(79, 1_073_741_903);
    assert_eq!(
        fixture.app.ui.focus(),
        criterion_ui::Focus::Card { row: 0, column: 1 }
    );
    fixture.key(40, 13);
    fixture.wait(|fixture| {
        fixture.native_ready("Listed01") && has_membership_caption(fixture, "IN MY LIST")
    });
    let record = enable_writes(&mut fixture);
    toggle(&mut fixture);
    fixture.wait(|_| write.entered.load(Ordering::SeqCst));
    *fixture.script.ids_result.lock().unwrap() = Ok(br#"{"watchlist":[],"positions":[]}"#.to_vec());
    *fixture.script.watch_list_body.lock().unwrap() =
        Some(br#"{"paging":{"page_limit":50},"type_counts":{"film":0},"playlist":[]}"#.to_vec());
    write.release.notify_one();
    fixture.wait(|fixture| has_membership_caption(fixture, "NOT IN MY LIST"));
    assert!(!record.lock().unwrap().1);
    fixture.key(41, 27);
    fixture.wait(|_| reload.entered.load(Ordering::SeqCst));
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    assert_eq!(
        fixture.app.ui.focus(),
        criterion_ui::Focus::MyListGroup(criterion_ui::MyListGroup::All)
    );
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            assert!(
                view.cards.is_empty(),
                "invalidated history cannot republish removed traversal"
            );
            assert_eq!(view.catalog.unwrap().first, 0);
        });
    reload.release.notify_one();
    fixture.wait(|fixture| status(fixture) == criterion_ui::LoadState::Empty);
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            Kind::RemoveWatchList,
            Kind::MyListIds,
            Kind::WatchList,
        ]
    );
}

fn choose_group(fixture: &mut Fixture, group: criterion_ui::MyListGroup) {
    use criterion_ui::{Focus, MyListGroup};
    const ORDER: [MyListGroup; 6] = [
        MyListGroup::All,
        MyListGroup::FilmsAndSeries,
        MyListGroup::Collections,
        MyListGroup::OriginalsAndFranchises,
        MyListGroup::Supplements,
        MyListGroup::Categories,
    ];
    if matches!(fixture.app.ui.focus(), Focus::Card { row: 0, .. }) {
        fixture.key(82, 1_073_741_906);
    }
    for _ in 0..6 {
        let Focus::MyListGroup(current) = fixture.app.ui.focus() else {
            panic!("actual My List group header must own focus");
        };
        if current == group {
            fixture.key(40, 13);
            return;
        }
        let current = ORDER.iter().position(|value| *value == current).unwrap();
        let target = ORDER.iter().position(|value| *value == group).unwrap();
        if current < target {
            fixture.key(79, 1_073_741_903);
        } else {
            fixture.key(80, 1_073_741_904);
        }
    }
    panic!("actual My List group was unavailable within six choices");
}

fn filtered_write_back_preserves_unknown_groups(write_kind: Kind) {
    use criterion_ui::MyListGroup;
    let write = Arc::new(Gate::default());
    let reload = Arc::new(Gate::default());
    let supplement = Arc::new(Gate::default());
    let all = Arc::new(Gate::default());
    let mut fixture = Fixture::new(
        false,
        vec![
            list(),
            Step {
                kind: Kind::CollectionWatchList,
                gate: None,
            },
            detail("Listed01", None),
            ids(None),
            Step {
                kind: write_kind,
                gate: Some(write.clone()),
            },
            ids(None),
            Step {
                kind: Kind::CollectionWatchList,
                gate: Some(reload.clone()),
            },
            Step {
                kind: Kind::SupplementWatchList,
                gate: Some(supplement.clone()),
            },
            Step {
                kind: Kind::WatchList,
                gate: Some(all.clone()),
            },
        ],
        5,
    );
    *fixture.script.native_kind.lock().unwrap() = "collection";
    *fixture.script.watch_list_body.lock().unwrap() = Some(br#"{"paging":{"page_limit":50},"type_counts":{"collection":2,"supplement":1},"playlist":[{"mediaid":"Listed01","title":"Synthetic listed film","contentType":"collection"}]}"#.to_vec());
    let (before, after, before_caption, after_caption) = match write_kind {
        Kind::AddWatchList => (
            br#"{"watchlist":[],"positions":[]}"#.as_slice(),
            br#"{"watchlist":["Listed01"],"positions":[]}"#.as_slice(),
            "NOT IN MY LIST",
            "IN MY LIST",
        ),
        Kind::RemoveWatchList => (
            br#"{"watchlist":["Listed01"],"positions":[]}"#.as_slice(),
            br#"{"watchlist":[],"positions":[]}"#.as_slice(),
            "IN MY LIST",
            "NOT IN MY LIST",
        ),
        _ => panic!("fixture requires one explicit Add or Remove"),
    };
    *fixture.script.ids_result.lock().unwrap() = Ok(before.to_vec());
    fixture.open_list();
    *fixture.script.watch_list_body.lock().unwrap() = Some(br#"{"paging":{"page_limit":50},"type_counts":{"collection":99},"playlist":[{"mediaid":"Listed01","title":"Synthetic listed film","contentType":"collection"}]}"#.to_vec());
    choose_group(&mut fixture, MyListGroup::Collections);
    fixture.wait(|fixture| status(fixture) == criterion_ui::LoadState::Ready);
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let list = view.my_list.unwrap();
            assert_eq!(list.selected, MyListGroup::Collections);
            assert_eq!(
                list.choices
                    .iter()
                    .map(|choice| (choice.group, choice.count))
                    .collect::<Vec<_>>(),
                [
                    (MyListGroup::All, Some(3)),
                    (MyListGroup::Collections, Some(2)),
                    (MyListGroup::Supplements, Some(1)),
                ],
                "a filtered count map is not a global count authority"
            );
        });
    select_listed(&mut fixture);
    fixture.wait(|fixture| {
        fixture.native_ready("Listed01") && has_membership_caption(fixture, before_caption)
    });
    let record = enable_writes(&mut fixture);
    toggle(&mut fixture);
    fixture.wait(|_| write.entered.load(Ordering::SeqCst));
    *fixture.script.ids_result.lock().unwrap() = Ok(after.to_vec());
    *fixture.script.watch_list_body.lock().unwrap() = Some(br#"{"paging":{"page_limit":50},"type_counts":{"collection":77},"playlist":[{"mediaid":"Related1","title":"Remaining collection","contentType":"collection"}]}"#.to_vec());
    write.release.notify_one();
    fixture.wait(|fixture| has_membership_caption(fixture, after_caption));
    assert!(!record.lock().unwrap().1);
    fixture.key(41, 27);
    fixture.wait(|_| reload.entered.load(Ordering::SeqCst));
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    assert_eq!(
        fixture.app.ui.focus(),
        criterion_ui::Focus::MyListGroup(MyListGroup::Collections)
    );
    reload.release.notify_one();
    fixture.wait(|fixture| status(fixture) == criterion_ui::LoadState::Ready);
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let list = view.my_list.unwrap();
            assert_eq!(list.selected, MyListGroup::Collections);
            assert_eq!(view.cards[0].title, "Remaining collection");
            assert_eq!(
                list.choices
                    .iter()
                    .map(|choice| (choice.group, choice.count))
                    .collect::<Vec<_>>(),
                [
                    (MyListGroup::All, None),
                    (MyListGroup::FilmsAndSeries, None),
                    (MyListGroup::Collections, None),
                    (MyListGroup::OriginalsAndFranchises, None),
                    (MyListGroup::Supplements, None),
                    (MyListGroup::Categories, None),
                ],
                "Back must preserve access to groups whose current counts are unknown"
            );
        });
    *fixture.script.watch_list_body.lock().unwrap() = Some(br#"{"paging":{"page_limit":50},"type_counts":{"supplement":88},"playlist":[{"mediaid":"Related1","title":"Still populated supplement","contentType":"supplement"}]}"#.to_vec());
    choose_group(&mut fixture, MyListGroup::Supplements);
    fixture.wait(|_| supplement.entered.load(Ordering::SeqCst));
    supplement.release.notify_one();
    fixture.wait(|fixture| status(fixture) == criterion_ui::LoadState::Ready);
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let list = view.my_list.unwrap();
            assert_eq!(list.selected, MyListGroup::Supplements);
            assert_eq!(view.cards[0].title, "Still populated supplement");
            assert!(list.choices.iter().all(|choice| choice.count.is_none()));
        });
    *fixture.script.watch_list_body.lock().unwrap() = Some(br#"{"paging":{"page_limit":50},"type_counts":{"collection":1,"supplement":0},"playlist":[{"mediaid":"Related1","title":"Remaining collection","contentType":"collection"}]}"#.to_vec());
    choose_group(&mut fixture, MyListGroup::All);
    fixture.wait(|_| all.entered.load(Ordering::SeqCst));
    all.release.notify_one();
    fixture.wait(|fixture| status(fixture) == criterion_ui::LoadState::Ready);
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let list = view.my_list.unwrap();
            assert_eq!(list.selected, MyListGroup::All);
            assert_eq!(
                list.choices
                    .iter()
                    .map(|choice| (choice.group, choice.count))
                    .collect::<Vec<_>>(),
                [
                    (MyListGroup::All, Some(1)),
                    (MyListGroup::Collections, Some(1)),
                ],
                "fresh All restores global counts and hides known-empty groups"
            );
        });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::CollectionWatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            write_kind,
            Kind::MyListIds,
            Kind::CollectionWatchList,
            Kind::SupplementWatchList,
            Kind::WatchList,
        ]
    );
    assert_eq!(fixture.script.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
}

#[test]
fn filtered_collection_add_back_keeps_other_groups_accessible_until_fresh_all_counts() {
    filtered_write_back_preserves_unknown_groups(Kind::AddWatchList);
}

#[test]
fn filtered_collection_remove_back_keeps_other_groups_accessible_until_fresh_all_counts() {
    filtered_write_back_preserves_unknown_groups(Kind::RemoveWatchList);
}

#[test]
fn departure_defers_new_detail_until_issued_write_settles_and_refuses_old_visit_publication() {
    let write = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(None),
        Step {
            kind: Kind::RemoveWatchList,
            gate: Some(write.clone()),
        },
        detail("Related1", None),
        ids(None),
        ids(None),
    ]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let _record = enable_writes(&mut fixture);
    toggle(&mut fixture);
    fixture.wait(|_| write.entered.load(Ordering::SeqCst));
    open_related(&mut fixture);
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
    assert_eq!(write.retired.load(Ordering::SeqCst), 0);
    write.release.notify_one();
    fixture.wait(|fixture| {
        fixture.native_ready("Related1") && has_membership_caption(fixture, "NOT IN MY LIST")
    });
    fixture.key(41, 27);
    fixture.wait(|fixture| {
        fixture.native_ready("Listed01") && has_membership_caption(fixture, "IN MY LIST")
    });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            Kind::RemoveWatchList,
            Kind::NativeDetail("Related1"),
            Kind::MyListIds,
            Kind::MyListIds,
        ]
    );
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
}

#[test]
fn unconfirmed_write_matching_read_and_restart_cannot_clear_the_global_fence() {
    for result in [
        Err(criterion_account::Error::Unavailable),
        Err(criterion_account::Error::HttpStatus(503)),
        Ok(br#"{"sync":"invalid"}"#.to_vec()),
    ] {
        let record = {
            let mut fixture = ordinary_detail(vec![
                list(),
                detail("Listed01", None),
                ids(None),
                Step {
                    kind: Kind::RemoveWatchList,
                    gate: None,
                },
                ids(None),
            ]);
            fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
            let record = enable_writes(&mut fixture);
            *fixture.script.write_result.lock().unwrap() = result;
            toggle(&mut fixture);
            fixture.wait(|fixture| has_membership_caption(fixture, "MY LIST UNAVAILABLE"));
            assert!(record.lock().unwrap().1);
            for _ in 0..8 {
                fixture.pump();
            }
            assert_eq!(
                fixture.script.calls.lock().unwrap().len(),
                4,
                "no retry or optimistic observation"
            );
            fixture.app.background();
            fixture.app.foreground(fixture.runtime.handle());
            fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
            fixture.key(40, 13);
            for _ in 0..8 {
                fixture.pump();
            }
            assert_eq!(fixture.script.calls.lock().unwrap().len(), 5);
            assert!(!fixture.app.accounts.write_ready());
            assert!(
                record.lock().unwrap().1,
                "a matching read does not authorize clear"
            );
            record
        };
        let mut restarted = ordinary_detail(vec![list(), detail("Listed01", None), ids(None)]);
        restarted.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
        let fence = criterion_platform::write_fence::Db8WriteFence::with_transport(Database(
            record.clone(),
        ))
        .unwrap();
        use criterion_platform::write_fence::IssuedWriteFence;
        assert_eq!(
            restarted.runtime.block_on(fence.state()).unwrap(),
            criterion_platform::write_fence::FenceState::PossiblyIssued
        );
        restarted.app.accounts.use_write_fence(Arc::new(fence));
        for _ in 0..128 {
            restarted.pump();
        }
        toggle(&mut restarted);
        for _ in 0..8 {
            restarted.pump();
        }
        assert!(!restarted.app.accounts.write_ready());
        assert_eq!(restarted.script.calls.lock().unwrap().len(), 3);
        assert!(record.lock().unwrap().1);
    }
}

#[test]
fn definite_not_issued_clears_durably_preserves_known_membership_and_never_retries() {
    for error in [
        criterion_account::Error::Busy,
        criterion_account::Error::InvalidRequest,
    ] {
        let mut fixture = ordinary_detail(vec![
            list(),
            detail("Listed01", None),
            ids(None),
            Step {
                kind: Kind::RemoveWatchList,
                gate: None,
            },
        ]);
        fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
        let record = enable_writes(&mut fixture);
        *fixture.script.write_result.lock().unwrap() = Err(error);
        toggle(&mut fixture);
        fixture.wait(|fixture| {
            fixture.app.accounts.write_ready() && has_membership_caption(fixture, "IN MY LIST")
        });
        for _ in 0..8 {
            fixture.pump();
        }
        assert!(!record.lock().unwrap().1);
        assert_eq!(
            *fixture.script.calls.lock().unwrap(),
            [
                Kind::WatchList,
                Kind::NativeDetail("Listed01"),
                Kind::MyListIds,
                Kind::RemoveWatchList,
            ]
        );
    }
}

#[test]
fn stale_root_visit_or_known_bit_refuses_before_durable_reservation_or_transport() {
    let mut fixture = ordinary_detail(vec![list(), detail("Listed01", None), ids(None)]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let record = enable_writes(&mut fixture);
    let visit = fixture.app.controller.membership_visit().unwrap();
    for (root, from_visit, expected_present) in [
        ("Other001", Some(visit), Some(true)),
        ("Listed01", Some(visit + 1), Some(true)),
        ("Listed01", None, Some(true)),
        ("Listed01", Some(visit), Some(false)),
        ("Listed01", Some(visit), None),
    ] {
        fixture.app.command(
            Command::ToggleList {
                root: criterion_provider::MediaId::new(root).unwrap(),
                from_visit,
                expected_present,
            },
            fixture.runtime.handle(),
        );
        fixture.pump();
    }
    assert_eq!(*record.lock().unwrap(), (7, false));
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    assert!(has_membership_caption(&fixture, "IN MY LIST"));
}

#[test]
fn every_ordinary_native_kind_adds_the_selected_root_with_its_exact_type_and_live_refuses() {
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
        let write = Arc::new(Gate::default());
        let mut steps = vec![list(), detail("Listed01", None)];
        if kind != "live" {
            steps.extend([
                ids(None),
                Step {
                    kind: Kind::AddWatchList,
                    gate: Some(write.clone()),
                },
            ]);
        }
        let mut fixture = Fixture::new(false, steps, 5);
        *fixture.script.native_kind.lock().unwrap() = kind;
        *fixture.script.ids_result.lock().unwrap() =
            Ok(br#"{"watchlist":[],"positions":[]}"#.to_vec());
        fixture.open_list();
        select_listed(&mut fixture);
        fixture.wait(|fixture| fixture.native_ready("Listed01"));
        fixture.wait(|fixture| {
            has_membership_caption(
                fixture,
                if kind == "live" {
                    "MY LIST UNAVAILABLE"
                } else {
                    "NOT IN MY LIST"
                },
            )
        });
        let record = enable_writes(&mut fixture);
        toggle(&mut fixture);
        if kind == "live" {
            for _ in 0..8 {
                fixture.pump();
            }
            assert_eq!(*record.lock().unwrap(), (7, false));
            assert_eq!(fixture.script.calls.lock().unwrap().len(), 2);
        } else {
            fixture.wait(|_| write.entered.load(Ordering::SeqCst));
            assert!(record.lock().unwrap().1);
            assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
            write.release.notify_one();
            assert!(fixture.app.finish(&fixture.runtime));
        }
    }
}

#[test]
fn held_write_completion_at_the_original_deadline_does_not_start_an_ids_observation() {
    let write = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(None),
        Step {
            kind: Kind::RemoveWatchList,
            gate: Some(write.clone()),
        },
    ]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let record = enable_writes(&mut fixture);
    toggle(&mut fixture);
    fixture.wait(|_| write.entered.load(Ordering::SeqCst));
    fixture.clock.0.store(64, Ordering::SeqCst);
    fixture.pump();
    assert!(has_membership_caption(&fixture, "UPDATING MY LIST"));
    assert_eq!(write.retired.load(Ordering::SeqCst), 0);
    fixture.clock.0.store(65, Ordering::SeqCst);
    write.release.notify_one();
    fixture.wait(|fixture| has_membership_caption(fixture, "MY LIST UNAVAILABLE"));
    assert!(
        !record.lock().unwrap().1,
        "native acknowledgment still earns durable completion"
    );
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
    assert_eq!(write.retired.load(Ordering::SeqCst), 1);
}

#[test]
fn expired_credentials_do_not_refresh_during_an_issued_write_or_replay_after_settlement() {
    let write = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(None),
        Step {
            kind: Kind::RemoveWatchList,
            gate: Some(write.clone()),
        },
    ]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let record = enable_writes(&mut fixture);
    toggle(&mut fixture);
    fixture.wait(|_| write.entered.load(Ordering::SeqCst));
    fixture.issuer.hold_refresh.store(true, Ordering::SeqCst);
    fixture.clock.0.store(3605, Ordering::SeqCst);
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(fixture.issuer.tokens.load(Ordering::SeqCst), 1);
    assert_eq!(write.retired.load(Ordering::SeqCst), 0);
    write.release.notify_one();
    fixture.wait(|fixture| fixture.issuer.tokens.load(Ordering::SeqCst) == 2);
    assert!(
        record.lock().unwrap().1,
        "expired admission cannot acknowledge the provider write"
    );
    fixture.issuer.release.notify_one();
    fixture.wait(|fixture| fixture.app.authentication.access_ready());
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(!fixture.app.accounts.write_ready());
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
    assert!(has_membership_caption(&fixture, "MY LIST UNAVAILABLE"));
}

#[test]
fn background_before_first_write_poll_never_reserves_or_contacts_provider() {
    let mut fixture = ordinary_detail(vec![list(), detail("Listed01", None), ids(None)]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let record = enable_writes(&mut fixture);
    let visit = fixture.app.controller.membership_visit();
    fixture.app.command(
        Command::ToggleList {
            root: criterion_provider::MediaId::new("Listed01").unwrap(),
            from_visit: visit,
            expected_present: Some(true),
        },
        fixture.runtime.handle(),
    );
    assert!(fixture.app.accounts.write_active());
    fixture.app.background();
    assert!(fixture.app.finish(&fixture.runtime));
    assert_eq!(
        *record.lock().unwrap(),
        (7, false),
        "retired unpolled intent must not reserve"
    );
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
}

#[derive(Default)]
struct StorageGate {
    entered: AtomicBool,
    released: Mutex<bool>,
    wake: std::sync::Condvar,
}
impl StorageGate {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_one();
    }
}
struct ReleaseStorage(Arc<StorageGate>);
impl Drop for ReleaseStorage {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct HoldingDatabase {
    database: Database,
    gate: Arc<StorageGate>,
    hold_issued: bool,
}
impl criterion_platform::write_fence::Db8Transport for HoldingDatabase {
    fn get(
        &mut self,
        deadline: std::time::Instant,
    ) -> Result<Vec<u8>, criterion_platform::write_fence::FenceError> {
        self.database.get(deadline)
    }
    fn put(
        &mut self,
        revision: u64,
        issued: bool,
        deadline: std::time::Instant,
    ) -> Result<Vec<u8>, criterion_platform::write_fence::FenceError> {
        let result = self.database.put(revision, issued, deadline);
        if issued == self.hold_issued {
            self.gate.entered.store(true, Ordering::SeqCst);
            let mut released = self.gate.released.lock().unwrap();
            while !*released {
                released = self.gate.wake.wait(released).unwrap();
            }
        }
        result
    }
}

#[test]
fn departed_detail_during_db8_reservation_cannot_later_start_its_provider_write() {
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(None),
        detail("Related1", None),
        ids(None),
    ]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let record = Arc::new(Mutex::new((7, false)));
    let gate = Arc::new(StorageGate::default());
    let _release_on_unwind = ReleaseStorage(gate.clone());
    let fence = criterion_platform::write_fence::Db8WriteFence::with_transport(HoldingDatabase {
        database: Database(record.clone()),
        gate: gate.clone(),
        hold_issued: true,
    })
    .unwrap();
    fixture.app.accounts.use_write_fence(Arc::new(fence));
    fixture.wait(|fixture| fixture.app.accounts.write_ready());
    toggle(&mut fixture);
    fixture.wait(|_| gate.entered.load(Ordering::SeqCst));
    assert!(record.lock().unwrap().1);
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    open_related(&mut fixture);
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 3);
    gate.release();
    fixture.wait(|fixture| !fixture.app.accounts.write_active());
    assert!(
        !fixture
            .script
            .calls
            .lock()
            .unwrap()
            .contains(&Kind::RemoveWatchList),
        "departed reservation cannot gain provider authority"
    );
    fixture.wait(|fixture| {
        fixture.native_ready("Related1") && has_membership_caption(fixture, "NOT IN MY LIST")
    });
    assert!(
        !record.lock().unwrap().1,
        "definite NotIssued earns joined durable completion"
    );
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            Kind::NativeDetail("Related1"),
            Kind::MyListIds,
        ]
    );
}

struct ReleaseWrite(Arc<Gate>);
impl Drop for ReleaseWrite {
    fn drop(&mut self) {
        self.0.release.notify_one();
    }
}

struct DelayedStorageRelease {
    gate: Arc<StorageGate>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl DelayedStorageRelease {
    fn new(gate: Arc<StorageGate>) -> Self {
        let release = gate.clone();
        Self {
            gate,
            thread: Some(std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                release.release();
            })),
        }
    }
}
impl Drop for DelayedStorageRelease {
    fn drop(&mut self) {
        self.gate.release();
        let joined = self.thread.take().unwrap().join();
        if !std::thread::panicking() {
            joined.expect("storage release thread must settle");
        }
    }
}

#[test]
fn departed_detail_waits_for_independently_delayed_db8_completion() {
    let write = Arc::new(Gate::default());
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(None),
        Step {
            kind: Kind::RemoveWatchList,
            gate: Some(write.clone()),
        },
        detail("Related1", None),
        ids(None),
        ids(None),
    ]);
    let _release_write_on_unwind = ReleaseWrite(write.clone());
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let record = Arc::new(Mutex::new((7, false)));
    let gate = Arc::new(StorageGate::default());
    let _release_on_unwind = ReleaseStorage(gate.clone());
    let fence = criterion_platform::write_fence::Db8WriteFence::with_transport(HoldingDatabase {
        database: Database(record.clone()),
        gate: gate.clone(),
        hold_issued: false,
    })
    .unwrap();
    fixture.app.accounts.use_write_fence(Arc::new(fence));
    fixture.wait(|fixture| fixture.app.accounts.write_ready());
    toggle(&mut fixture);
    fixture.wait(|_| write.entered.load(Ordering::SeqCst));
    open_related(&mut fixture);
    write.release.notify_one();
    // Acknowledge the independently blocked OS worker before testing the wait
    // budget. The product's controlled clock remains fixed throughout.
    fixture.runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !gate.entered.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("DB8 completion must reach the held transport boundary");
    });
    for _ in 0..128 {
        fixture.pump();
    }
    assert!(!*gate.released.lock().unwrap(), "completion is still held");
    assert!(fixture.app.accounts.write_active());
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
    assert_eq!(write.retired.load(Ordering::SeqCst), 1);
    assert_eq!(*record.lock().unwrap(), (9, false));
    assert_eq!(fixture.clock.now(), Duration::from_secs(5));
    println!("held DB8 completion remains pending after 128 current-thread pumps");
    let _delayed_release = DelayedStorageRelease::new(gate.clone());
    fixture.wait(|fixture| {
        fixture.native_ready("Related1") && has_membership_caption(fixture, "NOT IN MY LIST")
    });
    fixture.key(41, 27);
    fixture.wait(|fixture| {
        fixture.native_ready("Listed01") && has_membership_caption(fixture, "IN MY LIST")
    });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [
            Kind::WatchList,
            Kind::NativeDetail("Listed01"),
            Kind::MyListIds,
            Kind::RemoveWatchList,
            Kind::NativeDetail("Related1"),
            Kind::MyListIds,
            Kind::MyListIds,
        ]
    );
    assert_eq!(fixture.script.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(*record.lock().unwrap(), (9, false));
    assert_eq!(fixture.clock.now(), Duration::from_secs(5));
}

#[test]
fn panicked_write_task_retires_intent_and_keeps_uncertain_fence_closed() {
    let mut fixture = ordinary_detail(vec![
        list(),
        detail("Listed01", None),
        ids(None),
        Step {
            kind: Kind::RemoveWatchList,
            gate: None,
        },
    ]);
    fixture.wait(|fixture| has_membership_caption(fixture, "IN MY LIST"));
    let record = enable_writes(&mut fixture);
    fixture.script.write_panics.store(true, Ordering::SeqCst);
    toggle(&mut fixture);
    fixture.wait(|fixture| has_membership_caption(fixture, "MY LIST UNAVAILABLE"));
    assert!(fixture.app.list_write.is_none());
    assert!(!fixture.app.accounts.write_active());
    assert!(!fixture.app.accounts.write_ready());
    assert!(record.lock().unwrap().1);
    for _ in 0..8 {
        fixture.pump();
    }
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 4);
}
