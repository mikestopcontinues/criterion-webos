// SPDX-License-Identifier: GPL-3.0-or-later
//! Owned validated display data; the controller owns publication and request lifetimes.
mod account;
use criterion_artwork::ImageRole;
use criterion_provider::{
    BrowseOptions, CatalogPage, DiscoveryArtwork, DiscoveryBlock, DiscoveryPage, DiscoverySlide,
    EditorialImage, ImageLabel, MediaDetail, MediaId, MediaKind, MediaSummary, RailSource,
    SearchResults,
};
use criterion_ui::{
    Card, Detail, DetailKind, FilterGroup, FilterMenu, Hero, HeroAction, LoadState, Rail,
    SearchGroup, Target, ViewData,
};
use std::fmt::Write;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum ImageSource {
    Media {
        id: MediaId,
        label: ImageLabel,
        role: ImageRole,
    },
    Editorial(EditorialImage),
}
pub(crate) struct ImageBinding {
    pub(crate) key: String,
    pub(crate) source: ImageSource,
}
#[derive(Default, Debug, PartialEq, Eq)]
pub(crate) struct ProjectionGaps {
    pub(crate) extra_slides: usize,
    pub(crate) banners: usize,
    pub(crate) new_window_targets: usize,
    pub(crate) unavailable_heroes: usize,
    pub(crate) rail_actions: usize,
    pub(crate) account_rails: usize,
    pub(crate) gallery_presentations: usize,
    pub(crate) live_schedule_programs: usize,
}

pub(crate) struct Presentation {
    title: String,
    status: LoadState,
    total: u32,
    cards: Vec<OwnedCard>,
    rails: Vec<OwnedRail>,
    artwork: Vec<ImageBinding>,
    gaps: ProjectionGaps,
    hero: Option<OwnedHero>,
    detail: Option<OwnedDetail>,
    selected_playlist: Option<usize>,
    search_counts: [u32; 4],
    card_indices: Option<Vec<usize>>,
    filters: Option<Vec<OwnedFilter>>,
    live_schedule: Vec<criterion_provider::LiveProgram>,
    catalog_window: Option<criterion_ui::CatalogWindow>,
}
impl Presentation {
    pub(crate) fn loading(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            status: LoadState::Loading,
            total: 0,
            cards: Vec::new(),
            rails: Vec::new(),
            artwork: Vec::new(),
            gaps: ProjectionGaps::default(),
            hero: None,
            detail: None,
            selected_playlist: None,
            search_counts: [0; 4],
            card_indices: None,
            filters: None,
            live_schedule: Vec::new(),
            catalog_window: None,
        }
    }
    pub(crate) fn set_status(&mut self, status: LoadState) {
        self.status = status;
    }
    pub(crate) fn set_options(&mut self, options: &BrowseOptions) {
        self.filters = Some(
            options
                .filter_groups
                .iter()
                .map(|group| OwnedFilter {
                    label: group.label.clone(),
                    options: group
                        .options
                        .iter()
                        .map(|option| option.label.clone())
                        .collect(),
                })
                .collect(),
        );
    }
    pub(crate) fn catalog(title: impl Into<String>, page: CatalogPage) -> Self {
        let mut projection = Self::loading(title);
        projection.total = page.total;
        projection.status = if page.items.is_empty() {
            LoadState::Empty
        } else {
            LoadState::Ready
        };
        for media in page.items {
            let target = Target::Media(media.id.clone());
            let card = projection.media_card(media, target, ImageLabel::Landscape);
            projection.cards.push(card);
        }
        projection
    }
    pub(crate) fn set_catalog_total(&mut self, total: u32) {
        self.total = total;
    }
    pub(crate) fn catalog_len(&self) -> usize {
        self.cards.len()
    }
    pub(crate) fn set_catalog_window(&mut self, window: criterion_ui::CatalogWindow) {
        self.catalog_window = Some(window);
    }
    pub(crate) fn catalog_id(&self, index: usize) -> Option<MediaId> {
        self.cards
            .get(index)
            .and_then(|c| c.target.media_id())
            .cloned()
    }
    pub(crate) fn catalog_bytes(&self) -> usize {
        vec_bytes(&self.cards)
            + self.cards.iter().map(OwnedCard::heap_bytes).sum::<usize>()
            + vec_bytes(&self.artwork)
            + self
                .artwork
                .iter()
                .map(|b| {
                    b.key.capacity()
                        + match &b.source {
                            ImageSource::Media { id, .. } => id.as_str().len(),
                            ImageSource::Editorial(i) => i.url().as_str().len(),
                        }
                })
                .sum::<usize>()
    }
    pub(crate) fn merge_catalog(&mut self, mut projection: Self) {
        self.cards.append(&mut projection.cards);
        for binding in projection.artwork {
            if !self.artwork.iter().any(|b| b.key == binding.key) {
                self.artwork.push(binding);
            }
        }
        self.cards.shrink_to_fit();
        self.artwork.shrink_to_fit();
    }
    pub(crate) fn replace_catalog(&mut self, mut projection: Self) {
        projection.filters = self.filters.take();
        if self.catalog_window.is_some() {
            projection.total = self.total;
        }
        *self = projection;
    }
    pub(crate) fn drop_catalog_prefix(&mut self, count: usize) {
        self.cards.drain(..count);
        self.artwork.retain(|b| {
            self.cards
                .iter()
                .any(|c| c.artwork.as_ref() == Some(&b.key))
        });
        self.cards.shrink_to_fit();
        self.artwork.shrink_to_fit();
    }
    pub(crate) fn artwork_bindings(&self) -> &[ImageBinding] {
        &self.artwork
    }
    #[cfg(test)]
    pub(crate) fn gaps(&self) -> &ProjectionGaps {
        &self.gaps
    }
    pub(crate) fn detail(detail: MediaDetail) -> Self {
        let mut projection = Self::loading(detail.media.title.clone());
        projection.gaps.live_schedule_programs = detail.live_schedule.len();
        projection.live_schedule = detail.live_schedule;
        let kind = match detail.media.kind {
            MediaKind::Film | MediaKind::Original | MediaKind::Live => DetailKind::Film,
            MediaKind::Supplement => DetailKind::Supplement,
            MediaKind::Collection | MediaKind::Category | MediaKind::Series => {
                DetailKind::Collection
            }
        };
        let target = Target::Media(detail.media.id.clone());
        let card = projection.media_card_for_role(
            detail.media,
            target,
            ImageLabel::Landscape,
            ImageRole::Backdrop,
        );
        projection.detail = Some(OwnedDetail {
            card,
            kind,
            description: detail.description.unwrap_or_default(),
            directors: detail.directors.join(", "),
            starring: detail.starring.join(", "),
            countries: detail.countries.join(", "),
            languages: detail.languages.join(", "),
        });
        for playlist in detail.playlists {
            let mut cards = Vec::with_capacity(playlist.items.len());
            for media in playlist.items {
                let target = Target::Media(media.id.clone());
                cards.push(projection.media_card(media, target, ImageLabel::Landscape));
            }
            projection.rails.push(OwnedRail {
                title: playlist.title,
                cards,
            });
        }
        projection.selected_playlist = (!projection.rails.is_empty()).then_some(0);
        projection.total = projection
            .rails
            .iter()
            .map(|rail| rail.cards.len() as u32)
            .sum();
        projection.status = LoadState::Ready;
        projection
    }
    pub(crate) fn select_playlist(&mut self, index: usize) {
        if self.detail.is_some() && index < self.rails.len() {
            self.selected_playlist = Some(index);
        }
    }
    #[cfg(test)]
    pub(crate) fn selected_playlist(&self) -> Option<usize> {
        self.selected_playlist
    }
    #[cfg(test)]
    pub(crate) fn title(&self) -> &str {
        &self.title
    }
    #[cfg(test)]
    pub(crate) fn status(&self) -> LoadState {
        self.status
    }
    #[cfg(test)]
    pub(crate) fn total(&self) -> u32 {
        self.total
    }
    /// Unrepresented public schedule rows; no current-time selection or playback is inferred.
    #[cfg(test)]
    pub(crate) fn live_schedule(&self) -> &[criterion_provider::LiveProgram] {
        &self.live_schedule
    }
    /// Retained display/source estimate for history eviction, not allocator RSS.
    /// Counts container capacity and owned display String capacity; opaque provider
    /// wrappers are charged for exposed UTF-8 length (their private capacity is unavailable).
    pub(crate) fn estimated_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>() + self.title.capacity();
        bytes +=
            vec_bytes(&self.cards) + self.cards.iter().map(OwnedCard::heap_bytes).sum::<usize>();
        bytes += vec_bytes(&self.rails);
        for rail in &self.rails {
            bytes += rail.title.capacity()
                + vec_bytes(&rail.cards)
                + rail.cards.iter().map(OwnedCard::heap_bytes).sum::<usize>();
        }
        bytes += vec_bytes(&self.artwork);
        for binding in &self.artwork {
            bytes += binding.key.capacity()
                + match &binding.source {
                    ImageSource::Media { id, .. } => id.as_str().len(),
                    ImageSource::Editorial(image) => image.url().as_str().len(),
                };
        }
        if let Some(indices) = &self.card_indices {
            bytes += vec_bytes(indices);
        }
        if let Some(filters) = &self.filters {
            bytes += vec_bytes(filters);
            for filter in filters {
                bytes += filter.label.capacity()
                    + vec_bytes(&filter.options)
                    + filter.options.iter().map(String::capacity).sum::<usize>();
            }
        }
        if let Some(hero) = &self.hero {
            bytes += hero.card.heap_bytes()
                + hero.description.capacity()
                + hero.action.capacity()
                + hero.background.capacity()
                + hero.logo.as_ref().map_or(0, String::capacity);
        }
        if let Some(detail) = &self.detail {
            bytes += detail.card.heap_bytes()
                + detail.description.capacity()
                + detail.directors.capacity()
                + detail.starring.capacity()
                + detail.countries.capacity()
                + detail.languages.capacity();
        }
        bytes += vec_bytes(&self.live_schedule);
        for program in &self.live_schedule {
            bytes += program.title.capacity()
                + program.media_id.as_ref().map_or(0, |id| id.as_str().len())
                + program.starts_at.as_str().len()
                + program.ends_at.as_str().len();
        }
        bytes
    }
    pub(crate) fn search(results: SearchResults, group: SearchGroup) -> Self {
        let mut projection = Self::loading("Search");
        for count in results.type_counts {
            projection.search_counts[0] += count.count;
            match count.kind {
                MediaKind::Film => projection.search_counts[1] = count.count,
                MediaKind::Collection => projection.search_counts[2] = count.count,
                MediaKind::Supplement => projection.search_counts[3] = count.count,
                MediaKind::Category | MediaKind::Series | MediaKind::Original | MediaKind::Live => {
                }
            }
        }
        for media in results.items {
            let target = Target::Media(media.id.clone());
            let card = projection.media_card(media, target, ImageLabel::Landscape);
            projection.cards.push(card);
        }
        projection.card_indices = Some(Vec::new());
        projection.set_group(group);
        projection
    }
    pub(crate) fn set_group(&mut self, group: SearchGroup) {
        let Some(indices) = self.card_indices.as_mut() else {
            return;
        };
        indices.clear();
        let (index, kind) = match group {
            SearchGroup::All => (0, None),
            SearchGroup::Films => (1, Some(MediaKind::Film)),
            SearchGroup::Collections => (2, Some(MediaKind::Collection)),
            SearchGroup::Supplements => (3, Some(MediaKind::Supplement)),
        };
        indices.extend(
            self.cards
                .iter()
                .enumerate()
                .filter(|(_, card)| kind.is_none() || card.kind == kind)
                .map(|(index, _)| index),
        );
        self.total = self.search_counts[index];
        self.status = if indices.is_empty() {
            LoadState::Empty
        } else {
            LoadState::Ready
        };
    }
    pub(crate) fn discovery(page: DiscoveryPage) -> Self {
        let mut projection = Self::loading("");
        let mut saw_slideshow = false;
        for block in page.blocks {
            match block {
                DiscoveryBlock::Slideshow { slides, .. } => {
                    if saw_slideshow {
                        projection.gaps.extra_slides += slides.len();
                    } else {
                        saw_slideshow = true;
                        projection.gaps.extra_slides += slides.len().saturating_sub(1);
                        if let Some(slide) = slides.into_iter().next() {
                            projection.admit_hero(slide);
                        }
                    }
                }
                DiscoveryBlock::Banner {
                    opens_new_window,
                    target,
                    ..
                } => {
                    projection.gaps.banners += 1;
                    projection.gaps.new_window_targets +=
                        usize::from(opens_new_window && target.is_some());
                }
                DiscoveryBlock::Rail {
                    header,
                    cards,
                    image_label,
                    source,
                    cta,
                    target,
                    opens_new_window,
                    ..
                } => {
                    projection.gaps.gallery_presentations += 1;
                    projection.gaps.rail_actions += usize::from(cta.is_some() || target.is_some());
                    projection.gaps.new_window_targets +=
                        usize::from(opens_new_window && target.is_some());
                    projection.gaps.account_rails += usize::from(matches!(
                        source,
                        RailSource::Watchlist | RailSource::ContinueWatching
                    ));
                    let mut projected = Vec::with_capacity(cards.len());
                    for card in cards {
                        projected.push(projection.media_card(
                            card.media,
                            Target::Content(card.target),
                            image_label,
                        ));
                    }
                    projection.rails.push(OwnedRail {
                        title: header.unwrap_or_default(),
                        cards: projected,
                    });
                }
                DiscoveryBlock::Navigation { header, items, .. } => {
                    projection.gaps.gallery_presentations += 1;
                    let mut cards = Vec::with_capacity(items.len());
                    for item in items {
                        if item.opens_new_window {
                            projection.gaps.new_window_targets += 1;
                            continue;
                        }
                        let image = desktop_image(&item.artwork).cloned();
                        let artwork =
                            image.map(|image| projection.bind_image(ImageSource::Editorial(image)));
                        cards.push(OwnedCard {
                            target: Target::Content(item.target),
                            kind: None,
                            title: item.label,
                            year: String::new(),
                            duration_seconds: 0,
                            artwork,
                        });
                    }
                    projection.rails.push(OwnedRail {
                        title: header.unwrap_or_default(),
                        cards,
                    });
                }
            }
        }
        projection.total = projection
            .rails
            .iter()
            .map(|rail| rail.cards.len() as u32)
            .sum();
        projection.status = if projection.total == 0 && projection.hero.is_none() {
            LoadState::Empty
        } else {
            LoadState::Ready
        };
        projection
    }
    fn admit_hero(&mut self, slide: DiscoverySlide) {
        if slide.opens_new_window {
            self.gaps.new_window_targets += usize::from(slide.target.is_some());
            self.gaps.unavailable_heroes += 1;
            return;
        }
        let (Some(target), Some(action), Some(background)) = (
            slide.target,
            slide.cta,
            desktop_image(&slide.artwork).cloned(),
        ) else {
            self.gaps.unavailable_heroes += 1;
            return;
        };
        let background = self.bind_image(ImageSource::Editorial(background));
        let logo = slide
            .artwork
            .logo
            .map(|image| self.bind_image(ImageSource::Editorial(image)));
        self.hero = Some(OwnedHero {
            card: OwnedCard {
                target: Target::Content(target),
                kind: None,
                title: slide.title.unwrap_or_default(),
                year: String::new(),
                duration_seconds: 0,
                artwork: None,
            },
            description: slide.title_prefix.unwrap_or_default(),
            action,
            background,
            logo,
        });
    }
    fn media_card(&mut self, media: MediaSummary, target: Target, label: ImageLabel) -> OwnedCard {
        self.media_card_for_role(media, target, label, ImageRole::Card)
    }
    fn media_card_for_role(
        &mut self,
        media: MediaSummary,
        target: Target,
        label: ImageLabel,
        role: ImageRole,
    ) -> OwnedCard {
        let artwork = self.bind_image(ImageSource::Media {
            id: media.id,
            label,
            role,
        });
        OwnedCard {
            target,
            kind: Some(media.kind),
            title: media.title,
            year: media
                .release_date
                .map(|date| date.chars().take(4).collect())
                .unwrap_or_default(),
            duration_seconds: media.duration_seconds,
            artwork: Some(artwork),
        }
    }
    fn bind_image(&mut self, source: ImageSource) -> String {
        if let Some(binding) = self.artwork.iter().find(|binding| binding.source == source) {
            return binding.key.clone();
        }
        let mut digest = aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA256);
        digest.update(b"criterion-artwork-v2\0");
        match &source {
            ImageSource::Media { id, label, role } => {
                digest.update(b"media\0");
                digest.update(match role {
                    ImageRole::Card => b"card\0",
                    ImageRole::Backdrop => b"backdrop\0",
                });
                digest.update(id.as_str().as_bytes());
                digest.update(b"\0");
                digest.update(label.as_str().as_bytes());
            }
            ImageSource::Editorial(image) => {
                digest.update(b"editorial\0");
                digest.update(image.url().as_str().as_bytes());
            }
        }
        let mut key = String::with_capacity(64);
        for byte in digest.finish().as_ref() {
            write!(&mut key, "{byte:02x}").expect("formatting into an owned String");
        }
        self.artwork.push(ImageBinding {
            key: key.clone(),
            source,
        });
        key
    }
    pub(crate) fn with_view<R>(
        &self,
        login: criterion_ui::LoginView<'_>,
        consume: impl FnOnce(&ViewData<'_>) -> R,
    ) -> R {
        let cards: Vec<_> = match &self.card_indices {
            Some(indices) => indices
                .iter()
                .map(|index| self.cards[*index].view())
                .collect(),
            None => self.cards.iter().map(OwnedCard::view).collect(),
        };
        let rail_cards: Vec<Vec<_>> = self
            .rails
            .iter()
            .map(|rail| rail.cards.iter().map(OwnedCard::view).collect())
            .collect();
        let rails: Vec<_> = self
            .rails
            .iter()
            .zip(&rail_cards)
            .map(|(rail, cards)| Rail {
                title: &rail.title,
                cards,
            })
            .collect();
        let hero = self.hero.as_ref().map(|hero| Hero {
            card: hero.card.view(),
            description: &hero.description,
            action: &hero.action,
            action_kind: HeroAction::Open,
            background_key: Some(&hero.background),
            title_logo_key: hero.logo.as_deref(),
        });
        let detail = self.detail.as_ref().map(|detail| Detail {
            card: detail.card.view(),
            directors: &detail.directors,
            description: &detail.description,
            starring: &detail.starring,
            countries: &detail.countries,
            languages: &detail.languages,
            primary_action: "WATCH NOW",
            kind: detail.kind,
        });
        let options: Vec<Vec<&str>> = self
            .filters
            .iter()
            .flatten()
            .map(|group| group.options.iter().map(String::as_str).collect())
            .collect();
        let groups: Vec<_> = self
            .filters
            .iter()
            .flatten()
            .zip(&options)
            .map(|(group, options)| FilterGroup {
                label: &group.label,
                options,
            })
            .collect();
        let filters = self
            .filters
            .as_ref()
            .map(|_| FilterMenu { groups: &groups });
        consume(&ViewData {
            filters,
            detail,
            hero,
            rails: &rails,
            title: &self.title,
            status: self.status,
            total: self.total,
            cards: &cards,
            catalog: self.catalog_window,
            search_counts: self.search_counts,
            login,
        })
    }
}

// Select only a supplied desktop source for the 1920-wide logical canvas.
// Prefer the smallest adequate width, otherwise the largest supplied desktop width.
fn desktop_image(artwork: &DiscoveryArtwork) -> Option<&EditorialImage> {
    artwork
        .desktop
        .iter()
        .filter(|image| image.width >= 1920)
        .min_by_key(|image| image.width)
        .or_else(|| artwork.desktop.iter().max_by_key(|image| image.width))
        .map(|image| &image.image)
}
struct OwnedFilter {
    label: String,
    options: Vec<String>,
}
struct OwnedDetail {
    card: OwnedCard,
    kind: DetailKind,
    description: String,
    directors: String,
    starring: String,
    countries: String,
    languages: String,
}
struct OwnedHero {
    card: OwnedCard,
    description: String,
    action: String,
    background: String,
    logo: Option<String>,
}
struct OwnedRail {
    title: String,
    cards: Vec<OwnedCard>,
}

struct OwnedCard {
    target: Target,
    kind: Option<MediaKind>,
    title: String,
    year: String,
    duration_seconds: u32,
    artwork: Option<String>,
}
fn vec_bytes<T>(values: &Vec<T>) -> usize {
    values.capacity() * std::mem::size_of::<T>()
}
fn target_bytes(target: &Target) -> usize {
    use criterion_provider::ContentTarget;
    match target {
        Target::Media(id) => id.as_str().len(),
        Target::Content(ContentTarget::Media { id, slug, .. }) => {
            id.as_str().len() + slug.as_str().len()
        }
        Target::Content(ContentTarget::Discover(slug)) => slug.as_str().len(),
        Target::Content(_) => 0,
    }
}
impl OwnedCard {
    fn heap_bytes(&self) -> usize {
        self.title.capacity()
            + self.year.capacity()
            + self.artwork.as_ref().map_or(0, String::capacity)
            + target_bytes(&self.target)
    }
    fn view(&self) -> Card<'_> {
        Card {
            key: &self.target,
            artwork_key: self.artwork.as_deref(),
            title: &self.title,
            year: &self.year,
            duration_seconds: self.duration_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Presentation;
    use criterion_artwork::ImageRole;
    use criterion_ui::LoadState;

    #[test]
    fn loading_and_failure_lend_the_requested_title_without_inventing_content() {
        let mut presentation = Presentation::loading("Criterion 24/7");
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            assert_eq!(view.title, "Criterion 24/7");
            assert_eq!(view.status, LoadState::Loading);
            assert_eq!(view.total, 0);
            assert!(view.cards.is_empty());
            assert!(view.rails.is_empty());
            assert!(view.hero.is_none());
            assert!(view.detail.is_none());
        });
        presentation.set_status(LoadState::Offline);
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            assert_eq!(view.status, LoadState::Offline)
        });
    }

    #[test]
    fn catalog_lends_metadata_and_binds_an_exact_landscape_source() {
        use criterion_provider::{CatalogPage, ImageLabel, MediaId, MediaKind, MediaSummary};
        use criterion_ui::{LoadState, Target};
        let presentation = Presentation::catalog(
            "All Films",
            CatalogPage {
                items: vec![MediaSummary {
                    id: MediaId::new("ABCDEF12").unwrap(),
                    title: "Fixture Film".into(),
                    kind: MediaKind::Film,
                    duration_seconds: 5400,
                    release_date: Some("1980-07-01".into()),
                }],
                total: 37,
                next_cursor: None,
            },
        );
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            assert_eq!(view.title, "All Films");
            assert_eq!(view.status, LoadState::Ready);
            assert_eq!(view.total, 37);
            assert_eq!(view.cards.len(), 1);
            assert_eq!(
                *view.cards[0].key,
                Target::Media(MediaId::new("ABCDEF12").unwrap())
            );
            assert_eq!(view.cards[0].title, "Fixture Film");
            assert_eq!(view.cards[0].year, "1980");
            assert_eq!(view.cards[0].duration_seconds, 5400);
            assert_eq!(
                view.cards[0].artwork_key,
                Some("f7d84893135109963a5083d22d8d88d0c9568a3ba603d4cf57a4a980e6040c01")
            );
        });
        assert_eq!(presentation.artwork_bindings().len(), 1);
        assert_eq!(
            presentation.artwork_bindings()[0].source,
            super::ImageSource::Media {
                id: MediaId::new("ABCDEF12").unwrap(),
                label: ImageLabel::Landscape,
                role: ImageRole::Card,
            }
        );
    }

    #[test]
    fn search_groups_keep_provider_counts_and_all_includes_nonfilm_kinds() {
        use criterion_provider::{KindCount, MediaId, MediaKind, MediaSummary, SearchResults};
        use criterion_ui::SearchGroup;
        let items = [
            ("ABCDEF11", "Film fixture", MediaKind::Film),
            ("ABCDEF12", "Category fixture", MediaKind::Category),
            ("ABCDEF13", "Collection fixture", MediaKind::Collection),
            ("ABCDEF14", "Supplement fixture", MediaKind::Supplement),
            ("ABCDEF15", "Series fixture", MediaKind::Series),
            ("ABCDEF16", "Original fixture", MediaKind::Original),
        ]
        .into_iter()
        .map(|(id, title, kind)| MediaSummary {
            id: MediaId::new(id).unwrap(),
            title: title.into(),
            kind,
            duration_seconds: 0,
            release_date: None,
        })
        .collect();
        let mut presentation = Presentation::search(
            SearchResults {
                items,
                type_counts: vec![
                    KindCount {
                        kind: MediaKind::Film,
                        count: 11,
                    },
                    KindCount {
                        kind: MediaKind::Collection,
                        count: 4,
                    },
                    KindCount {
                        kind: MediaKind::Supplement,
                        count: 3,
                    },
                    KindCount {
                        kind: MediaKind::Category,
                        count: 2,
                    },
                    KindCount {
                        kind: MediaKind::Original,
                        count: 2,
                    },
                ],
            },
            SearchGroup::All,
        );
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            assert_eq!(view.search_counts, [22, 11, 4, 3]);
            assert_eq!(view.total, 22);
            assert_eq!(
                view.cards.iter().map(|card| card.title).collect::<Vec<_>>(),
                [
                    "Film fixture",
                    "Category fixture",
                    "Collection fixture",
                    "Supplement fixture",
                    "Series fixture",
                    "Original fixture"
                ]
            );
        });
        for (group, title, count) in [
            (SearchGroup::Films, "Film fixture", 11),
            (SearchGroup::Collections, "Collection fixture", 4),
            (SearchGroup::Supplements, "Supplement fixture", 3),
        ] {
            presentation.set_group(group);
            presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
                assert_eq!(view.cards.len(), 1);
                assert_eq!(view.cards[0].title, title);
                assert_eq!(view.total, count);
                assert_eq!(view.status, LoadState::Ready);
            });
        }
        assert_eq!(presentation.artwork_bindings().len(), 6);
    }

    #[test]
    fn original_detail_uses_the_single_item_layout_and_preserves_its_media_identity() {
        use criterion_provider::{MediaDetail, MediaId, MediaKind, MediaSummary};
        use criterion_ui::{DetailKind, Target};
        let id = MediaId::new("ABCDEF16").unwrap();
        let presentation = Presentation::detail(MediaDetail {
            media: MediaSummary {
                id: id.clone(),
                title: "Original fixture".into(),
                kind: MediaKind::Original,
                duration_seconds: 71,
                release_date: None,
            },
            description: None,
            directors: vec![],
            starring: vec![],
            countries: vec![],
            languages: vec![],
            genres: vec![],
            content_warnings: None,
            commentary_tracks: vec![],
            first_playlist_sortable: false,
            live_schedule: vec![],
            playlists: vec![],
        });
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.kind, DetailKind::Film);
            assert_eq!(detail.card.key, &Target::Media(id));
            assert_eq!(detail.card.duration_seconds, 71);
            assert!(view.rails.is_empty());
        });
    }

    fn artwork(filename: &str) -> criterion_provider::DiscoveryArtwork {
        use criterion_provider::{DiscoveryArtwork, EditorialImage, ResponsiveImage};
        DiscoveryArtwork {
            desktop: vec![ResponsiveImage {
                width: 1920,
                image: EditorialImage::new(
                    "https://cc.criterion.com/uploads/storyBlocks/493/thumbnails/",
                    filename,
                )
                .unwrap(),
            }],
            mobile: vec![],
            logo: None,
        }
    }
    fn gallery() -> criterion_provider::GalleryPresentation {
        criterion_provider::GalleryPresentation {
            aspect_ratio_percent: 56.25,
            cards_per_view: 4,
            layout: criterion_provider::GalleryLayout::Rail,
            variant: 1,
        }
    }

    #[test]
    fn discovery_preserves_interleaved_media_and_nonmedia_identity_and_sources() {
        use criterion_provider::{
            ContentTarget, DiscoveryBlock, DiscoveryCard, DiscoveryNavItem, DiscoveryPage,
            ImageLabel, MediaId, MediaKind, MediaSummary, RailSource,
        };
        use criterion_ui::Target;
        let page = DiscoveryPage {
            blocks: vec![
                DiscoveryBlock::Rail {
                    id: 537,
                    header: Some("Popular Movies".into()),
                    cta: None,
                    target: None,
                    opens_new_window: false,
                    source: RailSource::Provided { feed_id: None },
                    image_label: ImageLabel::Regalia,
                    presentation: gallery(),
                    cards: vec![DiscoveryCard {
                        media: MediaSummary {
                            id: MediaId::new("qvwT6mJ4").unwrap(),
                            title: "The Hitcher".into(),
                            kind: MediaKind::Film,
                            duration_seconds: 5851,
                            release_date: Some("1986-01-01".into()),
                        },
                        target: ContentTarget::parse("/films/qvwT6mJ4/the-hitcher").unwrap(),
                    }],
                },
                DiscoveryBlock::Navigation {
                    id: 99,
                    header: Some("Explore".into()),
                    presentation: gallery(),
                    items: vec![DiscoveryNavItem {
                        id: 100,
                        label: "International Cinema".into(),
                        target: ContentTarget::parse("/discover/international-cinema").unwrap(),
                        opens_new_window: false,
                        artwork: artwork("international.webp"),
                    }],
                },
                DiscoveryBlock::Rail {
                    id: 817,
                    header: Some("Leaving October 31".into()),
                    cta: None,
                    target: None,
                    opens_new_window: false,
                    source: RailSource::Provided { feed_id: None },
                    image_label: ImageLabel::Landscape,
                    presentation: gallery(),
                    cards: vec![],
                },
            ],
        };
        let presentation = Presentation::discovery(page);
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            assert!(
                view.hero.is_none(),
                "a rail is never manufactured into a hero"
            );
            assert_eq!(view.status, LoadState::Ready);
            assert_eq!(
                view.rails.iter().map(|rail| rail.title).collect::<Vec<_>>(),
                ["Popular Movies", "Explore", "Leaving October 31"]
            );
            assert_eq!(
                view.rails[0].cards[0].key,
                &Target::Content(ContentTarget::parse("/films/qvwT6mJ4/the-hitcher").unwrap())
            );
            assert_eq!(
                view.rails[1].cards[0].key,
                &Target::Content(ContentTarget::parse("/discover/international-cinema").unwrap())
            );
            assert_eq!(view.rails[1].cards[0].title, "International Cinema");
            assert_eq!(view.rails[1].cards[0].year, "");
        });
        assert_eq!(presentation.artwork_bindings().len(), 2);
        assert_eq!(
            presentation.artwork_bindings()[0].source,
            super::ImageSource::Media {
                id: MediaId::new("qvwT6mJ4").unwrap(),
                label: ImageLabel::Regalia,
                role: ImageRole::Card,
            }
        );
        assert_eq!(
            presentation.artwork_bindings()[1].source,
            super::ImageSource::Editorial(artwork("international.webp").desktop.remove(0).image)
        );
    }

    #[test]
    fn discovery_uses_a_supplied_logo_hero_and_reports_unrepresented_blocks() {
        use criterion_provider::{
            ContentTarget, DiscoveryBlock, DiscoveryPage, DiscoverySlide, EditorialImage,
            ImageLabel, RailSource,
        };
        use criterion_ui::{HeroAction, Target};
        let mut hero_artwork = artwork("POSSESSIONS_collection_hero_wide_1920x0.webp");
        hero_artwork.logo = Some(
            EditorialImage::new(
                "https://cc.criterion.com/uploads/storyBlocks/493/thumbnails/",
                "POSSESSIONS_collection_logo_default_760x0.webp",
            )
            .unwrap(),
        );
        let presentation = Presentation::discovery(DiscoveryPage {
            blocks: vec![
                DiscoveryBlock::Slideshow {
                    id: 493,
                    slides: vec![
                        DiscoverySlide {
                            id: 1595,
                            title: None,
                            title_prefix: None,
                            cta: Some("See more".into()),
                            target: Some(
                                ContentTarget::parse("/collections/fhQRpxw4/possessions").unwrap(),
                            ),
                            opens_new_window: false,
                            artwork: hero_artwork.clone(),
                        },
                        DiscoverySlide {
                            id: 1599,
                            title: Some("Made for TV Terror".into()),
                            title_prefix: None,
                            cta: Some("See more".into()),
                            target: Some(
                                ContentTarget::parse("/collections/p753ts71/made-for-tv-terror")
                                    .unwrap(),
                            ),
                            opens_new_window: false,
                            artwork: artwork("terror.webp"),
                        },
                    ],
                },
                DiscoveryBlock::Banner {
                    id: 111,
                    alt: Some("Banner fixture".into()),
                    target: None,
                    opens_new_window: false,
                    subscription_promo: false,
                    artwork: artwork("banner.webp"),
                },
                DiscoveryBlock::Rail {
                    id: 537,
                    header: Some("Popular Movies".into()),
                    cta: Some("See more".into()),
                    target: Some(ContentTarget::parse("/categories/QGtQj94z/popular-now").unwrap()),
                    opens_new_window: false,
                    source: RailSource::Watchlist,
                    image_label: ImageLabel::Regalia,
                    presentation: gallery(),
                    cards: vec![],
                },
            ],
        });
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            let hero = view
                .hero
                .as_ref()
                .expect("only the supplied slide supplies a hero");
            assert_eq!(
                hero.card.key,
                &Target::Content(
                    ContentTarget::parse("/collections/fhQRpxw4/possessions").unwrap()
                )
            );
            assert_eq!(
                hero.card.title, "",
                "a logo-only slide has no invented text title"
            );
            assert_eq!(hero.description, "");
            assert_eq!(hero.action, "See more");
            assert_eq!(hero.action_kind, HeroAction::Open);
            assert!(hero.background_key.is_some());
            assert!(hero.title_logo_key.is_some());
            assert_eq!(view.status, LoadState::Ready);
        });
        assert_eq!(
            presentation.gaps(),
            &super::ProjectionGaps {
                extra_slides: 1,
                banners: 1,
                rail_actions: 1,
                account_rails: 1,
                gallery_presentations: 1,
                ..Default::default()
            }
        );
        assert_eq!(
            presentation.artwork_bindings().len(),
            2,
            "unrepresented slides/banners are not loaded"
        );
        assert_eq!(
            presentation.artwork_bindings()[0].source,
            super::ImageSource::Editorial(hero_artwork.desktop.remove(0).image)
        );
        assert_eq!(
            presentation.artwork_bindings()[1].source,
            super::ImageSource::Editorial(hero_artwork.logo.unwrap())
        );
    }

    #[test]
    fn detail_lends_joined_metadata_and_exact_playlist_tabs_without_fabricated_routes() {
        use criterion_provider::{MediaDetail, MediaId, MediaKind, MediaSummary, Playlist};
        use criterion_ui::{DetailKind, Target};
        let summary = |id: &str, title: &str, kind| MediaSummary {
            id: MediaId::new(id).unwrap(),
            title: title.into(),
            kind,
            duration_seconds: 5851,
            release_date: Some("1986-01-01".into()),
        };
        let mut presentation = Presentation::detail(MediaDetail {
            media: summary("qvwT6mJ4", "The Hitcher", MediaKind::Film),
            description: Some("Fixture synopsis".into()),
            directors: vec!["Director One".into(), "Director Two".into()],
            starring: vec!["Actor One".into(), "Actor Two".into()],
            countries: vec!["US".into(), "FR".into()],
            languages: vec!["English".into()],
            genres: vec![],
            content_warnings: None,
            commentary_tracks: vec![],
            first_playlist_sortable: false,
            live_schedule: vec![],
            playlists: vec![
                Playlist {
                    key: "supplements".into(),
                    title: "Supplements".into(),
                    id: MediaId::new("ABCDEF11").unwrap(),
                    items: vec![summary(
                        "ABCDEF12",
                        "Supplement fixture",
                        MediaKind::Supplement,
                    )],
                },
                Playlist {
                    key: "collections".into(),
                    title: "Collections".into(),
                    id: MediaId::new("ABCDEF13").unwrap(),
                    items: vec![summary("fhQRpxw4", "Possessions", MediaKind::Collection)],
                },
            ],
        });
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(detail.kind, DetailKind::Film);
            assert_eq!(
                detail.card.key,
                &Target::Media(MediaId::new("qvwT6mJ4").unwrap())
            );
            assert_eq!(detail.description, "Fixture synopsis");
            assert_eq!(detail.directors, "Director One, Director Two");
            assert_eq!(detail.starring, "Actor One, Actor Two");
            assert_eq!(detail.countries, "US, FR");
            assert_eq!(detail.languages, "English");
            assert_eq!(detail.primary_action, "WATCH NOW");
            assert_eq!(
                view.rails.iter().map(|rail| rail.title).collect::<Vec<_>>(),
                ["Supplements", "Collections"]
            );
            assert_eq!(
                view.rails[0].cards[0].key,
                &Target::Media(MediaId::new("ABCDEF12").unwrap())
            );
            assert_eq!(
                view.rails[1].cards[0].key,
                &Target::Media(MediaId::new("fhQRpxw4").unwrap())
            );
        });
        assert_eq!(presentation.selected_playlist(), Some(0));
        presentation.select_playlist(1);
        assert_eq!(presentation.selected_playlist(), Some(1));
        presentation.select_playlist(2);
        assert_eq!(
            presentation.selected_playlist(),
            Some(1),
            "a nonexistent tab cannot replace the selected playlist"
        );
    }

    #[test]
    fn filter_labels_are_owned_after_source_options_leave_scope_and_keep_index_order() {
        use criterion_provider::{
            BrowseOptions, FilterGroup, FilterOption, FilterOptions, FilterValue,
        };
        let mut presentation = Presentation::loading("All Films");
        {
            let options = BrowseOptions {
                filter_groups: vec![
                    FilterOptions {
                        group: FilterGroup::Genres,
                        label: "Genres".into(),
                        options: vec![
                            FilterOption {
                                label: "Drama".into(),
                                value: FilterValue::new("drama").unwrap(),
                            },
                            FilterOption {
                                label: "Comedy".into(),
                                value: FilterValue::new("comedy").unwrap(),
                            },
                        ],
                    },
                    FilterOptions {
                        group: FilterGroup::Decades,
                        label: "Decades".into(),
                        options: vec![FilterOption {
                            label: "1980s".into(),
                            value: FilterValue::new("1980s").unwrap(),
                        }],
                    },
                ],
                sort_options: vec![],
            };
            presentation.set_options(&options);
        }
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            let groups = view.filters.as_ref().unwrap().groups;
            assert_eq!(
                groups.iter().map(|group| group.label).collect::<Vec<_>>(),
                ["Genres", "Decades"]
            );
            assert_eq!(groups[0].options, ["Drama", "Comedy"]);
            assert_eq!(groups[1].options, ["1980s"]);
        });
    }

    #[test]
    fn memory_estimate_accounts_for_reserved_owned_text_and_nested_filter_labels() {
        use criterion_provider::{
            BrowseOptions, FilterGroup, FilterOption, FilterOptions, FilterValue,
        };
        let mut title = String::with_capacity(8192);
        title.push_str("All Films");
        let mut presentation = Presentation::loading(title);
        assert_eq!(presentation.title(), "All Films");
        assert_eq!(presentation.status(), LoadState::Loading);
        assert_eq!(presentation.total(), 0);
        let before = presentation.estimated_bytes();
        assert!(
            before >= 8192,
            "history budgets must include reserved allocation, not just text length"
        );
        presentation.set_options(&BrowseOptions {
            filter_groups: vec![FilterOptions {
                group: FilterGroup::Genres,
                label: "Genres".into(),
                options: vec![
                    FilterOption {
                        label: "a".repeat(512),
                        value: FilterValue::new("a").unwrap(),
                    },
                    FilterOption {
                        label: "b".repeat(512),
                        value: FilterValue::new("b").unwrap(),
                    },
                ],
            }],
            sort_options: vec![],
        });
        assert!(
            presentation.estimated_bytes() >= before + 1024,
            "owned nested strings belong to the same cache budget"
        );
    }

    #[test]
    fn live_schedule_remains_explicit_without_inventing_ids_or_runtime() {
        use criterion_provider::{
            LiveProgram, MediaDetail, MediaId, MediaKind, MediaSummary, UtcTimestamp,
        };
        let presentation = Presentation::detail(MediaDetail {
            media: MediaSummary {
                id: MediaId::new("1emmgvqX").unwrap(),
                title: "Criterion 24/7".into(),
                kind: MediaKind::Live,
                duration_seconds: 0,
                release_date: None,
            },
            description: None,
            directors: vec![],
            starring: vec![],
            countries: vec![],
            languages: vec![],
            genres: vec![],
            content_warnings: None,
            commentary_tracks: vec![],
            playlists: vec![],
            first_playlist_sortable: false,
            live_schedule: vec![LiveProgram {
                media_id: None,
                title: "Unlinked program fixture".into(),
                kind: MediaKind::Film,
                duration_seconds: 3600,
                starts_at: UtcTimestamp::new("2026-10-09T15:00:00Z").unwrap(),
                ends_at: UtcTimestamp::new("2026-10-09T16:00:00Z").unwrap(),
            }],
        });
        presentation.with_view(criterion_ui::LoginView::SignedOut, |view| {
            let detail = view.detail.as_ref().unwrap();
            assert_eq!(
                detail.card.duration_seconds, 0,
                "absence remains explicit for the renderer"
            );
            assert_eq!(detail.card.year, "");
            assert!(
                view.rails.is_empty(),
                "schedule rows are not fabricated into catalog cards"
            );
        });
        assert_eq!(presentation.gaps().live_schedule_programs, 1);
        assert_eq!(presentation.live_schedule().len(), 1);
        assert!(presentation.live_schedule()[0].media_id.is_none());
        assert_eq!(
            presentation.live_schedule()[0].title,
            "Unlinked program fixture"
        );
        assert_eq!(
            presentation.live_schedule()[0].starts_at.as_str(),
            "2026-10-09T15:00:00Z"
        );
    }

    #[test]
    fn detail_backdrop_and_same_media_playlist_cards_have_distinct_bounded_sources() {
        use criterion_provider::{
            ImageLabel, MediaDetail, MediaId, MediaKind, MediaSummary, Playlist,
        };
        let summary = |id: &str| MediaSummary {
            id: MediaId::new(id).unwrap(),
            title: "Fixture film".into(),
            kind: MediaKind::Film,
            duration_seconds: 5400,
            release_date: None,
        };
        let presentation = Presentation::detail(MediaDetail {
            media: summary("ABCDEF12"),
            description: None,
            directors: vec![],
            starring: vec![],
            countries: vec![],
            languages: vec![],
            genres: vec![],
            content_warnings: None,
            commentary_tracks: vec![],
            first_playlist_sortable: false,
            live_schedule: vec![],
            playlists: vec![Playlist {
                id: MediaId::new("ABCDEF11").unwrap(),
                key: "fixture".into(),
                title: "Related".into(),
                items: vec![
                    summary("ABCDEF12"),
                    summary("ABCDEF13"),
                    summary("ABCDEF12"),
                ],
            }],
        });
        let bindings = presentation.artwork_bindings();
        assert_eq!(bindings.len(), 3);
        assert_eq!(
            bindings[0].source,
            super::ImageSource::Media {
                id: MediaId::new("ABCDEF12").unwrap(),
                label: ImageLabel::Landscape,
                role: ImageRole::Backdrop,
            }
        );
        assert_eq!(
            bindings[1].source,
            super::ImageSource::Media {
                id: MediaId::new("ABCDEF12").unwrap(),
                label: ImageLabel::Landscape,
                role: ImageRole::Card,
            }
        );
        assert_eq!(
            bindings[2].source,
            super::ImageSource::Media {
                id: MediaId::new("ABCDEF13").unwrap(),
                label: ImageLabel::Landscape,
                role: ImageRole::Card,
            }
        );
        presentation.with_view(criterion_ui::LoginView::SignedOut, |data| {
            let detail_key = data.detail.as_ref().unwrap().card.artwork_key.unwrap();
            let cards = data.rails[0].cards;
            assert_eq!(detail_key, bindings[0].key);
            assert_ne!(detail_key, cards[0].artwork_key.unwrap());
            assert_eq!(cards[0].artwork_key, cards[2].artwork_key);
            assert_eq!(cards[0].artwork_key.unwrap(), bindings[1].key);
            assert_eq!(cards[1].artwork_key.unwrap(), bindings[2].key);
        });
    }
}
