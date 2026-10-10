// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use criterion_account::{NativeDetailMetadata, NativeSeason};

fn media(id: &str) -> MediaSummary {
    MediaSummary {
        id: MediaId::new(id).unwrap(),
        title: "Synthetic Episode".into(),
        kind: MediaKind::Episode,
        duration: None,
        release_date: None,
        series_id: None,
        series_title: None,
    }
}
fn season(number: i32, episodes: &[&str]) -> NativeSeason {
    NativeSeason {
        number,
        title: "Synthetic season".into(),
        description: None,
        raw_episode_count: episodes.len(),
        episode_count: 999,
        episodes: episodes.iter().map(|id| media(id)).collect(),
    }
}
fn series(seasons: Vec<NativeSeason>) -> NativeDetail {
    let mut root = media("Root0001");
    root.kind = MediaKind::Series;
    NativeDetail {
        media: root,
        metadata: NativeDetailMetadata::default(),
        playlists: vec![NativePlaylist::Seasons(NativeSeasonsPlaylist {
            title: "Episodes".into(),
            seasons,
        })],
        featured: None,
        is_first_tab_sortable: None,
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
fn snapshot(rows: Vec<Position>) -> PositionsSnapshot {
    PositionsSnapshot::from_admitted(rows).unwrap()
}
fn outcome(
    detail: &NativeDetail,
    positions: Option<&PositionsSnapshot>,
) -> (Option<String>, String, usize) {
    let value = resolve_series(detail, positions);
    (
        value.target.map(|id| id.as_str().to_owned()),
        value.action,
        value.initial_season,
    )
}
#[test]
fn completed_episode_advances_in_source_order_across_seasons_and_terminal_means_watch_again() {
    let detail = series(vec![
        season(20, &["Ep000001", "Ep000002"]),
        season(3, &["Ep000003"]),
    ]);
    let progress = snapshot(vec![position("Ep000002", 95, 100)]);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (
            Some("Ep000003".into()),
            "WATCH SEASON 3, EPISODE 1".into(),
            1
        )
    );
    // Terminal completion does not claim that the earlier unpositioned rows ran.
    let progress = snapshot(vec![position("Ep000003", 95, 100)]);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (Some("Ep000001".into()), "WATCH AGAIN".into(), 0)
    );
    assert_eq!(
        outcome(&detail, None),
        (Some("Ep000001".into()), "WATCH FIRST EPISODE".into(), 0)
    );
}

#[test]
fn incomplete_selection_prioritizes_episode_index_then_season_index_over_ratio() {
    let detail = series(vec![
        season(20, &["Ep000001", "Ep000002", "Ep000003"]),
        season(3, &["Ep000004"]),
    ]);
    let progress = snapshot(vec![
        position("Ep000003", 1, 100),
        position("Ep000004", 94, 100),
    ]);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (
            Some("Ep000003".into()),
            "RESUME SEASON 20, EPISODE 3".into(),
            0
        )
    );
    let detail = series(vec![
        season(20, &["Ep000001", "Ep000002"]),
        season(3, &["Ep000003", "Ep000004"]),
    ]);
    let progress = snapshot(vec![
        position("Ep000002", 94, 100),
        position("Ep000004", 1, 100),
    ]);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (
            Some("Ep000004".into()),
            "RESUME SEASON 3, EPISODE 2".into(),
            1
        )
    );
}

#[test]
fn episode_float_duration_cutoff_and_saved_float_progress_have_distinct_owners() {
    let mut detail = series(vec![season(1, &["Ep000001", "Ep000002"])]);
    let progress = snapshot(vec![position("Ep000001", 95, 100)]);
    // Missing catalog duration is native zero; saved dur controls the ratio only.
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (Some("Ep000002".into()), "WATCH EPISODE 2".into(), 0)
    );
    let NativePlaylist::Seasons(group) = &mut detail.playlists[0] else {
        unreachable!()
    };
    group.seasons[0].episodes[0].duration = Some(299.999);
    assert_eq!(
        outcome(&detail, Some(&progress)).0.as_deref(),
        Some("Ep000002")
    );
    let NativePlaylist::Seasons(group) = &mut detail.playlists[0] else {
        unreachable!()
    };
    group.seasons[0].episodes[0].duration = Some(300.0);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (Some("Ep000001".into()), "RESUME EPISODE 1".into(), 0)
    );
    let progress = snapshot(vec![position("Ep000001", 98, 100)]);
    assert_eq!(
        outcome(&detail, Some(&progress)).0.as_deref(),
        Some("Ep000002")
    );
    let progress = snapshot(vec![position("Ep000002", -3, 100)]);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (Some("Ep000002".into()), "WATCH EPISODE 2".into(), 0)
    );
    let progress = snapshot(vec![position("Ep000002", 25, 0)]);
    assert_eq!(outcome(&detail, Some(&progress)).1, "WATCH FIRST EPISODE");
}

#[test]
fn empty_first_season_gates_primary_only_and_root_equal_targets_are_refused() {
    let detail = series(vec![season(20, &[]), season(3, &["Ep000003"])]);
    let progress = snapshot(vec![position("Ep000003", 20, 100)]);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (None, "WATCH FIRST EPISODE".into(), 1)
    );
    let mut detail = series(vec![season(20, &[])]);
    detail
        .playlists
        .push(NativePlaylist::Seasons(NativeSeasonsPlaylist {
            title: "Later supplied group".into(),
            seasons: vec![season(3, &["Ep000003"])],
        }));
    assert_eq!(outcome(&detail, Some(&progress)).0, None);
    let detail = series(vec![season(1, &["Root0001"])]);
    let progress = snapshot(vec![position("Root0001", 20, 100)]);
    assert_eq!(
        outcome(&detail, Some(&progress)).0,
        None,
        "project policy refuses a root-equal primary target"
    );
}

#[test]
fn primary_all_group_overlay_and_first_group_ui_selection_remain_separate() {
    use criterion_account::{NativeGenericPlaylist, NativePlaylistKey};
    let mut detail = series(vec![season(20, &["Ep000001"]), season(3, &["Ep000002"])]);
    let NativePlaylist::Seasons(group) = &mut detail.playlists[0] else {
        unreachable!()
    };
    // Typed internal witness: ordinary HTTP Seasons children are all Episodes.
    // An Original contributes no local ratio, while the same-ID Generic Episode
    // contributes to the primary all-group map. This is not account observation.
    group.seasons[1].episodes[0].kind = MediaKind::Original;
    detail
        .playlists
        .push(NativePlaylist::Generic(NativeGenericPlaylist {
            title: "Later Episode".into(),
            playlist_id: "witness".into(),
            key: NativePlaylistKey::Other,
            raw_child_count: 1,
            children: vec![media("Ep000002")],
        }));
    let progress = snapshot(vec![position("Ep000002", 20, 100)]);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (
            Some("Ep000002".into()),
            "RESUME SEASON 3, EPISODE 1".into(),
            0
        )
    );
    let mut original = media("Ep000002");
    original.kind = MediaKind::Original;
    assert_eq!(progress.progress(&original), None);
}

#[test]
fn one_response_preserves_last_supplied_exact_id_and_unmatched_signed_seconds() {
    let mut last = position("Ep000001", i64::MIN, i64::MAX);
    last.series_id = Some(MediaId::new("Parent01").unwrap());
    last.commentary_track = Some("Synthetic context".into());
    let progress = snapshot(vec![
        position("Ep000001", 90, 100),
        position("Other001", 20, 100),
        last,
    ]);
    assert_eq!(
        progress
            .position(&MediaId::new("Ep000001").unwrap())
            .unwrap()
            .pos,
        i64::MIN
    );
    assert_eq!(
        progress
            .position(&MediaId::new("Ep000001").unwrap())
            .unwrap()
            .dur,
        i64::MAX
    );
    assert_eq!(
        progress
            .position(&MediaId::new("Ep000001").unwrap())
            .unwrap()
            .series_id
            .as_ref()
            .unwrap()
            .as_str(),
        "Parent01"
    );
    assert_eq!(progress.progress(&media("Ep000001")), Some(0.0));
    assert_eq!(progress.progress(&media("Other001")), Some(0.2));
    assert_eq!(format!("{progress:?}"), "PositionsSnapshot([redacted])");
}

#[test]
fn snapshot_bounds_all_retained_records_and_reserved_private_context_before_deduplication() {
    let mut rows = Vec::with_capacity(513);
    rows.push(position("Ep000001", 20, 100));
    assert!(
        matches!(
            PositionsSnapshot::from_admitted(rows),
            Err(ResumeLimit::TooLarge)
        ),
        "retained capacity is charged even when length is small"
    );
    let rows = (0..513).map(|_| position("Ep000001", 20, 100)).collect();
    assert!(
        matches!(
            PositionsSnapshot::from_admitted(rows),
            Err(ResumeLimit::TooLarge)
        ),
        "duplicates count before last-ID selection"
    );
    let mut row = position("Other001", 20, 100);
    row.commentary_track = Some(String::with_capacity(65537));
    assert!(
        matches!(
            PositionsSnapshot::from_admitted(vec![row]),
            Err(ResumeLimit::TooLarge)
        ),
        "unmatched reserved context is private retained storage"
    );
    let mut row = position("Other001", 20, 100);
    row.series_title = Some(String::with_capacity(65537));
    assert!(matches!(
        PositionsSnapshot::from_admitted(vec![row]),
        Err(ResumeLimit::TooLarge)
    ));
    let mut rows = Vec::with_capacity(512);
    for i in 0..512 {
        rows.push(position(&format!("Ep{i:06}"), 20, 100));
    }
    let progress = snapshot(rows);
    assert_eq!(progress.progress(&media("Ep000511")), Some(0.2));
}

#[test]
fn terminal_primary_can_watch_again_while_local_ui_initializes_the_next_season() {
    use criterion_account::{NativeGenericPlaylist, NativePlaylistKey};
    let mut detail = series(vec![season(20, &["Ep000001"]), season(3, &["Ep000002"])]);
    let NativePlaylist::Seasons(group) = &mut detail.playlists[0] else {
        unreachable!()
    };
    // Typed source-contract witness, not an ordinary HTTP Seasons response.
    group.seasons[1].episodes[0].kind = MediaKind::Original;
    detail
        .playlists
        .push(NativePlaylist::Generic(NativeGenericPlaylist {
            title: "Later Episode".into(),
            playlist_id: "witness".into(),
            key: NativePlaylistKey::Other,
            raw_child_count: 1,
            children: vec![media("Ep000002")],
        }));
    let progress = snapshot(vec![
        position("Ep000001", 95, 100),
        position("Ep000002", 95, 100),
    ]);
    assert_eq!(
        outcome(&detail, Some(&progress)),
        (Some("Ep000001".into()), "WATCH AGAIN".into(), 1)
    );
}
