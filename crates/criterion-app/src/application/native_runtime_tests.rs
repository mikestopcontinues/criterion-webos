// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual Application keys, account decoding, projection and painted runtimes.
//! Only the native HTTP response body is a synthetic fixture.
use super::*;

fn header_region() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(150.0, 540.0), egui::pos2(1400.0, 600.0))
}
fn information_region() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(348.0, 826.0), egui::pos2(1570.0, 852.0))
}
fn runtime_bounds(
    output: &mut egui::FullOutput,
    expected: &str,
    region: egui::Rect,
) -> Option<egui::Rect> {
    // This CPU oracle inspects shapes without an SDL/GLES texture consumer.
    output.textures_delta.clear();
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text)
            if text.galley.job.text == expected && region.contains(text.pos) =>
        {
            let bounds = shape.shape.visual_bounding_rect();
            assert!(shape.clip_rect.contains_rect(bounds));
            assert!(region.contains_rect(bounds));
            Some(bounds)
        }
        _ => None,
    })
}

#[test]
fn native_film_runtime_reaches_header_information_and_back_through_actual_keys() {
    // The existing native middleware body independently supplies duration 90.5.
    // Native compact display discards fractional and remaining seconds.
    let mut fixture = Fixture::new(false, vec![list(), detail("Listed01", None)], 5);
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    assert_eq!(fixture.app.ui.focus(), Focus::DetailAction(0));
    let header = runtime_bounds(
        &mut fixture.app.take_output().unwrap(),
        "1m",
        header_region(),
    );

    fixture.key(79, 1_073_741_903);
    fixture.key(40, 13);
    assert_eq!(fixture.app.ui.focus(), Focus::InformationPrimary);
    let information = runtime_bounds(
        &mut fixture.app.take_output().unwrap(),
        "1m",
        information_region(),
    );
    assert_eq!(
        [header.is_some(), information.is_some()],
        [true, true],
        "the admitted native duration must reach both actual painted surfaces"
    );
    fixture.key(41, 27);
    assert_eq!(fixture.app.ui.focus(), Focus::DetailAction(1));
    assert!(
        runtime_bounds(
            &mut fixture.app.take_output().unwrap(),
            "1m",
            header_region()
        )
        .is_some()
    );
    fixture.key(41, 27);
    assert_eq!(fixture.app.ui.page(), Page::MyList);
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(
        *fixture.script.calls.lock().unwrap(),
        [Kind::WatchList, Kind::NativeDetail("Listed01")]
    );
}

fn assert_runtime(output: &mut egui::FullOutput, expected: &str, region: egui::Rect) {
    if expected.is_empty() {
        output.textures_delta.clear();
        assert!(
            !output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::Text(text)
                    if region.contains(text.pos) && !text.galley.job.text.is_empty()
            )),
            "absent native runtime must not paint a zero or public card value"
        );
    } else {
        assert!(
            runtime_bounds(output, expected, region).is_some(),
            "missing runtime {expected}"
        );
    }
}

#[test]
fn native_runtime_numeric_boundaries_reach_both_actual_application_surfaces() {
    // Literal expected strings follow the signed source arithmetic, independently
    // of the implementation: Header keeps Int64; Information first narrows Int32.
    for (duration, header, information) in [
        (None, "", ""),
        (Some("0"), "0m", "0m"),
        (Some("0.9"), "0m", "0m"),
        (Some("59.9"), "0m", "0m"),
        (Some("60.9"), "1m", "1m"),
        (Some("3600"), "1h", "1h"),
        (Some("7199"), "1h 59m", "1h 59m"),
        (Some("2147483648"), "596523h 14m", "0m"),
        (Some("4294967296"), "1193046h 28m", "0m"),
        (Some("4294971392"), "1193047h 36m", "1h 8m"),
        (Some("8796093022208"), "50m", "0m"),
        (Some("3.4028235e38"), "1011703407h 30m", "0m"),
    ] {
        let mut fixture = Fixture::new(false, vec![list(), detail("Listed01", None)], 5);
        let duration = duration.map_or(String::new(), |value| format!(",\"duration\":{value}"));
        *fixture.script.detail_body.lock().unwrap() = Some(format!(
            "{{\"contentType\":\"film\",\"mediaid\":\"Listed01\",\"title\":\"Synthetic runtime film\"{duration}}}"
        ).into_bytes());
        fixture.open_list();
        select_listed(&mut fixture);
        fixture.wait(|fixture| fixture.native_ready("Listed01"));
        assert_runtime(
            &mut fixture.app.take_output().unwrap(),
            header,
            header_region(),
        );
        fixture.key(79, 1_073_741_903);
        fixture.key(40, 13);
        assert_eq!(fixture.app.ui.focus(), Focus::InformationPrimary);
        assert_runtime(
            &mut fixture.app.take_output().unwrap(),
            information,
            information_region(),
        );
        assert!(
            fixture
                .public_requests
                .lock()
                .unwrap()
                .iter()
                .all(|path| path == "/"),
            "native metadata must not request public website Detail"
        );
    }
}

#[test]
fn native_episode_omits_header_runtime_while_information_keeps_it() {
    let mut fixture = Fixture::new(false, vec![list(), detail("Listed01", None)], 5);
    *fixture.script.detail_body.lock().unwrap() = Some(br#"{"contentType":"episode","mediaid":"Listed01","title":"Synthetic runtime episode","duration":7199}"#.to_vec());
    fixture.open_list();
    select_listed(&mut fixture);
    fixture.wait(|fixture| fixture.native_ready("Listed01"));
    assert_runtime(&mut fixture.app.take_output().unwrap(), "", header_region());
    fixture.key(79, 1_073_741_903);
    fixture.key(40, 13);
    assert_eq!(fixture.app.ui.focus(), Focus::InformationPrimary);
    assert_runtime(
        &mut fixture.app.take_output().unwrap(),
        "1h 59m",
        information_region(),
    );
}

#[path = "native_card_runtime_tests.rs"]
mod native_card_runtime_tests;
