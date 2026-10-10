//! Synthetic public projection fixtures from immutable signed native grammar
//! 42a5cade9a6fc1c7f4ac483d3cf1960cfcd05b18b738ab8c6eaf3f4012204a86.
//! These establish no stream URL, rights, request or device admission.
use crate::*;

fn read(body: &str) -> Result<NativePlaybackSelection, Error> {
    NativePlaybackSelection::from_response(&Response {
        status: 200,
        body: SecretBody::new(body.as_bytes().to_vec()),
    })
}
fn selected(body: &str) -> NativePlayback {
    match read(body).unwrap() {
        NativePlaybackSelection::Selected(playback) => playback,
        other => panic!("expected selected projection, got {other:?}"),
    }
}

#[test]
fn playback_selects_first_playlist_first_dash_with_exact_private_text() {
    let playback = selected(
        r#"{"playlist":[{"contentType":"episode","mediaid":"Ep000001","title":"Episode","series_id":"Series01","duration":12.9,"sources":[{"type":"audio/mp4"},{"type":"application/dash+xml","file":"  synthetic DASH\u0000  ","drm":{"widevine":{"url":""}}},{"type":"application/dash+xml","file":"later"}],"tracks":[{"kind":"subtitles"},{"kind":"thumbnails","file":1.20e+3}]}]}"#,
    );
    assert_eq!(playback.media.id.as_str(), "Ep000001");
    assert_eq!(playback.media.kind, MediaKind::Episode);
    assert_eq!(
        playback.media.series_id.as_ref().unwrap().as_str(),
        "Series01"
    );
    assert_eq!(playback.media.duration, Some(12.0));
    assert_eq!(playback.dash_file(), "  synthetic DASH\0  ");
    assert_eq!(playback.widevine_license(), Some(""));
    assert_eq!(playback.thumbnail(), Some("1.20e+3"));
    assert_eq!(format!("{playback:?}"), "NativePlayback([redacted])");
}

#[test]
fn playback_refuses_oversize_raw_response_before_schema_or_selection() {
    let body = format!("{{\"playlist\":[]}}{}", " ".repeat(524_288));
    assert!(matches!(read(&body), Err(Error::ResponseTooLarge)));
    let error = NativePlaybackSelection::from_response(&Response {
        status: 403,
        body: SecretBody::new(body.into_bytes()),
    })
    .unwrap_err();
    assert_eq!(error, Error::HttpStatus(403));
}

#[test]
fn playback_primitive_budget_applies_to_selected_and_discarded_content() {
    let boundary = "x".repeat(16_384);
    let item = |file: &str| {
        format!(
            r#"{{"playlist":[{{"contentType":"category","mediaid":"Cat00001","title":"Category","sources":[{{"type":"application/dash+xml","file":"{file}"}}]}}]}}"#
        )
    };
    assert_eq!(selected(&item(&boundary)).dash_file().len(), 16_384);
    // The cap is decoded content bytes, not the JSON escape spelling's length.
    assert_eq!(
        selected(&item(&"\\u0078".repeat(16_384))).dash_file(),
        boundary
    );
    let oversized = "x".repeat(16_385);
    for body in [
        item(&oversized),
        format!(r#"{{"playlist":[],"discarded":"{oversized}"}}"#),
        item("").replace(
            "\"sources\":",
            &format!("\"duration\":\"{oversized}\",\"sources\":"),
        ),
        item("").replace(
            "\"sources\":",
            &format!(
                "\"tracks\":[{{\"kind\":\"thumbnails\",\"file\":1e{}}}],\"sources\":",
                "9".repeat(16_384)
            ),
        ),
    ] {
        assert!(matches!(read(&body), Err(Error::InvalidResponse)));
    }
}

#[test]
fn playback_item_budget_is_one_total_across_all_playlists_and_sources() {
    let item = |sources: usize| {
        format!(
            r#"{{"contentType":"category","mediaid":"Cat00001","title":"Category","sources":[{}]}}"#,
            vec![r#"{"type":"audio/mp4"}"#; sources].join(",")
        )
    };
    let source_boundary = format!(r#"{{"playlist":[{}]}}"#, item(511));
    assert!(matches!(
        read(&source_boundary),
        Ok(NativePlaybackSelection::NoDash)
    ));
    let playlist_boundary = format!(r#"{{"playlist":[{}]}}"#, vec![item(0); 512].join(","));
    assert!(matches!(
        read(&playlist_boundary),
        Ok(NativePlaybackSelection::NoDash)
    ));
    for body in [
        format!(r#"{{"playlist":[{}]}}"#, item(512)),
        format!(r#"{{"playlist":[{},{}]}}"#, item(510), item(1)),
        format!(r#"{{"playlist":[{}]}}"#, vec![item(0); 513].join(",")),
    ] {
        assert!(matches!(read(&body), Err(Error::InvalidResponse)));
    }
}

fn item_json(kind: &str, fields: &str, sources: &str) -> String {
    format!(
        r#"{{"contentType":"{kind}","mediaid":"Test0001","title":"Fixture","sources":{sources}{fields}}}"#
    )
}
const DASH: &str = r#"[{"type":"application/dash+xml","file":"first"}]"#;
fn body_for(kind: &str, fields: &str) -> String {
    format!(r#"{{"playlist":[{}]}}"#, item_json(kind, fields, DASH))
}

#[test]
fn playback_valid_unselected_cases_never_promote_later_playlists_or_dash() {
    assert!(matches!(
        read(r#"{"playlist":[]}"#),
        Ok(NativePlaybackSelection::EmptyPlaylist)
    ));
    let first = item_json(
        "film",
        "",
        r#"[{"type":"audio/mp4","file":false,"drm":[]},{"type":"application/vnd.apple.mpegurl","file":null}]"#,
    );
    let second = item_json("film", "", DASH);
    assert!(matches!(
        read(&format!(r#"{{"playlist":[{first},{second}]}}"#)),
        Ok(NativePlaybackSelection::NoDash)
    ));
    let sources = r#"[{"type":"application/dash+xml","file":"","drm":null},{"type":"application/dash+xml","file":"later","drm":{"widevine":{"url":"later-license"}}}]"#;
    let playback = selected(&format!(
        r#"{{"playlist":[{}]}}"#,
        item_json("film", "", sources)
    ));
    assert_eq!(playback.dash_file(), "");
    assert_eq!(playback.widevine_license(), None);
}

#[test]
fn playback_validates_later_known_dto_fields_before_selection() {
    let first = item_json("film", "", DASH);
    for sources in [
        "null",
        "{}",
        "[null]",
        "[[]]",
        r#"[{}]"#,
        r#"[{"type":null}]"#,
        r#"[{"type":1}]"#,
        r#"[{"type":"application/DASH+xml","file":"bad"}]"#,
        r#"[{"type":"application/dash+xml"}]"#,
        r#"[{"type":"application/dash+xml","file":null}]"#,
        r#"[{"type":"application/dash+xml","file":1}]"#,
        r#"[{"type":"application/dash+xml","file":"ok","drm":{}}]"#,
        r#"[{"type":"application/dash+xml","file":"ok","drm":{"widevine":null}}]"#,
        r#"[{"type":"application/dash+xml","file":"ok","drm":{"widevine":{}}}]"#,
        r#"[{"type":"application/dash+xml","file":"ok","drm":{"widevine":{"url":false}}}]"#,
        r#"[{"type":"application/dash+xml","file":"ok"},{"type":"unproved"}]"#,
    ] {
        let second = item_json("film", "", sources);
        assert!(
            matches!(
                read(&format!(r#"{{"playlist":[{first},{second}]}}"#)),
                Err(Error::InvalidResponse)
            ),
            "{sources}"
        );
    }
    for second in [
        "null",
        "[]",
        "{}",
        r#"{"sources":[]}"#,
        r#"{"contentType":"film","mediaid":"Test0001","title":"Fixture"}"#,
        r#"{"contentType":"unknown","mediaid":"Test0001","title":"Fixture","sources":[]}"#,
        r#"{"contentType":"film","mediaid":"bad","title":"Fixture","sources":[]}"#,
        r#"{"contentType":"film","mediaid":"Test0001","title":null,"sources":[]}"#,
        r#"{"sources":[],"media":{"contentType":"film","mediaid":"Test0001","title":"Fixture"}}"#,
    ] {
        assert!(
            matches!(
                read(&format!(r#"{{"playlist":[{first},{second}]}}"#)),
                Err(Error::InvalidResponse)
            ),
            "{second}"
        );
    }
    let later = item_json("collection", ",\"duration\":{}", "[]");
    assert!(matches!(
        read(&format!(r#"{{"playlist":[{first},{later}]}}"#)),
        Err(Error::InvalidResponse)
    ));
}

#[test]
fn playback_tracks_stop_at_first_match_and_keep_exact_primitive_content() {
    for tracks in [
        "[]",
        r#"[{}, {"kind":null}, {"kind":false}, {"kind":12}]"#,
        r#"[{"kind":"thumbnails"}, {"kind":"thumbnails","file":"later"}, false]"#,
        r#"[{"kind":"thumbnails","file":null}, {"kind":"thumbnails","file":"later"}, {"kind":{}}]"#,
    ] {
        assert_eq!(
            selected(&body_for("film", &format!(",\"tracks\":{tracks}"))).thumbnail(),
            None
        );
    }
    for (file, expected) in [
        (r#""""#, ""),
        ("true", "true"),
        ("1.20e-3", "1.20e-3"),
        (r#""x\u0000 ""#, "x\0 "),
    ] {
        let fields = format!(
            r#", "tracks":[{{"kind":"subtitles","file":{{}}}},{{"kind":"thumbnails","file":{file}}},false,{{"kind":{{}}}}]"#
        );
        assert_eq!(
            selected(&body_for("film", &fields)).thumbnail(),
            Some(expected)
        );
    }
    for tracks in [
        "null",
        "{}",
        "[false]",
        "[null]",
        r#"[{"kind":{}}]"#,
        r#"[{"kind":[]}]"#,
        r#"[{"kind":"thumbnails","file":[]}]"#,
        r#"[{"kind":"thumbnails","file":{}}]"#,
    ] {
        assert!(
            matches!(
                read(&body_for("film", &format!(",\"tracks\":{tracks}"))),
                Err(Error::InvalidResponse)
            ),
            "{tracks}"
        );
    }
}

#[test]
fn playback_duration_normalization_and_shape_guard_precede_all_nine_subtypes() {
    for (kind, timed) in [
        ("category", false),
        ("collection", false),
        ("franchise", false),
        ("series", false),
        ("film", true),
        ("original", true),
        ("episode", true),
        ("live", true),
        ("supplement", true),
    ] {
        for shape in ["{}", "[]"] {
            assert!(
                matches!(
                    read(&body_for(kind, &format!(",\"duration\":{shape}"))),
                    Err(Error::InvalidResponse)
                ),
                "{kind}/{shape}"
            );
        }
        for (primitive, expected) in [
            (r#""0x1.fffffffffffff8p-1""#, 1.0),
            (r#""0x1.fffffffffffff7p-1""#, 0.0),
            (r#""NaN""#, 0.0),
            (r#""+Infinity""#, 2_147_483_648.0),
            ("1e9999999999999999999999999", 2_147_483_648.0),
            (r#""-0.9""#, 0.0),
            (r#""2147483647.9f""#, 2_147_483_648.0),
        ] {
            let playback = selected(&body_for(kind, &format!(",\"duration\":{primitive}")));
            assert_eq!(
                playback.media.duration,
                timed.then_some(expected),
                "{kind}/{primitive}"
            );
        }
        for primitive in [
            r#""1ef""#,
            r#"".NaN""#,
            "false",
            "null",
            r#""-Infinity""#,
            "-1.9",
        ] {
            let result = read(&body_for(kind, &format!(",\"duration\":{primitive}")));
            if timed {
                assert!(
                    matches!(result, Err(Error::InvalidResponse)),
                    "{kind}/{primitive}"
                );
            } else {
                assert!(
                    matches!(result, Ok(NativePlaybackSelection::Selected(_))),
                    "{kind}/{primitive}"
                );
            }
        }
        assert_eq!(selected(&body_for(kind, "")).media.duration, None);
    }
}

#[test]
fn playback_schema_and_declared_projection_exclusions_remain_explicit() {
    for body in [
        "[]",
        "{}",
        r#"{"playlist":null}"#,
        r#"{"playlist":{}}"#,
        r#"{"playlist":""}"#,
        r#"{"playlist":[],"cast_token":false}"#,
        r#"{"playlist":[],"license_end_date_time":1}"#,
        r#"{"playlist":[],"cast_token":[]}"#,
        r#"{"playlist":[],"x":NaN}"#,
    ] {
        assert!(matches!(read(body), Err(Error::InvalidResponse)), "{body}");
    }
    for fields in [
        "",
        r#", "cast_token":null,"license_end_date_time":null"#,
        r#", "cast_token":"","license_end_date_time":"""#,
        r#", "license_end_date_time":"unowned-full-date-grammar""#,
    ] {
        assert!(matches!(
            read(&format!(r#"{{"playlist":[]{fields}}}"#)),
            Ok(NativePlaybackSelection::EmptyPlaylist)
        ));
    }
    // Full optional recursive MediaDto metadata is discarded, not SDK-validated.
    let playback = selected(&body_for(
        "film",
        r#", "release_date":"2024-02-29", "description":[], "custom_rating":{}, "playlist_primary":false"#,
    ));
    assert_eq!(
        playback.media.release_date.unwrap().to_calendar_date(),
        (2024, time::Month::February, 29)
    );
    assert!(matches!(
        read(&body_for("film", r#", "release_date":"2024-02-30""#)),
        Err(Error::InvalidResponse)
    ));
    let depth = format!(
        r#"{{"playlist":[],"discarded":{}0{}}}"#,
        "[".repeat(64),
        "]".repeat(64)
    );
    assert!(matches!(read(&depth), Err(Error::InvalidResponse)));
}

#[test]
fn playback_discards_only_unowned_subtype_summary_fields_without_float_coercion() {
    for kind in ["category", "collection", "franchise", "live"] {
        let playback = selected(&body_for(
            kind,
            r#", "release_date":1e999999, "series_id":1e999999, "series_title":1e999999"#,
        ));
        assert_eq!(playback.media.release_date, None);
        assert_eq!(playback.media.series_id, None);
        assert_eq!(playback.media.series_title, None);
    }
    for kind in ["film", "original", "supplement", "series"] {
        let playback = selected(&body_for(
            kind,
            r#", "series_id":1e999999, "series_title":1e999999"#,
        ));
        assert_eq!(playback.media.series_id, None);
        assert_eq!(playback.media.series_title, None);
        assert!(matches!(
            read(&body_for(kind, r#", "release_date":1e999999"#)),
            Err(Error::InvalidResponse)
        ));
    }
    assert!(matches!(
        read(&body_for("episode", r#", "series_id":1e999999"#)),
        Err(Error::InvalidResponse)
    ));
    assert!(matches!(
        read(&body_for("episode", r#", "series_title":1e999999"#)),
        Err(Error::InvalidResponse)
    ));
}
