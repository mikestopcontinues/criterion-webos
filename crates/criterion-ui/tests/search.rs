use criterion_ui::{Action, AppUi, Command, Focus, SearchGroup, ViewData};
fn search(ui: &mut AppUi, data: &ViewData<'_>) {
    ui.handle(Action::Left, data);
    ui.handle(Action::Up, data);
    ui.handle(Action::Select, data);
}
#[test]
fn search_keyboard_emits_live_lowercase_queries_without_submission() {
    let data = ViewData::default();
    let mut ui = AppUi::new();
    search(&mut ui, &data);
    assert_eq!(ui.focus(), Focus::SearchKey(0));
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::Search {
            query: "a".into(),
            group: SearchGroup::All
        }]
    );
    ui.handle(Action::Right, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::Search {
            query: "ab".into(),
            group: SearchGroup::All
        }]
    );
    assert_eq!(ui.query(), "ab");
}
#[test]
fn search_frame_places_three_results_beside_the_keyboard() {
    let id = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let cards = vec![
        criterion_ui::Card {
            key: &id,
            artwork_key: None,
            title: "Fixture",
            year: "1986",
            duration_label: Some("1 h 37 min"),
            saved_fraction: None,
            action: criterion_ui::CardAction::Open,
        };
        7
    ];
    let data = ViewData {
        cards: &cards,
        status: criterion_ui::LoadState::Ready,
        search_counts: [7, 5, 1, 1],
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    search(&mut ui, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    let rects: Vec<_> = frame
        .visible_cards
        .iter()
        .take(3)
        .map(|card| {
            [
                card.image.left(),
                card.image.top(),
                card.image.width(),
                card.image.height(),
            ]
        })
        .collect();
    frame.output.textures_delta.clear();
    assert_eq!(
        rects,
        vec![
            [565.0, 360.0, 378.0, 213.0],
            [979.0, 360.0, 378.0, 213.0],
            [1393.0, 360.0, 378.0, 213.0]
        ]
    );
}
#[test]
fn search_detail_back_restores_query_group_card_then_home_focus() {
    let id = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let cards = [criterion_ui::Card {
        key: &id,
        artwork_key: None,
        title: "Fixture",
        year: "1986",
        duration_label: Some("1 h 37 min"),
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let rails = [criterion_ui::Rail {
        title: "Popular Movies",
        cards: &cards,
    }];
    let data = ViewData {
        cards: &cards,
        rails: &rails,
        status: criterion_ui::LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    search(&mut ui, &data);
    ui.handle(Action::Select, &data);
    for _ in 0..6 {
        ui.handle(Action::Right, &data);
    }
    ui.handle(Action::Down, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.search_group(), SearchGroup::Collections);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    ui.handle(Action::Back, &data);
    assert_eq!(ui.page(), criterion_ui::Page::Search);
    assert_eq!(ui.query(), "a");
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.search_group(), SearchGroup::Collections);
    ui.handle(Action::Back, &data);
    assert_eq!(ui.page(), criterion_ui::Page::Home);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.scroll_y(), 632.0);
}

#[test]
fn separate_search_visits_restore_their_query_and_group() {
    let data = ViewData::default();
    let mut ui = AppUi::new();
    search(&mut ui, &data);
    ui.handle(Action::Select, &data);
    for _ in 0..6 {
        ui.handle(Action::Right, &data);
    }
    ui.handle(Action::Down, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.search_group(), SearchGroup::Collections);
    for _ in 0..4 {
        ui.handle(Action::Left, &data);
    }
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.page(), criterion_ui::Page::Home);
    search(&mut ui, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        ..Default::default()
                    },
                },
                egui::Event::Text("Beta".into()),
            ],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    for _ in 0..6 {
        ui.handle(Action::Right, &data);
    }
    ui.handle(Action::Down, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.query(), "beta");
    assert_eq!(ui.search_group(), SearchGroup::Films);
    assert_eq!(
        ui.handle(Action::Back, &data),
        vec![Command::Restore(criterion_ui::Page::Home)]
    );
    assert_eq!(
        ui.handle(Action::Back, &data),
        vec![Command::Restore(criterion_ui::Page::Search)]
    );
    assert_eq!(ui.query(), "a");
    assert_eq!(ui.search_group(), SearchGroup::Collections);
}

#[test]
fn leaving_the_text_field_clears_transient_preedit() {
    let mut ui = AppUi::new();
    let data = ViewData::default();
    search(&mut ui, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "TRANSIENT".into(),
                active_range_chars: None,
            })],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    for _ in 0..6 {
        ui.handle(Action::Right, &data);
    }
    assert!(!ui.wants_text_input());
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(!frame.output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("TRANSIENT"))
    }));
}
