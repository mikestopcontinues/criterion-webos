use criterion_ui::{
    Action, AppUi, Card, Command, Focus, Hero, HeroAction, HeroCarousel, HeroDirection, LoadState,
    Page, Target, ViewData,
};

fn data(target: &Target) -> ViewData<'_> {
    ViewData {
        status: LoadState::Ready,
        hero: Some(Hero {
            card: Card {
                key: target,
                title: "Supplied title",
                year: "",
                duration_label: None,
                artwork_key: None,
                saved_fraction: None,
                action: Default::default(),
            },
            description: "Supplied description",
            action: "See more",
            action_kind: HeroAction::Open,
            background_key: None,
            title_logo_key: None,
        }),
        hero_carousel: Some(HeroCarousel {
            block: 493,
            index: 1,
            slide: 1599,
            total: 7,
            caption: "Slide 2 of 7",
            visit: Some(12),
        }),
        ..Default::default()
    }
}
#[test]
fn explicit_arrows_preserve_rail_and_prepare_activation_before_navigation() {
    let target = Target::Content(criterion_provider::ContentTarget::parse("/new").unwrap());
    let data = data(&target);
    let mut ui = AppUi::new();
    assert!(ui.handle(Action::Right, &data).is_empty());
    assert_eq!(ui.focus(), Focus::HeroPrevious);
    ui.handle(Action::Right, &data);
    let from = data.hero_carousel.unwrap().cursor().unwrap();
    assert_eq!(
        ui.handle(Action::Select, &data),
        [Command::MoveHero {
            page: Page::Home,
            from,
            direction: HeroDirection::Next
        }]
    );
    ui.handle(Action::Left, &data);
    ui.handle(Action::Left, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        [Command::ActivateHero {
            origin: Page::Home,
            from,
            target: target.clone()
        }]
    );
    assert_eq!(ui.page(), Page::Home);
    assert_eq!(ui.focus(), Focus::Hero);
    ui.handle(Action::Left, &data);
    assert!(matches!(ui.focus(), Focus::Rail(_)));
    ui.handle(Action::Right, &data);
    assert_eq!(ui.focus(), Focus::Hero);
    ui.commit_discovery_target(&target, data.login);
    assert_eq!(ui.page(), Page::New);
    assert_eq!(
        ui.handle(Action::Back, &data),
        [Command::Restore(Page::Home)]
    );
    assert_eq!(ui.focus(), Focus::Hero);
}
#[test]
fn painted_arrows_and_caption_fit_and_pointer_uses_the_same_raw_cursor() {
    let target = Target::Content(criterion_provider::ContentTarget::parse("/new").unwrap());
    let data = data(&target);
    let mut ui = AppUi::new();
    ui.handle(Action::Right, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    let text: Vec<_> = frame
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
        .collect();
    assert!(text.contains(&"Slide 2 of 7"));
    assert!(text.contains(&"Supplied title"));
    for x in [443.0, 561.0] {
        let area = egui::Rect::from_center_size(egui::pos2(x, 780.0), egui::vec2(40.0, 40.0));
        let icons: Vec<_> = frame
            .output
            .shapes
            .iter()
            .filter(|shape| {
                matches!(shape.shape, egui::Shape::Path(_))
                    && area.contains_rect(shape.shape.visual_bounding_rect())
            })
            .collect();
        assert_eq!(icons.len(), 1);
        assert!(
            icons[0]
                .clip_rect
                .contains_rect(icons[0].shape.visual_bounding_rect())
        );
        if let egui::Shape::Path(path) = &icons[0].shape {
            assert_eq!(
                path.stroke.color,
                egui::epaint::ColorMode::Solid(egui::Color32::from_rgb(239, 239, 239))
            );
        }
    }
    let mut frame = ui.render(
        egui::RawInput {
            events: [true, false]
                .into_iter()
                .map(|pressed| egui::Event::PointerButton {
                    pos: egui::pos2(561.0, 780.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                })
                .collect(),
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert_eq!(
        frame.commands,
        [Command::MoveHero {
            page: Page::Home,
            from: data.hero_carousel.unwrap().cursor().unwrap(),
            direction: HeroDirection::Next
        }]
    );
}

#[test]
fn loading_retires_an_unavailable_arrow_saved_as_the_rail_return_focus() {
    let target = Target::Content(criterion_provider::ContentTarget::parse("/new").unwrap());
    let mut data = data(&target);
    data.hero = None;
    let mut ui = AppUi::new();
    ui.handle(Action::Up, &data);
    assert_eq!(ui.focus(), Focus::HeroPrevious);
    ui.handle(Action::Left, &data);
    assert!(matches!(ui.focus(), Focus::Rail(_)));
    ui.handle(Action::Right, &ViewData::default());
    assert_eq!(ui.focus(), Focus::Hero);
}

#[test]
fn scrolled_controls_use_current_geometry_and_an_absent_visit_cannot_activate() {
    let target = Target::Content(criterion_provider::ContentTarget::parse("/new").unwrap());
    let cards = [Card {
        key: &target,
        title: "Visible card",
        year: "",
        duration_label: None,
        artwork_key: None,
        saved_fraction: None,
        action: Default::default(),
    }];
    let rails = [criterion_ui::Rail {
        action: None,
        title: "Supplied rail",
        cards: &cards,
    }];
    let mut data = data(&target);
    data.rails = &rails;
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    assert_eq!(ui.scroll_y(), 632.0);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    for x in [443.0, 561.0] {
        let area = egui::Rect::from_center_size(egui::pos2(x, 148.0), egui::vec2(40.0, 40.0));
        let icons: Vec<_> = frame
            .output
            .shapes
            .iter()
            .filter(|shape| {
                matches!(shape.shape, egui::Shape::Path(_))
                    && area.contains_rect(shape.shape.visual_bounding_rect())
            })
            .collect();
        assert_eq!(icons.len(), 1);
        assert!(
            icons[0]
                .clip_rect
                .contains_rect(icons[0].shape.visual_bounding_rect())
        );
    }
    let click = |y| egui::RawInput {
        events: [true, false]
            .into_iter()
            .map(|pressed| egui::Event::PointerButton {
                pos: egui::pos2(561.0, y),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            })
            .collect(),
        ..Default::default()
    };
    let mut old = ui.render(click(780.0), &data);
    old.output.textures_delta.clear();
    assert!(
        !old.commands
            .iter()
            .any(|command| matches!(command, Command::MoveHero { .. }))
    );
    let mut current = ui.render(click(148.0), &data);
    current.output.textures_delta.clear();
    assert_eq!(
        current.commands,
        [Command::MoveHero {
            page: Page::Home,
            from: data.hero_carousel.unwrap().cursor().unwrap(),
            direction: HeroDirection::Next
        }]
    );
    data.hero_carousel.as_mut().unwrap().visit = None;
    assert!(ui.handle(Action::Select, &data).is_empty());
    let mut refused = ui.render(click(148.0), &data);
    refused.output.textures_delta.clear();
    assert!(refused.commands.is_empty());
}
