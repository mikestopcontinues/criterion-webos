use criterion_ui::{Action, AppUi, Card, Detail, DetailKind, LoadState, ViewData};

fn assert_counter(frame: &criterion_ui::UiFrame, expected: &str) {
    let region = egui::Rect::from_min_max(egui::pos2(348.0, 859.0), egui::pos2(912.0, 903.0));
    let shape = frame
        .output
        .shapes
        .iter()
        .find(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == expected))
        .expect("long synopsis exposes the current page and the remaining content");
    let egui::Shape::Text(text) = &shape.shape else {
        unreachable!()
    };
    assert!(!text.galley.elided);
    assert!(region.contains_rect(shape.shape.visual_bounding_rect()));
    assert!(
        shape
            .clip_rect
            .contains_rect(shape.shape.visual_bounding_rect())
    );
}

#[test]
fn long_synopsis_exposes_paging_and_remote_navigation_retains_every_page() {
    let target =
        criterion_ui::Target::Native(criterion_provider::MediaId::new("Native01").unwrap());
    let card = Card {
        key: &target,
        artwork_key: None,
        title: "Synthetic synopsis fixture",
        year: "1986",
        duration_label: None,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let cards = [card];
    let rails = [criterion_ui::Rail {
        title: "Synthetic origin",
        cards: &cards,
    }];
    let description = format!("{}Final synopsis sentence.", "First page. ".repeat(35));
    let mut data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        detail: Some(Detail {
            card,
            header_metadata: "1986",
            information_metadata: "1986",
            directors: "Synthetic director",
            description: &description,
            starring: Some("Synthetic cast"),
            countries: Some("Synthetic country"),
            languages: Some("Synthetic language"),
            content_warnings: Some("Synthetic warning"),
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
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_counter(&frame, "Synopsis 1 / 2");
    ui.handle(Action::Down, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_counter(&frame, "Synopsis 2 / 2");
    assert!(frame.output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "Final synopsis sentence." && !text.galley.elided)));
    ui.handle(Action::Up, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_counter(&frame, "Synopsis 1 / 2");
    ui.handle(Action::Back, &data);
    data.detail.as_mut().unwrap().description = "Short complete synopsis.";
    ui.handle(Action::Select, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(
        !frame
            .output
            .shapes
            .iter()
            .any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text.starts_with("Synopsis ")))
    );
}
