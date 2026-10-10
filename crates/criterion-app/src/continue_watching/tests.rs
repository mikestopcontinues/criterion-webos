use super::*;
use criterion_account::{MediaKind, Position};
use criterion_provider::MediaId;

fn media(id: &str, title: &str, kind: MediaKind) -> MediaSummary {
    MediaSummary {
        id: MediaId::new(id).unwrap(),
        title: title.into(),
        kind,
        series_id: None,
        series_title: None,
        duration: None,
        release_date: None,
    }
}
fn position(id: &str, pos: i64, dur: i64) -> Position {
    Position {
        media_id: MediaId::new(id).unwrap(),
        pos,
        dur,
        commentary_track: None,
        series_id: None,
        series_title: None,
    }
}

#[test]
fn received_order_keeps_the_first_exact_id_and_its_original_media() {
    let mut first = media(
        "Z0000001",
        "Synthetic first retained title",
        MediaKind::Film,
    );
    first.duration = Some(95.25);
    first.release_date =
        Some(time::Date::from_calendar_date(2000, time::Month::February, 29).unwrap());
    let shelf = ContinueWatchingShelf::from_admitted(ContinueWatching {
        playlist: vec![
            first,
            media("A0000001", "Synthetic second title", MediaKind::Episode),
            media(
                "Z0000001",
                "Synthetic duplicate title",
                MediaKind::Supplement,
            ),
            media("z0000001", "Synthetic distinct case", MediaKind::Original),
        ],
        positions: vec![],
    })
    .unwrap();
    let rows = shelf.rows();
    assert_eq!(
        rows.iter()
            .map(|r| r.media().id.as_str())
            .collect::<Vec<_>>(),
        ["Z0000001", "A0000001", "z0000001"]
    );
    assert_eq!(rows[0].media().title, "Synthetic first retained title");
    assert_eq!(rows[0].media().kind, MediaKind::Film);
    assert_eq!(rows[0].media().duration, Some(95.25));
    assert_eq!(
        rows[0].media().release_date,
        Some(time::Date::from_calendar_date(2000, time::Month::February, 29).unwrap())
    );
    assert!(rows.iter().all(|r| r.saved_fraction().is_none()));
    assert!(!format!("{shelf:?} {:?}", rows[0]).contains("Synthetic"));
    assert!(!format!("{shelf:?} {:?}", rows[0]).contains("Z0000001"));
}

#[test]
fn saved_fraction_uses_the_last_exact_position_only_for_playable_native_kinds() {
    let cases = [
        ("Film0001", MediaKind::Film),
        ("Supp0001", MediaKind::Supplement),
        ("Epis0001", MediaKind::Episode),
        ("NegP0001", MediaKind::Film),
        ("Zero0001", MediaKind::Film),
        ("NegD0001", MediaKind::Episode),
        ("Miss0001", MediaKind::Supplement),
        ("Cat00001", MediaKind::Category),
        ("Coll0001", MediaKind::Collection),
        ("Seri0001", MediaKind::Series),
        ("Orig0001", MediaKind::Original),
        ("Fran0001", MediaKind::Franchise),
        ("Live0001", MediaKind::Live),
    ];
    let playlist = cases
        .into_iter()
        .map(|(id, kind)| {
            let mut item = media(id, "Synthetic saved title", kind);
            // Catalog duration has subtype-dependent units; it is not the denominator.
            item.duration = Some(3.5);
            item
        })
        .collect();
    let mut positions = vec![
        position("Film0001", 95, 100),
        position("Supp0001", 1, 100),
        position("Epis0001", i64::MAX, 1),
        position("NegP0001", i64::MIN, 100),
        position("Zero0001", 50, 100),
        position("NegD0001", 50, 100),
        position("Supp0001", 98, 100),
        position("Zero0001", 50, 0),
        position("NegD0001", 50, -100),
        // A distinct-case key and an unrelated saved position must not overlay a row.
        position("miss0001", 50, 100),
        position("Unkn0001", 50, 100),
    ];
    positions.extend(cases[7..].iter().map(|(id, _)| position(id, 50, 100)));
    positions[0].commentary_track = Some("Synthetic private commentary".into());
    positions[0].series_id = Some(MediaId::new("Seri0002").unwrap());
    positions[0].series_title = Some("Synthetic private series".into());
    let shelf = ContinueWatchingShelf::from_admitted(ContinueWatching {
        playlist,
        positions,
    })
    .unwrap();
    assert_eq!(shelf.rows().len(), 13);
    assert_eq!(
        shelf
            .rows()
            .iter()
            .map(ContinueWatchingRow::saved_fraction)
            .collect::<Vec<_>>(),
        [
            Some(0.95),
            Some(0.98),
            Some(1.0),
            Some(0.0),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ]
    );
    assert!(shelf.rows().iter().all(|r| r.media().duration == Some(3.5)));
}

#[test]
fn raw_record_limits_apply_before_duplicate_or_unmatched_rows_are_discarded() {
    let duplicate_rows = (0..513)
        .map(|_| media("Film0001", "Synthetic duplicate", MediaKind::Film))
        .collect();
    assert!(matches!(
        ContinueWatchingShelf::from_admitted(ContinueWatching {
            playlist: duplicate_rows,
            positions: vec![],
        }),
        Err(ShelfLimit::TooLarge)
    ));
    let unrelated_positions = (0..513).map(|_| position("Unkn0001", 1, 2)).collect();
    assert!(matches!(
        ContinueWatchingShelf::from_admitted(ContinueWatching {
            playlist: vec![],
            positions: unrelated_positions,
        }),
        Err(ShelfLimit::TooLarge)
    ));
}

#[test]
fn owned_title_capacity_is_bounded_even_when_visible_text_is_short() {
    let mut oversized = media("Film0001", "", MediaKind::Film);
    oversized.title = String::with_capacity(65_529);
    oversized.title.push('x');
    assert!(matches!(
        ContinueWatchingShelf::from_admitted(ContinueWatching {
            playlist: vec![oversized],
            positions: vec![],
        }),
        Err(ShelfLimit::TooLarge)
    ));

    // One validated eight-byte ID plus 65,528 title bytes exactly fits 64 KiB.
    let mut boundary = media("Film0001", "", MediaKind::Film);
    boundary.title = String::with_capacity(65_528);
    boundary.title.push('x');
    let shelf = ContinueWatchingShelf::from_admitted(ContinueWatching {
        playlist: vec![boundary],
        positions: vec![],
    })
    .unwrap();
    assert_eq!(shelf.rows()[0].media().title, "x");
    assert!(shelf.retained_bytes() > 65_536);
    assert!(shelf.retained_bytes() <= MAX_RETAINED_BYTES);
}

#[test]
fn saved_fraction_rounds_each_native_operand_to_float32_before_division() {
    let shelf = ContinueWatchingShelf::from_admitted(ContinueWatching {
        playlist: vec![media("Film0001", "Synthetic precision", MediaKind::Film)],
        positions: vec![position("Film0001", 16_777_217, 16_777_219)],
    })
    .unwrap();
    // Float32 operands round to 16,777,216 and 16,777,220. Their quotient is
    // four ULPs below one; dividing in Float64 then casting is only two below.
    assert_eq!(
        shelf.rows()[0].saved_fraction().unwrap().to_bits(),
        0x3f7f_fffc
    );
}

#[test]
fn the_full_record_boundary_preserves_every_row_and_its_display_fraction() {
    let playlist = (0..512)
        .map(|index| media(&format!("F{index:07}"), "f", MediaKind::Film))
        .collect();
    let positions = (0..512)
        .map(|index| position(&format!("F{index:07}"), index, 512))
        .collect();
    let shelf = ContinueWatchingShelf::from_admitted(ContinueWatching {
        playlist,
        positions,
    })
    .unwrap();
    assert_eq!(shelf.rows().len(), 512);
    assert_eq!(shelf.rows()[0].media().id.as_str(), "F0000000");
    assert_eq!(shelf.rows()[0].saved_fraction(), Some(0.0));
    assert_eq!(shelf.rows()[511].media().id.as_str(), "F0000511");
    assert_eq!(shelf.rows()[511].saved_fraction(), Some(0.9980469));
    assert!(shelf.retained_bytes() < MAX_RETAINED_BYTES);
}

#[test]
fn retained_storage_tracks_title_capacity_and_drops_unrelated_saved_metadata() {
    let shelf_with_capacity = |capacity, positions| {
        let mut item = media("Film0001", "", MediaKind::Film);
        item.title = String::with_capacity(capacity);
        item.title.push('x');
        ContinueWatchingShelf::from_admitted(ContinueWatching {
            playlist: vec![item],
            positions,
        })
        .unwrap()
    };
    let small = shelf_with_capacity(32, vec![]);
    let large = shelf_with_capacity(1024, vec![]);
    assert_eq!(large.retained_bytes() - small.retained_bytes(), 992);
    assert_eq!(large.rows()[0].media().title, small.rows()[0].media().title);

    let mut unrelated = position("Unkn0001", 1, 2);
    unrelated.commentary_track = Some("c".repeat(128));
    unrelated.series_id = Some(MediaId::new("Seri0001").unwrap());
    unrelated.series_title = Some("s".repeat(1024));
    let with_metadata = shelf_with_capacity(32, vec![unrelated]);
    assert_eq!(with_metadata.retained_bytes(), small.retained_bytes());
    assert_eq!(with_metadata.rows()[0].saved_fraction(), None);
}

#[test]
fn empty_response_is_an_empty_immutable_shelf_with_bounded_storage() {
    let shelf = ContinueWatchingShelf::from_admitted(ContinueWatching {
        playlist: vec![],
        positions: vec![],
    })
    .unwrap();
    assert!(shelf.rows().is_empty());
    assert!(shelf.retained_bytes() > 0);
    assert!(shelf.retained_bytes() < MAX_RETAINED_BYTES);
}

#[test]
fn only_episode_rows_keep_the_last_exact_saved_series_id_without_remapping_media() {
    let mut earlier = position("Epis0001", 10, 100);
    earlier.series_id = Some(MediaId::new("Seri0001").unwrap());
    let mut latest = position("Epis0001", 90, 100);
    latest.series_id = Some(MediaId::new("Seri0002").unwrap());
    let mut removed = position("Epis0002", 10, 100);
    removed.series_id = Some(MediaId::new("Seri0003").unwrap());
    let mut nonpositive = position("Epis0003", 10, 0);
    nonpositive.series_id = Some(MediaId::new("Seri0004").unwrap());
    let mut film = position("Film0001", 50, 100);
    film.series_id = Some(MediaId::new("Seri0005").unwrap());
    let mut franchise = position("Fran0001", 50, 100);
    franchise.series_id = Some(MediaId::new("Seri0006").unwrap());
    let shelf = ContinueWatchingShelf::from_admitted(ContinueWatching {
        playlist: vec![
            media("Epis0001", "Synthetic first episode", MediaKind::Episode),
            media("Epis0002", "Synthetic cleared series", MediaKind::Episode),
            media("Epis0003", "Synthetic zero duration", MediaKind::Episode),
            media(
                "Epis0004",
                "Synthetic unmatched episode",
                MediaKind::Episode,
            ),
            media("Film0001", "Synthetic film", MediaKind::Film),
            media("Fran0001", "Synthetic franchise", MediaKind::Franchise),
            media("Epis0001", "Synthetic duplicate", MediaKind::Film),
        ],
        positions: vec![
            earlier,
            latest,
            removed,
            position("Epis0002", 80, 100),
            nonpositive,
            film,
            franchise,
        ],
    })
    .unwrap();
    let rows = shelf.rows();
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[0].media().id.as_str(), "Epis0001");
    assert_eq!(rows[0].media().kind, MediaKind::Episode);
    assert_eq!(
        rows[0].saved_series_id().map(MediaId::as_str),
        Some("Seri0002")
    );
    assert_eq!(rows[0].saved_fraction(), Some(0.9));
    assert_eq!(rows[1].saved_series_id(), None);
    assert_eq!(rows[1].saved_fraction(), Some(0.8));
    assert_eq!(
        rows[2].saved_series_id().map(MediaId::as_str),
        Some("Seri0004")
    );
    assert_eq!(rows[2].saved_fraction(), None);
    assert_eq!(rows[3].saved_series_id(), None);
    assert_eq!(rows[4].saved_series_id(), None);
    assert_eq!(rows[4].saved_fraction(), Some(0.5));
    assert_eq!(rows[5].saved_series_id(), None);
    assert_eq!(rows[5].media().id.as_str(), "Fran0001");
    assert!(!format!("{shelf:?} {:?}", rows[0]).contains("Seri0002"));
}

#[test]
fn episode_series_ids_consume_text_budget_and_discarded_series_ids_do_not() {
    let build = |kind, title_capacity, series_id| {
        let mut item = media("Item0001", "", kind);
        item.title = String::with_capacity(title_capacity);
        item.title.push('x');
        let mut saved = position("Item0001", 1, 2);
        saved.series_id = series_id;
        ContinueWatchingShelf::from_admitted(ContinueWatching {
            playlist: vec![item],
            positions: vec![saved],
        })
    };
    assert!(matches!(
        build(
            MediaKind::Episode,
            65_521,
            Some(MediaId::new("Seri0001").unwrap())
        ),
        Err(ShelfLimit::TooLarge)
    ));
    // Two validated eight-byte IDs leave 65,520 bytes for the retained title.
    let boundary = build(
        MediaKind::Episode,
        65_520,
        Some(MediaId::new("Seri0001").unwrap()),
    )
    .unwrap();
    assert!(boundary.retained_bytes() > 65_536);
    assert!(boundary.retained_bytes() <= MAX_RETAINED_BYTES);
    let discarded = build(
        MediaKind::Film,
        65_528,
        Some(MediaId::new("Seri0001").unwrap()),
    )
    .unwrap();
    assert_eq!(discarded.rows()[0].saved_series_id(), None);
    let with_series = build(
        MediaKind::Episode,
        32,
        Some(MediaId::new("Seri0001").unwrap()),
    )
    .unwrap();
    let without_series = build(MediaKind::Episode, 32, None).unwrap();
    assert_eq!(
        with_series.retained_bytes() - without_series.retained_bytes(),
        8
    );
}

#[test]
fn native_episode_metadata_counts_actual_text_capacity_and_both_retained_series_ids() {
    let mut oversized = media("Epis0001", "", MediaKind::Episode);
    oversized.series_id = Some(MediaId::new("Meta0001").unwrap());
    oversized.series_title = Some(String::with_capacity(65_521));
    oversized.series_title.as_mut().unwrap().push('x');
    assert!(
        matches!(
            ContinueWatchingShelf::from_admitted(ContinueWatching {
                playlist: vec![oversized],
                positions: vec![],
            }),
            Err(ShelfLimit::TooLarge)
        ),
        "native title capacity and two eight-byte IDs exceed 64 KiB"
    );

    let with_metadata = |capacity| {
        let mut item = media("Epis0001", "", MediaKind::Episode);
        item.series_id = Some(MediaId::new("Meta0001").unwrap());
        item.series_title = Some(String::with_capacity(capacity));
        item.series_title.as_mut().unwrap().push('x');
        ContinueWatchingShelf::from_admitted(ContinueWatching {
            playlist: vec![item],
            positions: vec![],
        })
        .unwrap()
    };
    let small = with_metadata(32);
    let large = with_metadata(1024);
    assert_eq!(large.retained_bytes() - small.retained_bytes(), 992);
    assert_eq!(large.rows()[0].media().series_title.as_deref(), Some("x"));
    assert_eq!(
        large.rows()[0].media().series_id.as_ref().unwrap().as_str(),
        "Meta0001"
    );
    assert_eq!(
        large.rows()[0].saved_series_id(),
        None,
        "saved Position getter does not invent the metadata fallback"
    );

    // Original ID + metadata ID + saved override ID use24bytes of the64KiB text policy.
    let mut boundary = media("Epis0001", "", MediaKind::Episode);
    boundary.series_id = Some(MediaId::new("Meta0001").unwrap());
    boundary.series_title = Some(String::with_capacity(65_512));
    boundary.series_title.as_mut().unwrap().push('x');
    let mut saved = position("Epis0001", 25, 100);
    saved.series_id = Some(MediaId::new("Save0001").unwrap());
    let shelf = ContinueWatchingShelf::from_admitted(ContinueWatching {
        playlist: vec![boundary],
        positions: vec![saved],
    })
    .unwrap();
    assert_eq!(shelf.rows()[0].media().id.as_str(), "Epis0001");
    assert_eq!(
        shelf.rows()[0].saved_series_id().unwrap().as_str(),
        "Save0001"
    );
    assert!(shelf.retained_bytes() > 65_536 && shelf.retained_bytes() <= MAX_RETAINED_BYTES);
}
