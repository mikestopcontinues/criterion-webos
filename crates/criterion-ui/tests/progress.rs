use criterion_provider::MediaId;
use criterion_ui::{
    Action, AppUi, Card, Command, Focus, LoadState, Page, Rail, Target, UiFrame, ViewData,
};

fn enter_grid(ui: &mut AppUi, data: &ViewData<'_>) {
    for action in [Action::Left, Action::Down, Action::Down, Action::Select] {
        ui.handle(action, data);
    }
}

fn grid_frame(saved_fraction: Option<f32>) -> UiFrame {
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Saved fixture",
        year: "2020",
        duration_seconds: 7200,
        saved_fraction,
        action: criterion_ui::CardAction::Open,
    }];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    frame
}

fn bars(frame: &UiFrame) -> Vec<[f32; 4]> {
    fn collect(shape: &egui::Shape, out: &mut Vec<[f32; 4]>) {
        match shape {
            egui::Shape::Rect(shape) if shape.rect.height() == 3.0 && shape.rect.width() > 0.0 => {
                out.push([
                    shape.rect.left(),
                    shape.rect.top(),
                    shape.rect.width(),
                    shape.rect.height(),
                ]);
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, out);
                }
            }
            _ => (),
        }
    }
    let mut out = Vec::new();
    for shape in &frame.output.shapes {
        collect(&shape.shape, &mut out);
    }
    out
}

#[test]
fn saved_quarter_draws_a_full_track_and_quarter_fill_at_the_thumbnail_bottom() {
    let frame = grid_frame(Some(0.25));
    assert_eq!(
        bars(&frame),
        vec![[150.0, 458.0, 378.0, 3.0], [150.0, 458.0, 94.5, 3.0]]
    );
    assert_eq!(frame.visible_cards.len(), 1);
}

#[test]
fn progress_uses_the_existing_white_palette_and_source_supported_track_alpha() {
    let frame = grid_frame(Some(0.25));
    let paints: Vec<_> = frame
        .output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Rect(rect) = &shape.shape
                && rect.rect.height() == 3.0
                && rect.rect.width() > 0.0
            {
                Some((rect.rect.width(), rect.fill))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(paints.len(), 2);
    assert_eq!(paints[0].0, 378.0);
    // Native Float32 bits 0x3ecccccd represent 0.4; 255 * 0.4 = 102.
    assert_eq!(paints[0].1.a(), 102);
    assert_eq!(paints[1], (94.5, egui::Color32::from_rgb(239, 239, 239)));
}

#[test]
fn absent_and_nonpositive_saved_progress_keep_the_card_without_a_track() {
    for saved in [None, Some(0.0), Some(-0.0), Some(-0.5)] {
        let frame = grid_frame(saved);
        assert!(bars(&frame).is_empty(), "unexpected bar for {saved:?}");
        assert_eq!(frame.visible_cards.len(), 1);
    }
}

#[test]
fn positive_progress_remains_visible_through_completion_and_clamps_to_the_card_width() {
    for fraction in [0.95, 0.98, 1.0, 1.25] {
        let frame = grid_frame(Some(fraction));
        let shapes = bars(&frame);
        assert_eq!(shapes.len(), 2, "missing progress for {fraction}");
        assert_eq!(shapes[0], [150.0, 458.0, 378.0, 3.0]);
        assert!((shapes[1][2] - 378.0 * fraction.min(1.0)).abs() < 0.001);
        assert_eq!(frame.visible_cards.len(), 1);
    }
}

#[test]
fn nonfinite_saved_progress_is_hidden_and_cannot_poison_painted_geometry() {
    for fraction in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        let frame = grid_frame(Some(fraction));
        assert!(bars(&frame).is_empty(), "unexpected bar for {fraction}");
        assert_eq!(frame.visible_cards.len(), 1);
        for shape in &frame.output.shapes {
            let bounds = shape.shape.visual_bounding_rect();
            assert!(bounds.min.x.is_finite() && bounds.min.y.is_finite());
            assert!(bounds.max.x.is_finite() && bounds.max.y.is_finite());
        }
    }
}

#[test]
fn shared_home_and_new_rail_cards_scale_the_same_saved_fraction() {
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Saved fixture",
        year: "2020",
        duration_seconds: 7200,
        saved_fraction: Some(0.5),
        action: criterion_ui::CardAction::Open,
    }];
    let rails = [Rail {
        title: "Supplied rail",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        ..Default::default()
    };
    for page in [Page::Home, Page::New] {
        let mut ui = AppUi::new();
        if page == Page::New {
            for action in [Action::Left, Action::Down, Action::Select] {
                ui.handle(action, &data);
            }
        }
        ui.handle(Action::Down, &data);
        let mut frame = ui.render(egui::RawInput::default(), &data);
        frame.output.textures_delta.clear();
        let image = frame.visible_cards[0].image;
        assert_eq!(ui.page(), page);
        assert_eq!(image.width(), if page == Page::New { 516.0 } else { 378.0 });
        assert_eq!(
            bars(&frame),
            vec![
                [image.left(), image.bottom() - 3.0, image.width(), 3.0],
                [image.left(), image.bottom() - 3.0, image.width() / 2.0, 3.0],
            ]
        );
        assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    }
}

#[test]
fn saved_updates_preserve_metadata_focus_and_remote_or_pointer_activation() {
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &ViewData::default());
    let mut baseline = None;
    for saved_fraction in [None, Some(0.25), Some(1.0)] {
        let cards = [Card {
            key: &target,
            artwork_key: None,
            title: "Saved fixture",
            year: "2020",
            duration_seconds: 7200,
            saved_fraction,
            action: criterion_ui::CardAction::Open,
        }];
        let data = ViewData {
            cards: &cards,
            status: LoadState::Ready,
            ..Default::default()
        };
        let mut frame = ui.render(egui::RawInput::default(), &data);
        frame.output.textures_delta.clear();
        if let Some(layout) = &baseline {
            assert_eq!(&frame.visible_cards, layout);
        } else {
            baseline = Some(frame.visible_cards.clone());
        }
        let text: String = frame
            .output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text.galley.text())
                } else {
                    None
                }
            })
            .collect();
        assert!(text.contains("Saved fixture"));
        assert!(text.contains("2020"));
        assert!(text.contains("2 h 0 min"));
        assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
        assert_eq!(
            ui.handle(Action::Select, &data),
            vec![Command::Open(target.clone())]
        );
        ui.handle(Action::Back, &data);
        assert_eq!(ui.page(), Page::AllFilms);
        assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });

        // The progress strip is inside the card's existing pointer hit target.
        let mut pointer_ui = AppUi::new();
        enter_grid(&mut pointer_ui, &data);
        let pos = egui::pos2(300.0, 459.0);
        let events = [true, false]
            .into_iter()
            .map(|pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            })
            .collect();
        let mut pointer_frame = pointer_ui.render(
            egui::RawInput {
                events,
                ..Default::default()
            },
            &data,
        );
        pointer_frame.output.textures_delta.clear();
        assert_eq!(pointer_frame.commands, vec![Command::Open(target.clone())]);
    }
}

#[test]
fn progress_paints_only_the_bounded_visible_catalog_cards() {
    let target = Target::Media(MediaId::new("Film0001").unwrap());
    let cards = vec![
        Card {
            key: &target,
            artwork_key: None,
            title: "Saved fixture",
            year: "2020",
            duration_seconds: 7200,
            saved_fraction: Some(0.25),
            action: criterion_ui::CardAction::Open,
        };
        1000
    ];
    let data = ViewData {
        cards: &cards,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    enter_grid(&mut ui, &data);
    let mut frame = ui.render(egui::RawInput::default(), &data);
    frame.output.textures_delta.clear();
    assert_eq!(frame.visible_cards.len(), 12);
    assert_eq!(bars(&frame).len(), 24);
}
