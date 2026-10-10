// SPDX-License-Identifier: GPL-3.0-or-later
//! Supplied account gallery hydration; the controller owns live read authority.
use super::{ImageSource, NativeActivation, OwnedCard, Presentation, ProjectionLimit, RailSource};
use crate::continue_watching::ContinueWatchingShelf;
use criterion_artwork::ImageRole;
use criterion_provider::MediaId;
use criterion_ui::{LoadState, Target};

// Policy across all supplied slots, including duplicate gallery multiplicity.
const MAX_PRIVATE_CARDS: usize = 4096;
const MAX_PRIVATE_BYTES: usize = 512 * 1024;

#[derive(Default)]
pub(super) struct ContinueWatchingState {
    max_epoch: Option<u64>,
    retired_through: Option<u64>,
    read: ReadState,
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum ReadState {
    #[default]
    Unrequested,
    Pending(u64),
    Failed(u64),
    Ready(u64),
}

impl Presentation {
    pub(crate) fn native_activation(&self, target: &Target) -> Option<NativeActivation> {
        if !matches!(target, Target::Native(_)) {
            return None;
        }
        if self
            .detail
            .as_ref()
            .is_some_and(|detail| detail.native.is_some())
        {
            let index = self.selected_playlist?;
            let cards = self
                .native_season_cards(index)
                .or_else(|| self.rails.get(index).map(|rail| rail.cards.as_slice()))?;
            return cards
                .iter()
                .find(|card| &card.target == target)
                .and_then(|card| card.native_activation.clone());
        }
        self.cards
            .iter()
            .chain(self.rails.iter().flat_map(|rail| &rail.cards))
            .find(|card| &card.target == target)
            .and_then(|card| card.native_activation.clone())
    }
    /// Only an exact pending epoch may publish. The controller separately owns
    /// operation identity across navigation/cancellation in the same epoch.
    pub(crate) fn admit_continue_watching(
        &mut self,
        epoch: u64,
        shelf: &ContinueWatchingShelf,
    ) -> Result<bool, ProjectionLimit> {
        if self.continue_watching.read != ReadState::Pending(epoch) {
            return Ok(false);
        }
        let slot_count = self
            .rails
            .iter()
            .filter(|rail| matches!(rail.gallery, Some((RailSource::ContinueWatching, _, _))))
            .count();
        if shelf
            .rows()
            .len()
            .checked_mul(slot_count)
            .is_none_or(|count| count > MAX_PRIVATE_CARDS)
        {
            self.fail_continue_watching(epoch);
            return Err(ProjectionLimit::TooLarge);
        }
        let mut staged = Self::loading("");
        let mut slots = Vec::with_capacity(slot_count);
        let mut card_bytes = 0usize;
        for (index, rail) in self.rails.iter().enumerate() {
            let Some((RailSource::ContinueWatching, label, _)) = rail.gallery else {
                continue;
            };
            let mut cards = Vec::with_capacity(shelf.rows().len());
            let Some(bytes) = card_bytes.checked_add(super::vec_bytes(&cards)) else {
                self.fail_continue_watching(epoch);
                return Err(ProjectionLimit::TooLarge);
            };
            card_bytes = bytes;
            if card_bytes > MAX_PRIVATE_BYTES {
                self.fail_continue_watching(epoch);
                return Err(ProjectionLimit::TooLarge);
            }
            for row in shelf.rows() {
                let media = row.media();
                let source = ImageSource::Media {
                    id: media.id.clone(),
                    label,
                    role: ImageRole::Card,
                };
                let artwork = self
                    .artwork
                    .iter()
                    .find(|binding| {
                        binding.source == source && self.references_artwork(&binding.key, true)
                    })
                    .map(|binding| binding.key.clone())
                    .unwrap_or_else(|| staged.bind_image(source));
                let card = OwnedCard {
                    target: Target::Native(media.id.clone()),
                    kind: None,
                    title: media.title.clone(),
                    year: media
                        .release_date
                        .map(|date| date.year().to_string())
                        .unwrap_or_default(),
                    duration_label: super::native_card_duration(media),
                    artwork: Some(artwork),
                    saved_fraction: row.saved_fraction(),
                    native_activation: Some(native_action(media, row.saved_series_id(), true)),
                };
                let Some(bytes) = card_bytes.checked_add(card.heap_bytes()) else {
                    self.fail_continue_watching(epoch);
                    return Err(ProjectionLimit::TooLarge);
                };
                card_bytes = bytes;
                if card_bytes
                    .checked_add(staged.catalog_bytes())
                    .is_none_or(|bytes| bytes > MAX_PRIVATE_BYTES)
                {
                    self.fail_continue_watching(epoch);
                    return Err(ProjectionLimit::TooLarge);
                }
                cards.push(card);
            }
            slots.push((index, cards));
        }
        if slots.is_empty() {
            return Ok(false);
        }
        // Observe the final shared binding-vector capacity before moving anything.
        // Public bindings retain their existing inline charge; new capacity and
        // every converted private card/key/source are charged to this 512 KiB slice.
        // Opaque ID wrappers expose length, not capacity; allocator overhead and
        // temporary staging containers are outside this retained-view estimate.
        let public_bindings = self
            .artwork
            .iter()
            .filter(|binding| self.references_artwork(&binding.key, true))
            .count();
        let mut artwork = Vec::with_capacity(public_bindings + staged.artwork.len());
        let binding_bytes =
            (artwork.capacity() - public_bindings) * std::mem::size_of::<super::ImageBinding>();
        let binding_text = staged
            .artwork
            .iter()
            .map(|binding| {
                binding.key.capacity()
                    + match &binding.source {
                        ImageSource::Media { id, .. } => id.as_str().len(),
                        ImageSource::Editorial(image) => image.url().as_str().len(),
                    }
            })
            .sum::<usize>();
        if card_bytes
            .checked_add(binding_bytes)
            .and_then(|bytes| bytes.checked_add(binding_text))
            .is_none_or(|bytes| bytes > MAX_PRIVATE_BYTES)
        {
            self.fail_continue_watching(epoch);
            return Err(ProjectionLimit::TooLarge);
        }
        for binding in std::mem::take(&mut self.artwork) {
            if self.references_artwork(&binding.key, true) {
                artwork.push(binding);
            }
        }
        artwork.append(&mut staged.artwork);
        self.artwork = artwork;
        for (index, cards) in slots {
            self.rails[index].cards = cards;
            self.rails[index].private_epoch = Some(epoch);
        }
        self.prune_unused_artwork();
        self.refresh_discovery_count();
        self.continue_watching.read = ReadState::Ready(epoch);
        Ok(true)
    }
    pub(crate) fn continue_watching_needs_read(&self, epoch: u64) -> bool {
        let state = &self.continue_watching;
        !state
            .retired_through
            .is_some_and(|retired| epoch <= retired)
            && !state.max_epoch.is_some_and(|maximum| epoch < maximum)
            && match state.read {
                ReadState::Unrequested => true,
                ReadState::Pending(current)
                | ReadState::Failed(current)
                | ReadState::Ready(current) => epoch > current,
            }
            && self
                .rails
                .iter()
                .any(|rail| matches!(rail.gallery, Some((RailSource::ContinueWatching, _, _))))
    }
    pub(crate) fn mark_continue_watching_pending(&mut self, epoch: u64) {
        if self.continue_watching_needs_read(epoch) {
            if self
                .continue_watching
                .max_epoch
                .is_some_and(|maximum| epoch > maximum)
            {
                self.clear_private_rows();
            }
            self.continue_watching.max_epoch = Some(epoch);
            self.continue_watching.read = ReadState::Pending(epoch);
        }
    }
    pub(crate) fn fail_continue_watching(&mut self, epoch: u64) {
        if self.continue_watching.read == ReadState::Pending(epoch) {
            self.continue_watching.read = ReadState::Failed(epoch);
        }
    }
    /// A pending read may be requested again on return; settled rows survive.
    pub(crate) fn cancel_continue_watching(&mut self) {
        if matches!(self.continue_watching.read, ReadState::Pending(_)) {
            self.continue_watching.read = ReadState::Unrequested;
        }
    }
    /// Drops owned private view references and retires the observed epoch. This
    /// does not claim allocator overwriting, GPU-cache erasure or issuer cleanup.
    pub(crate) fn clear_private_rows(&mut self) {
        self.clear_native_resume();
        self.continue_watching.retired_through = self.continue_watching.max_epoch;
        self.continue_watching.read = ReadState::Unrequested;
        let mut cleared = false;
        for rail in &mut self.rails {
            if rail.private_epoch.take().is_some() {
                rail.cards = Vec::new();
                cleared = true;
            }
        }
        if cleared {
            self.prune_unused_artwork();
            self.artwork.shrink_to_fit();
            self.refresh_discovery_count();
        }
    }
    fn refresh_discovery_count(&mut self) {
        self.total = self.rails.iter().map(|rail| rail.cards.len() as u32).sum();
        self.status = if self.total == 0 && self.hero.is_none() {
            LoadState::Empty
        } else {
            LoadState::Ready
        };
    }
    fn prune_unused_artwork(&mut self) {
        let mut bindings = std::mem::take(&mut self.artwork);
        bindings.retain(|binding| self.references_artwork(&binding.key, false));
        self.artwork = bindings;
    }
    fn references_artwork(&self, key: &str, exclude_replaced_slots: bool) -> bool {
        let matches_card = |card: &OwnedCard| card.artwork.as_deref() == Some(key);
        self.cards.iter().any(matches_card)
            || self
                .rails
                .iter()
                .filter(|rail| {
                    !exclude_replaced_slots
                        || !matches!(rail.gallery, Some((RailSource::ContinueWatching, _, _)))
                })
                .any(|rail| rail.cards.iter().any(matches_card))
            || self
                .detail
                .as_ref()
                .is_some_and(|detail| matches_card(&detail.card))
            || self.hero.as_ref().is_some_and(|hero| {
                matches_card(&hero.card)
                    || hero.background == key
                    || hero.logo.as_deref() == Some(key)
            })
    }
}

/// Closed native detail intent; licensed player execution belongs to its owner.
pub(super) fn native_action(
    media: &criterion_account::MediaSummary,
    saved_series_id: Option<&MediaId>,
    non_episode_auto_play: bool,
) -> NativeActivation {
    use criterion_account::MediaKind;
    match media.kind {
        MediaKind::Episode => saved_series_id
            .or(media.series_id.as_ref())
            .cloned()
            .map_or(NativeActivation::Unsupported, |id| {
                NativeActivation::Detail {
                    id,
                    auto_play: true,
                }
            }),
        MediaKind::Live => NativeActivation::Unsupported,
        MediaKind::Film
        | MediaKind::Supplement
        | MediaKind::Category
        | MediaKind::Collection
        | MediaKind::Series
        | MediaKind::Original
        | MediaKind::Franchise => NativeActivation::Detail {
            id: media.id.clone(),
            auto_play: non_episode_auto_play,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use criterion_provider::{
        ContentTarget, DiscoveryBlock, DiscoveryCard, DiscoveryPage, GalleryLayout,
        GalleryPresentation, ImageLabel, RailSource,
    };
    use criterion_ui::{LoginView, Target};

    fn native_media(
        id: &str,
        title: &str,
        kind: criterion_account::MediaKind,
    ) -> criterion_account::MediaSummary {
        criterion_account::MediaSummary {
            id: MediaId::new(id).unwrap(),
            title: title.into(),
            kind,
            series_id: None,
            series_title: None,
            duration: Some(3.5),
            release_date: Some(
                time::Date::from_calendar_date(1999, time::Month::January, 1).unwrap(),
            ),
        }
    }

    fn public_rail(id: u32, header: &str) -> DiscoveryBlock {
        let mut block = rail(id, header, RailSource::Provided { feed_id: None });
        if let DiscoveryBlock::Rail { cards, .. } = &mut block {
            cards.push(DiscoveryCard {
                media: criterion_provider::MediaSummary {
                    id: MediaId::new("PubF0001").unwrap(),
                    title: "Public film".into(),
                    kind: criterion_provider::MediaKind::Film,
                    duration_seconds: 3600,
                    release_date: Some("2001-01-01".into()),
                },
                target: ContentTarget::parse("/films/PubF0001/public-film").unwrap(),
            });
        }
        block
    }

    fn shelf_from_admitted(
        data: criterion_account::ContinueWatching,
    ) -> Result<ContinueWatchingShelf, crate::continue_watching::ShelfLimit> {
        ContinueWatchingShelf::from_admitted(data).map(|(shelf, _)| shelf)
    }
    fn shelf_all_kinds() -> ContinueWatchingShelf {
        use criterion_account::{ContinueWatching, MediaKind, Position};
        let items = [
            ("Film0001", MediaKind::Film),
            ("Supp0001", MediaKind::Supplement),
            ("Epis0001", MediaKind::Episode),
            ("Cate0001", MediaKind::Category),
            ("Coll0001", MediaKind::Collection),
            ("Seri0001", MediaKind::Series),
            ("Orig0001", MediaKind::Original),
            ("Fran0001", MediaKind::Franchise),
            ("Live0001", MediaKind::Live),
        ];
        shelf_from_admitted(ContinueWatching {
            playlist: items
                .iter()
                .map(|(id, kind)| native_media(id, "Private native title", *kind))
                .collect(),
            positions: items
                .iter()
                .enumerate()
                .map(|(index, (id, _))| Position {
                    media_id: MediaId::new(id).unwrap(),
                    pos: match index {
                        0 => 100,
                        1 => 98,
                        _ => 50,
                    },
                    dur: 100,
                    commentary_track: None,
                    series_id: None,
                    series_title: None,
                })
                .collect(),
        })
        .unwrap()
    }

    fn rail(id: u32, header: &str, source: RailSource) -> DiscoveryBlock {
        DiscoveryBlock::Rail {
            id,
            header: Some(header.into()),
            cta: None,
            target: None,
            opens_new_window: false,
            source,
            cards: vec![],
            image_label: ImageLabel::Portrait,
            presentation: GalleryPresentation {
                aspect_ratio_percent: 150.0,
                cards_per_view: 6,
                layout: GalleryLayout::Grid,
                variant: 2,
            },
        }
    }

    #[test]
    fn only_a_supplied_continue_watching_source_demands_an_account_read() {
        let supplied = Presentation::discovery(DiscoveryPage {
            blocks: vec![rail(
                1,
                "Supplied account gallery",
                RailSource::ContinueWatching,
            )],
        });
        assert!(supplied.continue_watching_needs_read(1));
        let public = Presentation::discovery(DiscoveryPage {
            blocks: vec![
                rail(
                    2,
                    "Continue Watching",
                    RailSource::Provided { feed_id: None },
                ),
                rail(3, "Continue Watching", RailSource::Watchlist),
            ],
        });
        assert!(!public.continue_watching_needs_read(1));
        assert!(
            !Presentation::discovery(DiscoveryPage { blocks: vec![] })
                .continue_watching_needs_read(1)
        );
        assert!(!Presentation::loading("Offline Home").continue_watching_needs_read(1));
    }

    #[test]
    fn pending_failure_cancellation_and_retirement_keep_epoch_demand_separate() {
        let mut view = Presentation::discovery(DiscoveryPage {
            blocks: vec![rail(
                1,
                "Supplied account gallery",
                RailSource::ContinueWatching,
            )],
        });
        view.mark_continue_watching_pending(3);
        assert!(!view.continue_watching_needs_read(3));
        view.fail_continue_watching(3);
        assert!(!view.continue_watching_needs_read(3));
        view.cancel_continue_watching();
        assert!(!view.continue_watching_needs_read(3));
        assert!(view.continue_watching_needs_read(4));
        view.mark_continue_watching_pending(4);
        view.fail_continue_watching(3);
        assert!(!view.continue_watching_needs_read(4));
        view.cancel_continue_watching();
        assert!(view.continue_watching_needs_read(4));
        assert!(!view.continue_watching_needs_read(3));
        view.mark_continue_watching_pending(4);
        view.clear_private_rows();
        assert!(!view.continue_watching_needs_read(4));
        assert!(!view.continue_watching_needs_read(3));
        assert!(view.continue_watching_needs_read(5));
    }

    #[test]
    fn hydration_replaces_only_supplied_slots_in_place_and_keeps_exact_saved_fractions() {
        let mut second_slot = rail(4, "Saved editions", RailSource::ContinueWatching);
        if let DiscoveryBlock::Rail { image_label, .. } = &mut second_slot {
            *image_label = ImageLabel::Edition;
        }
        let mut view = Presentation::discovery(DiscoveryPage {
            blocks: vec![
                public_rail(1, "Public before"),
                rail(2, "Saved portraits", RailSource::ContinueWatching),
                public_rail(3, "Public after"),
                second_slot,
            ],
        });
        let shelf = shelf_all_kinds();
        view.mark_continue_watching_pending(7);
        assert_eq!(view.admit_continue_watching(7, &shelf), Ok(true));
        view.with_view(LoginView::SignedIn, |data| {
            assert_eq!(
                data.rails.iter().map(|r| r.title).collect::<Vec<_>>(),
                [
                    "Public before",
                    "Saved portraits",
                    "Public after",
                    "Saved editions"
                ]
            );
            for index in [0, 2] {
                let card = data.rails[index].cards[0];
                assert_eq!(card.title, "Public film");
                assert_eq!(card.year, "2001");
                assert_eq!(card.duration_label, Some("1 h 0 min"));
                assert_eq!(card.saved_fraction, None);
                assert_eq!(
                    card.key,
                    &Target::Content(ContentTarget::parse("/films/PubF0001/public-film").unwrap())
                );
            }
            for index in [1, 3] {
                let cards = data.rails[index].cards;
                assert_eq!(cards.len(), 9);
                assert_eq!(
                    cards
                        .iter()
                        .map(|c| c.key.media_id().unwrap().as_str())
                        .collect::<Vec<_>>(),
                    [
                        "Film0001", "Supp0001", "Epis0001", "Cate0001", "Coll0001", "Seri0001",
                        "Orig0001", "Fran0001", "Live0001"
                    ]
                );
                assert!(
                    cards
                        .iter()
                        .all(|c| c.key == &Target::Native(c.key.media_id().unwrap().clone()))
                );
                assert!(cards.iter().all(|c| c.year == "1999"));
                assert_eq!(
                    cards.iter().map(|c| c.duration_label).collect::<Vec<_>>(),
                    [
                        Some("0 min"),
                        Some("0 min"),
                        Some("0 min"),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None
                    ]
                );
                assert_eq!(
                    cards.iter().map(|c| c.saved_fraction).collect::<Vec<_>>(),
                    [
                        Some(1.0),
                        Some(0.98),
                        Some(0.5),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None
                    ]
                );
            }
        });
        for label in [ImageLabel::Portrait, ImageLabel::Edition] {
            assert!(view.artwork_bindings().iter().any(|binding| {
                matches!(&binding.source, super::super::ImageSource::Media {
                    id, label: supplied, role: criterion_artwork::ImageRole::Card
                } if id.as_str() == "Film0001" && *supplied == label)
            }));
        }
        assert_eq!(shelf.rows()[0].media().duration, Some(3.5));
        assert!(!view.continue_watching_needs_read(7));
        view.cancel_continue_watching();
        assert!(!view.continue_watching_needs_read(7));
        view.with_view(LoginView::SignedIn, |data| {
            assert_eq!(data.rails[1].cards.len(), 9)
        });
    }
    #[test]
    fn logout_scrubs_only_private_rows_and_keeps_public_shared_artwork_and_hero() {
        use criterion_provider::{
            DiscoveryArtwork, DiscoverySlide, EditorialImage, ResponsiveImage,
        };
        let hero = DiscoveryBlock::Slideshow {
            id: 12,
            slides: vec![DiscoverySlide {
                id: 13,
                title: Some("Public hero".into()),
                title_prefix: None,
                cta: Some("Explore".into()),
                target: Some(ContentTarget::parse("/collections/Hero0001/public-hero").unwrap()),
                opens_new_window: false,
                artwork: DiscoveryArtwork {
                    desktop: vec![ResponsiveImage {
                        width: 1920,
                        image: EditorialImage::new(
                            "https://cc.criterion.com/uploads/storyBlocks/493/thumbnails/",
                            "public-hero.webp",
                        )
                        .unwrap(),
                    }],
                    mobile: vec![],
                    logo: None,
                },
            }],
        };
        let mut view = Presentation::discovery(DiscoveryPage {
            blocks: vec![
                hero,
                public_rail(1, "Public films"),
                rail(2, "Saved gallery", RailSource::ContinueWatching),
            ],
        });
        let public_keys = view
            .artwork_bindings()
            .iter()
            .map(|b| b.key.clone())
            .collect::<Vec<_>>();
        let shelf = shelf_from_admitted(criterion_account::ContinueWatching {
            playlist: vec![
                native_media(
                    "PubF0001",
                    "Private shared title",
                    criterion_account::MediaKind::Film,
                ),
                native_media(
                    "Priv0001",
                    "Private unique title",
                    criterion_account::MediaKind::Film,
                ),
            ],
            positions: vec![],
        })
        .unwrap();
        view.mark_continue_watching_pending(7);
        assert_eq!(view.admit_continue_watching(7, &shelf), Ok(true));
        assert_eq!(
            view.artwork_bindings().len(),
            3,
            "shared exact source has one binding"
        );
        let private_bytes = view.estimated_bytes();
        view.clear_private_rows();
        view.with_view(LoginView::SignedOut, |data| {
            assert_eq!(data.rails.len(), 2);
            assert_eq!(data.rails[0].cards[0].title, "Public film");
            assert_eq!(data.rails[1].title, "Saved gallery");
            assert!(data.rails[1].cards.is_empty());
            assert_eq!(data.hero.as_ref().unwrap().card.title, "Public hero");
            assert_eq!(data.total, 1);
        });
        assert_eq!(
            view.artwork_bindings()
                .iter()
                .map(|b| &b.key)
                .collect::<Vec<_>>(),
            public_keys.iter().collect::<Vec<_>>()
        );
        assert!(view.estimated_bytes() < private_bytes);
        assert_eq!(view.admit_continue_watching(7, &shelf), Ok(false));
        view.mark_continue_watching_pending(7);
        assert_eq!(view.admit_continue_watching(7, &shelf), Ok(false));
        view.mark_continue_watching_pending(8);
        assert_eq!(view.admit_continue_watching(8, &shelf), Ok(true));
    }

    #[test]
    fn repeated_slots_refuse_global_card_or_heap_overflow_atomically() {
        for (rows, slots, title_len) in [(512, 9, 8), (64, 10, 1000)] {
            let shelf = shelf_from_admitted(criterion_account::ContinueWatching {
                playlist: (0..rows)
                    .map(|index| {
                        native_media(
                            &format!("F{index:07}"),
                            &"x".repeat(title_len),
                            criterion_account::MediaKind::Film,
                        )
                    })
                    .collect(),
                positions: vec![],
            })
            .unwrap();
            let mut blocks = vec![public_rail(0, "Public first")];
            blocks.extend(
                (1..=slots)
                    .map(|id| rail(id, "Supplied empty gallery", RailSource::ContinueWatching)),
            );
            let mut view = Presentation::discovery(DiscoveryPage { blocks });
            let keys = view
                .artwork_bindings()
                .iter()
                .map(|b| b.key.clone())
                .collect::<Vec<_>>();
            view.mark_continue_watching_pending(9);
            assert_eq!(
                view.admit_continue_watching(9, &shelf),
                Err(ProjectionLimit::TooLarge)
            );
            view.with_view(LoginView::SignedIn, |data| {
                assert_eq!(data.total, 1);
                assert_eq!(data.rails[0].cards[0].title, "Public film");
                assert!(data.rails[1..].iter().all(|rail| rail.cards.is_empty()));
            });
            assert_eq!(
                view.artwork_bindings()
                    .iter()
                    .map(|b| &b.key)
                    .collect::<Vec<_>>(),
                keys.iter().collect::<Vec<_>>()
            );
            assert!(
                !view.continue_watching_needs_read(9),
                "overflow suppresses automatic retry"
            );
            assert_eq!(view.admit_continue_watching(9, &shelf), Ok(false));
        }
    }

    #[test]
    fn duplicate_public_first_card_cannot_shadow_or_acquire_a_native_activation() {
        let id = MediaId::new("Film0001").unwrap();
        let mut view = Presentation::discovery(DiscoveryPage {
            blocks: vec![rail(1, "Continue Watching", RailSource::ContinueWatching)],
        });
        view.merge_catalog(Presentation::catalog(
            "Public catalog",
            criterion_provider::CatalogPage {
                items: vec![criterion_provider::MediaSummary {
                    id: id.clone(),
                    title: "Public film".into(),
                    kind: criterion_provider::MediaKind::Film,
                    duration_seconds: 3600,
                    release_date: None,
                }],
                total: 1,
                next_cursor: None,
            },
        ));
        view.mark_continue_watching_pending(1);
        assert_eq!(
            view.admit_continue_watching(1, &shelf_all_kinds()),
            Ok(true)
        );
        let (public, native) = view.with_view(LoginView::SignedIn, |cards| {
            (
                cards.cards[0].key.clone(),
                cards.rails[0].cards[0].key.clone(),
            )
        });
        assert_eq!(public, Target::Media(id.clone()));
        assert_eq!(native, Target::Native(id.clone()));
        assert_eq!(native.media_id(), Some(&id));
        assert_eq!(
            view.native_activation(&native),
            Some(NativeActivation::Detail {
                id,
                auto_play: true
            })
        );
        assert_eq!(view.native_activation(&public), None);
    }

    #[test]
    fn native_activation_uses_original_non_episode_ids_and_exact_targets_only() {
        let mut view = Presentation::discovery(DiscoveryPage {
            blocks: vec![
                public_rail(1, "Public"),
                rail(2, "Saved gallery", RailSource::ContinueWatching),
            ],
        });
        view.mark_continue_watching_pending(1);
        assert_eq!(
            view.admit_continue_watching(1, &shelf_all_kinds()),
            Ok(true)
        );
        for id in [
            "Film0001", "Supp0001", "Cate0001", "Coll0001", "Seri0001", "Orig0001", "Fran0001",
        ] {
            let id = MediaId::new(id).unwrap();
            assert_eq!(
                view.native_activation(&Target::Native(id.clone())),
                Some(NativeActivation::Detail {
                    id,
                    auto_play: true
                })
            );
        }
        for id in ["Epis0001", "Live0001"] {
            assert_eq!(
                view.native_activation(&Target::Native(MediaId::new(id).unwrap())),
                Some(NativeActivation::Unsupported)
            );
        }
        assert_eq!(
            view.native_activation(&Target::Content(
                ContentTarget::parse("/films/Film0001/native-film").unwrap()
            )),
            None
        );
        assert_eq!(
            view.native_activation(&Target::Native(MediaId::new("Absent01").unwrap())),
            None
        );
        assert_eq!(
            view.native_activation(&Target::Content(
                ContentTarget::parse("/films/PubF0001/public-film").unwrap()
            )),
            None
        );
        view.clear_private_rows();
        assert_eq!(
            view.native_activation(&Target::Native(MediaId::new("Film0001").unwrap())),
            None
        );
    }

    #[test]
    fn empty_success_is_settled_and_only_current_pending_epoch_can_publish() {
        let mut view = Presentation::discovery(DiscoveryPage {
            blocks: vec![
                public_rail(1, "Public"),
                rail(2, "Saved gallery", RailSource::ContinueWatching),
            ],
        });
        let shelf = shelf_all_kinds();
        assert_eq!(view.admit_continue_watching(2, &shelf), Ok(false));
        view.mark_continue_watching_pending(2);
        assert_eq!(view.admit_continue_watching(1, &shelf), Ok(false));
        view.cancel_continue_watching();
        assert!(view.continue_watching_needs_read(2));
        assert_eq!(view.admit_continue_watching(2, &shelf), Ok(false));
        view.mark_continue_watching_pending(2);
        let empty = shelf_from_admitted(criterion_account::ContinueWatching {
            playlist: vec![],
            positions: vec![],
        })
        .unwrap();
        assert_eq!(view.admit_continue_watching(2, &empty), Ok(true));
        view.with_view(LoginView::SignedIn, |data| {
            assert_eq!(data.total, 1);
            assert_eq!(data.status, LoadState::Ready);
            assert!(data.rails[1].cards.is_empty());
        });
        assert!(!view.continue_watching_needs_read(2));
        view.mark_continue_watching_pending(3);
        view.fail_continue_watching(2);
        assert_eq!(view.admit_continue_watching(3, &shelf), Ok(true));
        view.mark_continue_watching_pending(4);
        view.with_view(LoginView::SignedIn, |data| {
            assert!(
                data.rails[1].cards.is_empty(),
                "previous account private rows retired before next admission"
            );
            assert_eq!(data.rails[0].cards[0].title, "Public film");
        });
        assert_eq!(view.admit_continue_watching(3, &shelf), Ok(false));
        assert_eq!(view.admit_continue_watching(4, &shelf), Ok(true));
    }

    #[test]
    fn replaced_slot_artwork_is_private_storage_even_when_its_source_was_already_bound() {
        let shelf = shelf_from_admitted(criterion_account::ContinueWatching {
            playlist: (0..512)
                .map(|index| {
                    native_media(
                        &format!("F{index:07}"),
                        &"x".repeat(40),
                        criterion_account::MediaKind::Film,
                    )
                })
                .collect(),
            positions: vec![],
        })
        .unwrap();
        for prebound in [false, true] {
            let mut first = rail(1, "Supplied gallery", RailSource::ContinueWatching);
            if prebound && let DiscoveryBlock::Rail { cards, .. } = &mut first {
                *cards = (0..512)
                    .map(|index| {
                        let id = format!("F{index:07}");
                        DiscoveryCard {
                            media: criterion_provider::MediaSummary {
                                id: MediaId::new(&id).unwrap(),
                                title: "Supplied public title".into(),
                                kind: criterion_provider::MediaKind::Film,
                                duration_seconds: 60,
                                release_date: None,
                            },
                            target: ContentTarget::parse(&format!("/films/{id}/supplied-film"))
                                .unwrap(),
                        }
                    })
                    .collect();
            }
            let mut view = Presentation::discovery(DiscoveryPage {
                blocks: vec![
                    first,
                    rail(2, "Duplicate slot", RailSource::ContinueWatching),
                    rail(3, "Duplicate slot", RailSource::ContinueWatching),
                ],
            });
            view.mark_continue_watching_pending(1);
            assert_eq!(
                view.admit_continue_watching(1, &shelf),
                Err(ProjectionLimit::TooLarge),
                "replacement must charge every private-only binding, including existing sources"
            );
            view.with_view(LoginView::SignedIn, |data| {
                assert_eq!(data.rails[0].cards.len(), if prebound { 512 } else { 0 });
                assert!(data.rails[1..].iter().all(|rail| rail.cards.is_empty()));
            });
        }
    }
    #[test]
    fn episode_activation_prefers_last_saved_series_then_native_metadata_without_remapping_cards() {
        use criterion_account::{ContinueWatching, MediaKind, Position};
        let mut first = native_media("Epis0001", "Original first Episode", MediaKind::Episode);
        first.series_id = Some(MediaId::new("Meta0001").unwrap());
        first.series_title = Some("Native series title".into());
        let mut second = native_media("Epis0002", "Original second Episode", MediaKind::Episode);
        second.series_id = Some(MediaId::new("Meta0002").unwrap());
        let position = |id: &str, series: Option<&str>, dur| Position {
            media_id: MediaId::new(id).unwrap(),
            pos: 25,
            dur,
            commentary_track: None,
            series_id: series.map(|id| MediaId::new(id).unwrap()),
            series_title: None,
        };
        let shelf = shelf_from_admitted(ContinueWatching {
            playlist: vec![
                first,
                second,
                native_media("Epis0003", "Saved zero duration", MediaKind::Episode),
                native_media("Epis0004", "No admitted parent", MediaKind::Episode),
            ],
            positions: vec![
                position("Epis0001", Some("SaveOld1"), 100),
                position("Epis0001", Some("SaveNew1"), 100),
                position("Epis0002", Some("SaveOld2"), 100),
                position("Epis0002", None, 100),
                position("Epis0003", Some("SaveNew3"), 0),
            ],
        })
        .unwrap();
        let mut view = Presentation::discovery(DiscoveryPage {
            blocks: vec![rail(1, "Saved Episodes", RailSource::ContinueWatching)],
        });
        view.mark_continue_watching_pending(1);
        assert_eq!(view.admit_continue_watching(1, &shelf), Ok(true));
        for (episode, parent) in [
            ("Epis0001", "SaveNew1"),
            ("Epis0002", "Meta0002"),
            ("Epis0003", "SaveNew3"),
        ] {
            assert_eq!(
                view.native_activation(&Target::Native(MediaId::new(episode).unwrap())),
                Some(NativeActivation::Detail {
                    id: MediaId::new(parent).unwrap(),
                    auto_play: true
                })
            );
        }
        assert_eq!(
            view.native_activation(&Target::Native(MediaId::new("Epis0004").unwrap())),
            Some(NativeActivation::Unsupported)
        );
        view.with_view(LoginView::SignedIn, |data| {
            assert_eq!(
                data.rails[0]
                    .cards
                    .iter()
                    .map(|card| card.key.media_id().unwrap().as_str())
                    .collect::<Vec<_>>(),
                ["Epis0001", "Epis0002", "Epis0003", "Epis0004"]
            );
            assert_eq!(data.rails[0].cards[0].title, "Original first Episode");
            assert_eq!(
                data.rails[0]
                    .cards
                    .iter()
                    .map(|card| card.saved_fraction)
                    .collect::<Vec<_>>(),
                [Some(0.25), Some(0.25), None, None]
            );
        });
        assert!(view.artwork_bindings().iter().all(|binding| matches!(&binding.source, ImageSource::Media { id, .. } if id.as_str().starts_with("Epis"))));
        assert_eq!(
            shelf.rows()[0].media().series_id.as_ref().unwrap().as_str(),
            "Meta0001"
        );
        assert_eq!(shelf.rows()[1].saved_series_id(), None);
    }
    #[test]
    fn continue_watching_card_labels_use_catalog_duration_independently_of_positions() {
        use criterion_account::{ContinueWatching, MediaKind, Position};
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
                (Some(90.5), Some("1 min")),
                (Some(7199.0), Some("1 h 59 min")),
                (Some(4_294_967_296.0), Some("596523 h 14 min")),
            ] {
                let mut source = native_media("Fixture1", "Native saved card", kind);
                source.duration = duration;
                let shelf = shelf_from_admitted(ContinueWatching {
                    playlist: vec![source],
                    positions: vec![Position {
                        media_id: MediaId::new("Fixture1").unwrap(),
                        pos: 25,
                        dur: 100,
                        commentary_track: None,
                        series_id: None,
                        series_title: None,
                    }],
                })
                .unwrap();
                let mut presentation = Presentation::discovery(DiscoveryPage {
                    blocks: vec![rail(1, "Saved", RailSource::ContinueWatching)],
                });
                presentation.mark_continue_watching_pending(1);
                assert_eq!(presentation.admit_continue_watching(1, &shelf), Ok(true));
                presentation.with_view(LoginView::SignedIn, |view| {
                    let card = view.rails[0].cards[0];
                    let expected = match kind {
                        MediaKind::Film | MediaKind::Supplement | MediaKind::Episode => expected,
                        _ => None,
                    };
                    assert_eq!(card.duration_label, expected, "{kind:?} {duration:?}");
                    assert_eq!(
                        card.saved_fraction,
                        matches!(
                            kind,
                            MediaKind::Film | MediaKind::Supplement | MediaKind::Episode
                        )
                        .then_some(0.25)
                    );
                });
            }
        }
    }

    #[test]
    fn continue_watching_card_label_reserved_capacity_is_charged_to_history() {
        let mut source = native_media(
            "Fixture1",
            "Native saved card",
            criterion_account::MediaKind::Film,
        );
        source.duration = Some(7199.0);
        let shelf = shelf_from_admitted(criterion_account::ContinueWatching {
            playlist: vec![source],
            positions: vec![],
        })
        .unwrap();
        let mut presentation = Presentation::discovery(DiscoveryPage {
            blocks: vec![rail(1, "Saved", RailSource::ContinueWatching)],
        });
        presentation.mark_continue_watching_pending(1);
        assert_eq!(presentation.admit_continue_watching(1, &shelf), Ok(true));
        let before = presentation.estimated_bytes();
        let card = &mut presentation.rails[0].cards[0];
        let prior = card.duration_label.as_ref().unwrap().capacity();
        let mut label = String::with_capacity(8192);
        label.push_str("1 h 59 min");
        let added = label.capacity() - prior;
        card.duration_label = Some(label);
        assert_eq!(presentation.estimated_bytes() - before, added);
        presentation.with_view(LoginView::SignedIn, |view| {
            assert_eq!(view.rails[0].cards[0].duration_label, Some("1 h 59 min"))
        });
    }

    #[test]
    fn card_labels_are_charged_before_private_gallery_admission() {
        use criterion_account::{ContinueWatching, MediaKind};
        // A bounded source page fits its 64 KiB shelf, while three display slots
        // approach the separate 512 KiB retained projection budget.
        let mut saw_label_boundary = false;
        for title_len in 0..=160 {
            let admitted = |duration| {
                let shelf = shelf_from_admitted(ContinueWatching {
                    playlist: (0..384)
                        .map(|index| {
                            let mut media = native_media(
                                &format!("F{index:07}"),
                                &"x".repeat(title_len),
                                MediaKind::Film,
                            );
                            media.duration = duration;
                            media
                        })
                        .collect(),
                    positions: vec![],
                })
                .unwrap();
                let mut presentation = Presentation::discovery(DiscoveryPage {
                    blocks: (1..=3)
                        .map(|id| rail(id, "Saved", RailSource::ContinueWatching))
                        .collect(),
                });
                presentation.mark_continue_watching_pending(1);
                let result = presentation.admit_continue_watching(1, &shelf);
                if result.is_err() {
                    presentation.with_view(LoginView::SignedIn, |view| {
                        assert!(view.rails.iter().all(|rail| rail.cards.is_empty()))
                    });
                }
                result
            };
            if admitted(None) == Ok(true)
                && admitted(Some(f32::MAX)) == Err(ProjectionLimit::TooLarge)
            {
                saw_label_boundary = true;
                break;
            }
        }
        assert!(
            saw_label_boundary,
            "real label storage must consume the private admission budget"
        );
    }
}
