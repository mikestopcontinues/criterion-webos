use criterion_provider::MediaId;
use criterion_ui::{Action, AppUi, Card, Command, LoadState, ViewData};
#[test]
fn activated_card_emits_its_validated_media_identity() {
    let id = MediaId::new("qvwT6mJ4").unwrap();
    let target = criterion_ui::Target::Media(id.clone());
    let cards = [Card {
        key: &target,
        artwork_key: Some("qvwT6mJ4/default_16x9/480"),
        title: "The Hitcher",
        year: "1986",
        duration_seconds: 5820,
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
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::Open(target)]
    );
}
#[test]
fn hero_watch_action_emits_play_without_changing_the_discovery_page() {
    let id = MediaId::new("qvwT6mJ4").unwrap();
    let target = criterion_ui::Target::Media(id.clone());
    let data = ViewData {
        hero: Some(criterion_ui::Hero {
            card: Card {
                key: &target,
                artwork_key: None,
                title: "Fixture",
                year: "1986",
                duration_seconds: 5820,
                saved_fraction: None,
                action: criterion_ui::CardAction::Open,
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
#[test]
fn discovery_navigation_forwards_validated_content_without_a_fake_media_id() {
    let target = criterion_ui::Target::Content(
        criterion_provider::ContentTarget::parse("/discover/popular-movies").unwrap(),
    );
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Popular Movies",
        year: "",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let rails = [criterion_ui::Rail {
        title: "Explore",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::Open(target)]
    );
    assert_eq!(ui.page(), criterion_ui::Page::Discovery);
}
#[test]
fn my_list_keeps_visible_grid_focus_above_the_first_row() {
    let target = criterion_ui::Target::Content(criterion_provider::ContentTarget::MyList);
    let data = ViewData {
        login: criterion_ui::LoginView::SignedIn,
        hero: Some(criterion_ui::Hero {
            card: Card {
                key: &target,
                artwork_key: None,
                title: "My List",
                year: "",
                duration_seconds: 0,
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
    ui.handle(Action::Select, &data);
    ui.handle(Action::Up, &data);
    assert_eq!(ui.page(), criterion_ui::Page::MyList);
    assert_eq!(ui.focus(), criterion_ui::Focus::Card { row: 0, column: 0 });
}
#[test]
fn same_page_content_activation_ends_the_departed_pointer_batch() {
    let first = criterion_ui::Target::Content(
        criterion_provider::ContentTarget::parse("/discover/first").unwrap(),
    );
    let next = criterion_ui::Target::Content(
        criterion_provider::ContentTarget::parse("/discover/second").unwrap(),
    );
    let card = |key| Card {
        key,
        artwork_key: None,
        title: "Explore",
        year: "",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let entry = ViewData {
        hero: Some(criterion_ui::Hero {
            card: card(&first),
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
    ui.handle(Action::Select, &entry);
    let cards = [card(&next)];
    let rails = [criterion_ui::Rail {
        title: "Explore",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let click = |pressed| egui::Event::PointerButton {
        pos: egui::pos2(250.0, 980.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click(true), click(false), click(true), click(false)],
            ..Default::default()
        },
        &data,
    );
    let commands = std::mem::take(&mut frame.commands);
    frame.output.textures_delta.clear();
    assert_eq!(commands, vec![Command::Open(next)]);
}
