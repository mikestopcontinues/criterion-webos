use criterion_provider::MediaId;
use criterion_ui::{
    Action, AppUi, Card, CatalogTail, CatalogWindow, Command, Focus, LoadState, LoginView, Page,
    Target, ViewData,
};

fn enter_list(ui: &mut AppUi, data: &ViewData<'_>) {
    for action in [Action::Left, Action::Down, Action::Down, Action::Select] {
        ui.handle(action, data);
    }
    assert_eq!(ui.page(), Page::MyList);
}

#[test]
fn my_list_waits_at_the_native_page_boundary_with_the_existing_catalog_command() {
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        50
    ];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::More,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    for _ in 0..12 {
        ui.handle(Action::Down, &data);
    }
    assert_eq!(ui.focus(), Focus::Card { row: 12, column: 0 });
    assert_eq!(
        ui.handle(Action::Down, &data),
        vec![Command::Catalog {
            anchor: 48,
            target: 52
        }]
    );
    assert_eq!(
        ui.focus(),
        Focus::Card { row: 12, column: 0 },
        "unloaded native records never acquire focus"
    );
}

#[test]
fn remote_group_choices_skip_hidden_types_and_request_the_native_group() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: Some(12),
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(3),
        },
        MyListChoice {
            group: MyListGroup::Supplements,
            count: Some(1),
        },
    ];
    let data = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Empty,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    ui.handle(Action::Up, &data);
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::All));
    ui.handle(Action::Right, &data);
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::MyListGroup(MyListGroup::Collections)]
    );
    ui.handle(Action::Right, &data);
    assert_eq!(
        ui.focus(),
        Focus::MyListGroup(MyListGroup::Supplements),
        "an old selected publication must not overwrite the new header focus"
    );
}

fn text(frame: &criterion_ui::UiFrame) -> String {
    fn collect(shape: &egui::Shape, result: &mut String) {
        match shape {
            egui::Shape::Text(value) => {
                result.push_str(&value.galley.job.text);
                result.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, result);
                }
            }
            _ => (),
        }
    }
    let mut result = String::new();
    for shape in &frame.output.shapes {
        collect(&shape.shape, &mut result);
    }
    result
}

#[test]
fn painted_groups_share_pointer_geometry_and_omit_an_unavailable_count() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: None,
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(3),
        },
    ];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Admitted fixture",
        year: "",
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let data = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        cards: &cards,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    let labels = text(&frame);
    assert!(
        labels.contains("All\n"),
        "unknown aggregate is a label without a fabricated zero"
    );
    assert!(labels.contains("Collections 3"));
    assert!(!labels.contains("Films & Series"));
    assert_eq!(
        frame.visible_cards[0].image,
        egui::Rect::from_min_size(egui::pos2(150.0, 248.0), egui::vec2(378.0, 213.0))
    );
    frame.output.textures_delta.clear();
    let events = [true, false].map(|pressed| egui::Event::PointerButton {
        pos: egui::pos2(370.0, 170.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    });
    let mut frame = ui.render(
        egui::RawInput {
            events: events.into(),
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert_eq!(
        frame.commands,
        vec![Command::MyListGroup(MyListGroup::Collections)]
    );
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
    frame.output.textures_delta.clear();
}

#[test]
fn selected_group_header_enters_the_admitted_grid_through_shared_catalog_demand() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [MyListChoice {
        group: MyListGroup::All,
        count: Some(2),
    }];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Admitted fixture",
        year: "",
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let data = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        cards: &cards,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    ui.handle(Action::Up, &data);
    assert_eq!(
        ui.handle(Action::Down, &data),
        vec![Command::Catalog {
            anchor: 0,
            target: 0
        }]
    );
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    ui.handle(Action::Up, &data);
    ui.handle(Action::Left, &data);
    assert_eq!(ui.focus(), Focus::Rail(criterion_ui::RailItem::MyList));
    ui.handle(Action::Right, &data);
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::All));
}

#[test]
fn my_list_tail_error_paints_and_pointer_retries_without_losing_the_global_anchor() {
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        50
    ];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::Error,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    for _ in 0..13 {
        ui.handle(Action::Down, &data);
    }
    assert_eq!(ui.focus(), Focus::CatalogRetry);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(text(&frame).contains("Unable to load — retry"));
    let events = [true, false].map(|pressed| egui::Event::PointerButton {
        pos: egui::pos2(400.0, 1008.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    });
    let mut frame = ui.render(
        egui::RawInput {
            events: events.into(),
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert_eq!(frame.commands, vec![Command::RetryCatalog]);
    assert_eq!(ui.focus(), Focus::Card { row: 12, column: 0 });
    ui.handle(Action::Left, &data);
    assert_eq!(ui.focus(), Focus::Rail(criterion_ui::RailItem::MyList));
}

fn click(ui: &mut AppUi, data: &ViewData<'_>, x: f32, y: f32) -> Vec<Command> {
    let events = [true, false].map(|pressed| egui::Event::PointerButton {
        pos: egui::pos2(x, y),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    });
    let mut frame = ui.render(
        egui::RawInput {
            events: events.into(),
            ..Default::default()
        },
        data,
    );
    frame.output.textures_delta.clear();
    frame.commands
}

#[test]
fn warm_group_return_restores_its_global_card_and_scroll_without_a_history_push() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: Some(100),
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(4),
        },
    ];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        100
    ];
    let all = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &all);
    for _ in 0..12 {
        ui.handle(Action::Down, &all);
    }
    let scroll = ui.scroll_y();
    assert_eq!(
        click(&mut ui, &all, 370.0, 170.0),
        vec![Command::MyListGroup(MyListGroup::Collections)]
    );
    let collection = ViewData {
        cards: &cards[..4],
        my_list: Some(MyListView {
            selected: MyListGroup::Collections,
            choices: &choices,
        }),
        ..all
    };
    let mut frame = ui.render(egui::RawInput::default(), &collection);
    frame.output.textures_delta.clear();
    ui.handle(Action::Down, &collection);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(
        click(&mut ui, &collection, 200.0, 170.0),
        vec![Command::MyListGroup(MyListGroup::All)]
    );
    let restored = ViewData {
        cards: &cards,
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        ..collection
    };
    let mut frame = ui.render(egui::RawInput::default(), &restored);
    frame.output.textures_delta.clear();
    assert_eq!(
        ui.handle(Action::Down, &restored),
        vec![Command::Catalog {
            anchor: 48,
            target: 48
        }]
    );
    assert_eq!(ui.focus(), Focus::Card { row: 12, column: 0 });
    assert_eq!(ui.scroll_y(), scroll);
    assert_eq!(
        ui.handle(Action::Back, &restored),
        vec![
            Command::Catalog {
                anchor: 48,
                target: 48
            },
            Command::Restore(Page::Home)
        ],
        "group changes do not manufacture navigation history"
    );
}

#[test]
fn all_six_labels_remain_legible_without_a_truncated_large_numeric_count() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: Some(u64::MAX),
        },
        MyListChoice {
            group: MyListGroup::FilmsAndSeries,
            count: Some(2),
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(1),
        },
        MyListChoice {
            group: MyListGroup::OriginalsAndFranchises,
            count: Some(2),
        },
        MyListChoice {
            group: MyListGroup::Supplements,
            count: Some(1),
        },
        MyListChoice {
            group: MyListGroup::Categories,
            count: Some(0),
        },
    ];
    let data = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::Categories,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Loading,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    ui.handle(Action::Up, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    let labels = text(&frame);
    for expected in [
        "All\n",
        "Films & Series 2",
        "Collections 1",
        "Originals & Franchises 2",
        "Supplements 1",
        "Categories 0",
        "Loading",
    ] {
        assert!(labels.contains(expected), "missing {expected}: {labels}");
    }
    assert!(
        !labels.contains("184467"),
        "an exact count that does not fit is omitted as a whole"
    );
    assert!(
        frame.visible_cards.is_empty(),
        "counts do not fabricate loaded records"
    );
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Categories));
}

#[test]
fn group_request_does_not_activate_old_group_cards_before_authoritative_publication() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: Some(1),
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(1),
        },
    ];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Admitted fixture",
        year: "",
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    let mut events = Vec::new();
    for pos in [egui::pos2(370.0, 170.0), egui::pos2(320.0, 350.0)] {
        for pressed in [true, false] {
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
    }
    let mut frame = ui.render(
        egui::RawInput {
            events,
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert_eq!(
        frame.commands,
        vec![Command::MyListGroup(MyListGroup::Collections)]
    );
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
    assert!(
        click(&mut ui, &data, 320.0, 350.0).is_empty(),
        "later stale selected views still cannot activate the old group"
    );
    let published = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::Collections,
            choices: &choices,
        }),
        ..data
    };
    assert_eq!(
        click(&mut ui, &published, 320.0, 350.0),
        vec![
            Command::Catalog {
                anchor: 0,
                target: 0
            },
            Command::Open(target)
        ]
    );
}

#[test]
fn authoritative_group_change_never_reuses_the_previous_groups_global_position() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: Some(60),
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(1),
        },
    ];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        60
    ];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    for _ in 0..12 {
        ui.handle(Action::Down, &data);
    }
    let collection = ViewData {
        cards: &cards[..1],
        catalog: data.catalog,
        my_list: Some(MyListView {
            selected: MyListGroup::Collections,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &collection);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
    assert_eq!(
        ui.handle(Action::Down, &collection),
        vec![Command::Catalog {
            anchor: 0,
            target: 0
        }]
    );
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::All));
    assert_eq!(
        ui.handle(Action::Down, &data),
        vec![Command::Catalog {
            anchor: 48,
            target: 48
        }]
    );
}

#[test]
fn removed_group_focus_returns_to_the_authoritative_visible_choice() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: None,
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(1),
        },
    ];
    let data = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Empty,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Right, &data);
    let updated = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices[..1],
        }),
        ..data
    };
    let mut frame = ui.render(egui::RawInput::default(), &updated);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::All));
    assert!(frame.visible_cards.is_empty());
    assert!(ui.handle(Action::Down, &updated).is_empty());
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::All));
    assert_eq!(
        ui.handle(Action::Back, &updated),
        vec![Command::Restore(Page::Home)]
    );
}

#[test]
fn rail_reentry_restores_the_warm_global_anchor_in_a_bounded_window() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView, RailItem};
    let choices = [MyListChoice {
        group: MyListGroup::All,
        count: Some(60),
    }];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        60
    ];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    for _ in 0..12 {
        ui.handle(Action::Down, &data);
    }
    let scroll = ui.scroll_y();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Up, &data);
    assert_eq!(ui.focus(), Focus::Rail(RailItem::Home));
    ui.handle(Action::Select, &data);
    assert_eq!(ui.page(), Page::Home);
    enter_list(&mut ui, &data);
    let warm = ViewData {
        cards: &cards[40..],
        catalog: Some(CatalogWindow {
            first: 40,
            tail: CatalogTail::End,
        }),
        my_list: data.my_list,
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &warm);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 12, column: 0 });
    assert_eq!(ui.scroll_y(), scroll);
    assert!(
        frame
            .visible_cards
            .iter()
            .any(|card| card.row == 12 && card.column == 0)
    );
    assert_eq!(
        frame.commands,
        vec![Command::Catalog {
            anchor: 48,
            target: 48
        }]
    );
}

#[test]
fn logout_and_relink_cannot_resurrect_group_anchors_from_public_history() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView, RailItem};
    let choices = [MyListChoice {
        group: MyListGroup::All,
        count: Some(60),
    }];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        60
    ];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    for _ in 0..12 {
        ui.handle(Action::Down, &data);
    }
    ui.handle(Action::Left, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Select, &data);
    ui.handle(Action::Left, &data);
    for _ in 0..4 {
        ui.handle(Action::Down, &data);
    }
    assert_eq!(ui.focus(), Focus::Rail(RailItem::Login));
    ui.handle(Action::Select, &data);
    assert_eq!(ui.handle(Action::Select, &data), vec![Command::Logout]);
    for login in [
        LoginView::SigningOut,
        LoginView::SignedOut,
        LoginView::Requesting,
        LoginView::SignedIn,
    ] {
        let mut frame = ui.render(
            egui::RawInput::default(),
            &ViewData {
                login,
                ..Default::default()
            },
        );
        frame.output.textures_delta.clear();
    }
    assert_eq!(
        ui.handle(
            Action::Back,
            &ViewData {
                login: LoginView::SignedIn,
                ..Default::default()
            }
        ),
        vec![Command::Restore(Page::Home)]
    );
    enter_list(&mut ui, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.scroll_y(), 0.0);
    assert_eq!(
        ui.handle(Action::Back, &data),
        vec![
            Command::Catalog {
                anchor: 0,
                target: 0
            },
            Command::Restore(Page::Home)
        ]
    );
    assert_ne!(
        ui.page(),
        Page::MyList,
        "subscriber shelf snapshots were retired during logout"
    );
}

#[test]
fn empty_and_initial_failure_keep_group_navigation_and_remote_retry_available() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: None,
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(0),
        },
    ];
    let empty = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::Collections,
            choices: &choices,
        }),
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Empty,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &empty);
    let mut frame = ui.render(egui::RawInput::default(), &empty);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
    assert!(frame.visible_cards.is_empty());
    assert!(text(&frame).contains("No items found"));
    assert!(ui.handle(Action::Down, &empty).is_empty());
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
    assert_eq!(
        ui.handle(Action::Back, &empty),
        vec![Command::Restore(Page::Home)]
    );
    let error = ViewData {
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::Error,
        }),
        status: LoadState::Error,
        ..empty
    };
    enter_list(&mut ui, &error);
    let mut frame = ui.render(egui::RawInput::default(), &error);
    frame.output.textures_delta.clear();
    assert!(text(&frame).contains("Unable to load items"));
    ui.handle(Action::Down, &error);
    assert_eq!(ui.focus(), Focus::CatalogRetry);
    assert_eq!(
        ui.handle(Action::Select, &error),
        vec![Command::RetryCatalog]
    );
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
}

#[test]
fn a_terminal_smaller_group_clamps_a_saved_anchor_to_an_admitted_card() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [MyListChoice {
        group: MyListGroup::All,
        count: None,
    }];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        60
    ];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    for _ in 0..12 {
        ui.handle(Action::Down, &data);
    }
    let smaller = ViewData {
        cards: &cards[..1],
        catalog: data.catalog,
        my_list: data.my_list,
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &smaller);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.scroll_y(), 0.0);
    assert_eq!(
        frame.commands,
        vec![Command::Catalog {
            anchor: 0,
            target: 0
        }]
    );
}

#[test]
fn choosing_the_published_group_supersedes_a_different_pending_group() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: None,
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(1),
        },
    ];
    let data = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Empty,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Right, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::MyListGroup(MyListGroup::Collections)]
    );
    assert!(
        ui.handle(Action::Select, &data).is_empty(),
        "a repeated select does not duplicate the pending request"
    );
    ui.handle(Action::Left, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::MyListGroup(MyListGroup::All)],
        "the old published group is a new intent while a different group is pending"
    );
}

#[test]
fn detail_back_restores_the_selected_group_and_its_global_grid_position() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: None,
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(60),
        },
    ];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        60
    ];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::Collections,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    for _ in 0..12 {
        ui.handle(Action::Down, &data);
    }
    let scroll = ui.scroll_y();
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![
            Command::Catalog {
                anchor: 48,
                target: 48
            },
            Command::Open(target.clone())
        ]
    );
    assert_eq!(
        ui.handle(
            Action::Back,
            &ViewData {
                login: LoginView::SignedIn,
                ..Default::default()
            }
        ),
        vec![Command::Restore(Page::MyList)]
    );
    let warm = ViewData {
        cards: &cards[40..],
        catalog: Some(CatalogWindow {
            first: 40,
            tail: CatalogTail::End,
        }),
        my_list: data.my_list,
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &warm);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 12, column: 0 });
    assert_eq!(ui.scroll_y(), scroll);
    assert_eq!(
        frame.commands,
        vec![Command::Catalog {
            anchor: 48,
            target: 48
        }]
    );
}

#[test]
fn pointer_release_after_a_group_change_cannot_open_an_identical_old_card() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: None,
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(1),
        },
    ];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Admitted fixture",
        year: "",
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    let event = |pressed| egui::Event::PointerButton {
        pos: egui::pos2(320.0, 350.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![event(true)],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    let changed = ViewData {
        my_list: Some(MyListView {
            selected: MyListGroup::Collections,
            choices: &choices,
        }),
        ..data
    };
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![event(false)],
            ..Default::default()
        },
        &changed,
    );
    frame.output.textures_delta.clear();
    assert!(frame.commands.is_empty());
    assert_eq!(ui.page(), Page::MyList);
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
}

#[test]
fn pending_group_ignores_old_retry_controls_and_keeps_latest_remote_selection() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [
        MyListChoice {
            group: MyListGroup::All,
            count: Some(1),
        },
        MyListChoice {
            group: MyListGroup::Collections,
            count: Some(1),
        },
    ];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Admitted fixture",
        year: "",
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::Error,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Right, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::MyListGroup(MyListGroup::Collections)]
    );
    assert!(click(&mut ui, &data, 400.0, 1008.0).is_empty());
    assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::Collections));
    ui.handle(Action::Left, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::MyListGroup(MyListGroup::All)]
    );
}

#[test]
fn back_to_home_preserves_the_new_warm_group_anchor_for_reentry() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView};
    let choices = [MyListChoice {
        group: MyListGroup::All,
        count: Some(60),
    }];
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Admitted fixture",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        60
    ];
    let data = ViewData {
        cards: &cards,
        catalog: Some(CatalogWindow {
            first: 0,
            tail: CatalogTail::End,
        }),
        my_list: Some(MyListView {
            selected: MyListGroup::All,
            choices: &choices,
        }),
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_list(&mut ui, &data);
    for _ in 0..12 {
        ui.handle(Action::Down, &data);
    }
    let scroll = ui.scroll_y();
    assert_eq!(
        ui.handle(Action::Back, &data),
        vec![
            Command::Catalog {
                anchor: 48,
                target: 48
            },
            Command::Restore(Page::Home)
        ]
    );
    enter_list(&mut ui, &data);
    let warm = ViewData {
        cards: &cards[40..],
        catalog: Some(CatalogWindow {
            first: 40,
            tail: CatalogTail::End,
        }),
        my_list: data.my_list,
        login: LoginView::SignedIn,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &warm);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 12, column: 0 });
    assert_eq!(ui.scroll_y(), scroll);
    assert_eq!(
        frame.commands,
        vec![Command::Catalog {
            anchor: 48,
            target: 48
        }]
    );
}

#[test]
fn failed_refresh_from_group_or_retry_focus_reaches_login_and_keeps_public_origin() {
    use criterion_ui::{MyListChoice, MyListGroup, MyListView, RailItem};
    let choices = [MyListChoice {
        group: MyListGroup::All,
        count: None,
    }];
    for retry in [false, true] {
        let private = ViewData {
            catalog: Some(CatalogWindow {
                first: 0,
                tail: if retry {
                    CatalogTail::Error
                } else {
                    CatalogTail::Loading
                },
            }),
            my_list: Some(MyListView {
                selected: MyListGroup::All,
                choices: &choices,
            }),
            login: LoginView::SignedIn,
            status: if retry {
                LoadState::Error
            } else {
                LoadState::Loading
            },
            ..Default::default()
        };
        let mut ui = AppUi::new();
        enter_list(&mut ui, &private);
        let mut frame = ui.render(egui::RawInput::default(), &private);
        frame.output.textures_delta.clear();
        assert_eq!(ui.focus(), Focus::MyListGroup(MyListGroup::All));
        if retry {
            ui.handle(Action::Down, &private);
            assert_eq!(ui.focus(), Focus::CatalogRetry);
        }
        let failed = ViewData {
            login: LoginView::Error,
            status: LoadState::Error,
            ..Default::default()
        };
        let mut frame = ui.render(egui::RawInput::default(), &failed);
        frame.output.textures_delta.clear();
        ui.handle(Action::Left, &failed);
        assert_eq!(ui.focus(), Focus::Rail(RailItem::Login));
        assert_eq!(
            ui.handle(Action::Select, &failed),
            vec![Command::Authenticate]
        );
        assert_eq!(ui.page(), Page::Login);
        let awaiting = ViewData {
            login: LoginView::Requesting,
            ..Default::default()
        };
        let mut frame = ui.render(egui::RawInput::default(), &awaiting);
        frame.output.textures_delta.clear();
        assert_eq!(
            ui.handle(Action::Back, &awaiting),
            vec![Command::CancelAuthentication, Command::Restore(Page::Home)]
        );
        assert_eq!(ui.focus(), Focus::Hero);
    }
}
