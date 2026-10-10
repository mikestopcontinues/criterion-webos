use criterion_ui::{Action, AppUi, Card, CardAction, LoadState, Target, ViewData};

#[test]
fn borrowed_card_captions_keep_exact_words_and_fit_the_caption_row() {
    for (key, runtime) in [
        (
            Target::Native(criterion_provider::MediaId::new("Native01").unwrap()),
            None,
        ),
        (
            Target::Native(criterion_provider::MediaId::new("Native01").unwrap()),
            Some("0 min"),
        ),
        (
            Target::Native(criterion_provider::MediaId::new("Native01").unwrap()),
            Some("1 min"),
        ),
        (
            Target::Native(criterion_provider::MediaId::new("Native01").unwrap()),
            Some("1 h 0 min"),
        ),
        (
            Target::Native(criterion_provider::MediaId::new("Native01").unwrap()),
            Some("596523 h 14 min"),
        ),
        (
            Target::Media(criterion_provider::MediaId::new("Public01").unwrap()),
            Some("1193046 h 28 min"),
        ),
    ] {
        let cards = [Card {
            key: &key,
            artwork_key: None,
            title: "Runtime fixture",
            year: "1986",
            duration_label: runtime,
            saved_fraction: Some(0.25),
            action: CardAction::Open,
        }];
        let data = ViewData {
            cards: &cards,
            status: LoadState::Ready,
            ..Default::default()
        };
        let mut ui = AppUi::new();
        for action in [Action::Left, Action::Down, Action::Down, Action::Select] {
            ui.handle(action, &data);
        }
        let mut frame = ui.render(egui::RawInput::default(), &data);
        frame.output.textures_delta.clear();
        let captions: Vec<_> = frame
            .output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.job.text.ends_with(" min")
                {
                    Some((shape, text.galley.job.text.as_str()))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            captions.iter().map(|(_, text)| *text).collect::<Vec<_>>(),
            runtime.into_iter().collect::<Vec<_>>()
        );
        if let Some((caption, _)) = captions.first() {
            let bounds = caption.shape.visual_bounding_rect();
            assert!(caption.clip_rect.contains_rect(bounds));
            assert!(
                egui::Rect::from_min_max(egui::pos2(150.0, 507.0), egui::pos2(528.0, 538.0))
                    .contains_rect(bounds)
            );
            let year = frame.output.shapes.iter().find(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text=="1986")).unwrap();
            assert!(year.shape.visual_bounding_rect().right() + 8.0 < bounds.left());
        }
    }
}
