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

const MAX_NATIVE_ITEMS: usize = 512;
const MAX_NATIVE_GROUPS: usize = 32;
const MAX_NATIVE_SEASONS: usize = 64;
const MAX_NATIVE_BYTES: usize = 512 * 1024;

pub(super) struct NativeDetailState {
    pub(super) seasons_tab: Option<usize>,
    pub(super) selected_season: usize,
    pub(super) seasons: Vec<OwnedSeason>,
    private_resume: bool,
}
pub(super) struct OwnedSeason {
    pub(super) number: i32,
    pub(super) rail: OwnedRail,
}
impl NativeDetailState {
    pub(super) fn heap_bytes(&self) -> usize {
        super::vec_bytes(&self.seasons)
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
            seasons_tab: None,
            selected_season: resolution.as_ref().map_or(0, |value| value.initial_season),
            seasons: Vec::new(),
            private_resume: positions.is_some(),
        };
        for playlist in detail.playlists {
            match playlist {
                NativePlaylist::Generic(value) => {
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
        let card = projection.native_detail_card(detail.media, ImageRole::Backdrop, None);
        projection.detail = Some(OwnedDetail {
            primary_playback_target,
            primary_action: resolution.map_or_else(|| "WATCH NOW".into(), |value| value.action),
            native: Some(native),
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
                .sum::<usize>(),
        )
        .map_err(|_| ProjectionLimit::TooLarge)?;
        projection.status = LoadState::Ready;
        if projection.estimated_bytes() > MAX_NATIVE_BYTES {
            return Err(ProjectionLimit::TooLarge);
        }
        Ok(projection)
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
                + native.seasons[index].rail.cards.len(),
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
        OwnedCard {
            target: Target::Native(media.id),
            kind: None,
            title: media.title,
            year: media
                .release_date
                .map(|date| date.year().to_string())
                .unwrap_or_default(),
            duration_seconds: 0,
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
        {
            card.saved_fraction = None;
        }
        self.total = u32::try_from(
            self.rails
                .iter()
                .map(|rail| rail.cards.len())
                .sum::<usize>()
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
