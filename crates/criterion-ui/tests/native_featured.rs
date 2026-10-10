use criterion_provider::MediaId;
use criterion_ui::{
    Action, AppUi, Card, CardAction, Command, Detail, DetailKind, Featured, Focus, Hero,
    HeroAction, LoadState, Page, Rail, Target, ViewData,
};

fn card(target: &Target, action: CardAction) -> Card<'_> {
    Card {
        key: target,
        title: "Synthetic Feature card",
        year: "",
        artwork_key: Some("synthetic-feature-art"),
        duration_label: Some("1 min"),
        saved_fraction: Some(0.2),
        action,
    }
}
fn detail<'a>(root: &'a Target, title: Option<&'a str>, cards: &'a [Card<'a>]) -> Detail<'a> {
    Detail {
        card: card(root, CardAction::Open),
        header_metadata: "",
        information_metadata: "",
        directors: "",
        description: "Synthetic Feature description",
        starring: None,
        countries: None,
        languages: None,
        content_warnings: None,
        primary_action: "WATCH NOW",
        primary_playback_target: None,
        selected_playlist: Some(0),
        seasons: None,
        featured: Some(Featured { title, cards }),
        kind: DetailKind::Collection,
        membership: criterion_ui::ListMembership::SignedOut,
    }
}
fn open(ui: &mut AppUi, root: &Target) {
    let view = ViewData {
        hero: Some(Hero {
            card: card(root, CardAction::Open),
            description: "",
            action: "SEE MORE",
            action_kind: HeroAction::Open,
            background_key: None,
            title_logo_key: None,
        }),
        ..Default::default()
    };
    assert_eq!(
        ui.handle(Action::Select, &view),
        [Command::Open(root.clone())]
    );
}
fn frame(ui: &mut AppUi, data: &ViewData<'_>, events: Vec<egui::Event>) -> criterion_ui::UiFrame {
    let mut result = ui.render(
        egui::RawInput {
            events,
            ..Default::default()
        },
        data,
    );
    result.output.textures_delta.clear();
    result
}
fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Default::default(),
    }
}

#[test]
fn feature_row_has_its_own_address_artwork_and_actions_outside_tab_indices() {
    let root = Target::Native(MediaId::new("Root0001").unwrap());
    let same = Target::Native(MediaId::new("Shared01").unwrap());
    let feature = [card(&same, CardAction::Open)];
    let ordinary = [card(&same, CardAction::Play)];
    let rails = [Rail {
        title: "Synthetic tab",
        cards: &ordinary,
    }];
    let data = ViewData {
        detail: Some(detail(&root, Some("Synthetic supplied Feature"), &feature)),
        rails: &rails,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    open(&mut ui, &root);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::FeaturedCard(0));
    let painted = frame(&mut ui, &data, vec![]);
    let selected = painted
        .visible_cards
        .iter()
        .find(|card| card.focus == Focus::FeaturedCard(0))
        .unwrap();
    assert_eq!(selected.key, same);
    assert_eq!(selected.image.min, egui::pos2(150.0, 415.0));
    assert_eq!(
        painted
            .visible_artwork
            .iter()
            .filter(|key| key.as_str() == "synthetic-feature-art")
            .count(),
        1
    );
    let text_bounds = |value: &str| {
        painted
            .output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == value => {
                    Some(shape.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let tab = text_bounds("Synthetic tab")[0];
    let feature_title = text_bounds("Synthetic Feature card")[0];
    let runtime = text_bounds("1 min")[0];
    assert!(
        feature_title.bottom() < tab.top() && runtime.bottom() < tab.top(),
        "all Feature card labels must finish before the first playlist tab"
    );
    assert!(painted.visible_cards.len() <= criterion_ui::MAX_VISIBLE_CARDS);
    assert_eq!(
        ui.handle(Action::Select, &data),
        [Command::ActivateCard {
            target: same.clone(),
            focus: Focus::FeaturedCard(0)
        }]
    );
    assert_eq!(
        ui.handle(Action::Back, &data),
        [Command::Restore(Page::Detail)]
    );
    assert_eq!(ui.focus(), Focus::FeaturedCard(0));
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::DetailTab(0));
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(
        ui.handle(Action::Select, &data),
        [Command::ActivateCard {
            target: same.clone(),
            focus: Focus::Card { row: 0, column: 0 }
        }]
    );
    assert_eq!(ui.page(), Page::Detail);
    assert_eq!(
        ui.handle(Action::Back, &data),
        [Command::Restore(Page::Home)],
        "Episode Play adds no UI history"
    );
}

#[test]
fn same_id_action_change_and_removed_feature_cancel_a_pressed_pointer() {
    let root = Target::Native(MediaId::new("Root0001").unwrap());
    let same = Target::Native(MediaId::new("Shared01").unwrap());
    let open_card = [card(&same, CardAction::Open)];
    let play_card = [card(&same, CardAction::Play)];
    let data = |cards| ViewData {
        detail: Some(detail(&root, None, cards)),
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    open(&mut ui, &root);
    ui.handle(Action::Down, &data(&open_card));
    ui.handle(Action::Down, &data(&open_card));
    let pos = frame(&mut ui, &data(&open_card), vec![]).visible_cards[0]
        .image
        .center();
    assert!(
        frame(&mut ui, &data(&open_card), vec![pointer(pos, true)])
            .commands
            .is_empty()
    );
    assert!(
        frame(&mut ui, &data(&play_card), vec![pointer(pos, false)])
            .commands
            .is_empty()
    );
    assert_eq!(ui.page(), Page::Detail);
    assert!(
        frame(&mut ui, &data(&open_card), vec![pointer(pos, true)])
            .commands
            .is_empty()
    );
    let mut removed = data(&open_card);
    removed.detail.as_mut().unwrap().featured = None;
    assert!(
        frame(&mut ui, &removed, vec![pointer(pos, false)])
            .commands
            .is_empty()
    );
    assert_eq!(ui.focus(), Focus::DetailDescription);
    assert_eq!(ui.scroll_y(), 0.0);
}

#[test]
fn present_empty_feature_does_not_fabricate_heading_tab_or_card_focus() {
    let root = Target::Native(MediaId::new("Root0001").unwrap());
    let mut ui = AppUi::new();
    open(&mut ui, &root);
    let data = ViewData {
        detail: Some(detail(&root, None, &[])),
        status: LoadState::Ready,
        ..Default::default()
    };
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::DetailDescription);
    let painted = frame(&mut ui, &data, vec![]);
    assert!(painted.visible_cards.is_empty());
    assert!(!painted.output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Featured")));
    assert!(ui.handle(Action::Select, &data).is_empty());
}

#[test]
fn unsupported_feature_card_refuses_remote_and_pointer_activation() {
    let root = Target::Native(MediaId::new("Root0001").unwrap());
    let live = Target::Native(MediaId::new("Live0001").unwrap());
    let cards = [card(&live, CardAction::Unsupported)];
    let data = ViewData {
        detail: Some(detail(&root, None, &cards)),
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    open(&mut ui, &root);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    assert!(ui.handle(Action::Select, &data).is_empty());
    let pos = frame(&mut ui, &data, vec![]).visible_cards[0]
        .image
        .center();
    assert!(
        frame(
            &mut ui,
            &data,
            vec![pointer(pos, true), pointer(pos, false)]
        )
        .commands
        .is_empty()
    );
    assert_eq!(ui.page(), Page::Detail);
    assert_eq!(
        ui.handle(Action::Back, &data),
        [Command::Restore(Page::Home)]
    );
}
