use criterion_provider::MediaId;
use criterion_ui::{Action, AppUi, Card, Focus, LoadState, Page, Target, ViewData};

#[test]
fn approaching_the_catalog_tail_requests_continuation_without_navigation() {
    let targets: Vec<_> = (0..60)
        .map(|i| Target::Media(MediaId::new(&format!("Film{i:04}")).unwrap()))
        .collect();
    let cards: Vec<_> = targets
        .iter()
        .map(|key| Card {
            key,
            artwork_key: None,
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        })
        .collect();
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        total: 120,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.page(), Page::AllFilms);
    let mut continuation = Vec::new();
    for _ in 0..12 {
        continuation = ui.handle(Action::Down, &data);
    }
    assert_eq!(ui.focus(), Focus::Card { row: 12, column: 0 });
    assert!(
        !continuation.is_empty(),
        "approaching the tail must request the next admitted page"
    );
    assert_eq!(
        ui.page(),
        Page::AllFilms,
        "paging must not add a navigation entry"
    );
}

#[test]
fn retry_completion_restores_the_requested_card_and_keeps_back_history() {
    let id = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &id,
            artwork_key: None,
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        60
    ];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::Error,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    for _ in 0..15 {
        ui.handle(Action::Down, &data);
    }
    assert_eq!(ui.focus(), Focus::CatalogRetry);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![criterion_ui::Command::RetryCatalog]
    );
    let appended = vec![cards[0]; 120];
    let ready = ViewData {
        cards: &appended,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &ready);
    frame.output.textures_delta.clear();
    assert_eq!(
        ui.focus(),
        Focus::Card { row: 15, column: 0 },
        "retry must finish the still-current boundary intent"
    );
}

fn enter_grid(ui: &mut AppUi, data: &ViewData<'_>) {
    ui.handle(Action::Left, data);
    ui.handle(Action::Down, data);
    ui.handle(Action::Down, data);
    ui.handle(Action::Select, data);
}
#[test]
fn window_offset_preserves_global_card_identity_geometry_and_back() {
    let targets: Vec<_> = (0..240)
        .map(|i| Target::Media(MediaId::new(&format!("Film{i:04X}")).unwrap()))
        .collect();
    let cards: Vec<_> = targets
        .iter()
        .map(|key| Card {
            key,
            artwork_key: Some("synthetic-image"),
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        })
        .collect();
    let initial = ViewData {
        cards: &cards[..180],
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &initial);
    for _ in 0..43 {
        ui.handle(Action::Down, &initial);
    }
    let trimmed = ViewData {
        cards: &cards[60..],
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 60,
            tail: criterion_ui::CatalogTail::End,
        }),
        ..ViewData::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &trimmed);
    frame.output.textures_delta.clear();
    assert!(frame.visible_cards.len() <= criterion_ui::MAX_VISIBLE_CARDS);
    let focused = frame
        .visible_cards
        .iter()
        .find(|c| c.focus == (Focus::Card { row: 43, column: 0 }))
        .unwrap();
    assert_eq!(focused.key, targets[172]);
    assert_eq!(focused.image.min, egui::pos2(150.0, 420.0));
    assert_eq!(frame.visible_artwork, vec!["synthetic-image"]);
    let commands = ui.handle(Action::Select, &trimmed);
    assert!(commands.contains(&criterion_ui::Command::ActivateCard {
        target: targets[172].clone(),
        focus: Focus::Card { row: 43, column: 0 }
    }));
    assert_eq!(ui.page(), Page::Detail);
    ui.handle(Action::Back, &ViewData::default());
    assert_eq!(ui.page(), Page::AllFilms);
    assert_eq!(ui.focus(), Focus::Card { row: 43, column: 0 });
}
#[test]
fn moving_away_before_append_completion_does_not_apply_the_pending_down() {
    let id = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &id,
            artwork_key: None,
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        120
    ];
    let initial = ViewData {
        cards: &cards[..60],
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &initial);
    for _ in 0..15 {
        ui.handle(Action::Down, &initial);
    }
    assert_eq!(ui.focus(), Focus::Card { row: 14, column: 0 });
    ui.handle(Action::Up, &initial);
    let appended = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &appended);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 13, column: 0 });
}
#[test]
fn terminal_short_row_clamps_and_initial_error_has_remote_retry() {
    let id = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &id,
            artwork_key: None,
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        5
    ];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::End,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &data);
    for _ in 0..3 {
        ui.handle(Action::Right, &data);
    }
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 1, column: 0 });
    assert!(ui.handle(Action::Down, &data).is_empty());
    let failed = ViewData {
        status: LoadState::Error,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::Error,
        }),
        ..ViewData::default()
    };
    assert_eq!(
        ui.handle(Action::Select, &failed),
        vec![criterion_ui::Command::RetryCatalog]
    );
}
#[test]
fn leaving_filter_header_waits_for_page_zero_without_focusing_an_absent_card() {
    let id = Target::Media(MediaId::new("Film003C").unwrap());
    let cards = vec![
        Card {
            key: &id,
            artwork_key: None,
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        60
    ];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 60,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &ViewData::default());
    ui.handle(Action::Up, &data);
    assert_eq!(ui.focus(), Focus::FilterButton);
    let commands = ui.handle(Action::Down, &data);
    assert!(!commands.is_empty());
    assert_eq!(
        ui.focus(),
        Focus::FilterButton,
        "a pending head restore must keep its committed focus"
    );
    let first = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &first);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
}
#[test]
fn rehydration_error_and_retry_preserve_the_saved_global_focus() {
    let id = Target::Media(MediaId::new("Film00AC").unwrap());
    let cards = vec![
        Card {
            key: &id,
            artwork_key: None,
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        180
    ];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &data);
    for _ in 0..43 {
        ui.handle(Action::Down, &data);
    }
    ui.handle(Action::Select, &data);
    ui.handle(Action::Back, &ViewData::default());
    let saved_scroll = ui.scroll_y();
    let error = ViewData {
        status: LoadState::Error,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::Error,
        }),
        ..ViewData::default()
    };
    assert_eq!(
        ui.handle(Action::Select, &error),
        vec![criterion_ui::Command::RetryCatalog]
    );
    let restored = ViewData {
        cards: &cards[..60],
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 120,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &restored);
    frame.output.textures_delta.clear();
    assert_eq!(
        ui.focus(),
        Focus::Card { row: 43, column: 0 },
        "rehydration retry must preserve the saved global anchor"
    );
    assert_eq!(ui.scroll_y(), saved_scroll);
}
#[test]
fn pending_down_clamps_to_a_short_final_row_after_continuation() {
    let id = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &id,
            artwork_key: None,
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        61
    ];
    let initial = ViewData {
        cards: &cards[..60],
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &initial);
    for _ in 0..14 {
        ui.handle(Action::Down, &initial);
    }
    for _ in 0..3 {
        ui.handle(Action::Right, &initial);
    }
    ui.handle(Action::Down, &initial);
    let terminal = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::End,
        }),
        ..ViewData::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &terminal);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 15, column: 0 });
}
#[test]
fn pending_down_after_rehydration_demands_the_still_absent_position() {
    let id = Target::Media(MediaId::new("Film00B0").unwrap());
    let cards = vec![
        Card {
            key: &id,
            artwork_key: None,
            title: "Synthetic",
            year: "",
            duration_label: None,
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        180
    ];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &data);
    for _ in 0..44 {
        ui.handle(Action::Down, &data);
    }
    let loading = ViewData {
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::Loading,
        }),
        ..ViewData::default()
    };
    ui.handle(Action::Down, &loading);
    let restored = ViewData {
        cards: &cards[..60],
        status: LoadState::Ready,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 120,
            tail: criterion_ui::CatalogTail::More,
        }),
        ..ViewData::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &restored);
    frame.output.textures_delta.clear();
    assert!(
        frame.commands.contains(&criterion_ui::Command::Catalog {
            anchor: 176,
            target: 180
        }),
        "an unchanged pending Down must continue after rehydration finishes"
    );
}
#[test]
fn empty_error_back_restores_the_search_origin_instead_of_reentering_retry_focus() {
    let data = ViewData::default();
    let mut ui = AppUi::new();
    for action in [Action::Left, Action::Up, Action::Select] {
        ui.handle(action, &data);
    }
    assert_eq!(ui.page(), Page::Search);
    for action in [
        Action::Left,
        Action::Down,
        Action::Down,
        Action::Down,
        Action::Select,
    ] {
        ui.handle(action, &data);
    }
    assert_eq!(ui.page(), Page::AllFilms);
    let failed = ViewData {
        status: LoadState::Error,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::Error,
        }),
        ..ViewData::default()
    };
    assert_eq!(
        ui.handle(Action::Back, &failed),
        vec![criterion_ui::Command::Restore(Page::Search)]
    );
    assert_eq!(ui.page(), Page::Search);
}
#[test]
fn initial_error_without_history_can_still_leave_through_the_rail() {
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &ViewData::default());
    let failed = ViewData {
        status: LoadState::Error,
        catalog: Some(criterion_ui::CatalogWindow {
            first: 0,
            tail: criterion_ui::CatalogTail::Error,
        }),
        ..ViewData::default()
    };
    ui.handle(Action::Left, &failed);
    assert_eq!(ui.focus(), Focus::Rail(criterion_ui::RailItem::AllFilms));
    ui.handle(Action::Up, &failed);
    ui.handle(Action::Up, &failed);
    assert_eq!(
        ui.handle(Action::Select, &failed),
        vec![criterion_ui::Command::Navigate(Page::Home)]
    );
}
