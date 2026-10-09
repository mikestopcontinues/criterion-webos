// SPDX-License-Identifier: GPL-3.0-or-later
//! Native account display data; request and session ownership remain in the runtime.
use super::{ImageSource, OwnedCard, Presentation};
use criterion_account::WatchList;
use criterion_provider::ImageLabel;
use criterion_ui::{LoadState, Target};

impl Presentation {
    pub(crate) fn my_list(watch_list: WatchList) -> Self {
        let mut presentation = Self::loading("My List");
        presentation.total = watch_list.playlist.len() as u32;
        presentation.status = if watch_list.playlist.is_empty() {
            LoadState::Empty
        } else {
            LoadState::Ready
        };
        for media in watch_list.playlist {
            let target = Target::Media(media.id.clone());
            let artwork = presentation.bind_image(ImageSource::Media {
                id: media.id,
                label: ImageLabel::Landscape,
                role: criterion_artwork::ImageRole::Card,
            });
            presentation.cards.push(OwnedCard {
                target,
                kind: None,
                title: media.title,
                year: media
                    .release_date
                    .map(|date| format!("{:04}", date.year()))
                    .unwrap_or_default(),
                duration_seconds: 0,
                artwork: Some(artwork),
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

    #[test]
    fn empty_my_list_lends_an_empty_shelf_without_placeholder_content() {
        let presentation = Presentation::my_list(watch_list(Vec::new()));
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

        let presentation = Presentation::my_list(native_list);
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
                    &Target::Media(MediaId::new("Fixture9").unwrap()),
                    &Target::Media(MediaId::new("Fixture8").unwrap()),
                    &Target::Media(MediaId::new("Fixture7").unwrap()),
                    &Target::Media(MediaId::new("Fixture6").unwrap()),
                    &Target::Media(MediaId::new("Fixture5").unwrap()),
                    &Target::Media(MediaId::new("Fixture4").unwrap()),
                    &Target::Media(MediaId::new("Fixture3").unwrap()),
                    &Target::Media(MediaId::new("Fixture2").unwrap()),
                    &Target::Media(MediaId::new("Fixture1").unwrap()),
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
    fn my_list_retains_duplicate_cards_while_sharing_only_identical_artwork() {
        let presentation = Presentation::my_list(watch_list(vec![
            media("FixtureA", "Synthetic repeated title", MediaKind::Film),
            media("FixtureB", "Synthetic repeated title", MediaKind::Film),
            media(
                "FixtureA",
                "Synthetic alternate title",
                MediaKind::Supplement,
            ),
        ]));
        presentation.with_view(LoginView::SignedOut, |view| {
            assert_eq!(view.total, 3);
            assert_eq!(view.cards.len(), 3);
            let first_artwork = view.cards[0].artwork_key.expect("native card artwork");
            let second_artwork = view.cards[1].artwork_key.expect("native card artwork");
            let third_artwork = view.cards[2].artwork_key.expect("native card artwork");
            assert_eq!(first_artwork, third_artwork);
            assert_ne!(first_artwork, second_artwork);
            assert_eq!(view.cards[0].title, "Synthetic repeated title");
            assert_eq!(view.cards[2].title, "Synthetic alternate title");
            assert_eq!(view.cards[0].key, view.cards[2].key);
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
}
