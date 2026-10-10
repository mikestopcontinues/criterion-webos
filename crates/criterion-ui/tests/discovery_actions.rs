use criterion_ui::{
    Action, AppUi, Card, Command, Focus, LoadState, Page, Rail, RailAction, RailActionCursor,
    Target, UiFrame, ViewData,
};

fn content(path: &str) -> Target {
    Target::Content(criterion_provider::ContentTarget::parse(path).unwrap())
}

fn card<'a>(target: &'a Target, title: &'a str) -> Card<'a> {
    Card {
        key: target,
        title,
        year: "",
        duration_label: None,
        artwork_key: None,
        saved_fraction: None,
        action: Default::default(),
    }
}

fn data<'a>(rails: &'a [Rail<'a>]) -> ViewData<'a> {
    ViewData {
        rails,
        discovery_visit: Some(12),
        status: LoadState::Ready,
        ..Default::default()
    }
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

fn render(ui: &mut AppUi, data: &ViewData<'_>, events: Vec<egui::Event>) -> UiFrame {
    let mut frame = ui.render(
        egui::RawInput {
            events,
            ..Default::default()
        },
        data,
    );
    frame.output.textures_delta.clear();
    frame
}

#[test]
fn populated_rail_down_preserves_card_column_and_clamps_only_a_short_row() {
    let target = content("/discover/newly-added");
    let cards = [
        card(&target, "First"),
        card(&target, "Second"),
        card(&target, "Third"),
    ];
    let short = [card(&target, "Only")];
    let rails = [
        Rail {
            title: "First row",
            cards: &cards,
            action: Some(RailAction {
                block: 825,
                label: "See all",
                target: &target,
            }),
        },
        Rail {
            title: "Second row",
            cards: &cards,
            action: None,
        },
        Rail {
            title: "Short final row",
            cards: &short,
            action: None,
        },
    ];
    let data = data(&rails);
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Right, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 2 });
    assert!(ui.handle(Action::Down, &data).is_empty());
    assert_eq!(ui.focus(), Focus::Card { row: 1, column: 2 });
    assert!(ui.handle(Action::Down, &data).is_empty());
    assert_eq!(ui.focus(), Focus::Card { row: 2, column: 0 });
}

#[test]
fn supplied_header_prepares_exact_activation_and_back_restores_its_row_and_scroll() {
    let target = content("/discover/newly-added");
    let cards = [card(&target, "First"), card(&target, "Second")];
    let rails = [
        Rail {
            title: "First row",
            cards: &cards,
            action: Some(RailAction {
                block: 825,
                label: "See all",
                target: &target,
            }),
        },
        Rail {
            title: "Second row",
            cards: &cards,
            action: Some(RailAction {
                block: 826,
                label: "See more",
                target: &target,
            }),
        },
    ];
    let data = data(&rails);
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Up, &data);
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 1, column: 0 });
    assert_eq!(ui.scroll_y(), 1029.0);
    assert_eq!(
        ui.handle(Action::Select, &data),
        [Command::ActivateRail {
            origin: Page::Home,
            from: RailActionCursor {
                visit: 12,
                block: 826,
                row: 1
            },
            target: target.clone(),
        }]
    );
    assert_eq!(ui.page(), Page::Home);
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 1, column: 0 });
    assert_eq!(ui.scroll_y(), 1029.0);

    ui.commit_discovery_target(&target, data.login);
    assert_eq!(ui.page(), Page::Discovery);
    assert_eq!(
        ui.handle(Action::Back, &data),
        [Command::Restore(Page::Home)]
    );
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 1, column: 0 });
    assert_eq!(ui.scroll_y(), 1029.0);
    ui.handle(Action::Left, &data);
    assert_eq!(ui.focus(), Focus::Rail(criterion_ui::RailItem::Home));
    assert!(ui.handle(Action::Back, &data).is_empty());
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 1, column: 0 });
    assert_eq!(ui.scroll_y(), 1029.0);
    ui.handle(Action::Right, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 1, column: 0 });
    ui.handle(Action::Up, &data);
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 1, column: 0 });
}

#[test]
fn action_only_rows_are_reachable_and_empty_rows_do_not_trap_remote_focus() {
    let target = content("/discover/newly-added");
    let cards = [card(&target, "Last row card")];
    let rails = [
        Rail {
            title: "Action only",
            cards: &[],
            action: Some(RailAction {
                block: 825,
                label: "See all",
                target: &target,
            }),
        },
        Rail {
            title: "Empty",
            cards: &[],
            action: None,
        },
        Rail {
            title: "Last row",
            cards: &cards,
            action: Some(RailAction {
                block: 827,
                label: "See more",
                target: &target,
            }),
        },
    ];
    let data = data(&rails);
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 0 });
    assert_eq!(ui.scroll_y(), 632.0);
    assert!(ui.handle(Action::Right, &data).is_empty());
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 0 });
    assert_eq!(
        ui.handle(Action::Select, &data),
        [Command::ActivateRail {
            origin: Page::Home,
            from: RailActionCursor {
                visit: 12,
                block: 825,
                row: 0
            },
            target: target.clone(),
        }]
    );
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 2, column: 0 });
    assert_eq!(ui.scroll_y(), 1426.0);
    ui.handle(Action::Up, &data);
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 2, column: 0 });
    ui.handle(Action::Up, &data);
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 0 });
    assert_eq!(ui.scroll_y(), 632.0);
}

#[test]
fn pointer_release_refuses_a_different_row_with_equal_block_and_target() {
    let target = content("/discover/newly-added");
    let cards = [card(&target, "Card")];
    let rails = [
        Rail {
            title: "First row",
            cards: &cards,
            action: Some(RailAction {
                block: 825,
                label: "See all",
                target: &target,
            }),
        },
        Rail {
            title: "Second row",
            cards: &cards,
            action: Some(RailAction {
                block: 825,
                label: "See all",
                target: &target,
            }),
        },
    ];
    let data = data(&rails);
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    let first = egui::pos2(1620.0, 282.0);
    let second = egui::pos2(1620.0, 679.0);
    assert!(
        render(&mut ui, &data, vec![pointer(first, true)])
            .commands
            .is_empty()
    );
    assert!(
        render(&mut ui, &data, vec![pointer(second, false)])
            .commands
            .is_empty()
    );
    assert_eq!(ui.page(), Page::Home);
    assert_eq!(
        render(
            &mut ui,
            &data,
            vec![pointer(second, true), pointer(second, false)]
        )
        .commands,
        [Command::ActivateRail {
            origin: Page::Home,
            from: RailActionCursor {
                visit: 12,
                block: 825,
                row: 1
            },
            target: target.clone(),
        }]
    );
}

#[test]
fn first_rail_activation_ends_pointer_batch_before_later_header_events() {
    let first_target = content("/discover/newly-added");
    let second_target = content("/discover/leaving-soon");
    let cards = [card(&first_target, "Card")];
    let rails = [
        Rail {
            title: "First row",
            cards: &cards,
            action: Some(RailAction {
                block: 825,
                label: "See all",
                target: &first_target,
            }),
        },
        Rail {
            title: "Second row",
            cards: &cards,
            action: Some(RailAction {
                block: 826,
                label: "See more",
                target: &second_target,
            }),
        },
    ];
    let data = data(&rails);
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    let first = egui::pos2(1620.0, 282.0);
    let second = egui::pos2(1620.0, 679.0);
    let frame = render(
        &mut ui,
        &data,
        vec![
            pointer(first, true),
            pointer(first, false),
            pointer(second, true),
            pointer(second, false),
        ],
    );
    assert_eq!(
        frame.commands,
        [Command::ActivateRail {
            origin: Page::Home,
            from: RailActionCursor {
                visit: 12,
                block: 825,
                row: 0
            },
            target: first_target.clone(),
        }]
    );
    assert_eq!(ui.page(), Page::Home);
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 0 });
    ui.commit_discovery_target(&first_target, data.login);
    assert_eq!(
        ui.handle(Action::Back, &data),
        [Command::Restore(Page::Home)]
    );
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 0 });
}

#[test]
fn pointer_release_refuses_changed_block_visit_or_target_then_accepts_a_fresh_press() {
    let target = content("/discover/newly-added");
    let replacement = content("/discover/leaving-soon");
    let cards = [card(&target, "Card")];
    let rails = [Rail {
        title: "Supplied row",
        cards: &cards,
        action: Some(RailAction {
            block: 825,
            label: "See all",
            target: &target,
        }),
    }];
    let reblocked = [Rail {
        title: "Supplied row",
        cards: &cards,
        action: Some(RailAction {
            block: 826,
            label: "See all",
            target: &target,
        }),
    }];
    let replaced = [Rail {
        title: "Supplied row",
        cards: &cards,
        action: Some(RailAction {
            block: 825,
            label: "See all",
            target: &replacement,
        }),
    }];
    let current = data(&rails);
    let cases = [
        ("block", data(&reblocked), 12, 826, &target),
        (
            "visit",
            ViewData {
                discovery_visit: Some(13),
                ..data(&rails)
            },
            13,
            825,
            &target,
        ),
        ("target", data(&replaced), 12, 825, &replacement),
    ];
    for (changed, replacement_data, visit, block, expected_target) in cases {
        let mut ui = AppUi::new();
        ui.handle(Action::Down, &current);
        let pos = egui::pos2(1620.0, 282.0);
        assert!(
            render(&mut ui, &current, vec![pointer(pos, true)])
                .commands
                .is_empty()
        );
        assert!(
            render(&mut ui, &replacement_data, vec![pointer(pos, false)])
                .commands
                .is_empty(),
            "a changed {changed} must retire the pressed action",
        );
        assert_eq!(ui.page(), Page::Home);
        assert_eq!(
            render(
                &mut ui,
                &replacement_data,
                vec![pointer(pos, true), pointer(pos, false)]
            )
            .commands,
            [Command::ActivateRail {
                origin: Page::Home,
                from: RailActionCursor {
                    visit,
                    block,
                    row: 0
                },
                target: expected_target.clone(),
            }],
            "the current {changed} must accept its own fresh press",
        );
    }
}

#[test]
fn absent_visit_or_unready_display_refuses_remote_and_pending_pointer_activation() {
    let target = content("/discover/newly-added");
    let cards = [card(&target, "Card")];
    let rails = [Rail {
        title: "Supplied row",
        cards: &cards,
        action: Some(RailAction {
            block: 825,
            label: "See all",
            target: &target,
        }),
    }];
    let current = data(&rails);
    let cases = [
        ViewData {
            discovery_visit: None,
            ..data(&rails)
        },
        ViewData {
            status: LoadState::Loading,
            ..data(&rails)
        },
        ViewData {
            status: LoadState::Empty,
            ..data(&rails)
        },
        ViewData {
            status: LoadState::Offline,
            ..data(&rails)
        },
        ViewData {
            status: LoadState::Error,
            ..data(&rails)
        },
    ];
    for unavailable in cases {
        let mut ui = AppUi::new();
        ui.handle(Action::Down, &current);
        ui.handle(Action::Up, &current);
        assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 0 });
        assert!(ui.handle(Action::Select, &unavailable).is_empty());
        assert_eq!(ui.page(), Page::Home);
        let pos = egui::pos2(1620.0, 282.0);
        assert!(
            render(&mut ui, &current, vec![pointer(pos, true)])
                .commands
                .is_empty()
        );
        assert!(
            render(&mut ui, &unavailable, vec![pointer(pos, false)])
                .commands
                .is_empty()
        );
        assert!(
            render(
                &mut ui,
                &unavailable,
                vec![pointer(pos, true), pointer(pos, false)]
            )
            .commands
            .is_empty()
        );
        assert_eq!(ui.page(), Page::Home);
        assert_eq!(
            ui.handle(Action::Select, &current),
            [Command::ActivateRail {
                origin: Page::Home,
                from: RailActionCursor {
                    visit: 12,
                    block: 825,
                    row: 0
                },
                target: target.clone(),
            }]
        );
    }
}

#[test]
fn pointer_header_press_keeps_the_painted_card_window_until_release() {
    let target = content("/discover/newly-added");
    let cards = [card(&target, "Card"); 8];
    let rails = [Rail {
        title: "Supplied row",
        cards: &cards,
        action: Some(RailAction {
            block: 825,
            label: "See all",
            target: &target,
        }),
    }];
    let data = data(&rails);
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    for _ in 0..5 {
        ui.handle(Action::Right, &data);
    }
    let before = render(&mut ui, &data, vec![]).visible_cards;
    assert_eq!(
        before.first().unwrap().focus,
        Focus::Card { row: 0, column: 2 }
    );
    assert_eq!(
        before.last().unwrap().focus,
        Focus::Card { row: 0, column: 6 }
    );
    let pos = egui::pos2(1620.0, 282.0);
    assert_eq!(
        render(&mut ui, &data, vec![pointer(pos, true)]).visible_cards,
        before
    );
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 5 });
    assert_eq!(render(&mut ui, &data, vec![]).visible_cards, before);
    let released = render(&mut ui, &data, vec![pointer(pos, false)]);
    assert_eq!(released.visible_cards, before);
    assert_eq!(
        released.commands,
        [Command::ActivateRail {
            origin: Page::Home,
            from: RailActionCursor {
                visit: 12,
                block: 825,
                row: 0
            },
            target: target.clone(),
        }]
    );
}

#[test]
fn header_paint_is_bounded_without_changing_home_new_or_discovery_card_geometry() {
    let target = content("/discover/newly-added");
    let media = Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let cards = [card(&media, "First"), card(&media, "Second")];
    let long_label = format!("Supplied {}", "W".repeat(800));
    let plain = [Rail {
        title: "Supplied row",
        cards: &cards,
        action: None,
    }];
    let supplied = [Rail {
        title: "Supplied row",
        cards: &cards,
        action: Some(RailAction {
            block: 825,
            label: &long_label,
            target: &target,
        }),
    }];
    let destinations = [
        None,
        Some(content("/new")),
        Some(content("/discover/newly-added")),
    ];
    for destination in destinations {
        let mut ui = AppUi::new();
        if let Some(destination) = destination {
            ui.commit_discovery_target(&destination, criterion_ui::LoginView::SignedOut);
        }
        ui.handle(Action::Down, &data(&plain));
        let before = render(&mut ui, &data(&plain), vec![]);
        let supplied_data = data(&supplied);
        let painted = render(&mut ui, &supplied_data, vec![]);
        assert_eq!(painted.visible_cards, before.visible_cards);
        assert_eq!(painted.visible_artwork, before.visible_artwork);
        let expected_card = if ui.page() == Page::New {
            egui::Rect::from_min_max(egui::pos2(150.0, 324.0), egui::pos2(666.0, 614.25))
        } else {
            egui::Rect::from_min_max(egui::pos2(150.0, 324.0), egui::pos2(528.0, 536.625))
        };
        assert_eq!(painted.visible_cards[0].image, expected_card);
        let header = egui::Rect::from_min_max(egui::pos2(1470.0, 256.0), egui::pos2(1770.0, 308.0));
        let labels: Vec<_> = painted
            .output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.job.text.starts_with("Supplied W")
                {
                    Some((shape, text))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(labels.len(), 1);
        let (shape, text) = labels[0];
        assert!(text.galley.job.text.chars().count() <= 256);
        assert_eq!(text.galley.rows.len(), 1);
        assert!(header.contains_rect(shape.shape.visual_bounding_rect()));
        assert!(
            shape
                .clip_rect
                .contains_rect(shape.shape.visual_bounding_rect())
        );
    }
}

#[test]
fn header_visit_and_warm_back_preserve_the_same_row_card_window_and_column() {
    let target = content("/discover/newly-added");
    let cards = [card(&target, "Card"); 8];
    let rails = [Rail {
        title: "Supplied row",
        cards: &cards,
        action: Some(RailAction {
            block: 825,
            label: "See all",
            target: &target,
        }),
    }];
    let data = data(&rails);
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    for _ in 0..5 {
        ui.handle(Action::Right, &data);
    }
    let before = render(&mut ui, &data, vec![]).visible_cards;
    ui.handle(Action::Up, &data);
    assert_eq!(
        render(&mut ui, &data, vec![]).visible_cards,
        before,
        "visiting the header must not reset the painted card window"
    );
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 5 });
    ui.handle(Action::Up, &data);
    assert_eq!(ui.handle(Action::Select, &data).len(), 1);
    ui.commit_discovery_target(&target, data.login);
    assert_eq!(
        ui.handle(Action::Back, &data),
        [Command::Restore(Page::Home)]
    );
    assert_eq!(
        render(&mut ui, &data, vec![]).visible_cards,
        before,
        "warm Back must preserve the header's card window"
    );
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 5 });
}

#[test]
fn retained_header_column_clamps_to_current_cards_and_handles_an_action_only_row() {
    let target = content("/discover/newly-added");
    let cards = [card(&target, "Card"); 8];
    let rail = |cards| Rail {
        title: "Supplied",
        cards,
        action: Some(RailAction {
            block: 825,
            label: "See all",
            target: &target,
        }),
    };
    let full = [rail(&cards[..])];
    let shortened = [rail(&cards[..2])];
    let empty = [rail(&cards[..0])];
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data(&full));
    for _ in 0..5 {
        ui.handle(Action::Right, &data(&full));
    }
    ui.handle(Action::Up, &data(&full));
    render(&mut ui, &data(&shortened), vec![]);
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 1 });
    ui.handle(Action::Down, &data(&shortened));
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 1 });
    ui.handle(Action::Up, &data(&shortened));
    render(&mut ui, &data(&empty), vec![]);
    assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 0, column: 0 });
    assert_eq!(
        ui.handle(Action::Select, &data(&empty)),
        [Command::ActivateRail {
            origin: Page::Home,
            from: RailActionCursor {
                visit: 12,
                block: 825,
                row: 0
            },
            target: target.clone()
        }]
    );
}
