// SPDX-License-Identifier: GPL-3.0-or-later
//! Native root metadata and child summaries remain separate from website Detail.
use super::{
    ImageSource, NativeActivation, OwnedCard, OwnedDetail, OwnedRail, Presentation, ProjectionLimit,
};
use crate::native_resume::{PositionsSnapshot, resolve_series};
use criterion_account::{MediaKind, NativePlaylist};
use criterion_artwork::ImageRole;
use criterion_provider::ImageLabel;
use criterion_ui::{DetailKind, LoadState, Target};
mod sort;

const MAX_NATIVE_ITEMS: usize = 512;
const MAX_NATIVE_GROUPS: usize = 32;
const MAX_NATIVE_SEASONS: usize = 64;
const MAX_NATIVE_BYTES: usize = 512 * 1024;

pub(super) struct NativeDetailState {
    kind: MediaKind,
    pub(super) seasons_tab: Option<usize>,
    pub(super) selected_season: usize,
    pub(super) seasons: Vec<OwnedSeason>,
    pub(super) featured: Option<OwnedFeatured>,
    private_resume: bool,
    sort: Option<sort::NativeSortState>,
}
pub(super) struct OwnedFeatured {
    pub(super) title: Option<String>,
    pub(super) cards: Vec<OwnedCard>,
}
pub(super) struct OwnedSeason {
    pub(super) number: i32,
    pub(super) rail: OwnedRail,
}
impl NativeDetailState {
    pub(super) fn heap_bytes(&self) -> usize {
        super::vec_bytes(&self.seasons)
            + self
                .sort
                .as_ref()
                .map_or(0, sort::NativeSortState::heap_bytes)
            + self.featured.as_ref().map_or(0, |featured| {
                featured.title.as_ref().map_or(0, String::capacity)
                    + super::vec_bytes(&featured.cards)
                    + featured
                        .cards
                        .iter()
                        .map(OwnedCard::heap_bytes)
                        .sum::<usize>()
            })
            + self
                .seasons
                .iter()
                .map(|season| {
                    season.rail.title.capacity()
                        + super::vec_bytes(&season.rail.cards)
                        + season
                            .rail
                            .cards
                            .iter()
                            .map(OwnedCard::heap_bytes)
                            .sum::<usize>()
                })
                .sum::<usize>()
    }
}
impl Presentation {
    /// Ordinary native Detail identity stays distinct from its selected Episode.
    pub(crate) fn native_root(&self) -> Option<(&criterion_provider::MediaId, MediaKind)> {
        let detail = self.detail.as_ref()?;
        let native = detail.native.as_ref()?;
        if native.kind == MediaKind::Live {
            return None;
        }
        Some((detail.card.target.media_id()?, native.kind))
    }

    pub(crate) fn native_detail(
        detail: criterion_account::NativeDetail,
        positions: Option<&PositionsSnapshot>,
    ) -> Result<Self, ProjectionLimit> {
        if detail.estimated_bytes() > MAX_NATIVE_BYTES {
            return Err(ProjectionLimit::TooLarge);
        }
        check_cardinality(&detail)?;
        let mut projection = Self::loading(detail.media.title.clone());
        let kind = detail_kind(detail.media.kind);
        let resolution =
            (detail.media.kind == MediaKind::Series).then(|| resolve_series(&detail, positions));
        let primary_playback_target = match detail.media.kind {
            MediaKind::Film | MediaKind::Original | MediaKind::Episode | MediaKind::Supplement => {
                Some(detail.media.id.clone())
            }
            MediaKind::Series => resolution.as_ref().and_then(|value| value.target.clone()),
            MediaKind::Category
            | MediaKind::Collection
            | MediaKind::Franchise
            | MediaKind::Live => None,
        };
        let mut native = NativeDetailState {
            kind: detail.media.kind,
            seasons_tab: None,
            selected_season: resolution.as_ref().map_or(0, |value| value.initial_season),
            seasons: Vec::new(),
            featured: None,
            private_resume: positions.is_some(),
            sort: None,
        };
        let sortable = matches!(
            detail.media.kind,
            MediaKind::Category | MediaKind::Collection | MediaKind::Film | MediaKind::Supplement
        ) && detail.is_first_tab_sortable == Some(true)
            && matches!(detail.playlists.first(), Some(NativePlaylist::Generic(_)));
        if let Some(featured) = detail.featured {
            let mut cards = Vec::with_capacity(featured.children.len());
            for child in featured.children {
                cards.push(projection.native_detail_card(child, ImageRole::Card, positions));
            }
            native.featured = Some(OwnedFeatured {
                title: featured.title,
                cards,
            });
        }
        for (raw_index, playlist) in detail.playlists.into_iter().enumerate() {
            match playlist {
                NativePlaylist::Generic(value) => {
                    if sortable && raw_index == 0 {
                        native.sort = Some(sort::NativeSortState::new(
                            &value.children,
                            MAX_NATIVE_BYTES.saturating_sub(projection.estimated_bytes()),
                        )?);
                    }
                    let rail =
                        projection.native_detail_rail(value.title, value.children, positions);
                    projection.rails.push(rail);
                }
                NativePlaylist::Seasons(value)
                    if kind == DetailKind::Series && native.seasons_tab.is_none() =>
                {
                    native.seasons_tab = Some(0);
                    native.seasons = Vec::with_capacity(value.seasons.len());
                    for season in value.seasons {
                        native.seasons.push(OwnedSeason {
                            number: season.number,
                            rail: projection.native_detail_rail(
                                season.title,
                                season.episodes,
                                positions,
                            ),
                        });
                    }
                }
                NativePlaylist::Seasons(_) => (),
            }
        }
        if native.seasons_tab.is_some() {
            projection.rails.insert(
                0,
                OwnedRail {
                    title: "Episodes".into(),
                    cards: Vec::new(),
                    gallery: None,
                    private_epoch: None,
                },
            );
        }
        // Native catalog Float32 seconds truncate/saturate to signed Int64.
        // Do not route these values through the public card's integer runtime.
        let seconds = detail.media.duration.map(|value| value as i64);
        let card = projection.native_detail_card(detail.media, ImageRole::Backdrop, None);
        let header_runtime = matches!(
            kind,
            DetailKind::Film | DetailKind::Original | DetailKind::Supplement
        )
        .then(|| seconds.map(header_runtime))
        .flatten();
        let information_runtime = matches!(
            kind,
            DetailKind::Film | DetailKind::Original | DetailKind::Supplement | DetailKind::Episode
        )
        .then(|| seconds.map(information_runtime))
        .flatten();
        let header_year = if matches!(
            kind,
            DetailKind::Film | DetailKind::Original | DetailKind::Supplement | DetailKind::Series
        ) {
            card.year.as_str()
        } else {
            ""
        };
        let information_year = if matches!(
            kind,
            DetailKind::Film
                | DetailKind::Original
                | DetailKind::Supplement
                | DetailKind::Series
                | DetailKind::Episode
        ) {
            card.year.as_str()
        } else {
            ""
        };
        let header_metadata = metadata_line(header_year, header_runtime.as_deref());
        let information_metadata = metadata_line(information_year, information_runtime.as_deref());
        projection.detail = Some(OwnedDetail {
            primary_playback_target,
            primary_action: resolution.map_or_else(|| "WATCH NOW".into(), |value| value.action),
            native: Some(native),
            header_metadata,
            information_metadata,
            card,
            kind,
            description: [
                detail.metadata.description_long,
                detail.metadata.description_medium,
                detail.metadata.description,
            ]
            .into_iter()
            .flatten()
            .find(|text| !text.is_empty())
            .unwrap_or_default(),
            directors: detail.metadata.director.unwrap_or_default().join(", "),
            starring: detail.metadata.starring.and_then(super::joined),
            countries: detail.metadata.country.and_then(super::joined),
            languages: detail.metadata.language.and_then(super::joined),
            content_warnings: detail.metadata.content_warnings,
        });
        projection.selected_playlist = (!projection.rails.is_empty()).then_some(0);
        projection.total = u32::try_from(
            projection
                .rails
                .iter()
                .enumerate()
                .map(|(index, rail)| {
                    projection
                        .native_season_cards(index)
                        .map_or(rail.cards.len(), <[OwnedCard]>::len)
                })
                .sum::<usize>()
                + projection
                    .native_featured_cards()
                    .map_or(0, <[OwnedCard]>::len),
        )
        .map_err(|_| ProjectionLimit::TooLarge)?;
        projection.status = LoadState::Ready;
        if projection.estimated_bytes() > MAX_NATIVE_BYTES {
            return Err(ProjectionLimit::TooLarge);
        }
        Ok(projection)
    }
    pub(crate) fn native_sort_action(&mut self, action: criterion_ui::DetailSortAction) {
        if let Some(sort) = self
            .detail
            .as_mut()
            .and_then(|detail| detail.native.as_mut())
            .and_then(|native| native.sort.as_mut())
        {
            sort.action(action);
        }
    }
    pub(crate) fn close_native_sort(&mut self) {
        if let Some(sort) = self
            .detail
            .as_mut()
            .and_then(|detail| detail.native.as_mut())
            .and_then(|native| native.sort.as_mut())
        {
            sort.close();
        }
    }
    pub(super) fn native_sort_order(&self, row: usize) -> Option<&[usize]> {
        if row != 0 {
            return None;
        }
        self.detail
            .as_ref()?
            .native
            .as_ref()?
            .sort
            .as_ref()
            .map(|sort| sort.order.as_slice())
    }
    pub(crate) fn native_sort_view(&self) -> Option<criterion_ui::DetailSortView> {
        self.detail
            .as_ref()?
            .native
            .as_ref()?
            .sort
            .as_ref()
            .map(sort::NativeSortState::view)
    }
    pub(crate) fn select_native_season(&mut self, index: usize) {
        let Some(native) = self
            .detail
            .as_mut()
            .and_then(|detail| detail.native.as_mut())
        else {
            return;
        };
        if native.seasons_tab.is_none()
            || native.seasons_tab != self.selected_playlist
            || index >= native.seasons.len()
        {
            return;
        }
        native.selected_season = index;
        self.total = u32::try_from(
            self.rails
                .iter()
                .map(|rail| rail.cards.len())
                .sum::<usize>()
                + native.seasons[index].rail.cards.len()
                + native
                    .featured
                    .as_ref()
                    .map_or(0, |value| value.cards.len()),
        )
        .expect("bounded native Detail display");
    }
    pub(super) fn native_season_cards(&self, index: usize) -> Option<&[OwnedCard]> {
        let native = self.detail.as_ref()?.native.as_ref()?;
        if native.seasons_tab != Some(index) || self.selected_playlist != Some(index) {
            return None;
        }
        native
            .seasons
            .get(native.selected_season)
            .map(|season| season.rail.cards.as_slice())
    }
    pub(super) fn native_featured_cards(&self) -> Option<&[OwnedCard]> {
        self.detail
            .as_ref()?
            .native
            .as_ref()?
            .featured
            .as_ref()
            .map(|value| value.cards.as_slice())
    }
    fn native_detail_rail(
        &mut self,
        title: String,
        media: Vec<criterion_account::MediaSummary>,
        positions: Option<&PositionsSnapshot>,
    ) -> OwnedRail {
        let mut cards = Vec::with_capacity(media.len());
        for child in media {
            cards.push(self.native_detail_card(child, ImageRole::Card, positions));
        }
        OwnedRail {
            title,
            cards,
            gallery: None,
            private_epoch: None,
        }
    }
    fn native_detail_card(
        &mut self,
        media: criterion_account::MediaSummary,
        role: ImageRole,
        positions: Option<&PositionsSnapshot>,
    ) -> OwnedCard {
        let artwork = self.bind_image(ImageSource::Media {
            id: media.id.clone(),
            label: ImageLabel::Landscape,
            role,
        });
        let native_activation = match media.kind {
            MediaKind::Episode => NativeActivation::Play {
                id: media.id.clone(),
            },
            MediaKind::Live => NativeActivation::Unsupported,
            MediaKind::Film
            | MediaKind::Original
            | MediaKind::Supplement
            | MediaKind::Series
            | MediaKind::Category
            | MediaKind::Collection
            | MediaKind::Franchise => NativeActivation::Detail {
                id: media.id.clone(),
                auto_play: false,
            },
        };
        let saved_fraction = positions.and_then(|positions| positions.progress(&media));
        let duration_label = super::native_card_duration(&media);
        OwnedCard {
            target: Target::Native(media.id),
            kind: None,
            title: media.title,
            year: media
                .release_date
                .map(|date| date.year().to_string())
                .unwrap_or_default(),
            duration_label,
            artwork: Some(artwork),
            saved_fraction,
            native_activation: Some(native_activation),
        }
    }
    /// Drop derived private selection while preserving the anonymous metadata.
    pub(crate) fn clear_native_resume(&mut self) {
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        let Some(native) = detail
            .native
            .as_mut()
            .filter(|native| native.private_resume)
        else {
            return;
        };
        native.private_resume = false;
        native.selected_season = 0;
        if detail.kind == DetailKind::Series {
            detail.primary_playback_target = native
                .seasons
                .first()
                .and_then(|season| season.rail.cards.first())
                .and_then(|card| card.target.media_id())
                .filter(|id| Some(*id) != detail.card.target.media_id())
                .cloned();
            detail.primary_action = "WATCH FIRST EPISODE".into();
        }
        for card in self
            .rails
            .iter_mut()
            .flat_map(|rail| &mut rail.cards)
            .chain(
                native
                    .seasons
                    .iter_mut()
                    .flat_map(|season| &mut season.rail.cards),
            )
            .chain(
                native
                    .featured
                    .iter_mut()
                    .flat_map(|value| &mut value.cards),
            )
        {
            card.saved_fraction = None;
        }
        self.total = u32::try_from(
            self.rails
                .iter()
                .map(|rail| rail.cards.len())
                .sum::<usize>()
                + native
                    .featured
                    .as_ref()
                    .map_or(0, |value| value.cards.len())
                + if self.selected_playlist == native.seasons_tab {
                    native
                        .seasons
                        .first()
                        .map_or(0, |season| season.rail.cards.len())
                } else {
                    0
                },
        )
        .expect("bounded native detail");
    }
}

fn metadata_line(year: &str, runtime: Option<&str>) -> String {
    match (year.is_empty(), runtime) {
        (false, Some(runtime)) => format!("{year}   {runtime}"),
        (false, None) => year.into(),
        (true, Some(runtime)) => runtime.into(),
        (true, None) => String::new(),
    }
}

fn header_runtime(seconds: i64) -> String {
    if seconds <= 0 {
        return "0m".into();
    }
    // The source header divides before narrowing each displayed part.
    let minutes = seconds / 60;
    compact_runtime((minutes / 60) as i32, (minutes % 60) as i32)
}

fn information_runtime(seconds: i64) -> String {
    // The source modal wraps to signed Int32 before its positive-value guard.
    let seconds = seconds as i32;
    if seconds <= 0 {
        return "0m".into();
    }
    compact_runtime(seconds / 3600, (seconds % 3600) / 60)
}

fn compact_runtime(hours: i32, minutes: i32) -> String {
    if hours <= 0 {
        format!("{minutes}m")
    } else if minutes <= 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {minutes}m")
    }
}

fn detail_kind(kind: MediaKind) -> DetailKind {
    match kind {
        MediaKind::Film => DetailKind::Film,
        MediaKind::Original => DetailKind::Original,
        MediaKind::Episode => DetailKind::Episode,
        MediaKind::Supplement => DetailKind::Supplement,
        MediaKind::Series => DetailKind::Series,
        MediaKind::Collection => DetailKind::Collection,
        MediaKind::Category => DetailKind::Category,
        MediaKind::Franchise => DetailKind::Franchise,
        MediaKind::Live => DetailKind::Live,
    }
}

fn check_cardinality(detail: &criterion_account::NativeDetail) -> Result<(), ProjectionLimit> {
    if detail.playlists.len() > MAX_NATIVE_GROUPS {
        return Err(ProjectionLimit::TooLarge);
    }
    let mut items = 1usize;
    let mut seasons = 0usize;
    for playlist in &detail.playlists {
        match playlist {
            NativePlaylist::Generic(value) => {
                count_items(&mut items, value.raw_child_count.max(value.children.len()))?
            }
            NativePlaylist::Seasons(value) => {
                seasons = seasons
                    .checked_add(value.seasons.len())
                    .filter(|count| *count <= MAX_NATIVE_SEASONS)
                    .ok_or(ProjectionLimit::TooLarge)?;
                for season in &value.seasons {
                    count_items(
                        &mut items,
                        season.raw_episode_count.max(season.episodes.len()),
                    )?;
                }
            }
        }
    }
    if let Some(featured) = &detail.featured {
        count_items(
            &mut items,
            featured.raw_child_count.max(featured.children.len()),
        )?;
    }
    Ok(())
}
fn count_items(items: &mut usize, count: usize) -> Result<(), ProjectionLimit> {
    *items = items
        .checked_add(count)
        .filter(|value| *value <= MAX_NATIVE_ITEMS)
        .ok_or(ProjectionLimit::TooLarge)?;
    Ok(())
}

#[cfg(test)]
mod tests;
