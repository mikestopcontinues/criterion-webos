use criterion_ui::{Action, AppUi, Focus, ViewData};

fn text(frame: &criterion_ui::UiFrame) -> String {
    frame
        .output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_vector_at(frame: &criterion_ui::UiFrame, center: egui::Pos2) -> bool {
    let area = egui::Rect::from_center_size(center, egui::vec2(40.0, 40.0));
    frame.output.shapes.iter().any(|shape| {
        matches!(
            shape.shape,
            egui::Shape::LineSegment { .. } | egui::Shape::Path(_) | egui::Shape::Circle(_)
        ) && area.contains_rect(shape.shape.visual_bounding_rect())
    })
}

#[test]
fn search_controls_use_vectors_without_symbol_font_glyphs() {
    let data = ViewData::default();
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    let painted_text = text(&frame);
    for symbol in ["⌕", "⌂", "✧", "◉", "♙", "␣", "←", "MIC"] {
        assert!(!painted_text.contains(symbol), "text icon {symbol}");
    }
    for center in [
        egui::pos2(75.0, 208.0),
        egui::pos2(75.0, 356.0),
        egui::pos2(75.0, 430.0),
        egui::pos2(75.0, 504.0),
        egui::pos2(75.0, 649.0),
        egui::pos2(243.5, 606.0),
        egui::pos2(435.5, 606.0),
        egui::pos2(615.0, 165.0),
        egui::pos2(749.0, 165.0),
    ] {
        assert!(has_vector_at(&frame, center), "missing icon at {center}");
    }
    assert_eq!(ui.focus(), Focus::SearchKey(0));
    assert!(painted_text.contains('A'));
}

#[test]
fn filter_and_information_controls_use_vector_marks_and_keep_remote_actions() {
    let target = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let card = criterion_ui::Card {
        key: &target,
        artwork_key: None,
        title: "Fixture",
        year: "1986",
        duration_label: Some("1 h 37 min"),
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let groups = [criterion_ui::FilterGroup {
        label: "Genres",
        options: &["Fixture genre"],
    }];
    let data = ViewData {
        cards: &cards,
        filters: Some(criterion_ui::FilterMenu { groups: &groups }),
        detail: Some(criterion_ui::Detail {
            card,
            header_metadata: "1986   1h 37m",
            information_metadata: "1986   1h 37m",
            directors: "Fixture director",
            description: "Fixture synopsis",
            starring: None,
            countries: None,
            languages: None,
            content_warnings: None,
            primary_action: "WATCH NOW",
            primary_playback_target: Some(target.media_id().unwrap()),
            selected_playlist: None,
            seasons: None,
            kind: criterion_ui::DetailKind::Film,
            membership: criterion_ui::ListMembership::SignedOut,
        }),
        status: criterion_ui::LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    for action in [
        Action::Left,
        Action::Down,
        Action::Down,
        Action::Select,
        Action::Up,
        Action::Select,
    ] {
        ui.handle(action, &data);
    }
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    for symbol in ["×", "↑", "↓", "↶", ">", "···"] {
        assert!(!text(&frame).contains(symbol), "text icon {symbol}");
    }
    assert!(has_vector_at(&frame, egui::pos2(1671.0, 180.0)));
    assert!(has_vector_at(&frame, egui::pos2(251.0, 909.0)));
    assert!(has_vector_at(&frame, egui::pos2(1156.0, 295.0)));
    for action in [Action::Down, Action::Right, Action::Select] {
        ui.handle(action, &data);
    }
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(!text(&frame).contains('✓'));
    assert_eq!(ui.focus(), Focus::FilterOption(0));
    assert!(text(&frame).contains("Fixture genre"));
    ui.handle(Action::Back, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::InformationPrimary);
    assert!(!text(&frame).contains('×'));
    assert!(has_vector_at(&frame, egui::pos2(1531.0, 149.0)));
}
