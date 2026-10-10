// SPDX-License-Identifier: GPL-3.0-or-later
//! Immutable saved-progress display from one admitted native response.
use criterion_account::{ContinueWatching, MediaKind, MediaSummary};
use criterion_provider::MediaId;
use std::collections::{HashMap, HashSet};

pub(crate) const MAX_ROWS: usize = 512;
pub(crate) const MAX_OWNED_TEXT_BYTES: usize = 64 * 1024;
pub(crate) const MAX_RETAINED_BYTES: usize = MAX_OWNED_TEXT_BYTES
    + MAX_ROWS * std::mem::size_of::<ContinueWatchingRow>()
    + std::mem::size_of::<ContinueWatchingShelf>();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShelfLimit {
    TooLarge,
}

pub(crate) struct ContinueWatchingShelf {
    rows: Vec<ContinueWatchingRow>,
}
pub(crate) struct ContinueWatchingRow {
    media: MediaSummary,
    saved_fraction: Option<f32>,
    series_id: Option<MediaId>,
}
impl ContinueWatchingShelf {
    /// Uses admitted DTOs without repeating their wire validation. The two raw
    /// vectors must each contain at most 512 records; all input playlist title
    /// and series-title capacities plus validated media/series ID lengths must
    /// fit 64 KiB before join scratch. Retained text, including Episode saved
    /// series IDs in addition to native metadata, also fits 64 KiB.
    pub(crate) fn from_admitted(data: ContinueWatching) -> Result<Self, ShelfLimit> {
        if data.playlist.len() > MAX_ROWS || data.positions.len() > MAX_ROWS {
            return Err(ShelfLimit::TooLarge);
        }
        let input_text_bytes = data.playlist.iter().try_fold(0usize, |used, media| {
            used.checked_add(text_bytes(media)?)
                .ok_or(ShelfLimit::TooLarge)
        })?;
        if input_text_bytes > MAX_OWNED_TEXT_BYTES {
            return Err(ShelfLimit::TooLarge);
        }

        let mut rows = Vec::with_capacity(data.playlist.len());
        let mut seen = HashSet::with_capacity(data.playlist.len());
        let mut positions: HashMap<_, _> = data
            .positions
            .into_iter()
            .map(|position| {
                (
                    position.media_id,
                    (position.pos, position.dur, position.series_id),
                )
            })
            .collect();
        for media in data.playlist {
            if seen.insert(media.id.clone()) {
                let (saved_fraction, series_id) =
                    positions
                        .remove(&media.id)
                        .map_or((None, None), |(pos, dur, series_id)| {
                            (
                                (matches!(
                                    media.kind,
                                    MediaKind::Film | MediaKind::Supplement | MediaKind::Episode
                                ) && dur > 0)
                                    .then(|| ((pos as f32) / (dur as f32)).clamp(0.0, 1.0)),
                                if media.kind == MediaKind::Episode {
                                    series_id
                                } else {
                                    None
                                },
                            )
                        });
                rows.push(ContinueWatchingRow {
                    media,
                    saved_fraction,
                    series_id,
                });
            }
        }
        let shelf = Self { rows };
        if shelf.storage_bytes()? > MAX_RETAINED_BYTES {
            return Err(ShelfLimit::TooLarge);
        }
        Ok(shelf)
    }
    fn storage_bytes(&self) -> Result<usize, ShelfLimit> {
        let row_bytes = self
            .rows
            .capacity()
            .checked_mul(std::mem::size_of::<ContinueWatchingRow>())
            .ok_or(ShelfLimit::TooLarge)?;
        let retained_text_bytes = self.rows.iter().try_fold(0usize, |used, row| {
            let row_text_bytes = text_bytes(&row.media)?
                .checked_add(row.series_id.as_ref().map_or(0, |id| id.as_str().len()))
                .ok_or(ShelfLimit::TooLarge)?;
            used.checked_add(row_text_bytes).ok_or(ShelfLimit::TooLarge)
        })?;
        if retained_text_bytes > MAX_OWNED_TEXT_BYTES {
            return Err(ShelfLimit::TooLarge);
        }
        row_bytes
            .checked_add(std::mem::size_of::<Self>())
            .and_then(|used| used.checked_add(retained_text_bytes))
            .ok_or(ShelfLimit::TooLarge)
    }
    pub(crate) fn rows(&self) -> &[ContinueWatchingRow] {
        &self.rows
    }
    /// Owned shelf, actual row-vector/title/series-title capacities and validated
    /// media/native-series/saved-series ID lengths.
    /// ID capacity is unavailable; allocator overhead, temporary join scratch,
    /// transport and GPU memory are outside this retained-storage estimate.
    #[cfg(test)]
    pub(crate) fn retained_bytes(&self) -> usize {
        self.storage_bytes().expect("admitted immutable storage")
    }
}

fn text_bytes(media: &MediaSummary) -> Result<usize, ShelfLimit> {
    media
        .title
        .capacity()
        .checked_add(media.id.as_str().len())
        .and_then(|bytes| {
            bytes.checked_add(media.series_title.as_ref().map_or(0, String::capacity))
        })
        .and_then(|bytes| {
            bytes.checked_add(media.series_id.as_ref().map_or(0, |id| id.as_str().len()))
        })
        .ok_or(ShelfLimit::TooLarge)
}
impl ContinueWatchingRow {
    pub(crate) fn media(&self) -> &MediaSummary {
        &self.media
    }
    pub(crate) fn saved_fraction(&self) -> Option<f32> {
        self.saved_fraction
    }
    /// Last saved Position's series ID for this Episode, without the native
    /// Episode-metadata fallback. The original media ID is preserved.
    pub(crate) fn saved_series_id(&self) -> Option<&MediaId> {
        self.series_id.as_ref()
    }
}
impl std::fmt::Debug for ContinueWatchingShelf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContinueWatchingShelf([redacted])")
    }
}
impl std::fmt::Debug for ContinueWatchingRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContinueWatchingRow([redacted])")
    }
}
#[cfg(test)]
mod tests;
