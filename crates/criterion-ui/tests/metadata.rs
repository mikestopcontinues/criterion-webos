use criterion_ui::{Action, AppUi, Card, Detail, DetailKind, LoadState, ViewData};

fn text(frame: &criterion_ui::UiFrame) -> String {
    frame
        .output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                Some(text.galley.job.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn unavailable_duration_is_omitted_from_grid_detail_and_information() {
    let target = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let card = Card {
        key: &target,
        artwork_key: None,
        title: "Fixture",
        year: "1986",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        detail: Some(Detail {
            card,
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
            kind: DetailKind::Film,
        }),
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(!text(&frame).contains("0 min"));
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(!text(&frame).contains("0 min"));
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(!text(&frame).contains("0 min"));
}

#[test]
fn native_information_omits_headings_for_absent_metadata() {
    let target =
        criterion_ui::Target::Native(criterion_provider::MediaId::new("Native01").unwrap());
    let card = Card {
        key: &target,
        artwork_key: None,
        title: "Native fixture",
        year: "1986",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let rails = [criterion_ui::Rail {
        title: "Native origin",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        detail: Some(Detail {
            card,
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
            kind: DetailKind::Film,
        }),
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        [criterion_ui::Command::Open(target.clone())]
    );
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.focus(), criterion_ui::Focus::InformationPrimary);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    let painted = text(&frame);
    assert!(painted.contains("Native fixture"));
    for heading in ["Starring", "Countries", "Languages"] {
        assert!(
            !painted.lines().any(|line| line == heading),
            "absent metadata must not paint {heading}"
        );
    }
}

fn populated_native_information(value: &str) -> criterion_ui::UiFrame {
    let target =
        criterion_ui::Target::Native(criterion_provider::MediaId::new("Native01").unwrap());
    let card = Card {
        key: &target,
        artwork_key: None,
        title: "Native fixture",
        year: "1986",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let rails = [criterion_ui::Rail {
        title: "Native origin",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        detail: Some(Detail {
            card,
            directors: "Fixture director",
            description: "Fixture synopsis",
            starring: Some(value),
            countries: Some(value),
            languages: Some(value),
            content_warnings: Some(value),
            primary_action: "WATCH NOW",
            primary_playback_target: Some(target.media_id().unwrap()),
            selected_playlist: None,
            seasons: None,
            kind: DetailKind::Film,
        }),
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.focus(), criterion_ui::Focus::InformationPrimary);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    frame
}

#[test]
fn native_information_retains_headings_for_present_empty_values() {
    let frame = populated_native_information("");
    let painted = text(&frame);
    for heading in ["Starring", "Countries", "Languages", "Content Warnings"] {
        assert!(painted.lines().any(|line| line == heading));
    }
}

#[test]
fn all_four_information_fields_fit_without_overlap_or_clipping() {
    let frame = populated_native_information("English, Egypt, psychological horror");
    let mut bounds = Vec::new();
    for shape in &frame.output.shapes {
        if let egui::Shape::Text(text) = &shape.shape
            && [
                "Starring",
                "Countries",
                "Languages",
                "Content Warnings",
                "English, Egypt, psychological horror",
            ]
            .contains(&text.galley.job.text.as_str())
        {
            let rect = shape.shape.visual_bounding_rect();
            assert!(
                shape.clip_rect.contains_rect(rect),
                "metadata must be fully visible"
            );
            bounds.push(rect);
        }
    }
    assert_eq!(bounds.len(), 8, "four headings and their four values");
    bounds.sort_by(|left, right| left.top().total_cmp(&right.top()));
    for pair in bounds.windows(2) {
        assert!(
            pair[0].bottom() < pair[1].top(),
            "metadata rows must not overlap"
        );
    }
    assert!(
        bounds.last().unwrap().bottom() < 826.0,
        "metadata must fit above year/runtime"
    );
}
