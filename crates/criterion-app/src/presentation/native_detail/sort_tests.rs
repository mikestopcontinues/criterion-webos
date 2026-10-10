//! Literal source-derived orders through the real native projection/view seam.
use super::*;
use criterion_ui::{DetailSortAction as Action, DetailSortField as Field};

fn source(children: Vec<MediaSummary>) -> NativeDetail {
    let mut source = detail(MediaKind::Collection);
    source.is_first_tab_sortable = Some(true);
    source.playlists.push(generic("First", children));
    source
}
fn apply(view: &mut Presentation, field: Field, descending: bool) {
    view.native_sort_action(Action::Open);
    // Start each request from Default so this helper specifies a direction rather than toggling prior state.
    view.native_sort_action(Action::Choose(Field::Default));
    view.native_sort_action(Action::Choose(field));
    if descending {
        view.native_sort_action(Action::Choose(field));
    }
    view.native_sort_action(Action::Apply);
}
fn order(view: &Presentation) -> Vec<String> {
    view.with_view(LoginView::SignedOut, |data| {
        data.rails[0]
            .cards
            .iter()
            .map(|card| card.key.media_id().unwrap().as_str().to_owned())
            .collect()
    })
}
fn children(titles: &[&str]) -> Vec<MediaSummary> {
    titles
        .iter()
        .enumerate()
        .map(|(index, title)| media(&format!("Sort{index:04}"), title, MediaKind::Film))
        .collect()
}

#[test]
fn only_exact_eligible_root_true_flag_and_first_raw_generic_offer_sorting_including_empty() {
    for kind in [
        MediaKind::Category,
        MediaKind::Collection,
        MediaKind::Film,
        MediaKind::Supplement,
        MediaKind::Series,
        MediaKind::Original,
        MediaKind::Episode,
        MediaKind::Franchise,
        MediaKind::Live,
    ] {
        for flag in [None, Some(false), Some(true)] {
            let mut source = source(vec![]);
            source.media.kind = kind;
            source.is_first_tab_sortable = flag;
            let view = Presentation::native_detail(source, None).unwrap();
            assert_eq!(
                view.native_sort_view().is_some(),
                flag == Some(true)
                    && matches!(
                        kind,
                        MediaKind::Category
                            | MediaKind::Collection
                            | MediaKind::Film
                            | MediaKind::Supplement
                    ),
                "{kind:?}/{flag:?}"
            );
        }
    }
    let mut supplied = source(children(&["Later"]));
    supplied.playlists.insert(
        0,
        NativePlaylist::Seasons(NativeSeasonsPlaylist {
            title: "First raw Seasons".into(),
            seasons: vec![],
        }),
    );
    let view = Presentation::native_detail(supplied, None).unwrap();
    assert!(
        view.native_sort_view().is_none(),
        "never skip the first raw Seasons to a later Generic"
    );
}

#[test]
fn full_title_lowercase_utf16_stable_descending_and_default_have_literal_orders() {
    let mut view =
        Presentation::native_detail(source(children(&["bETA", "ALPHA", "beta", "Beta"])), None)
            .unwrap();
    apply(&mut view, Field::Title, false);
    assert_eq!(
        order(&view),
        ["Sort0001", "Sort0000", "Sort0002", "Sort0003"]
    );
    apply(&mut view, Field::Title, true);
    assert_eq!(
        order(&view),
        ["Sort0000", "Sort0002", "Sort0003", "Sort0001"]
    );
    apply(&mut view, Field::Default, false);
    assert_eq!(
        order(&view),
        ["Sort0000", "Sort0001", "Sort0002", "Sort0003"]
    );

    let mut view = Presentation::native_detail(
        source(children(&[
            "\u{e000}",
            "\u{10000}",
            "İ",
            "i\u{307}",
            "ΟΣ",
            "ΟΣΑ",
        ])),
        None,
    )
    .unwrap();
    apply(&mut view, Field::Title, false);
    assert_eq!(
        order(&view),
        [
            "Sort0002", "Sort0003", "Sort0004", "Sort0005", "Sort0001", "Sort0000"
        ]
    );
    let prefix = "Same complete prefix ".repeat(20);
    let mut view = Presentation::native_detail(
        source(children(&[&format!("{prefix}z"), &format!("{prefix}a")])),
        None,
    )
    .unwrap();
    apply(&mut view, Field::Title, false);
    assert_eq!(
        order(&view),
        ["Sort0001", "Sort0000"],
        "the title key must extend beyond painted elision"
    );
    let mut view = Presentation::native_detail(
        source(children(&[
            "ß", "ss", "ẞ", "Film 2", "Film 10", "é", "e\u{301}",
        ])),
        None,
    )
    .unwrap();
    apply(&mut view, Field::Title, false);
    assert_eq!(
        order(&view),
        [
            "Sort0006", "Sort0004", "Sort0003", "Sort0001", "Sort0000", "Sort0002", "Sort0005"
        ],
        "no casefold, natural numeric ordering or Unicode normalization"
    );
}

#[test]
fn full_date_uses_only_native_dated_subtypes_and_reverses_missing_direction() {
    let mut items = children(&[
        "Film May",
        "Supplement January",
        "Series December",
        "Episode excluded",
        "Original excluded",
    ]);
    items[0].release_date =
        Some(time::Date::from_calendar_date(2000, time::Month::May, 1).unwrap());
    for (index, kind, year, month) in [
        (1, MediaKind::Supplement, 2000, time::Month::January),
        (2, MediaKind::Series, 1999, time::Month::December),
        (3, MediaKind::Episode, 1990, time::Month::January),
        (4, MediaKind::Original, 1980, time::Month::January),
    ] {
        items[index].kind = kind;
        items[index].release_date = Some(time::Date::from_calendar_date(year, month, 1).unwrap());
    }
    let mut view = Presentation::native_detail(source(items), None).unwrap();
    apply(&mut view, Field::ReleaseDate, false);
    assert_eq!(
        order(&view),
        ["Sort0002", "Sort0001", "Sort0000", "Sort0003", "Sort0004"]
    );
    view.with_view(LoginView::SignedOut, |data| {
        assert_eq!(data.rails[0].cards[1].year, data.rails[0].cards[2].year)
    });
    apply(&mut view, Field::ReleaseDate, true);
    assert_eq!(
        order(&view),
        ["Sort0003", "Sort0004", "Sort0000", "Sort0001", "Sort0002"]
    );
}

#[test]
fn signed_runtime_uses_saturated_seconds_default_zero_and_fresh_source_ties() {
    let mut items = children(&[
        "unsupported Original",
        "zero Episode",
        "z fraction Film",
        "a fraction Supplement",
        "unsupported Series",
        "saturated Film",
    ]);
    items[0].kind = MediaKind::Original;
    items[0].duration = Some(1.0);
    items[1].kind = MediaKind::Episode;
    items[2].duration = Some(90.9);
    items[3].kind = MediaKind::Supplement;
    items[3].duration = Some(90.1);
    items[4].kind = MediaKind::Series;
    items[4].duration = Some(2.0);
    items[5].duration = Some(f32::MAX);
    let mut view = Presentation::native_detail(source(items), None).unwrap();
    apply(&mut view, Field::Title, false);
    apply(&mut view, Field::Runtime, false);
    assert_eq!(
        order(&view),
        [
            "Sort0001", "Sort0002", "Sort0003", "Sort0005", "Sort0000", "Sort0004"
        ]
    );
    view.with_view(LoginView::SignedOut, |data| {
        assert_eq!(
            data.rails[0].cards[0].duration_label, None,
            "the sort key default must not invent a caption"
        )
    });
    apply(&mut view, Field::Runtime, true);
    assert_eq!(
        order(&view),
        [
            "Sort0000", "Sort0004", "Sort0005", "Sort0002", "Sort0003", "Sort0001"
        ]
    );
}

#[test]
fn runtime_direct_int32_saturation_keeps_large_ties_and_zero_caption_absence_distinct() {
    let mut items = children(&[
        "missing",
        "zero",
        "fraction",
        "2pow31",
        "2pow32",
        "finite max",
    ]);
    for (item, duration) in items.iter_mut().zip([
        None,
        Some(0.0),
        Some(7199.75),
        Some(2_147_483_648.0),
        Some(4_294_967_296.0),
        Some(f32::MAX),
    ]) {
        item.duration = duration;
    }
    let mut view = Presentation::native_detail(source(items), None).unwrap();
    apply(&mut view, Field::Runtime, false);
    assert_eq!(
        order(&view),
        [
            "Sort0000", "Sort0001", "Sort0002", "Sort0003", "Sort0004", "Sort0005"
        ]
    );
    view.with_view(LoginView::SignedOut, |data| {
        assert_eq!(data.rails[0].cards[0].duration_label, None);
        assert_eq!(data.rails[0].cards[1].duration_label, Some("0 min"));
        assert_eq!(data.rails[0].cards[2].duration_label, Some("1 h 59 min"));
    });
    apply(&mut view, Field::Runtime, true);
    assert_eq!(
        order(&view),
        [
            "Sort0003", "Sort0004", "Sort0005", "Sort0002", "Sort0000", "Sort0001"
        ]
    );
}

#[test]
fn sorted_display_address_resolves_its_owned_action_and_keeps_feature_and_tail_separate() {
    let mut items = children(&["z Film", "a Episode"]);
    items[1].kind = MediaKind::Episode;
    let mut supplied = source(items);
    supplied.featured = Some(criterion_account::NativeFeatured {
        title: Some("Feature".into()),
        children: vec![media("Sort0001", "Same-ID Feature Film", MediaKind::Film)],
        raw_child_count: 1,
    });
    supplied.playlists.push(generic(
        "Tail",
        vec![media("Tail0001", "Tail unchanged", MediaKind::Film)],
    ));
    let mut view = Presentation::native_detail(supplied, None).unwrap();
    apply(&mut view, Field::Title, false);
    let target = Target::Native(MediaId::new("Sort0001").unwrap());
    assert!(
        matches!(view.selected_card_action(criterion_ui::Page::Detail, criterion_ui::Focus::Card { row: 0, column: 0 }, &target), Some(Some(NativeActivation::Play { id })) if id.as_str() == "Sort0001")
    );
    assert!(matches!(
        view.selected_card_action(
            criterion_ui::Page::Detail,
            criterion_ui::Focus::FeaturedCard(0),
            &target
        ),
        Some(Some(NativeActivation::Detail { .. }))
    ));
    assert!(
        view.selected_card_action(
            criterion_ui::Page::Detail,
            criterion_ui::Focus::Card { row: 0, column: 1 },
            &target
        )
        .is_none()
    );
    view.select_playlist(1);
    assert!(
        view.selected_card_action(
            criterion_ui::Page::Detail,
            criterion_ui::Focus::Card { row: 0, column: 0 },
            &target
        )
        .is_none()
    );
    view.with_view(LoginView::SignedOut, |data| {
        assert_eq!(data.rails[1].cards[0].title, "Tail unchanged")
    });
}

#[test]
fn sort_keys_charge_real_capacities_and_share_the_whole_native_projection_budget() {
    let items = children(&["İ", "ΟΣ", "\u{10000}"]);
    let without = {
        let mut value = source(items.clone());
        value.is_first_tab_sortable = None;
        Presentation::native_detail(value, None).unwrap()
    };
    let mut with = Presentation::native_detail(source(items), None).unwrap();
    let before = with.estimated_bytes();
    assert!(before > without.estimated_bytes() + 3 * std::mem::size_of::<usize>());
    apply(&mut with, Field::Title, true);
    assert_eq!(
        with.estimated_bytes(),
        before,
        "Apply must reuse its retained keys/order allocation"
    );
    let items = (0..160)
        .map(|index| {
            media(
                &format!("Sort{index:04}"),
                &"İ".repeat(512),
                MediaKind::Film,
            )
        })
        .collect();
    let supplied = source(items);
    assert!(supplied.estimated_bytes() < 512 * 1024);
    assert!(
        Presentation::native_detail(supplied, None).is_err(),
        "normalized keys and cards must fit together within the existing512KiB"
    );
}
