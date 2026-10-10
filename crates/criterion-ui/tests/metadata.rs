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
            starring: "",
            countries: "",
            languages: "",
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
