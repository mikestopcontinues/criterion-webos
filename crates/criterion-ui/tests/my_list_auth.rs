use criterion_provider::ContentTarget;
use criterion_ui::{Action, AppUi, Card, Command, Focus, LoginView, Page, Rail, Target, ViewData};

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
