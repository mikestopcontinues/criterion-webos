// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual Application child-card input, decoding, projection and CPU captions.
use super::*;

#[test]
fn native_child_runtime_is_painted_after_actual_remote_navigation() {
    let mut fixture = Fixture::new(false, vec![list(), detail("Listed01", None)], 5);
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"film","mediaid":"Listed01","title":"Native runtime root","duration":3600,"playlists":[{"type":"GENERIC_PLAYLIST","title":"Related","playlistId":"related","playlist":[{"contentType":"film","mediaid":"Related1","title":"Native runtime child","duration":7199}]}]}"#.to_vec());
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    for _ in 0..3 {
        fixture.key(81, 1_073_741_905);
    }
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    let mut output = fixture.app.take_output().unwrap();
    output.textures_delta.clear();
    let caption = output.shapes.iter().find(|shape| {
        matches!(
            &shape.shape, egui::Shape::Text(text) if text.galley.job.text=="1 h 59 min"
        )
    });
    assert!(
        caption.is_some(),
        "the actual native child duration must reach its painted card caption"
    );
    let caption = caption.unwrap();
    let bounds = caption.shape.visual_bounding_rect();
    assert!(caption.clip_rect.contains_rect(bounds));
    assert!(
        egui::Rect::from_min_max(egui::pos2(150.0, 694.0), egui::pos2(528.0, 725.0))
            .contains_rect(bounds)
    );
    fixture.key(41, 27);
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::WatchList, Kind::NativeDetail("Listed01")]
    );
}

#[test]
fn native_child_card_captions_preserve_zero_omission_subtypes_and_int32_saturation() {
    for (kind, duration, expected) in [
        ("film", None, None),
        ("film", Some("0"), Some("0 min")),
        ("film", Some("0.9"), Some("0 min")),
        ("film", Some("59.9"), Some("0 min")),
        ("film", Some("90.5"), Some("1 min")),
        ("film", Some("3600"), Some("1 h 0 min")),
        ("film", Some("2147483648"), Some("596523 h 14 min")),
        ("film", Some("4294967296"), Some("596523 h 14 min")),
        ("film", Some("3.4028235e38"), Some("596523 h 14 min")),
        ("supplement", Some("0"), Some("0 min")),
        ("supplement", Some("7199"), Some("1 h 59 min")),
        ("episode", Some("0"), Some("0 min")),
        ("episode", Some("90.5"), Some("1 min")),
        ("episode", Some("4294967296"), Some("596523 h 14 min")),
        ("original", Some("7199"), None),
        ("series", Some("7199"), None),
        ("collection", Some("7199"), None),
        ("category", Some("7199"), None),
        ("franchise", Some("7199"), None),
        ("live", Some("7199"), None),
    ] {
        let duration = duration
            .map(|value| format!(",\"duration\":{value}"))
            .unwrap_or_default();
        let mut fixture = Fixture::new(false, vec![list(), detail("Listed01", None)], 5);
        *fixture.script.detail_body.lock().unwrap() = Some(format!(r#"{{"contentType":"film","mediaid":"Listed01","title":"Native runtime root","duration":3600,"playlists":[{{"type":"GENERIC_PLAYLIST","title":"Related","playlistId":"related","playlist":[{{"contentType":"{kind}","mediaid":"Related1","title":"Native runtime child","release_date":"2000-01-01"{duration}}}]}}]}}"#).into_bytes());
        fixture.open_list();
        select_listed(&mut fixture);
        fixture.wait(|fixture| fixture.native_ready("Listed01"));
        for _ in 0..3 {
            fixture.key(81, 1_073_741_905);
        }
        assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 0, column: 0 });
        let mut output = fixture.app.take_output().unwrap();
        output.textures_delta.clear();
        let captions: Vec<_> = output
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
            expected.into_iter().collect::<Vec<_>>(),
            "{kind} {duration}"
        );
        if let Some((caption, _)) = captions.first() {
            let bounds = caption.shape.visual_bounding_rect();
            assert!(caption.clip_rect.contains_rect(bounds));
            assert!(
                egui::Rect::from_min_max(egui::pos2(150.0, 694.0), egui::pos2(528.0, 725.0))
                    .contains_rect(bounds),
                "{kind} {duration}"
            );
            let year = output.shapes.iter().find(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text=="2000")).unwrap();
            assert!(year.shape.visual_bounding_rect().right() + 8.0 < bounds.left());
        }
        assert_eq!(
            *fixture.script.calls.lock().unwrap(),
            [Kind::WatchList, Kind::NativeDetail("Listed01")]
        );
    }
}
