// SPDX-License-Identifier: GPL-3.0-or-later
//! Native account display data; request and session ownership remain in the runtime.
use super::{ImageSource, OwnedCard, Presentation};
use crate::my_list::{MyListState, Tail};
use criterion_account::WatchListFilter;
use criterion_provider::ImageLabel;
use criterion_ui::{CatalogTail, CatalogWindow, LoadState, MyListChoice, MyListGroup, Target};

pub(crate) fn my_list_filter(group: MyListGroup) -> WatchListFilter {
    match group {
        MyListGroup::All => WatchListFilter::All,
        MyListGroup::FilmsAndSeries => WatchListFilter::FilmSeries,
        MyListGroup::Collections => WatchListFilter::Collection,
        MyListGroup::OriginalsAndFranchises => WatchListFilter::OriginalFranchise,
        MyListGroup::Supplements => WatchListFilter::Supplement,
        MyListGroup::Categories => WatchListFilter::Category,
    }
}

fn group_for(filter: WatchListFilter) -> MyListGroup {
    match filter {
        WatchListFilter::All => MyListGroup::All,
        WatchListFilter::FilmSeries => MyListGroup::FilmsAndSeries,
        WatchListFilter::Collection => MyListGroup::Collections,
        WatchListFilter::OriginalFranchise => MyListGroup::OriginalsAndFranchises,
        WatchListFilter::Supplement => MyListGroup::Supplements,
        WatchListFilter::Category => MyListGroup::Categories,
    }
}

impl Presentation {
    pub(crate) fn my_list(state: &MyListState) -> Self {
        let view = state.view();
        let mut presentation = Self::loading("My List");
        // The window end is geometry, never provider EOF or a request limit.
        presentation.total =
            u32::try_from(view.first + view.rows.len()).expect("bounded native key ordinals");
        presentation.my_list_selected = Some(group_for(view.filter));
        presentation.my_list_choices = state
            .groups()
            .into_iter()
            .filter(|group| group.available || group.filter == view.filter)
            .map(|group| MyListChoice {
                group: group_for(group.filter),
                count: u64::try_from(group.count).ok().filter(|count| *count > 0),
            })
            .collect();
        presentation.catalog_window = Some(CatalogWindow {
            first: view.first,
            tail: match view.tail {
                Tail::More => CatalogTail::More,
                Tail::Loading => CatalogTail::Loading,
                Tail::Error(_) => CatalogTail::Error,
                Tail::End => CatalogTail::End,
            },
        });
        presentation.status = if !view.rows.is_empty() {
            LoadState::Ready
        } else if view.tail == Tail::Loading {
            LoadState::Loading
        } else if matches!(view.tail, Tail::Error(_)) {
            LoadState::Error
        } else if view.loaded {
            LoadState::Empty
        } else {
            LoadState::Loading
        };
        for media in view.rows {
            let target = Target::Native(media.id.clone());
            let artwork = presentation.bind_image(ImageSource::Media {
                id: media.id.clone(),
                label: ImageLabel::Landscape,
                role: criterion_artwork::ImageRole::Card,
            });
            presentation.cards.push(OwnedCard {
                target,
                kind: None,
                title: media.title.clone(),
                year: media
                    .release_date
                    .map(|date| format!("{:04}", date.year()))
                    .unwrap_or_default(),
                duration_seconds: 0,
                artwork: Some(artwork),
                saved_fraction: None,
                native_activation: Some(super::continue_watching::native_action(
                    media, None, false,
                )),
            });
        }
        presentation
    }
}

#[cfg(test)]
mod tests {
    use super::Presentation;
    use criterion_account::{MediaKind, MediaSummary, PagingInfo, TypeCount, WatchList};
    use criterion_provider::{MediaId, PageCursor};
    use criterion_ui::{LoadState, LoginView, Target};

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

    fn watch_list(playlist: Vec<criterion_account::MediaSummary>) -> WatchList {
        WatchList {
            paging: PagingInfo {
                page_limit: 60,
                next_pagination_key: None,
            },
            type_counts: Vec::new(),
            playlist,
        }
    }

    fn presentation(list: WatchList) -> Presentation {
        let mut state = crate::my_list::MyListState::new();
        let read = state
            .select(criterion_account::WatchListFilter::All)
            .unwrap()
            .unwrap();
        let _ = state.admit(&read, list).unwrap();
        Presentation::my_list(&state)
    }

    #[test]
    fn empty_my_list_lends_an_empty_shelf_without_placeholder_content() {
        let presentation = presentation(watch_list(Vec::new()));
        presentation.with_view(LoginView::SignedOut, |view| {
            assert_eq!(view.title, "My List");
            assert_eq!(view.status, LoadState::Empty);
            assert_eq!(view.total, 0);
            assert!(view.cards.is_empty());
            assert!(view.rails.is_empty());
            assert!(view.hero.is_none());
            assert!(view.detail.is_none());
        });
        assert!(presentation.artwork_bindings().is_empty());
    }

    #[test]
    fn my_list_lends_all_native_kinds_in_supplied_order_with_calendar_years() {
        let mut playlist = vec![
            media("Fixture9", "Synthetic supplement", MediaKind::Supplement),
            media("Fixture8", "Synthetic film", MediaKind::Film),
            media("Fixture7", "Synthetic live", MediaKind::Live),
            media("Fixture6", "Synthetic franchise", MediaKind::Franchise),
            media("Fixture5", "Synthetic episode", MediaKind::Episode),
            media("Fixture4", "Synthetic original", MediaKind::Original),
            media("Fixture3", "Synthetic series", MediaKind::Series),
            media("Fixture2", "Synthetic collection", MediaKind::Collection),
            media("Fixture1", "Synthetic category", MediaKind::Category),
        ];
        playlist[0].duration = Some(90.5);
        playlist[0].release_date =
            Some(time::Date::from_calendar_date(2000, time::Month::February, 29).unwrap());
        playlist[1].duration = Some(5400.0);
        playlist[1].release_date =
            Some(time::Date::from_calendar_date(7, time::Month::January, 1).unwrap());
        playlist[2].duration = Some(0.0);
        let mut native_list = watch_list(playlist);
        native_list.paging.next_pagination_key = Some(PageCursor::new("synthetic-cursor").unwrap());
        native_list.type_counts = vec![TypeCount {
            content_type: "film".into(),
            count: 997,
        }];

        let presentation = presentation(native_list);
        presentation.with_view(LoginView::SignedOut, |view| {
            assert_eq!(view.title, "My List");
            assert_eq!(view.status, LoadState::Ready);
            assert_eq!(view.total, 9);
            assert_eq!(
                view.cards.iter().map(|card| card.title).collect::<Vec<_>>(),
                [
                    "Synthetic supplement",
                    "Synthetic film",
                    "Synthetic live",
                    "Synthetic franchise",
                    "Synthetic episode",
                    "Synthetic original",
                    "Synthetic series",
                    "Synthetic collection",
                    "Synthetic category",
                ]
            );
            assert_eq!(
                view.cards.iter().map(|card| card.key).collect::<Vec<_>>(),
                [
                    &Target::Native(MediaId::new("Fixture9").unwrap()),
                    &Target::Native(MediaId::new("Fixture8").unwrap()),
                    &Target::Native(MediaId::new("Fixture7").unwrap()),
                    &Target::Native(MediaId::new("Fixture6").unwrap()),
                    &Target::Native(MediaId::new("Fixture5").unwrap()),
                    &Target::Native(MediaId::new("Fixture4").unwrap()),
                    &Target::Native(MediaId::new("Fixture3").unwrap()),
                    &Target::Native(MediaId::new("Fixture2").unwrap()),
                    &Target::Native(MediaId::new("Fixture1").unwrap()),
                ]
            );
            assert_eq!(
                view.cards.iter().map(|card| card.year).collect::<Vec<_>>(),
                ["2000", "0007", "", "", "", "", "", "", ""]
            );
            assert!(view.cards.iter().all(|card| card.duration_seconds == 0));
            assert!(view.filters.is_none());
            assert!(view.hero.is_none());
            assert!(view.detail.is_none());
            assert!(view.rails.is_empty());
        });
    }

    #[test]
    fn my_list_projection_uses_the_reducers_first_row_key_and_original_metadata() {
        let presentation = presentation(watch_list(vec![
            media("FixtureA", "Synthetic repeated title", MediaKind::Film),
            media("FixtureB", "Synthetic repeated title", MediaKind::Film),
            media(
                "FixtureA",
                "Synthetic alternate title",
                MediaKind::Supplement,
            ),
        ]));
        presentation.with_view(LoginView::SignedOut, |view| {
            assert_eq!(view.total, 2);
            assert_eq!(view.cards.len(), 2);
            let first_artwork = view.cards[0].artwork_key.expect("native card artwork");
            let second_artwork = view.cards[1].artwork_key.expect("native card artwork");
            assert_ne!(first_artwork, second_artwork);
            assert_eq!(view.cards[0].title, "Synthetic repeated title");
            assert_eq!(view.cards[1].title, "Synthetic repeated title");
            assert_ne!(view.cards[0].key, view.cards[1].key);
        });
        assert_eq!(presentation.artwork_bindings().len(), 2);
        assert_eq!(
            presentation.artwork_bindings()[0].source,
            super::ImageSource::Media {
                id: MediaId::new("FixtureA").unwrap(),
                label: criterion_provider::ImageLabel::Landscape,
                role: criterion_artwork::ImageRole::Card,
            }
        );
        assert_eq!(
            presentation.artwork_bindings()[1].source,
            super::ImageSource::Media {
                id: MediaId::new("FixtureB").unwrap(),
                label: criterion_provider::ImageLabel::Landscape,
                role: criterion_artwork::ImageRole::Card,
            }
        );
    }
    #[test]
    fn my_list_episode_uses_native_metadata_only_and_keeps_original_card_identity() {
        let mut episode = media("Epis0001", "Original Episode title", MediaKind::Episode);
        episode.series_id = Some(MediaId::new("Meta0001").unwrap());
        episode.series_title = Some("Native series title".into());
        let view = presentation(watch_list(vec![
            episode,
            media("Live0001", "Synthetic live", MediaKind::Live),
            media("Fran0001", "Synthetic franchise", MediaKind::Franchise),
            media("Film0001", "Synthetic film", MediaKind::Film),
        ]));
        assert_eq!(
            view.native_activation(&Target::Native(MediaId::new("Epis0001").unwrap())),
            Some(super::super::NativeActivation::Detail {
                id: MediaId::new("Meta0001").unwrap(),
                auto_play: true
            })
        );
        assert_eq!(
            view.native_activation(&Target::Native(MediaId::new("Live0001").unwrap())),
            Some(super::super::NativeActivation::Unsupported)
        );
        assert_eq!(
            view.native_activation(&Target::Native(MediaId::new("Fran0001").unwrap())),
            Some(super::super::NativeActivation::Detail {
                id: MediaId::new("Fran0001").unwrap(),
                auto_play: false
            })
        );
        assert_eq!(
            view.native_activation(&Target::Native(MediaId::new("Film0001").unwrap())),
            Some(super::super::NativeActivation::Detail {
                id: MediaId::new("Film0001").unwrap(),
                auto_play: false
            })
        );
        view.with_view(LoginView::SignedIn, |data| {
            assert_eq!(
                data.cards[0].key,
                &Target::Native(MediaId::new("Epis0001").unwrap())
            );
            assert_eq!(data.cards[0].title, "Original Episode title");
            assert!(data.cards.iter().all(|card| card.saved_fraction.is_none()));
        });
    }
}
