use criterion_provider::MediaId;
use criterion_ui::{
    Action, AppUi, Card, CardAction, Command, Detail, DetailKind, Focus, Hero, HeroAction,
    ListMembership, LoadState, LoginView, Page, Target, UiFrame, ViewData,
};

fn data<'a>(target: &'a Target, membership: ListMembership, login: LoginView<'a>) -> ViewData<'a> {
    let card = Card {
        key: target,
        artwork_key: None,
        title: "Membership fixture",
        year: "",
        duration_label: None,
        saved_fraction: None,
        action: CardAction::Open,
    };
    ViewData {
        hero: Some(Hero {
            card,
            description: "",
            action: "SEE MORE",
            action_kind: HeroAction::Open,
            background_key: None,
            title_logo_key: None,
        }),
        detail: Some(Detail {
            card,
            header_metadata: "1986   1h 37m",
            information_metadata: "1986   1h 37m",
            directors: "Fixture director",
            description: "Fixture description",
            starring: None,
            countries: None,
            languages: None,
            content_warnings: None,
            primary_action: "WATCH NOW",
            primary_playback_target: target.media_id(),
            selected_playlist: None,
            featured: None,
            seasons: None,
            kind: DetailKind::Film,
            sort: None,
            membership,
        }),
        status: LoadState::Ready,
        login,
        ..Default::default()
    }
}

fn open_list_control(ui: &mut AppUi, data: &ViewData<'_>, target: &Target) {
    assert_eq!(
        ui.handle(Action::Select, data),
        [Command::Open(target.clone())]
    );
    assert_eq!(ui.page(), Page::Detail);
    ui.handle(Action::Right, data);
    ui.handle(Action::Right, data);
    assert_eq!(ui.focus(), Focus::DetailAction(2));
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

fn click(ui: &mut AppUi, data: &ViewData<'_>, pos: egui::Pos2) -> Vec<Command> {
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(pos, true), pointer(pos, false)],
            ..Default::default()
        },
        data,
    );
    frame.output.textures_delta.clear();
    frame.commands
}

#[test]
fn signed_in_membership_is_read_only_for_remote_and_pointer() {
    let target = Target::Native(MediaId::new("Film0001").unwrap());
    for login in [LoginView::SignedIn, LoginView::SigningOut] {
        for membership in [
            ListMembership::SignedOut,
            ListMembership::Pending,
            ListMembership::Known { present: true },
            ListMembership::Known { present: false },
            ListMembership::Unavailable,
        ] {
            let data = data(&target, membership, login);
            let mut remote = AppUi::new();
            open_list_control(&mut remote, &data, &target);
            let remote_commands = remote.handle(Action::Select, &data);
            let mut pointer = AppUi::new();
            open_list_control(&mut pointer, &data, &target);
            let pointer_commands = click(&mut pointer, &data, egui::pos2(773.0, 661.0));
            assert_eq!(
                (remote_commands, pointer_commands),
                (vec![], vec![]),
                "{membership:?}"
            );
        }
    }
}

#[test]
fn live_refuses_list_action_even_when_signed_out() {
    let target = Target::Native(MediaId::new("Live0001").unwrap());
    let mut data = data(&target, ListMembership::SignedOut, LoginView::SignedOut);
    data.detail.as_mut().unwrap().kind = DetailKind::Live;
    let mut remote = AppUi::new();
    open_list_control(&mut remote, &data, &target);
    let remote_commands = remote.handle(Action::Select, &data);
    let mut pointer = AppUi::new();
    open_list_control(&mut pointer, &data, &target);
    let pointer_commands = click(&mut pointer, &data, egui::pos2(773.0, 661.0));
    assert_eq!((remote_commands, pointer_commands), (vec![], vec![]));
}

#[test]
fn signed_out_ordinary_detail_keeps_exact_activation_command() {
    let id = MediaId::new("Film0001").unwrap();
    for target in [Target::Native(id.clone()), Target::Media(id.clone())] {
        for kind in [
            DetailKind::Film,
            DetailKind::Supplement,
            DetailKind::Series,
            DetailKind::Collection,
        ] {
            let mut data = data(&target, ListMembership::SignedOut, LoginView::SignedOut);
            let detail = data.detail.as_mut().unwrap();
            detail.kind = kind;
            let list_x = if kind == DetailKind::Collection {
                detail.primary_playback_target = None;
                293.0
            } else {
                773.0
            };
            let mut remote = AppUi::new();
            open_list_control(&mut remote, &data, &target);
            assert_eq!(
                remote.handle(Action::Select, &data),
                [Command::ToggleList(id.clone())]
            );
            let mut pointer = AppUi::new();
            open_list_control(&mut pointer, &data, &target);
            assert_eq!(
                click(&mut pointer, &data, egui::pos2(list_x, 661.0)),
                [Command::ToggleList(id.clone())]
            );
        }
    }
}

fn caption<'a>(frame: &'a UiFrame, expected: &str, left: f32) -> &'a egui::epaint::ClippedShape {
    let shape = frame.output.shapes.iter().find(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == expected)
    }).expect("membership caption must be painted");
    let bounds = shape.shape.visual_bounding_rect();
    assert!(
        egui::Rect::from_min_size(egui::pos2(left, 716.0), egui::vec2(340.0, 44.0))
            .contains_rect(bounds)
    );
    assert!(shape.clip_rect.contains_rect(bounds));
    shape
}

#[test]
fn pending_membership_paints_bounded_caption_and_ascii_checking_mark() {
    let target = Target::Native(MediaId::new("Film0001").unwrap());
    let data = data(&target, ListMembership::Pending, LoginView::SignedIn);
    let mut ui = AppUi::new();
    open_list_control(&mut ui, &data, &target);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    caption(&frame, "CHECKING MY LIST", 732.0);
    assert!(frame.output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "...")
            && egui::Rect::from_min_size(egui::pos2(732.0, 620.0), egui::vec2(82.0, 82.0))
                .contains_rect(shape.shape.visual_bounding_rect())
    }));
}

#[test]
fn membership_states_paint_bounded_status_and_keep_original_action_geometry() {
    let target = Target::Native(MediaId::new("Film0001").unwrap());
    for primary in [true, false] {
        for (membership, expected, symbol) in [
            (ListMembership::SignedOut, "MY LIST", Some("+")),
            (ListMembership::Known { present: true }, "IN MY LIST", None),
            (
                ListMembership::Known { present: false },
                "NOT IN MY LIST",
                Some("+"),
            ),
            (
                ListMembership::Unavailable,
                "MY LIST UNAVAILABLE",
                Some("?"),
            ),
        ] {
            let login = if membership == ListMembership::SignedOut {
                LoginView::SignedOut
            } else {
                LoginView::SignedIn
            };
            let mut data = data(&target, membership, login);
            if !primary {
                data.detail.as_mut().unwrap().primary_playback_target = None;
            }
            let mut ui = AppUi::new();
            open_list_control(&mut ui, &data, &target);
            let mut frame = ui.render(egui::RawInput::default(), &data);
            frame.output.textures_delta.clear();
            let left = if primary { 732.0 } else { 252.0 };
            caption(&frame, expected, left);
            let control =
                egui::Rect::from_min_size(egui::pos2(left, 620.0), egui::vec2(82.0, 82.0));
            assert!(
                frame.output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect == control)
                }),
                "82px list control is preserved"
            );
            if let Some(symbol) = symbol {
                assert!(frame.output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == symbol)
                        && control.contains_rect(shape.shape.visual_bounding_rect())
                }));
            } else {
                let mark = egui::Rect::from_center_size(control.center(), egui::vec2(40.0, 40.0));
                assert!(
                    frame.output.shapes.iter().any(|shape| {
                        matches!(&shape.shape, egui::Shape::Path(path) if path.points.len() == 3)
                            && mark.contains_rect(shape.shape.visual_bounding_rect())
                            && shape
                                .clip_rect
                                .contains_rect(shape.shape.visual_bounding_rect())
                    }),
                    "present membership uses a bounded vector check"
                );
                assert!(!frame.output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Text(text) if ["+", "✓", "✔"].contains(&text.galley.job.text.as_str()))
                        && control.intersects(shape.shape.visual_bounding_rect())
                }));
            }
            assert!(frame.output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "1986   1h 37m" && text.pos == egui::pos2(169.0, 563.0))
            }), "original header metadata remains in place");
            assert_eq!(ui.focus(), Focus::DetailAction(2));
            assert!(click(&mut ui, &data, egui::pos2(left + 10.0, 730.0)).is_empty());
            if membership == ListMembership::SignedOut {
                for pos in [
                    egui::pos2(left - 1.0, 661.0),
                    egui::pos2(left + 83.0, 661.0),
                    egui::pos2(left + 10.0, 703.0),
                ] {
                    assert!(
                        click(&mut ui, &data, pos).is_empty(),
                        "outside original list hit bounds"
                    );
                }
            }
        }
    }
}

#[test]
fn activation_restores_detail_origin_without_replaying_list_action() {
    let target = Target::Native(MediaId::new("Film0001").unwrap());
    let unsigned = data(&target, ListMembership::SignedOut, LoginView::SignedOut);
    let mut ui = AppUi::new();
    open_list_control(&mut ui, &unsigned, &target);
    assert_eq!(
        ui.handle(Action::Select, &unsigned),
        [Command::ToggleList(target.media_id().unwrap().clone())]
    );
    assert_eq!(ui.begin_authentication(), [Command::Authenticate]);
    let signed = data(
        &target,
        ListMembership::Known { present: false },
        LoginView::SignedIn,
    );
    let mut frame = ui.render(egui::RawInput::default(), &signed);
    frame.output.textures_delta.clear();
    assert!(frame.commands.is_empty());
    assert_eq!(
        ui.handle(Action::Back, &signed),
        [Command::Restore(Page::Detail)]
    );
    assert_eq!(ui.focus(), Focus::DetailAction(2));
    assert!(ui.handle(Action::Select, &signed).is_empty());
}
