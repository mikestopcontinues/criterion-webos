use criterion_ui::{
    Action, AppUi, Card, Command, Focus, LoadState, LoginView, Page, Rail, RailAction,
    RailActionCursor, Target, UiFrame, ViewData,
};
use egui::{Pos2, Rect, Vec2};

fn content(path: &str) -> Target {
    Target::Content(criterion_provider::ContentTarget::parse(path).unwrap())
}

fn film<'a>(target: &'a Target, title: &'a str, year: &'a str, runtime: &'a str) -> Card<'a> {
    Card {
        key: target,
        title,
        year,
        duration_label: Some(runtime),
        artwork_key: None,
        saved_fraction: None,
        action: Default::default(),
    }
}

fn data<'a>(rails: &'a [Rail<'a>]) -> ViewData<'a> {
    ViewData {
        rails,
        discovery_visit: Some(52),
        status: LoadState::Ready,
        ..Default::default()
    }
}

fn enter(ui: &mut AppUi, page: Page) {
    match page {
        Page::Home => (),
        Page::New => ui.commit_discovery_target(&content("/new"), LoginView::SignedOut),
        Page::Discovery => {
            ui.commit_discovery_target(&content("/discover/leaving-soon"), LoginView::SignedOut)
        }
        _ => unreachable!("this fixture exercises public rail pages"),
    }
    assert_eq!(ui.page(), page);
}

fn render(ui: &mut AppUi, data: &ViewData<'_>, events: Vec<egui::Event>) -> UiFrame {
    let mut frame = ui.render(
        egui::RawInput {
            events,
            ..Default::default()
        },
        data,
    );
    frame.output.textures_delta.clear();
    frame
}

fn text_bounds(frame: &UiFrame, label: &str) -> Rect {
    let bounds: Vec<_> = frame
        .output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text == label
            {
                Some(shape.shape.visual_bounding_rect())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(bounds.len(), 1, "{label} must paint once and completely");
    bounds[0]
}

fn button_bounds(frame: &UiFrame, label: &str) -> Rect {
    let text = text_bounds(frame, label);
    frame
        .output
        .shapes
        .iter()
        .filter(|shape| !matches!(shape.shape, egui::Shape::Text(_)))
        .map(|shape| shape.shape.visual_bounding_rect())
        .filter(|bounds| bounds.contains_rect(text))
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the painted CTA must contain its text")
}

fn image_bounds(frame: &UiFrame, focus: Focus) -> Rect {
    let image = frame
        .visible_cards
        .iter()
        .find(|card| card.focus == focus)
        .expect("the requested card must be visible")
        .image;
    assert!(
        frame
            .output
            .shapes
            .iter()
            .any(|shape| shape.shape.visual_bounding_rect() == image),
        "the reported card image must have a painted shape",
    );
    image
}

fn first_row_frame(page: Page) -> UiFrame {
    let first = Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let later = Target::Media(criterion_provider::MediaId::new("zxlDvz82").unwrap());
    let destination = content("/discover/newly-added");
    let first_cards = [film(
        &first,
        "The Discreet Charm of the Bourgeoisie",
        "1972",
        "596523 h 14 min",
    )];
    let later_cards = [film(
        &later,
        "The Night of the Hunter",
        "1955",
        "1 h 33 min",
    )];
    let rails = [
        Rail {
            title: "Earlier films",
            cards: &first_cards,
            action: Some(RailAction {
                block: 101,
                label: "Browse earlier films",
                target: &destination,
            }),
        },
        Rail {
            title: "Later films",
            cards: &later_cards,
            action: Some(RailAction {
                block: 202,
                label: "Browse later films",
                target: &destination,
            }),
        },
    ];
    let data = data(&rails);
    let mut ui = AppUi::new();
    enter(&mut ui, page);
    ui.handle(Action::Down, &data);
    render(&mut ui, &data, vec![])
}

fn assert_complete_card_precedes_next_header(frame: &UiFrame) {
    let heading = text_bounds(frame, "Later films");
    let action = button_bounds(frame, "Browse later films");
    let parts = [
        (
            "image",
            image_bounds(frame, Focus::Card { row: 0, column: 0 }),
        ),
        ("title", text_bounds(frame, "The Discreet Charm of the B…")),
        ("year", text_bounds(frame, "1972")),
        ("runtime", text_bounds(frame, "596523 h 14 min")),
    ];
    for (name, bounds) in parts {
        assert!(
            bounds.bottom() <= heading.top().min(action.top()),
            "the card {name} {bounds:?} must finish before the next heading {heading:?} and action {action:?}",
        );
        assert!(
            !bounds.intersects(heading),
            "the card {name} overlaps the next heading"
        );
        assert!(
            !bounds.intersects(action),
            "the card {name} overlaps the next action"
        );
    }
}

#[test]
fn new_card_image_title_year_and_runtime_finish_before_the_next_rail() {
    assert_complete_card_precedes_next_header(&first_row_frame(Page::New));
}

#[test]
fn home_and_discovery_card_metadata_also_finish_before_the_next_rail() {
    for page in [Page::Home, Page::Discovery] {
        assert_complete_card_precedes_next_header(&first_row_frame(page));
    }
}

fn assert_selected_card_visible(frame: &UiFrame) -> Rect {
    let viewport = Rect::from_min_size(
        Pos2::ZERO,
        Vec2::new(criterion_ui::LOGICAL_SIZE[0], criterion_ui::LOGICAL_SIZE[1]),
    );
    let image = image_bounds(frame, Focus::Card { row: 1, column: 3 });
    for bounds in [
        image,
        text_bounds(frame, "Selected D"),
        text_bounds(frame, "1963"),
        text_bounds(frame, "1 h 14 min"),
    ] {
        assert!(
            viewport.contains_rect(bounds),
            "the focused card must remain fully visible"
        );
    }
    image
}

#[test]
fn remote_and_painted_pointer_cta_restore_the_same_visible_row_and_card_column() {
    let media = Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let destination = content("/discover/newly-added");
    let first_cards = [
        film(&media, "Origin A", "1940", "1 h 1 min"),
        film(&media, "Origin B", "1941", "1 h 2 min"),
        film(&media, "Origin C", "1942", "1 h 3 min"),
        film(&media, "Origin D", "1943", "1 h 4 min"),
    ];
    let later_cards = [
        film(&media, "Selected A", "1960", "1 h 11 min"),
        film(&media, "Selected B", "1961", "1 h 12 min"),
        film(&media, "Selected C", "1962", "1 h 13 min"),
        film(&media, "Selected D", "1963", "1 h 14 min"),
    ];
    let rails = [
        Rail {
            title: "Earlier films",
            cards: &first_cards,
            action: Some(RailAction {
                block: 101,
                label: "Browse earlier films",
                target: &destination,
            }),
        },
        Rail {
            title: "Later films",
            cards: &later_cards,
            action: Some(RailAction {
                block: 202,
                label: "Browse later films",
                target: &destination,
            }),
        },
    ];
    let data = data(&rails);
    for origin in [Page::Home, Page::New, Page::Discovery] {
        let mut ui = AppUi::new();
        enter(&mut ui, origin);
        ui.handle(Action::Down, &data);
        for _ in 0..3 {
            ui.handle(Action::Right, &data);
        }
        ui.handle(Action::Down, &data);
        assert_eq!(ui.focus(), Focus::Card { row: 1, column: 3 });
        let before = render(&mut ui, &data, vec![]);
        let image = assert_selected_card_visible(&before);
        let scroll = ui.scroll_y();
        ui.handle(Action::Up, &data);
        assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 1, column: 3 });
        assert_eq!(
            assert_selected_card_visible(&render(&mut ui, &data, vec![])),
            image
        );
        let expected = [Command::ActivateRail {
            origin,
            from: RailActionCursor {
                visit: 52,
                block: 202,
                row: 1,
            },
            target: destination.clone(),
        }];
        assert_eq!(ui.handle(Action::Select, &data), expected);
        assert_eq!(ui.page(), origin);
        ui.commit_discovery_target(&destination, data.login);
        assert_eq!(ui.handle(Action::Back, &data), [Command::Restore(origin)]);
        assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 1, column: 3 });
        assert_eq!(ui.scroll_y(), scroll);
        ui.handle(Action::Down, &data);
        assert_eq!(ui.focus(), Focus::Card { row: 1, column: 3 });
        let restored = render(&mut ui, &data, vec![]);
        assert_eq!(assert_selected_card_visible(&restored), image);

        let action = button_bounds(&restored, "Browse later films");
        let label = text_bounds(&restored, "Browse later films");
        assert!(action.contains(label.center()));
        let clicked = render(
            &mut ui,
            &data,
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::PointerButton {
                    pos: label.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                })
                .collect(),
        );
        assert_eq!(clicked.commands, expected);
        assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 1, column: 3 });
        ui.commit_discovery_target(&destination, data.login);
        assert_eq!(ui.handle(Action::Back, &data), [Command::Restore(origin)]);
        assert_eq!(ui.focus(), Focus::DiscoveryRailAction { row: 1, column: 3 });
        assert_eq!(ui.scroll_y(), scroll);
        ui.handle(Action::Down, &data);
        assert_eq!(ui.focus(), Focus::Card { row: 1, column: 3 });
        assert_eq!(
            assert_selected_card_visible(&render(&mut ui, &data, vec![])),
            image
        );
    }
}
