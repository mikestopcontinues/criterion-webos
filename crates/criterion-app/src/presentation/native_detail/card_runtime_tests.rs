// SPDX-License-Identifier: GPL-3.0-or-later
//! Connected native card labels, subtype gates and retained display admission.
use super::*;

#[test]
fn native_root_and_child_card_labels_use_direct_signed_int32_seconds() {
    for kind in [
        MediaKind::Film,
        MediaKind::Supplement,
        MediaKind::Episode,
        MediaKind::Original,
        MediaKind::Series,
        MediaKind::Collection,
        MediaKind::Category,
        MediaKind::Franchise,
        MediaKind::Live,
    ] {
        for (duration, expected) in [
            (None, None),
            (Some(0.0), Some("0 min")),
            (Some(0.9), Some("0 min")),
            (Some(59.9), Some("0 min")),
            (Some(90.5), Some("1 min")),
            (Some(3600.0), Some("1 h 0 min")),
            (Some(7199.0), Some("1 h 59 min")),
            (Some(2_147_483_648.0), Some("596523 h 14 min")),
            (Some(4_294_967_296.0), Some("596523 h 14 min")),
            (Some(f32::MAX), Some("596523 h 14 min")),
        ] {
            let mut source = detail(kind);
            source.media.duration = duration;
            let mut child = media("Child001", "Native child", kind);
            child.duration = duration;
            source.playlists.push(generic("Related", vec![child]));
            let presentation = Presentation::native_detail(source, None).unwrap();
            let expected = match kind {
                MediaKind::Film | MediaKind::Supplement | MediaKind::Episode => expected,
                _ => None,
            };
            presentation.with_view(LoginView::SignedOut, |view| {
                assert_eq!(
                    view.detail.as_ref().unwrap().card.duration_label,
                    expected,
                    "root {kind:?} {duration:?}"
                );
                assert_eq!(
                    view.rails[0].cards[0].duration_label, expected,
                    "child {kind:?} {duration:?}"
                );
            });
        }
    }
}

#[test]
fn season_episode_labels_preserve_selected_and_dormant_source_durations() {
    let mut source = series();
    let NativePlaylist::Seasons(group) = &mut source.playlists[1] else {
        panic!("seasons fixture")
    };
    group.seasons[0].episodes[0].duration = Some(0.0);
    group.seasons[0].episodes[1].duration = Some(90.5);
    group.seasons[1].episodes[0].duration = Some(4_294_967_296.0);
    let mut presentation = Presentation::native_detail(source, None).unwrap();
    presentation.with_view(LoginView::SignedOut, |view| {
        assert_eq!(
            view.rails[0]
                .cards
                .iter()
                .map(|card| card.duration_label)
                .collect::<Vec<_>>(),
            [Some("0 min"), Some("1 min")]
        );
        assert_eq!(view.detail.as_ref().unwrap().header_metadata, "");
    });
    presentation.select_native_season(1);
    presentation.with_view(LoginView::SignedOut, |view| {
        assert_eq!(
            view.rails[0].cards[0].duration_label,
            Some("596523 h 14 min")
        );
        assert_eq!(view.detail.as_ref().unwrap().header_metadata, "");
    });
}

#[test]
fn root_visible_and_dormant_card_label_capacities_are_charged() {
    for kind in [MediaKind::Film, MediaKind::Series] {
        let mut source = if kind == MediaKind::Series {
            series()
        } else {
            detail(kind)
        };
        source.media.duration = Some(90.5);
        if kind == MediaKind::Film {
            let mut child = media("Child001", "Related Film", MediaKind::Film);
            child.duration = Some(7199.0);
            source.playlists.push(generic("Related", vec![child]));
        }
        let mut presentation = Presentation::native_detail(source, None).unwrap();
        let before = presentation.estimated_bytes();
        let mut added = 0;
        let reserve = |card: &mut super::super::super::OwnedCard, capacity, text: &str| {
            let prior = card.duration_label.as_ref().map_or(0, String::capacity);
            let mut label = String::with_capacity(capacity);
            label.push_str(text);
            let added = label.capacity() - prior;
            card.duration_label = Some(label);
            added
        };
        if kind == MediaKind::Film {
            added += reserve(
                &mut presentation.detail.as_mut().unwrap().card,
                8192,
                "1 min",
            );
            added += reserve(&mut presentation.rails[0].cards[0], 16384, "1 h 59 min");
        } else {
            let native = presentation
                .detail
                .as_mut()
                .unwrap()
                .native
                .as_mut()
                .unwrap();
            added += reserve(&mut native.seasons[1].rail.cards[0], 32768, "0 min");
        }
        assert_eq!(presentation.estimated_bytes() - before, added);
    }
}

#[test]
fn card_label_bytes_are_included_before_exact_native_detail_admission() {
    let source = || {
        let mut source = detail(MediaKind::Film);
        source.media.duration = Some(f32::MAX);
        let mut child = media("Child001", "Native child", MediaKind::Episode);
        child.duration = Some(f32::MAX);
        source.playlists.push(generic("Related", vec![child]));
        source
    };
    let baseline = Presentation::native_detail(source(), None)
        .unwrap()
        .estimated_bytes();
    for (extra, accepted) in [(0, true), (1, false)] {
        let mut source = source();
        let mut description = String::with_capacity(512 * 1024 - baseline + extra);
        description.push('d');
        source.metadata.description_long = Some(description);
        assert!(source.estimated_bytes() < 512 * 1024);
        let result = Presentation::native_detail(source, None);
        assert_eq!(result.is_ok(), accepted);
        if let Ok(presentation) = result {
            assert_eq!(presentation.estimated_bytes(), 512 * 1024);
            presentation.with_view(LoginView::SignedOut, |view| {
                assert_eq!(
                    view.detail.as_ref().unwrap().card.duration_label,
                    Some("596523 h 14 min")
                );
                assert_eq!(
                    view.rails[0].cards[0].duration_label,
                    Some("596523 h 14 min")
                );
            });
        }
    }
}
