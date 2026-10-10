use criterion_provider::ContentTarget;
use criterion_ui::{Action, AppUi, Card, Command, Focus, LoginView, Page, Rail, Target, ViewData};

#[test]
fn subscriber_can_open_my_list_without_a_home_navigation_card() {
    for status in [
        criterion_ui::LoadState::Loading,
        criterion_ui::LoadState::Empty,
        criterion_ui::LoadState::Offline,
    ] {
        let mut ui = AppUi::new();
        let data = ViewData {
            login: LoginView::SignedIn,
            status,
            ..Default::default()
        };
        ui.handle(Action::Left, &data);
        ui.handle(Action::Down, &data);
        ui.handle(Action::Down, &data);
        assert_eq!(
            ui.handle(Action::Select, &data),
            vec![Command::Navigate(Page::MyList)]
        );
        assert_eq!(ui.page(), Page::MyList);
        assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    }
}

fn rail_pointer(y: f32, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(75.0, y),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

#[test]
fn subscriber_rail_paints_observed_labels_and_owns_their_pointer_positions() {
    let data = ViewData {
        login: LoginView::SignedIn,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    let expected = [
        ("SEARCH", 208.0),
        ("HOME", 356.0),
        ("NEW", 430.0),
        ("MY LIST", 504.0),
        ("ALL FILMS", 578.0),
        ("ACCOUNT", 726.0),
    ];
    for (label, y) in expected {
        let shape = frame
            .output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => Some(text),
                _ => None,
            })
            .expect("observed subscriber rail label must be painted");
        assert_eq!(shape.pos.x, 126.0);
        assert_eq!(shape.pos.y, y - 15.0);
    }
    for (y, page) in [
        (504.0, Page::MyList),
        (578.0, Page::AllFilms),
        (726.0, Page::Login),
    ] {
        let mut ui = AppUi::new();
        let mut frame = ui.render(
            egui::RawInput {
                events: vec![rail_pointer(y, true), rail_pointer(y, false)],
                ..Default::default()
            },
            &data,
        );
        frame.output.textures_delta.clear();
        assert_eq!(frame.commands, vec![Command::Navigate(page)]);
        assert_eq!(ui.page(), page);
    }
    let anonymous = ViewData::default();
    let mut ui = AppUi::new();
    ui.handle(Action::Left, &anonymous);
    let mut frame = ui.render(egui::RawInput::default(), &anonymous);
    frame.output.textures_delta.clear();
    assert!(!painted_text(&frame.output.shapes).contains("MY LIST"));
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![rail_pointer(504.0, true), rail_pointer(504.0, false)],
            ..Default::default()
        },
        &anonymous,
    );
    frame.output.textures_delta.clear();
    assert_eq!(frame.commands, vec![Command::Navigate(Page::AllFilms)]);
}

#[test]
fn session_departure_cancels_my_list_pointer_and_normalizes_hidden_rail_focus() {
    let signed = ViewData {
        login: LoginView::SignedIn,
        ..Default::default()
    };
    for login in [
        LoginView::SignedOut,
        LoginView::SigningOut,
        LoginView::Error,
    ] {
        let mut ui = AppUi::new();
        let mut frame = ui.render(
            egui::RawInput {
                events: vec![rail_pointer(504.0, true)],
                ..Default::default()
            },
            &signed,
        );
        frame.output.textures_delta.clear();
        assert_eq!(ui.focus(), Focus::Rail(criterion_ui::RailItem::MyList));
        let departed = ViewData {
            login,
            ..Default::default()
        };
        let mut frame = ui.render(
            egui::RawInput {
                events: vec![rail_pointer(504.0, false)],
                ..Default::default()
            },
            &departed,
        );
        frame.output.textures_delta.clear();
        assert!(
            frame.commands.is_empty(),
            "departed press must not activate All Films at the shifted position"
        );
        assert_eq!(ui.focus(), Focus::Rail(criterion_ui::RailItem::Login));
        assert!(!painted_text(&frame.output.shapes).contains("MY LIST"));
    }
}

#[test]
fn fixed_subscriber_rail_back_restores_the_originating_card_and_scroll() {
    let target = Target::Content(ContentTarget::AllFilms);
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Explore",
        year: "",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let rails = [Rail {
        title: "Explore",
        cards: &cards,
    }];
    let data = ViewData {
        login: LoginView::SignedIn,
        rails: &rails,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    let origin = ui.focus();
    let scroll = ui.scroll_y();
    for action in [Action::Left, Action::Down, Action::Down, Action::Select] {
        ui.handle(action, &data);
    }
    assert_eq!(ui.page(), Page::MyList);
    assert_eq!(
        ui.handle(Action::Back, &data),
        vec![Command::Restore(Page::Home)]
    );
    assert_eq!(ui.focus(), origin);
    assert_eq!(ui.scroll_y(), scroll);
}

#[test]
fn departed_subscriber_back_skips_the_list_and_preserves_public_focus() {
    let signed = ViewData {
        login: LoginView::SignedIn,
        ..Default::default()
    };
    for login in [
        LoginView::SignedOut,
        LoginView::SigningOut,
        LoginView::Error,
    ] {
        let mut ui = AppUi::new();
        let origin = ui.focus();
        for action in [
            Action::Left,
            Action::Down,
            Action::Down,
            Action::Select,
            Action::Left,
            Action::Down,
            Action::Down,
            Action::Select,
        ] {
            ui.handle(action, &signed);
        }
        assert_eq!(ui.page(), Page::Login);
        let departed = ViewData {
            login,
            ..Default::default()
        };
        assert_eq!(
            ui.handle(Action::Back, &departed),
            vec![Command::Restore(Page::Home)]
        );
        assert_eq!(ui.focus(), origin);
        assert_eq!(ui.page(), Page::Home);
    }
}

fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> String {
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(shape) => {
                text.push_str(&shape.galley.job.text);
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, text);
                }
            }
            _ => (),
        }
    }
    let mut text = String::new();
    for shape in shapes {
        collect(&shape.shape, &mut text);
    }
    text
}

#[test]
fn signed_out_my_list_shows_activation_and_cancel_restores_the_home_card_once() {
    let target = Target::Content(ContentTarget::MyList);
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "My List",
        year: "",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let rails = [Rail {
        title: "Explore",
        cards: &cards,
    }];
    let home = ViewData {
        rails: &rails,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &home);
    let origin = Focus::Card { row: 0, column: 0 };
    let origin_scroll = ui.scroll_y();
    assert_eq!(ui.focus(), origin);
    assert_eq!(
        ui.handle(Action::Select, &home),
        vec![Command::Open(target.clone())]
    );
    assert_eq!(ui.page(), Page::Login);

    // The runtime handles the exact Open target, then publishes activation progress.
    let requesting = ViewData {
        login: LoginView::Requesting,
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &requesting);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::LoginCancel);
    let text = painted_text(&frame.output.shapes);
    assert!(text.contains("Requesting your activation code"));
    assert!(text.contains("CANCEL"));

    let awaiting = ViewData {
        login: LoginView::Awaiting {
            generation: 1,
            user_code: "MYLIST-TEST-ONLY",
            verification_uri_complete: "https://login.criterion.com/activate?test=my-list",
            remaining_seconds: 120,
        },
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &awaiting);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::LoginCancel);
    let text = painted_text(&frame.output.shapes);
    assert!(text.contains("MYLIST-TEST-ONLY"));
    assert!(text.contains("login.criterion.com/activate"));
    assert!(text.contains("CANCEL"));
    assert!(frame.output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.len() > 100)
    }));
    assert_eq!(
        ui.handle(Action::Select, &awaiting),
        vec![Command::CancelAuthentication, Command::Restore(Page::Home)]
    );
    assert_eq!(ui.page(), Page::Home);
    assert_eq!(ui.focus(), origin);
    assert_eq!(ui.scroll_y(), origin_scroll);
    assert_eq!(ui.handle(Action::Back, &home), vec![Command::Exit]);
}

#[test]
fn signed_in_my_list_opens_the_grid_and_back_restores_its_origin() {
    let target = Target::Content(ContentTarget::MyList);
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "My List",
        year: "",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let rails = [Rail {
        title: "Explore",
        cards: &cards,
    }];
    let data = ViewData {
        login: LoginView::SignedIn,
        rails: &rails,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    let origin_scroll = ui.scroll_y();
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::Open(target.clone())]
    );
    assert_eq!(ui.page(), Page::MyList);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(
        ui.handle(Action::Back, &data),
        vec![Command::Restore(Page::Home)]
    );
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.scroll_y(), origin_scroll);
    assert_eq!(ui.handle(Action::Back, &data), vec![Command::Exit]);
}

#[test]
fn my_list_hero_requires_an_active_session_and_subscribe_stays_on_login() {
    let awaiting = LoginView::Awaiting {
        generation: 2,
        user_code: "TEST-ONLY",
        verification_uri_complete: "https://login.criterion.com/activate?test=my-list",
        remaining_seconds: 60,
    };
    for login in [
        LoginView::SignedOut,
        LoginView::Requesting,
        awaiting,
        LoginView::SigningOut,
        LoginView::Expired,
        LoginView::Denied,
        LoginView::Error,
        LoginView::SignedIn,
    ] {
        for content in [ContentTarget::MyList, ContentTarget::Subscribe] {
            let is_my_list = content == ContentTarget::MyList;
            let target = Target::Content(content);
            let data = ViewData {
                login,
                hero: Some(criterion_ui::Hero {
                    card: Card {
                        key: &target,
                        artwork_key: None,
                        title: "Explore",
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
                ..Default::default()
            };
            let mut ui = AppUi::new();
            assert_eq!(
                ui.handle(Action::Select, &data),
                vec![Command::Open(target)]
            );
            assert_eq!(
                ui.page(),
                if is_my_list && matches!(login, LoginView::SignedIn) {
                    Page::MyList
                } else {
                    Page::Login
                }
            );
        }
    }
}

#[test]
fn signed_out_my_list_pointer_activation_uses_the_same_login_route() {
    let target = Target::Content(ContentTarget::MyList);
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "My List",
        year: "",
        duration_seconds: 0,
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    }];
    let rails = [Rail {
        title: "Explore",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        ..Default::default()
    };
    let click = |pressed| egui::Event::PointerButton {
        pos: egui::pos2(250.0, 980.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let mut ui = AppUi::new();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click(true), click(false)],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert_eq!(frame.commands, vec![Command::Open(target)]);
    assert_eq!(ui.page(), Page::Login);
}
