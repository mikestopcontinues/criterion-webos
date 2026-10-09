use criterion_ui::{Action, AppUi, Command, Focus, LoginView, Page, ViewData};

#[test]
fn deferred_play_authentication_cancels_and_restores_prior_focus() {
    let mut ui = AppUi::new();
    let data = ViewData::default();
    assert_eq!(ui.begin_authentication(), vec![Command::Authenticate]);
    assert_eq!(ui.page(), Page::Login);
    let awaiting = ViewData {
        login: LoginView::Awaiting {
            generation: 1,
            user_code: "TEST-ONLY",
            verification_uri_complete: "https://login.criterion.com/activate?test=fixture",
            remaining_seconds: 120,
        },
        ..data
    };
    assert_eq!(ui.handle(Action::Down, &awaiting), vec![]);
    assert_eq!(ui.focus(), Focus::LoginCancel);
    assert_eq!(
        ui.handle(Action::Back, &awaiting),
        vec![Command::CancelAuthentication, Command::Restore(Page::Home)]
    );
    assert_eq!(ui.focus(), Focus::Hero);
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
fn activation_frame_shows_private_instructions_without_catalog_loading_state() {
    let mut ui = AppUi::new();
    ui.begin_authentication();
    let data = ViewData {
        login: LoginView::Awaiting {
            generation: 2,
            user_code: "TEST-ONLY",
            verification_uri_complete: "https://login.criterion.com/activate?test=fixture",
            remaining_seconds: 120,
        },
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &data);
    let text = painted_text(&frame.output.shapes);
    frame.output.textures_delta.clear();
    assert!(text.contains("TEST-ONLY"));
    assert!(text.contains("login.criterion.com/activate"));
    assert!(text.contains("Expires in 2:00"));
    assert!(!text.contains("Loading…"));
    assert_eq!(ui.focus(), Focus::LoginCancel);
    assert!(!frame.wants_text_input);
    assert!(frame.visible_artwork.is_empty());
}

fn click_event(pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(400.0, 880.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}
#[test]
fn activation_cancel_button_accepts_pointer_and_restores_page() {
    let mut ui = AppUi::new();
    ui.begin_authentication();
    let data = ViewData {
        login: LoginView::Requesting,
        ..Default::default()
    };
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click_event(true), click_event(false)],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert_eq!(
        frame.commands,
        vec![Command::CancelAuthentication, Command::Restore(Page::Home)]
    );
    assert_eq!(ui.page(), Page::Home);
}

#[test]
fn remote_action_cancels_an_in_progress_activation_pointer_press() {
    let mut ui = AppUi::new();
    ui.begin_authentication();
    let data = ViewData {
        login: LoginView::Requesting,
        ..Default::default()
    };
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click_event(true)],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    ui.handle(Action::Down, &data);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click_event(false)],
            ..Default::default()
        },
        &data,
    );
    frame.output.textures_delta.clear();
    assert!(frame.commands.is_empty());
    assert_eq!(ui.page(), Page::Login);
}

#[test]
fn leaving_pending_activation_through_rail_cancels_only_linking() {
    let mut ui = AppUi::new();
    ui.begin_authentication();
    let data = ViewData {
        login: LoginView::Requesting,
        ..Default::default()
    };
    ui.handle(Action::Left, &data);
    ui.handle(Action::Up, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![
            Command::CancelAuthentication,
            Command::Navigate(Page::AllFilms)
        ]
    );
}

#[test]
fn expired_activation_removes_private_display_and_offers_retry() {
    let mut ui = AppUi::new();
    ui.begin_authentication();
    let awaiting = ViewData {
        login: LoginView::Awaiting {
            generation: 3,
            user_code: "TEST-ONLY",
            verification_uri_complete: "https://login.criterion.com/activate?test=fixture",
            remaining_seconds: 1,
        },
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &awaiting);
    frame.output.textures_delta.clear();
    let qr = frame
        .output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Mesh(mesh) = &shape.shape {
                Some(mesh)
            } else {
                None
            }
        })
        .expect("activation has a vector QR");
    assert!(qr.vertices.len() > 100);
    assert!(qr.vertices.len() <= 64_000);
    assert!(qr.vertices.iter().all(|vertex| {
        vertex.pos.x > 1225.0
            && vertex.pos.x < 1745.0
            && vertex.pos.y > 260.0
            && vertex.pos.y < 780.0
    }));
    let expired = ViewData {
        login: LoginView::Expired,
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &expired);
    frame.output.textures_delta.clear();
    let text = painted_text(&frame.output.shapes);
    assert!(!text.contains("TEST-ONLY"));
    assert!(!text.contains("login.criterion.com/activate"));
    assert!(
        !frame
            .output
            .shapes
            .iter()
            .any(|shape| matches!(shape.shape, egui::Shape::Mesh(_)))
    );
    assert_eq!(ui.focus(), Focus::LoginPrimary);
    assert_eq!(
        ui.handle(Action::Select, &expired),
        vec![Command::RetryAuthentication]
    );
}
#[test]
fn account_back_preserves_session_and_issued_logout() {
    for login in [LoginView::SignedIn, LoginView::SigningOut] {
        let mut ui = AppUi::new();
        ui.begin_authentication();
        let data = ViewData {
            login,
            ..Default::default()
        };
        assert_eq!(
            ui.handle(Action::Back, &data),
            vec![Command::Restore(Page::Home)]
        );
    }
    let mut ui = AppUi::new();
    ui.begin_authentication();
    let data = ViewData {
        login: LoginView::SignedIn,
        ..Default::default()
    };
    assert_eq!(ui.handle(Action::Select, &data), vec![Command::Logout]);
    for login in [LoginView::Denied, LoginView::Error] {
        let data = ViewData {
            login,
            ..Default::default()
        };
        assert_eq!(
            ui.handle(Action::Select, &data),
            vec![Command::RetryAuthentication]
        );
    }
}
#[test]
fn replacement_challenge_cancels_the_old_pointer_press() {
    let mut ui = AppUi::new();
    ui.begin_authentication();
    let challenge = |generation| ViewData {
        login: LoginView::Awaiting {
            generation,
            user_code: "TEST-ONLY",
            verification_uri_complete: "https://login.criterion.com/activate?test=fixture",
            remaining_seconds: 30,
        },
        ..Default::default()
    };
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click_event(true)],
            ..Default::default()
        },
        &challenge(4),
    );
    frame.output.textures_delta.clear();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click_event(false)],
            ..Default::default()
        },
        &challenge(5),
    );
    frame.output.textures_delta.clear();
    assert!(frame.commands.is_empty());
    assert_eq!(ui.page(), Page::Login);
}

#[test]
fn signed_in_rail_opens_account_without_starting_a_new_activation() {
    let mut ui = AppUi::new();
    let data = ViewData {
        login: LoginView::SignedIn,
        ..Default::default()
    };
    ui.handle(Action::Left, &data);
    for _ in 0..3 {
        ui.handle(Action::Down, &data);
    }
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(painted_text(&frame.output.shapes).contains("ACCOUNT"));
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::Navigate(Page::Login)]
    );
    assert_eq!(ui.focus(), Focus::LoginPrimary);
}

#[test]
fn longest_admitted_code_remains_complete_and_inside_its_display_area() {
    let mut ui = AppUi::new();
    ui.begin_authentication();
    let code = "W".repeat(128);
    let data = ViewData {
        login: LoginView::Awaiting {
            generation: 7,
            user_code: &code,
            verification_uri_complete: "https://login.criterion.com/activate?test=fixture",
            remaining_seconds: 60,
        },
        ..Default::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    let text = frame
        .output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text == code
            {
                Some(text)
            } else {
                None
            }
        })
        .expect("admitted code is painted");
    assert!(!text.galley.elided);
    assert!(text.pos.x + text.galley.rect.right() <= 1160.0);
    assert!(text.pos.y + text.galley.rect.bottom() < 735.0);
}

#[test]
fn pending_activation_and_logout_reentry_do_not_request_another_link() {
    for login in [
        LoginView::Requesting,
        LoginView::SigningOut,
        LoginView::Awaiting {
            generation: 8,
            user_code: "TEST-ONLY",
            verification_uri_complete: "https://login.criterion.com/activate?test=fixture",
            remaining_seconds: 30,
        },
    ] {
        let mut ui = AppUi::new();
        ui.begin_authentication();
        let data = ViewData {
            login,
            ..Default::default()
        };
        ui.handle(Action::Left, &data);
        assert_eq!(
            ui.handle(Action::Select, &data),
            vec![Command::Navigate(Page::Login)]
        );
    }
}

#[test]
fn authentication_from_information_modal_owns_input_and_restores_the_modal() {
    let mut ui = AppUi::new();
    let target = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let card = criterion_ui::Card {
        key: &target,
        artwork_key: None,
        title: "Fixture",
        year: "1986",
        duration_seconds: 5820,
    };
    let detail = || criterion_ui::Detail {
        card,
        directors: "Fixture director",
        description: "MODAL-ONLY-SYNOPSIS",
        starring: "Fixture cast",
        countries: "Fixture country",
        languages: "English",
        primary_action: "WATCH NOW",
        kind: criterion_ui::DetailKind::Film,
    };
    let data = ViewData {
        hero: Some(criterion_ui::Hero {
            card,
            description: "Fixture",
            action: "SEE MORE",
            action_kind: criterion_ui::HeroAction::Open,
            background_key: None,
            title_logo_key: None,
        }),
        detail: Some(detail()),
        ..Default::default()
    };
    ui.handle(Action::Select, &data);
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.focus(), Focus::InformationPrimary);
    assert_eq!(
        ui.handle(Action::Select, &data),
        vec![Command::Play(target.media_id().unwrap().clone())]
    );
    ui.begin_authentication();
    let login = ViewData {
        login: LoginView::Requesting,
        detail: Some(detail()),
        ..Default::default()
    };
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click_event(true), click_event(false)],
            ..Default::default()
        },
        &login,
    );
    frame.output.textures_delta.clear();
    assert!(!painted_text(&frame.output.shapes).contains("MODAL-ONLY-SYNOPSIS"));
    assert_eq!(
        frame.commands,
        vec![
            Command::CancelAuthentication,
            Command::Restore(Page::Detail)
        ]
    );
    assert_eq!(ui.focus(), Focus::InformationPrimary);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert!(painted_text(&frame.output.shapes).contains("MODAL-ONLY-SYNOPSIS"));
}

#[test]
fn activation_can_leave_and_restore_an_open_filter_without_interception() {
    let mut ui = AppUi::new();
    let data = ViewData::default();
    ui.handle(Action::Left, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Down, &data);
    ui.handle(Action::Select, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.focus(), Focus::FilterGroup(0));
    ui.begin_authentication();
    let login = ViewData {
        login: LoginView::Requesting,
        ..Default::default()
    };
    ui.handle(Action::Left, &login);
    ui.handle(Action::Up, &login);
    assert_eq!(ui.focus(), Focus::Rail(criterion_ui::RailItem::AllFilms));
    ui.handle(Action::Down, &login);
    ui.handle(Action::Right, &login);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![click_event(true), click_event(false)],
            ..Default::default()
        },
        &login,
    );
    frame.output.textures_delta.clear();
    assert_eq!(
        frame.commands,
        vec![
            Command::CancelAuthentication,
            Command::Restore(Page::AllFilms)
        ]
    );
    assert_eq!(ui.focus(), Focus::FilterGroup(0));
}
