use criterion_ui::{Action, AppUi, Card, Command, LoadState, ViewData};
fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}
#[test]
fn pointer_release_activates_only_the_current_pressed_media_identity() {
    let id = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let cards = [Card {
        key: &id,
        artwork_key: None,
        title: "The Hitcher",
        year: "1986",
        duration_label: Some("1 h 37 min"),
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![
                pointer(egui::pos2(300.0, 330.0), true),
                pointer(egui::pos2(300.0, 330.0), false),
            ],
            ..Default::default()
        },
        &data,
    );
    let commands = std::mem::take(&mut frame.commands);
    frame.output.textures_delta.clear();
    assert_eq!(
        commands,
        vec![Command::ActivateCard {
            target: id,
            focus: criterion_ui::Focus::Card { row: 0, column: 0 }
        }]
    );
}
#[test]
fn committed_text_and_ime_are_bounded_and_only_search_field_edits_publish() {
    let data = ViewData::default();
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![
                egui::Event::Ime(egui::ImeEvent::Preedit {
                    text: "未".into(),
                    active_range_chars: None,
                }),
                egui::Event::Ime(egui::ImeEvent::Commit("Taxi".into())),
            ],
            ..Default::default()
        },
        &data,
    );
    assert_eq!(ui.query(), "taxi");
    assert!(frame.wants_text_input);
    assert_eq!(frame.commands.len(), 1);
    frame.output.textures_delta.clear();
    ui.handle(Action::Right, &data);
    for _ in 0..5 {
        ui.handle(Action::Right, &data);
    }
    ui.handle(Action::Down, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![egui::Event::Text("ignored".into())],
            ..Default::default()
        },
        &data,
    );
    assert_eq!(ui.query(), "taxi");
    assert!(!frame.wants_text_input);
    assert!(frame.commands.is_empty());
    frame.output.textures_delta.clear();
}
#[test]
fn pointer_gone_and_replaced_media_cancel_pending_activation() {
    let first = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let second = criterion_ui::Target::Media(criterion_provider::MediaId::new("zxlDvz82").unwrap());
    let card = |key| Card {
        key,
        artwork_key: None,
        title: "Fixture",
        year: "1986",
        duration_label: Some("1 h 37 min"),
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card(&first)];
    let replaced = [card(&second)];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let replacement = ViewData {
        cards: &replaced,
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(300.0, 330.0), true)],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(300.0, 330.0), false)],
            ..Default::default()
        },
        &replacement,
    );
    assert!(frame.commands.is_empty());
    frame.output.textures_delta.clear();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![
                pointer(egui::pos2(300.0, 330.0), true),
                egui::Event::PointerGone,
                pointer(egui::pos2(300.0, 330.0), false),
            ],
            ..Default::default()
        },
        &data,
    );
    assert!(frame.commands.is_empty());
    frame.output.textures_delta.clear();
}
#[test]
fn native_query_admission_bounds_unicode_and_drops_controls() {
    let data = ViewData::default();
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![egui::Event::Text(format!("\n{}", "東京".repeat(500)))],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert!(ui.query().len() <= 256);
    assert!(!ui.query().contains('\n'));
}
#[test]
fn hero_press_cannot_activate_replacement_media() {
    let first = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let second = criterion_ui::Target::Media(criterion_provider::MediaId::new("zxlDvz82").unwrap());
    let data = |key| ViewData {
        hero: Some(criterion_ui::Hero {
            card: Card {
                key,
                artwork_key: None,
                title: "Fixture",
                year: "1986",
                duration_label: Some("1 h 37 min"),
                saved_fraction: None,
                action: criterion_ui::CardAction::Open,
            },
            description: "",
            action: "SEE MORE",
            action_kind: criterion_ui::HeroAction::Open,
            background_key: None,
            title_logo_key: None,
        }),
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(220.0, 770.0), true)],
            ..Default::default()
        },
        &data(&first),
    );
    frame.output.textures_delta.clear();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(220.0, 770.0), false)],
            ..Default::default()
        },
        &data(&second),
    );
    let commands = std::mem::take(&mut frame.commands);
    frame.output.textures_delta.clear();
    assert!(commands.is_empty());
}
#[test]
fn pointer_and_text_events_preserve_focus_order() {
    let data = ViewData::default();
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![
                pointer(egui::pos2(600.0, 280.0), true),
                pointer(egui::pos2(600.0, 280.0), false),
                egui::Event::Ime(egui::ImeEvent::Commit("ignored".into())),
            ],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert_eq!(ui.query(), "");
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![
                pointer(egui::pos2(950.0, 165.0), true),
                pointer(egui::pos2(950.0, 165.0), false),
                egui::Event::Text("Taxi".into()),
            ],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert_eq!(ui.query(), "taxi");
}
#[test]
fn filter_press_keeps_the_painted_option_window_until_release() {
    let options: Vec<_> = (0..20).map(|_| "Fixture option").collect();
    let groups = [criterion_ui::FilterGroup {
        label: "Genres",
        options: &options,
    }];
    let menu = criterion_ui::FilterMenu { groups: &groups };
    let data = ViewData {
        filters: Some(menu),
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Select, &data);
    ui.handle(Action::Down, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(800.0, 670.0), true)],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(800.0, 670.0), false)],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    ui.handle(Action::Left, &data);
    for _ in 0..4 {
        ui.handle(Action::Down, &data);
    }
    let commands = ui.handle(Action::Select, &data);
    assert!(
        matches!(&commands[..],[Command::ApplyFilters(selection)] if selection.options==vec![(0,8)])
    );
}
#[test]
fn detail_tab_press_is_bound_to_the_owning_detail_identity() {
    let first = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let second = criterion_ui::Target::Media(criterion_provider::MediaId::new("zxlDvz82").unwrap());
    let card = |key| Card {
        key,
        artwork_key: None,
        title: "Fixture",
        year: "1986",
        duration_label: Some("1 h 37 min"),
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card(&first)];
    let rails = [criterion_ui::Rail {
        title: "Supplements",
        cards: &cards,
    }];
    let data = |key| ViewData {
        cards: &cards,
        rails: &rails,
        detail: Some(criterion_ui::Detail {
            card: card(key),
            header_metadata: "1986   1h 37m",
            information_metadata: "1986   1h 37m",
            directors: "",
            description: "",
            starring: None,
            countries: None,
            languages: None,
            content_warnings: None,
            primary_action: "WATCH NOW",
            primary_playback_target: key.media_id(),
            selected_playlist: None,
            featured: None,
            seasons: None,
            kind: criterion_ui::DetailKind::Film,
            sort: None,
            membership: criterion_ui::ListMembership::SignedOut,
        }),
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    let original = data(&first);
    ui.handle(Action::Left, &original);
    ui.handle(Action::Down, &original);
    ui.handle(Action::Down, &original);
    ui.handle(Action::Select, &original);
    ui.handle(Action::Select, &original);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(250.0, 990.0), true)],
            ..Default::default()
        },
        &original,
    );
    frame.output.textures_delta.clear();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(250.0, 990.0), false)],
            ..Default::default()
        },
        &data(&second),
    );
    let commands = std::mem::take(&mut frame.commands);
    frame.output.textures_delta.clear();
    assert!(commands.is_empty());
}
