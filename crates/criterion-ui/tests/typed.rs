use criterion_provider::MediaId;
use criterion_ui::{Action, AppUi, Card, Command, LoadState, ViewData};
#[test]
fn activated_card_emits_its_validated_media_identity() {
    let id = MediaId::new("qvwT6mJ4").unwrap();
    let cards = [Card {
        key: &id,
        artwork_key: Some("qvwT6mJ4/default_16x9/480"),
        title: "The Hitcher",
        year: "1986",
        duration_seconds: 5820,
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
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::OpenMedia(id)]
    );
}
#[test]
fn hero_watch_action_emits_play_without_changing_the_discovery_page() {
    let id = MediaId::new("qvwT6mJ4").unwrap();
    let data = ViewData {
        hero: Some(criterion_ui::Hero {
            card: Card {
                key: &id,
                artwork_key: None,
                title: "Fixture",
                year: "1986",
                duration_seconds: 5820,
            },
            description: "",
            action: "WATCH NOW",
            action_kind: criterion_ui::HeroAction::Play,
            background_key: None,
            title_logo_key: None,
        }),
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    assert_eq!(ui.handle(Action::Select, &data), vec![Command::Play(id)]);
    assert_eq!(ui.page(), criterion_ui::Page::Home);
}
