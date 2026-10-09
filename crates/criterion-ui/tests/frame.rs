use criterion_ui::{Action, AppUi, Card, LoadState, Page, ViewData};
fn grid(ui: &mut AppUi) {
    let data = ViewData::default();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.page(), Page::AllFilms);
}
#[test]
fn cpu_frame_uses_observed_four_landscape_columns_and_visible_budget() {
    let mut ui = AppUi::new();
    grid(&mut ui);
    let id = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let cards = vec![
        Card {
            key: &id,
            artwork_key: Some("qvwT6mJ4/default_16x9/480"),
            title: "The Hitcher",
            year: "1986",
            duration_seconds: 5820
        };
        1000
    ];
    let data = ViewData {
        title: "All Films",
        hero: None,
        rails: &[],
        cards: &cards,
        total: 1000,
        status: LoadState::Ready,
        filters: None,
        detail: None,
        catalog: None,
        search_counts: [0; 4],
        login: criterion_ui::LoginView::SignedOut,
    };
    let mut frame = ui.render(egui::RawInput::default(), &data);
    assert_eq!(frame.visible_cards.len(), 12);
    let rects: Vec<_> = frame
        .visible_cards
        .iter()
        .take(4)
        .map(|c| {
            [
                c.image.left(),
                c.image.top(),
                c.image.width(),
                c.image.height(),
            ]
        })
        .collect();
    assert_eq!(
        rects,
        vec![
            [150.0, 248.0, 378.0, 213.0],
            [564.0, 248.0, 378.0, 213.0],
            [978.0, 248.0, 378.0, 213.0],
            [1392.0, 248.0, 378.0, 213.0]
        ]
    );
    assert!(!frame.output.shapes.is_empty());
    frame.output.textures_delta.clear();
}
