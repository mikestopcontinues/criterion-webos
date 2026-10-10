// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure selection from one admitted response. This grants no playback capability.
use criterion_account::{
    MediaKind, MediaSummary, NativeDetail, NativePlaylist, NativeSeasonsPlaylist, Position,
};
use criterion_provider::MediaId;
use std::collections::HashMap;

const MAX_POSITIONS: usize = 512;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_INDEX_CAPACITY: usize = MAX_POSITIONS;
const MAX_RETAINED_BYTES: usize = MAX_TEXT_BYTES
    + MAX_POSITIONS * std::mem::size_of::<Position>()
    + MAX_INDEX_CAPACITY * std::mem::size_of::<usize>()
    + std::mem::size_of::<PositionsSnapshot>();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResumeLimit {
    TooLarge,
}

pub(crate) struct PositionsSnapshot {
    records: Vec<Position>,
    index: Vec<usize>,
}
impl PositionsSnapshot {
    /// Last supplied exact ID within this response preserves existing CW
    /// project behavior. It is not server chronology or a cross-response merge.
    pub(crate) fn from_admitted(records: Vec<Position>) -> Result<Self, ResumeLimit> {
        if records.len() > MAX_POSITIONS || records.capacity() > MAX_POSITIONS {
            return Err(ResumeLimit::TooLarge);
        }
        let bytes = records.iter().try_fold(0usize, |used, row| {
            used.checked_add(row.media_id.as_str().len())
                .and_then(|n| {
                    n.checked_add(row.series_id.as_ref().map_or(0, |id| id.as_str().len()))
                })
                .and_then(|n| n.checked_add(row.series_title.as_ref().map_or(0, String::capacity)))
                .and_then(|n| {
                    n.checked_add(row.commentary_track.as_ref().map_or(0, String::capacity))
                })
                .ok_or(ResumeLimit::TooLarge)
        })?;
        if bytes > MAX_TEXT_BYTES {
            return Err(ResumeLimit::TooLarge);
        }
        let mut index: Vec<_> = (0..records.len()).collect();
        index.sort_unstable_by(|a, b| {
            records[*a]
                .media_id
                .as_str()
                .cmp(records[*b].media_id.as_str())
                .then_with(|| a.cmp(b))
        });
        let value = Self { records, index };
        // Charge actual retained allocation capacities before admission. MediaId
        // exposes length but not allocation capacity; allocator overhead and
        // bounded selection scratch are outside this owned-data estimate.
        let retained = value
            .records
            .capacity()
            .checked_mul(std::mem::size_of::<Position>())
            .and_then(|n| {
                value
                    .index
                    .capacity()
                    .checked_mul(std::mem::size_of::<usize>())
                    .and_then(|index| n.checked_add(index))
            })
            .and_then(|n| n.checked_add(bytes))
            .and_then(|n| n.checked_add(std::mem::size_of::<Self>()))
            .ok_or(ResumeLimit::TooLarge)?;
        if value.index.capacity() > MAX_INDEX_CAPACITY || retained > MAX_RETAINED_BYTES {
            return Err(ResumeLimit::TooLarge);
        }
        Ok(value)
    }
    pub(crate) fn position(&self, id: &MediaId) -> Option<&Position> {
        // Sorted source ordinals give bounded O(log 512) lookup without copied
        // IDs or an opaque hash-table allocation. The last equal ID is the last
        // supplied row in this one response, never a chronology inference.
        let after = self
            .index
            .partition_point(|ordinal| self.records[*ordinal].media_id.as_str() <= id.as_str());
        let row = self.records.get(*self.index.get(after.checked_sub(1)?)?)?;
        (row.media_id == *id).then_some(row)
    }
    pub(crate) fn progress(&self, media: &MediaSummary) -> Option<f32> {
        if !matches!(
            media.kind,
            MediaKind::Film | MediaKind::Supplement | MediaKind::Episode
        ) {
            return None;
        }
        let position = self.position(&media.id)?;
        (position.dur > 0).then(|| ((position.pos as f32) / (position.dur as f32)).clamp(0.0, 1.0))
    }
}
impl std::fmt::Debug for PositionsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PositionsSnapshot([redacted])")
    }
}

pub(crate) struct SeriesResolution {
    pub(crate) target: Option<MediaId>,
    pub(crate) action: String,
    pub(crate) initial_season: usize,
}
#[derive(Clone, Copy)]
struct Entry<'a> {
    media: &'a MediaSummary,
    season: usize,
    episode: usize,
    ratio: Option<f32>,
    threshold: f32,
}

pub(crate) fn resolve_series(
    detail: &NativeDetail,
    positions: Option<&PositionsSnapshot>,
) -> SeriesResolution {
    let first = detail.playlists.iter().find_map(|playlist| match playlist {
        NativePlaylist::Seasons(value) => Some(value),
        NativePlaylist::Generic(_) => None,
    });
    let default = SeriesResolution {
        target: first
            .and_then(|g| g.seasons.first())
            .and_then(|s| s.episodes.first())
            .filter(|e| e.id != detail.media.id)
            .map(|e| e.id.clone()),
        action: "WATCH FIRST EPISODE".into(),
        initial_season: 0,
    };
    let Some(group) = first.filter(|g| g.seasons.first().is_some_and(|s| !s.episodes.is_empty()))
    else {
        return default;
    };
    // Primary Play reads every group; initial UI selection independently reads
    // the displayed first Seasons group. Do not substitute one result for both.
    let mut primary = HashMap::new();
    for playlist in &detail.playlists {
        match playlist {
            NativePlaylist::Generic(value) => {
                insert_progress(&mut primary, &value.children, positions)
            }
            NativePlaylist::Seasons(value) => {
                for season in &value.seasons {
                    insert_progress(&mut primary, &season.episodes, positions);
                }
            }
        }
    }
    let mut displayed = HashMap::new();
    for season in &group.seasons {
        insert_progress(&mut displayed, &season.episodes, positions);
    }
    let selected = select(group, &primary);
    let initial_season = match select(group, &displayed) {
        Selection::Next { entry, .. } => entry.season,
        Selection::First | Selection::Complete => 0,
    };
    let (selected, resume) = match selected {
        Selection::Next { entry, resume } => (entry, resume),
        Selection::First => {
            return SeriesResolution {
                initial_season,
                ..default
            };
        }
        Selection::Complete => {
            return SeriesResolution {
                action: "WATCH AGAIN".into(),
                initial_season,
                ..default
            };
        }
    };
    let prefix = if resume { "RESUME" } else { "WATCH" };
    // Semantic actions/operands are source proved; this deterministic English
    // wording is project presentation, not admitted reference localization.
    let action = if group.seasons.len() > 1 {
        format!(
            "{prefix} SEASON {}, EPISODE {}",
            group.seasons[selected.season].number,
            selected.episode + 1
        )
    } else {
        format!("{prefix} EPISODE {}", selected.episode + 1)
    };
    SeriesResolution {
        target: (selected.media.id != detail.media.id).then(|| selected.media.id.clone()),
        action,
        initial_season,
    }
}
fn insert_progress(
    map: &mut HashMap<MediaId, f32>,
    children: &[MediaSummary],
    positions: Option<&PositionsSnapshot>,
) {
    for child in children {
        if let Some(ratio) = positions.and_then(|positions| positions.progress(child)) {
            map.insert(child.id.clone(), ratio);
        }
    }
}
enum Selection<'a> {
    First,
    Complete,
    Next { entry: Entry<'a>, resume: bool },
}
fn select<'a>(group: &'a NativeSeasonsPlaylist, progress: &HashMap<MediaId, f32>) -> Selection<'a> {
    // Indices are source enumeration, never the supplied season labels/counts.
    let entries: Vec<_> = group
        .seasons
        .iter()
        .enumerate()
        .flat_map(|(season, value)| {
            value
                .episodes
                .iter()
                .enumerate()
                .map(move |(episode, media)| {
                    let seconds = if media.kind == MediaKind::Episode {
                        media.duration.unwrap_or(0.0) as i32
                    } else {
                        0
                    };
                    Entry {
                        media,
                        season,
                        episode,
                        ratio: progress.get(&media.id).copied(),
                        threshold: if seconds < 300 { 0.95 } else { 0.98 },
                    }
                })
        })
        .collect();
    let mut best: Option<Entry<'_>> = None;
    for entry in &entries {
        if let Some(ratio) = entry.ratio.filter(|ratio| *ratio < entry.threshold)
            && best.as_ref().is_none_or(|current| {
                (entry.episode, entry.season)
                    .cmp(&(current.episode, current.season))
                    .then_with(|| ratio.total_cmp(&current.ratio.expect("incomplete candidate")))
                    == std::cmp::Ordering::Greater
            })
        {
            best = Some(*entry);
        }
    }
    if let Some(entry) = best {
        return Selection::Next {
            resume: entry.ratio.is_some_and(|ratio| ratio > 0.0),
            entry,
        };
    }
    let Some(completed) = entries
        .iter()
        .rposition(|entry| entry.ratio.is_some_and(|ratio| ratio >= entry.threshold))
    else {
        return Selection::First;
    };
    entries
        .get(completed + 1)
        .copied()
        .map_or(Selection::Complete, |entry| Selection::Next {
            entry,
            resume: false,
        })
}

#[cfg(test)]
mod tests;
