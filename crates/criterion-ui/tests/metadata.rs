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
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        detail: Some(Detail {
            card,
            header_metadata: "1986",
            information_metadata: "1986",
            directors: "Fixture director",
            description: "Fixture synopsis",
            starring: None,
            countries: None,
            languages: None,
            content_warnings: None,
            primary_action: "WATCH NOW",
            primary_playback_target: Some(target.media_id().unwrap()),
            selected_playlist: None,
            featured: None,
            seasons: None,
            kind: DetailKind::Film,
            sort: None,
            membership: criterion_ui::ListMembership::SignedOut,
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
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let rails = [criterion_ui::Rail {
        action: None,
        title: "Native origin",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        detail: Some(Detail {
            card,
            header_metadata: "1986",
            information_metadata: "1986",
            directors: "Fixture director",
            description: "Fixture synopsis",
            starring: None,
            countries: None,
            languages: None,
            content_warnings: None,
            primary_action: "WATCH NOW",
            primary_playback_target: Some(target.media_id().unwrap()),
            selected_playlist: None,
            featured: None,
            seasons: None,
            kind: DetailKind::Film,
            sort: None,
            membership: criterion_ui::ListMembership::SignedOut,
        }),
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        [criterion_ui::Command::ActivateCard {
            target: target.clone(),
            focus: criterion_ui::Focus::Card { row: 0, column: 0 }
        }]
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

#[test]
fn header_and_information_paint_their_independent_borrowed_runtime_lines() {
    let target =
        criterion_ui::Target::Native(criterion_provider::MediaId::new("Native01").unwrap());
    let card = Card {
        key: &target,
        artwork_key: None,
        title: "Independent runtime fixture",
        year: "1986",
        duration_label: Some("1 h 0 min"),
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let rails = [criterion_ui::Rail {
        action: None,
        title: "Native origin",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        detail: Some(Detail {
            card,
            header_metadata: "85   596523h 14m",
            information_metadata: "85   0m",
            directors: "",
            description: "",
            starring: None,
            countries: None,
            languages: None,
            content_warnings: None,
            primary_action: "WATCH NOW",
            primary_playback_target: Some(target.media_id().unwrap()),
            selected_playlist: None,
            featured: None,
            seasons: None,
            kind: DetailKind::Film,
            sort: None,
            membership: criterion_ui::ListMembership::SignedOut,
        }),
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    let header_region =
        egui::Rect::from_min_max(egui::pos2(150.0, 540.0), egui::pos2(1400.0, 600.0));
    let information_region =
        egui::Rect::from_min_max(egui::pos2(348.0, 826.0), egui::pos2(1570.0, 852.0));
    let assert_line = |frame: &criterion_ui::UiFrame, expected: &str, region: egui::Rect| {
        let shape = frame
            .output
            .shapes
            .iter()
            .find(|shape| {
                matches!(
                    &shape.shape, egui::Shape::Text(text)
                        if region.contains(text.pos) && text.galley.job.text == expected
                )
            })
            .expect("independent metadata line on intended surface");
        let bounds = shape.shape.visual_bounding_rect();
        assert!(shape.clip_rect.contains_rect(bounds));
        assert!(region.contains_rect(bounds));
        assert!(
            !text(frame).contains("1986   1h 0m"),
            "card metadata is not reformatted by either renderer"
        );
    };
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_line(&frame, "85   596523h 14m", header_region);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.focus(), criterion_ui::Focus::InformationPrimary);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_line(&frame, "85   0m", information_region);
    assert_line(&frame, "85   596523h 14m", header_region);
}

fn populated_native_information(value: &str) -> criterion_ui::UiFrame {
    let target =
        criterion_ui::Target::Native(criterion_provider::MediaId::new("Native01").unwrap());
    let card = Card {
        key: &target,
        artwork_key: None,
        title: "Native fixture",
        year: "1986",
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let rails = [criterion_ui::Rail {
        action: None,
        title: "Native origin",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        detail: Some(Detail {
            card,
            header_metadata: "1986",
            information_metadata: "1986",
            directors: "Fixture director",
            description: "Fixture synopsis",
            starring: Some(value),
            countries: Some(value),
            languages: Some(value),
            content_warnings: Some(value),
            primary_action: "WATCH NOW",
            primary_playback_target: Some(target.media_id().unwrap()),
            selected_playlist: None,
            featured: None,
            seasons: None,
            kind: DetailKind::Film,
            sort: None,
            membership: criterion_ui::ListMembership::SignedOut,
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
