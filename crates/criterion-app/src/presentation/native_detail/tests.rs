// SPDX-License-Identifier: GPL-3.0-or-later
use super::super::{NativeActivation, Presentation};
use criterion_account::{
    MediaKind, MediaSummary, NativeDetail, NativeDetailMetadata, NativeGenericPlaylist,
    NativePlaylist, NativePlaylistKey, NativeSeason, NativeSeasonsPlaylist,
};
use criterion_provider::MediaId;
use criterion_ui::{CardAction, DetailKind, LoginView, Target};

#[test]
fn featured_presence_order_and_duplicate_cross_list_identity_survive_projection() {
    let mut source = detail(MediaKind::Collection);
    source.featured = Some(criterion_account::NativeFeatured {
        title: None,
        children: vec![
            media("Shared01", "First Feature Film", MediaKind::Film),
            media("Child002", "Second Feature Film", MediaKind::Film),
        ],
        raw_child_count: 2,
    });
    source.playlists.push(generic(
        "Ordinary",
        vec![media("Shared01", "Ordinary Episode", MediaKind::Episode)],
    ));
    let presentation = Presentation::native_detail(source, None).unwrap();
    presentation.with_view(LoginView::SignedOut, |view| {
        let featured = view.detail.as_ref().unwrap().featured.as_ref().unwrap();
        assert_eq!(featured.title, None);
        assert_eq!(
            featured
                .cards
                .iter()
                .map(|card| card.title)
                .collect::<Vec<_>>(),
            ["First Feature Film", "Second Feature Film"]
        );
        assert_eq!(view.rails.len(), 1);
        assert_eq!(view.rails[0].cards[0].title, "Ordinary Episode");
        assert_eq!(
            featured.cards[0].artwork_key, view.rails[0].cards[0].artwork_key,
            "identical artwork sources share one binding across independent lists"
        );
        assert_eq!(view.total, 3);
    });
    let target = Target::Native(MediaId::new("Shared01").unwrap());
    assert_eq!(
        presentation.native_activation(&target),
        None,
        "unaddressed duplicate identity is ambiguous"
    );
    assert_eq!(
        presentation.selected_card_action(
            criterion_ui::Page::Detail,
            criterion_ui::Focus::FeaturedCard(0),
            &target
        ),
        Some(Some(NativeActivation::Detail {
            id: MediaId::new("Shared01").unwrap(),
            auto_play: false
        }))
    );
    assert_eq!(
        presentation.selected_card_action(
            criterion_ui::Page::Detail,
            criterion_ui::Focus::Card { row: 0, column: 0 },
            &target
        ),
        Some(Some(NativeActivation::Play {
            id: MediaId::new("Shared01").unwrap()
        }))
    );
}

#[test]
fn empty_feature_retains_nullable_supplied_heading_without_an_extra_tab() {
    for title in [
        None,
        Some(String::new()),
        Some("Supplied empty Feature".into()),
    ] {
        let mut source = detail(MediaKind::Category);
        source.featured = Some(criterion_account::NativeFeatured {
            title: title.clone(),
            children: vec![],
            raw_child_count: 0,
        });
        let presentation = Presentation::native_detail(source, None).unwrap();
        presentation.with_view(LoginView::SignedOut, |view| {
            let featured = view.detail.as_ref().unwrap().featured.as_ref().unwrap();
            assert_eq!(featured.title, title.as_deref());
            assert!(featured.cards.is_empty());
            assert!(view.rails.is_empty());
            assert_eq!(view.detail.as_ref().unwrap().selected_playlist, None);
        });
    }
}

#[test]
fn featured_raw_items_and_all_owned_caption_capacity_consume_native_budget() {
    let mut source = detail(MediaKind::Collection);
    source.featured = Some(criterion_account::NativeFeatured {
        title: Some("Supplied".into()),
        children: (0..510)
            .map(|index| media(&format!("F{index:07}"), "Feature Film", MediaKind::Film))
            .collect(),
        raw_child_count: 510,
    });
    source.playlists.push(generic(
        "Ordinary",
        vec![media("Shared01", "Ordinary Film", MediaKind::Film)],
    ));
    let mut admitted = Presentation::native_detail(source.clone(), None).unwrap();
    let before = admitted.estimated_bytes();
    let title = admitted
        .detail
        .as_mut()
        .unwrap()
        .native
        .as_mut()
        .unwrap()
        .featured
        .as_mut()
        .unwrap()
        .title
        .as_mut()
        .unwrap();
    let prior = title.capacity();
    title.reserve_exact(8192);
    let added = title.capacity() - prior;
    assert_eq!(admitted.estimated_bytes() - before, added);
    let bytes = admitted.estimated_bytes();
    admitted.with_view(LoginView::SignedOut, |view| {
        assert_eq!(
            view.detail
                .as_ref()
                .unwrap()
                .featured
                .as_ref()
                .unwrap()
                .cards
                .len(),
            510
        )
    });
    assert_eq!(
        admitted.estimated_bytes(),
        bytes,
        "borrowed display cannot duplicate retained Feature state"
    );
    source.featured.as_mut().unwrap().raw_child_count = 511;
    assert!(
        matches!(
            Presentation::native_detail(source, None),
            Err(super::super::ProjectionLimit::TooLarge)
        ),
        "root plus Feature raw rows plus ordinary rows is bounded to512"
    );
    let mut source = detail(MediaKind::Franchise);
    let mut heading = String::with_capacity(512 * 1024);
    heading.push_str("Supplied");
    source.featured = Some(criterion_account::NativeFeatured {
        title: Some(heading),
        children: vec![],
        raw_child_count: 0,
    });
    assert!(
        matches!(
            Presentation::native_detail(source, None),
            Err(super::super::ProjectionLimit::TooLarge)
        ),
        "a short heading cannot hide retained reserved capacity"
    );
}

fn media(id: &str, title: &str, kind: MediaKind) -> MediaSummary {
    MediaSummary {
        id: MediaId::new(id).unwrap(),
        title: title.into(),
        kind,
        duration: None,
        release_date: None,
        series_id: None,
        series_title: None,
    }
}
fn detail(kind: MediaKind) -> NativeDetail {
    NativeDetail {
        media: media("Root0001", "Native root", kind),
        metadata: NativeDetailMetadata::default(),
        playlists: Vec::new(),
        featured: None,
        is_first_tab_sortable: None,
    }
}
fn generic(title: &str, children: Vec<MediaSummary>) -> NativePlaylist {
    NativePlaylist::Generic(NativeGenericPlaylist {
        title: title.into(),
        playlist_id: "native:playlist".into(),
        key: NativePlaylistKey::Other,
        raw_child_count: children.len(),
        children,
    })
}
fn season(number: i32, title: &str, episodes: Vec<MediaSummary>) -> NativeSeason {
    NativeSeason {
        number,
        title: title.into(),
        description: None,
        raw_episode_count: episodes.len(),
        episode_count: 99,
        episodes,
    }
}
fn seasons(values: Vec<NativeSeason>) -> NativePlaylist {
    NativePlaylist::Seasons(NativeSeasonsPlaylist {
        title: "Supplied Seasons title".into(),
        seasons: values,
    })
}
fn series() -> NativeDetail {
    let mut source = detail(MediaKind::Series);
    source.playlists = vec![
        generic(
            "Related",
            vec![media("Child001", "Related film", MediaKind::Film)],
        ),
        seasons(vec![
            season(
                20,
                "Volume B",
                vec![
                    media("Ep000001", "Episode one", MediaKind::Episode),
                    media("Ep000002", "Episode two", MediaKind::Episode),
                ],
            ),
            season(
                3,
                "Volume A",
                vec![media("Ep000003", "Episode three", MediaKind::Episode)],
            ),
        ]),
        generic(
            "Supplements",
            vec![media("Child002", "Supplement", MediaKind::Supplement)],
        ),
        seasons(vec![season(
            1,
            "Ignored later group",
            vec![media("Ep999999", "Ignored episode", MediaKind::Episode)],
        )]),
    ];
    source
}

#[test]
fn film_lends_native_root_long_description_and_own_primary_target() {
    let mut source = detail(MediaKind::Film);
    source.metadata.description_long = Some("Long description".into());
    source.metadata.description_medium = Some("Medium description".into());
    source.metadata.description = Some("Base description".into());
    let presentation = Presentation::native_detail(source, None).unwrap();
    presentation.with_view(LoginView::SignedOut, |view| {
        let native = view.detail.as_ref().expect("native Detail header");
        assert_eq!(native.kind, DetailKind::Film);
        assert_eq!(native.description, "Long description");
        assert_eq!(native.card.title, "Native root");
        assert_eq!(
            native.card.key,
            &Target::Native(MediaId::new("Root0001").unwrap())
        );
        assert_eq!(
            native.primary_playback_target,
            Some(&MediaId::new("Root0001").unwrap())
        );
    });
}

#[test]
fn nine_native_root_kinds_keep_identity_and_only_proved_primary_targets() {
    for (kind, displayed, playable) in [
        (MediaKind::Film, DetailKind::Film, true),
        (MediaKind::Original, DetailKind::Original, true),
        (MediaKind::Episode, DetailKind::Episode, true),
        (MediaKind::Supplement, DetailKind::Supplement, true),
        (MediaKind::Series, DetailKind::Series, false),
        (MediaKind::Collection, DetailKind::Collection, false),
        (MediaKind::Category, DetailKind::Category, false),
        (MediaKind::Franchise, DetailKind::Franchise, false),
        (MediaKind::Live, DetailKind::Live, false),
    ] {
        let presentation = Presentation::native_detail(detail(kind), None).unwrap();
        presentation.with_view(LoginView::SignedOut, |view| {
            let native = view.detail.as_ref().unwrap();
            assert_eq!(native.kind, displayed);
            assert_eq!(native.card.key.media_id().unwrap().as_str(), "Root0001");
            assert_eq!(
                native.primary_playback_target.map(MediaId::as_str),
                playable.then_some("Root0001")
            );
        });
    }
}

#[test]
fn description_uses_first_length_positive_variant_without_trimming_or_editorial_substitution() {
    for (long, medium, base, expected) in [
        (Some(""), Some("Medium"), Some("Base"), "Medium"),
        (None, Some(""), Some("Base"), "Base"),
        (Some("  "), Some("Medium"), Some("Base"), "  "),
        (Some(""), Some(""), Some(""), ""),
        (None, None, None, ""),
    ] {
        let mut source = detail(MediaKind::Film);
        source.metadata.description_long = long.map(String::from);
        source.metadata.description_medium = medium.map(String::from);
        source.metadata.description = base.map(String::from);
        source.metadata.description_staff = Some("Staff description".into());
        source.metadata.description_pull_quote = Some("Pull quote".into());
        let presentation = Presentation::native_detail(source, None).unwrap();
        presentation.with_view(LoginView::SignedOut, |view| {
            assert_eq!(view.detail.as_ref().unwrap().description, expected);
        });
    }
}

#[test]
fn series_displays_one_episodes_tab_from_first_seasons_before_ordered_generic_tabs() {
    let presentation = Presentation::native_detail(series(), None).unwrap();
    presentation.with_view(LoginView::SignedOut, |view| {
        let native = view.detail.as_ref().unwrap();
        assert_eq!(native.kind, DetailKind::Series);
        assert_eq!(
            native.primary_playback_target.map(MediaId::as_str),
            Some("Ep000001")
        );
        assert_eq!(native.selected_playlist, Some(0));
        assert_eq!(
            view.rails.iter().map(|rail| rail.title).collect::<Vec<_>>(),
            ["Episodes", "Related", "Supplements"]
        );
        assert_eq!(
            view.rails[0]
                .cards
                .iter()
                .map(|card| card.key.media_id().unwrap().as_str())
                .collect::<Vec<_>>(),
            ["Ep000001", "Ep000002"]
        );
        assert!(view.rails[1].cards.is_empty());
        assert!(view.rails[2].cards.is_empty());
        let selector = native.seasons.as_ref().unwrap();
        assert_eq!(selector.selected, 0);
        assert_eq!(
            selector
                .choices
                .iter()
                .map(|choice| (choice.number, choice.title, choice.episode_count))
                .collect::<Vec<_>>(),
            [(20, "Volume B", 2), (3, "Volume A", 1)]
        );
    });
    assert!(!presentation.artwork_bindings().iter().any(|binding| matches!(&binding.source, super::super::ImageSource::Media { id, .. } if id.as_str() == "Ep999999")));
}

#[test]
fn season_selection_changes_only_active_episodes_and_survives_a_generic_tab_visit() {
    let mut presentation = Presentation::native_detail(series(), None).unwrap();
    let bytes = presentation.estimated_bytes();
    presentation.select_native_season(1);
    presentation.with_view(LoginView::SignedOut, |view| {
        let native = view.detail.as_ref().unwrap();
        assert_eq!(native.seasons.as_ref().unwrap().selected, 1);
        assert_eq!(
            view.rails[0].cards[0].key.media_id().unwrap().as_str(),
            "Ep000003"
        );
        assert_eq!(
            native.primary_playback_target.map(MediaId::as_str),
            Some("Ep000001")
        );
    });
    presentation.select_native_season(2);
    presentation.select_playlist(1);
    presentation.select_native_season(0);
    presentation.with_view(LoginView::SignedOut, |view| {
        assert!(view.detail.as_ref().unwrap().seasons.is_none());
        assert!(view.rails[0].cards.is_empty());
        assert_eq!(
            view.rails[1].cards[0].key.media_id().unwrap().as_str(),
            "Child001"
        );
    });
    presentation.select_playlist(0);
    presentation.with_view(LoginView::SignedOut, |view| {
        assert_eq!(
            view.detail
                .as_ref()
                .unwrap()
                .seasons
                .as_ref()
                .unwrap()
                .selected,
            1
        );
        assert_eq!(
            view.rails[0].cards[0].key.media_id().unwrap().as_str(),
            "Ep000003"
        );
    });
    assert_eq!(presentation.estimated_bytes(), bytes);
}

#[test]
fn native_child_activation_uses_exact_episode_id_and_never_public_target_meaning() {
    let kinds = [
        MediaKind::Film,
        MediaKind::Original,
        MediaKind::Supplement,
        MediaKind::Series,
        MediaKind::Category,
        MediaKind::Collection,
        MediaKind::Franchise,
        MediaKind::Episode,
        MediaKind::Live,
    ];
    let mut source = detail(MediaKind::Collection);
    let children = kinds
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            let mut child = media(&format!("Child{:03}", index + 1), "Native child", kind);
            if kind == MediaKind::Episode {
                child.series_id = Some(MediaId::new("Series01").unwrap());
                child.series_title = Some("Episode source series".into());
            }
            child
        })
        .collect();
    source.playlists.push(generic("Native children", children));
    let presentation = Presentation::native_detail(source, None).unwrap();
    for (index, kind) in kinds.into_iter().enumerate() {
        let id = MediaId::new(&format!("Child{:03}", index + 1)).unwrap();
        let expected = match kind {
            MediaKind::Episode => NativeActivation::Play { id: id.clone() },
            MediaKind::Live => NativeActivation::Unsupported,
            _ => NativeActivation::Detail {
                id: id.clone(),
                auto_play: false,
            },
        };
        assert_eq!(
            presentation.native_activation(&Target::Native(id.clone())),
            Some(expected)
        );
        assert_eq!(presentation.native_activation(&Target::Media(id)), None);
    }
    presentation.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.rails[0].cards[7].action, CardAction::Play);
        assert_eq!(
            view.rails[0].cards[7].key.media_id().unwrap().as_str(),
            "Child008"
        );
        assert!(
            view.rails[0]
                .cards
                .iter()
                .all(|card| card.saved_fraction.is_none() && card.duration_label.is_none())
        );
    });
}

#[test]
fn root_joins_only_full_metadata_and_binds_exact_landscape_backdrop_and_child_card() {
    let mut source = detail(MediaKind::Film);
    source.metadata.director = Some(vec!["Director A".into(), "Director B".into()]);
    source.metadata.starring = Some(vec!["Actor A".into(), "Actor B".into()]);
    source.metadata.country = Some(vec!["Country A".into(), "Country B".into()]);
    source.metadata.language = Some(vec!["Language A".into(), "Language B".into()]);
    source.media.release_date =
        Some(time::Date::from_calendar_date(1980, time::Month::July, 1).unwrap());
    source.media.duration = Some(93.5);
    let mut child = media("Child001", "Summary title", MediaKind::Original);
    child.duration = Some(45.25);
    source.playlists.push(generic("Related", vec![child]));
    let presentation = Presentation::native_detail(source, None).unwrap();
    presentation.with_view(LoginView::SignedOut, |view| {
        let native = view.detail.as_ref().unwrap();
        assert_eq!(
            (
                native.directors,
                native.starring,
                native.countries,
                native.languages
            ),
            (
                "Director A, Director B",
                Some("Actor A, Actor B"),
                Some("Country A, Country B"),
                Some("Language A, Language B")
            )
        );
        assert_eq!(native.card.year, "1980");
        assert_eq!(native.card.duration_label, Some("1 min"));
        assert_eq!(view.rails[0].cards[0].title, "Summary title");
        assert_eq!(view.rails[0].cards[0].duration_label, None);
    });
    let bindings = presentation.artwork_bindings();
    assert_eq!(bindings.len(), 2);
    for (id, role) in [
        ("Root0001", criterion_artwork::ImageRole::Backdrop),
        ("Child001", criterion_artwork::ImageRole::Card),
    ] {
        assert!(bindings.iter().any(|binding| binding.source
            == super::super::ImageSource::Media {
                id: MediaId::new(id).unwrap(),
                label: criterion_provider::ImageLabel::Landscape,
                role
            }));
    }
}

#[test]
fn raw_duplicate_cardinality_is_bounded_before_selecting_display_groups() {
    let mut source = detail(MediaKind::Collection);
    let mut playlist = match generic(
        "Duplicates",
        vec![media("Child001", "Admitted first child", MediaKind::Film)],
    ) {
        NativePlaylist::Generic(value) => value,
        _ => unreachable!(),
    };
    playlist.raw_child_count = 512;
    source.playlists.push(NativePlaylist::Generic(playlist));
    assert!(matches!(
        Presentation::native_detail(source, None),
        Err(super::super::ProjectionLimit::TooLarge)
    ));
}

#[test]
fn oversized_input_allocation_refuses_even_for_discarded_metadata() {
    let mut source = detail(MediaKind::Film);
    let mut oversized = String::with_capacity(512 * 1024);
    oversized.push('x');
    source.metadata.deeplink = Some(oversized);
    assert!(source.estimated_bytes() > 512 * 1024);
    assert!(matches!(
        Presentation::native_detail(source, None),
        Err(super::super::ProjectionLimit::TooLarge)
    ));
}

#[test]
fn final_retained_projection_budget_includes_both_owned_root_title_uses() {
    let mut source = detail(MediaKind::Film);
    source.media.title = "T".repeat(256 * 1024);
    assert!(source.estimated_bytes() < 512 * 1024);
    assert!(matches!(
        Presentation::native_detail(source, None),
        Err(super::super::ProjectionLimit::TooLarge)
    ));
}

#[test]
fn dormant_season_title_and_episode_allocations_remain_metered_without_view_clones() {
    let mut source = series();
    let NativePlaylist::Seasons(first) = &mut source.playlists[1] else {
        unreachable!()
    };
    let mut dormant_title = String::with_capacity(32 * 1024);
    dormant_title.push('s');
    first.seasons[1].title = dormant_title;
    let mut dormant_episode = String::with_capacity(256 * 1024);
    dormant_episode.push('e');
    first.seasons[1].episodes[0].title = dormant_episode;
    let mut presentation = Presentation::native_detail(source, None).unwrap();
    let bytes = presentation.estimated_bytes();
    assert!(bytes >= 288 * 1024);
    presentation.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.rails[0].cards[0].title, "Episode one");
    });
    presentation.select_native_season(1);
    presentation.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.rails[0].cards[0].title, "e");
        assert_eq!(
            view.detail
                .as_ref()
                .unwrap()
                .seasons
                .as_ref()
                .unwrap()
                .choices[1]
                .title,
            "s"
        );
    });
    assert_eq!(presentation.estimated_bytes(), bytes);
}

#[test]
fn equal_native_ids_take_activation_only_from_the_active_group_and_season() {
    let mut source = detail(MediaKind::Series);
    source.playlists = vec![
        seasons(vec![
            season(
                1,
                "First",
                vec![media(
                    "Shared01",
                    "Episode with shared ID",
                    MediaKind::Episode,
                )],
            ),
            season(
                2,
                "Second",
                vec![media("Unique01", "Dormant episode", MediaKind::Episode)],
            ),
        ]),
        generic(
            "Related",
            vec![media("Shared01", "Film with shared ID", MediaKind::Film)],
        ),
    ];
    let mut presentation = Presentation::native_detail(source, None).unwrap();
    let shared = Target::Native(MediaId::new("Shared01").unwrap());
    let unique = Target::Native(MediaId::new("Unique01").unwrap());
    assert_eq!(
        presentation.native_activation(&shared),
        Some(NativeActivation::Play {
            id: MediaId::new("Shared01").unwrap()
        })
    );
    assert_eq!(presentation.native_activation(&unique), None);
    presentation.select_playlist(1);
    assert_eq!(
        presentation.native_activation(&shared),
        Some(NativeActivation::Detail {
            id: MediaId::new("Shared01").unwrap(),
            auto_play: false
        })
    );
    assert_eq!(presentation.native_activation(&unique), None);
    presentation.select_playlist(0);
    presentation.select_native_season(1);
    assert_eq!(presentation.native_activation(&shared), None);
    assert_eq!(
        presentation.native_activation(&unique),
        Some(NativeActivation::Play {
            id: MediaId::new("Unique01").unwrap()
        })
    );
}

#[test]
fn cardinality_boundaries_include_root_all_groups_and_ignored_later_seasons() {
    for (raw_count, accepted) in [(511, true), (512, false), (usize::MAX, false)] {
        let mut source = detail(MediaKind::Collection);
        let NativePlaylist::Generic(mut playlist) =
            generic("First", vec![media("Child001", "First", MediaKind::Film)])
        else {
            unreachable!()
        };
        playlist.raw_child_count = raw_count;
        source.playlists.push(NativePlaylist::Generic(playlist));
        assert_eq!(Presentation::native_detail(source, None).is_ok(), accepted);
    }
    for (group_count, accepted) in [(32, true), (33, false)] {
        let mut source = detail(MediaKind::Category);
        source.playlists = (0..group_count).map(|_| generic("", Vec::new())).collect();
        assert_eq!(Presentation::native_detail(source, None).is_ok(), accepted);
    }
    for (season_count, accepted) in [(64, true), (65, false)] {
        let mut source = detail(MediaKind::Series);
        source.playlists = vec![seasons(
            (0..64)
                .map(|number| season(number, "Season", Vec::new()))
                .collect(),
        )];
        if season_count == 65 {
            source
                .playlists
                .push(seasons(vec![season(65, "Ignored group", Vec::new())]));
        }
        assert_eq!(Presentation::native_detail(source, None).is_ok(), accepted);
    }
}

#[test]
fn raw_count_cannot_hide_actual_children_or_discarded_featured_and_season_rows() {
    let mut source = detail(MediaKind::Collection);
    let children = (0..512)
        .map(|index| media(&format!("Child{index:03}"), "Native child", MediaKind::Film))
        .collect();
    let NativePlaylist::Generic(mut playlist) = generic("Actual children", children) else {
        unreachable!()
    };
    playlist.raw_child_count = 0;
    source.playlists.push(NativePlaylist::Generic(playlist));
    assert!(matches!(
        Presentation::native_detail(source, None),
        Err(super::super::ProjectionLimit::TooLarge)
    ));
    let mut source = detail(MediaKind::Collection);
    source.featured = Some(criterion_account::NativeFeatured {
        title: None,
        children: Vec::new(),
        raw_child_count: 511,
    });
    source.playlists.push(generic(
        "One more",
        vec![media("Child001", "Child", MediaKind::Film)],
    ));
    assert!(matches!(
        Presentation::native_detail(source, None),
        Err(super::super::ProjectionLimit::TooLarge)
    ));
    let mut source = detail(MediaKind::Series);
    let mut ignored = season(2, "Ignored season", Vec::new());
    ignored.raw_episode_count = 512;
    source.playlists = vec![seasons(Vec::new()), seasons(vec![ignored])];
    assert!(matches!(
        Presentation::native_detail(source, None),
        Err(super::super::ProjectionLimit::TooLarge)
    ));
}

#[test]
fn missing_series_entries_never_invent_primary_episode_or_consult_later_seasons() {
    for playlists in [
        Vec::new(),
        vec![generic("Generic only", Vec::new())],
        vec![seasons(Vec::new())],
        vec![seasons(vec![
            season(1, "Empty first", Vec::new()),
            season(
                2,
                "Later",
                vec![media("Ep000002", "Later episode", MediaKind::Episode)],
            ),
        ])],
        vec![
            seasons(Vec::new()),
            seasons(vec![season(
                2,
                "Later group",
                vec![media("Ep000002", "Later episode", MediaKind::Episode)],
            )]),
        ],
    ] {
        let mut source = detail(MediaKind::Series);
        source.playlists = playlists;
        let mut presentation = Presentation::native_detail(source, None).unwrap();
        presentation.select_native_season(usize::MAX);
        presentation.with_view(LoginView::SignedOut, |view| {
            assert!(
                view.detail
                    .as_ref()
                    .unwrap()
                    .primary_playback_target
                    .is_none()
            );
            assert!(view.rails.iter().all(|rail| rail.cards.is_empty()));
        });
    }
}

#[test]
fn franchise_generic_titles_and_children_keep_source_order_without_kind_coercion() {
    let mut source = detail(MediaKind::Franchise);
    source.playlists = vec![
        generic(
            "Second editorial title",
            vec![media("Child002", "Native Original", MediaKind::Original)],
        ),
        seasons(vec![season(
            1,
            "Not a Franchise tab",
            vec![media("Ep000001", "Ignored episode", MediaKind::Episode)],
        )]),
        generic(
            "First editorial title",
            vec![media(
                "Child001",
                "Native Supplement",
                MediaKind::Supplement,
            )],
        ),
    ];
    let mut presentation = Presentation::native_detail(source, None).unwrap();
    presentation.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.detail.as_ref().unwrap().kind, DetailKind::Franchise);
        assert!(
            view.detail
                .as_ref()
                .unwrap()
                .primary_playback_target
                .is_none()
        );
        assert_eq!(
            view.rails.iter().map(|rail| rail.title).collect::<Vec<_>>(),
            ["Second editorial title", "First editorial title"]
        );
        assert_eq!(
            view.rails[0].cards[0].key.media_id().unwrap().as_str(),
            "Child002"
        );
    });
    presentation.select_playlist(1);
    presentation.with_view(LoginView::SignedOut, |view| {
        assert_eq!(
            view.rails[1].cards[0].key.media_id().unwrap().as_str(),
            "Child001"
        );
        assert!(view.detail.as_ref().unwrap().seasons.is_none());
    });
}

#[test]
fn retained_byte_limit_accepts_exactly_512_kib_and_refuses_one_byte_more() {
    let baseline = Presentation::native_detail(detail(MediaKind::Film), None)
        .unwrap()
        .estimated_bytes();
    for (extra, accepted) in [(0, true), (1, false)] {
        let mut source = detail(MediaKind::Film);
        let mut description = String::with_capacity(512 * 1024 - baseline + extra);
        description.push('d');
        source.metadata.description_long = Some(description);
        assert!(source.estimated_bytes() < 512 * 1024);
        let result = Presentation::native_detail(source, None);
        assert_eq!(result.is_ok(), accepted);
        if let Ok(presentation) = result {
            assert_eq!(presentation.estimated_bytes(), 512 * 1024);
            presentation.with_view(LoginView::SignedOut, |view| {
                assert_eq!(view.detail.as_ref().unwrap().description, "d")
            });
        }
    }
}

#[test]
fn series_primary_rejects_first_episode_id_equal_to_root_without_selecting_a_later_episode() {
    let mut source = detail(MediaKind::Series);
    source.playlists = vec![seasons(vec![season(
        1,
        "First season",
        vec![
            media("Root0001", "Episode sharing root ID", MediaKind::Episode),
            media("Ep000002", "Later episode", MediaKind::Episode),
        ],
    )])];
    let presentation = Presentation::native_detail(source, None).unwrap();
    presentation.with_view(LoginView::SignedOut, |view| {
        let native = view.detail.as_ref().unwrap();
        assert_eq!(native.kind, DetailKind::Series);
        assert_eq!(native.card.key.media_id().unwrap().as_str(), "Root0001");
        assert_eq!(native.primary_playback_target.map(MediaId::as_str), None);
        assert_eq!(
            view.rails[0]
                .cards
                .iter()
                .map(|card| card.key.media_id().unwrap().as_str())
                .collect::<Vec<_>>(),
            ["Root0001", "Ep000002"]
        );
        assert_eq!(view.rails[0].cards[0].action, CardAction::Play);
    });
    assert_eq!(
        presentation.native_activation(&Target::Native(MediaId::new("Root0001").unwrap())),
        Some(NativeActivation::Play {
            id: MediaId::new("Root0001").unwrap()
        })
    );
}

#[test]
fn native_information_preserves_absence_and_present_empty_metadata() {
    for (items, warnings, expected) in [
        (None, None, None),
        (Some(Vec::new()), None, None),
        (Some(vec![String::new()]), Some(String::new()), Some("")),
        (
            Some(vec!["First".into(), "Second".into()]),
            Some("Flashing lights".into()),
            Some("First, Second"),
        ),
    ] {
        let mut source = detail(MediaKind::Film);
        source.metadata.starring = items.clone();
        source.metadata.country = items.clone();
        source.metadata.language = items;
        source.metadata.content_warnings = warnings.clone();
        let presentation = Presentation::native_detail(source, None).unwrap();
        presentation.with_view(LoginView::SignedOut, |view| {
            let native = view.detail.as_ref().unwrap();
            assert_eq!(native.starring, expected);
            assert_eq!(native.countries, expected);
            assert_eq!(native.languages, expected);
            assert_eq!(native.content_warnings, warnings.as_deref());
        });
    }
}

#[test]
fn native_root_and_child_years_use_unpadded_calendar_years() {
    for (year, expected) in [(85, "85"), (0, "0"), (-5, "-5"), (1986, "1986")] {
        let mut source = detail(MediaKind::Film);
        let date = time::Date::from_calendar_date(year, time::Month::January, 1).unwrap();
        source.media.release_date = Some(date);
        let mut child = media("Child001", "Native child", MediaKind::Original);
        child.release_date = Some(date);
        source.playlists.push(generic("Related", vec![child]));
        let presentation = Presentation::native_detail(source, None).unwrap();
        presentation.with_view(LoginView::SignedOut, |view| {
            assert_eq!(view.detail.as_ref().unwrap().card.year, expected);
            assert_eq!(view.rails[0].cards[0].year, expected);
        });
    }
}

#[test]
fn native_runtime_and_year_have_distinct_header_and_information_subtype_gates() {
    for (kind, header, information) in [
        (MediaKind::Film, "85   1h 59m", "85   1h 59m"),
        (MediaKind::Original, "85   1h 59m", "85   1h 59m"),
        (MediaKind::Supplement, "85   1h 59m", "85   1h 59m"),
        (MediaKind::Episode, "", "85   1h 59m"),
        (MediaKind::Series, "85", "85"),
        (MediaKind::Category, "", ""),
        (MediaKind::Collection, "", ""),
        (MediaKind::Franchise, "", ""),
        (MediaKind::Live, "", ""),
    ] {
        let mut source = detail(kind);
        source.media.duration = Some(7199.0);
        source.media.release_date =
            Some(time::Date::from_calendar_date(85, time::Month::January, 1).unwrap());
        let mut child = media("Child001", "Child runtime", MediaKind::Film);
        child.duration = Some(3600.0);
        source.playlists.push(generic("Related", vec![child]));
        let presentation = Presentation::native_detail(source, None).unwrap();
        presentation.with_view(LoginView::SignedOut, |view| {
            let native = view.detail.as_ref().unwrap();
            assert_eq!(native.header_metadata, header, "{kind:?}");
            assert_eq!(native.information_metadata, information, "{kind:?}");
            assert_eq!(
                native.card.duration_label,
                matches!(
                    kind,
                    MediaKind::Film | MediaKind::Supplement | MediaKind::Episode
                )
                .then_some("1 h 59 min")
            );
            assert_eq!(view.rails[0].cards[0].duration_label, Some("1 h 0 min"));
        });
    }
}

#[test]
fn native_runtime_absence_preserves_year_without_inventing_zero() {
    for kind in [
        MediaKind::Film,
        MediaKind::Original,
        MediaKind::Supplement,
        MediaKind::Episode,
    ] {
        for (duration, header, information) in
            [(None, "85", "85"), (Some(0.0), "85   0m", "85   0m")]
        {
            let mut source = detail(kind);
            source.media.duration = duration;
            source.media.release_date =
                Some(time::Date::from_calendar_date(85, time::Month::January, 1).unwrap());
            let presentation = Presentation::native_detail(source, None).unwrap();
            presentation.with_view(LoginView::SignedOut, |view| {
                let native = view.detail.as_ref().unwrap();
                assert_eq!(
                    native.header_metadata,
                    if kind == MediaKind::Episode {
                        ""
                    } else {
                        header
                    }
                );
                assert_eq!(native.information_metadata, information);
            });
        }
    }
}

#[test]
fn both_owned_runtime_lines_charge_reserved_capacity_to_retained_budget() {
    let mut source = detail(MediaKind::Film);
    source.media.duration = Some(7199.0);
    let mut presentation = Presentation::native_detail(source, None).unwrap();
    {
        let native = presentation.detail.as_mut().unwrap();
        native.header_metadata.clear();
        native.header_metadata.shrink_to_fit();
        native.information_metadata.clear();
        native.information_metadata.shrink_to_fit();
    }
    let baseline = presentation.estimated_bytes();
    {
        let native = presentation.detail.as_mut().unwrap();
        native.header_metadata = String::with_capacity(8192);
        native.header_metadata.push_str("1h 59m");
        native.information_metadata = String::with_capacity(12288);
        native.information_metadata.push_str("1h 59m");
    }
    assert_eq!(presentation.estimated_bytes() - baseline, 20480);
    presentation.with_view(LoginView::SignedOut, |view| {
        let native = view.detail.as_ref().unwrap();
        assert_eq!(native.header_metadata, "1h 59m");
        assert_eq!(native.information_metadata, "1h 59m");
    });
}

#[test]
fn native_runtime_lines_are_included_before_exact_projection_admission() {
    let source = || {
        let mut source = detail(MediaKind::Film);
        source.media.duration = Some(f32::MAX);
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
                let native = view.detail.as_ref().unwrap();
                assert_eq!(native.header_metadata, "1011703407h 30m");
                assert_eq!(native.information_metadata, "0m");
            });
        }
    }
}

#[path = "card_runtime_tests.rs"]
mod card_runtime_tests;
