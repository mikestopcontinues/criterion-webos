//! Source-bound native keys; display order never replaces source-owned cards.
use super::super::{ProjectionLimit, vec_bytes};
use criterion_account::{MediaKind, MediaSummary};
use criterion_ui::{
    DetailSortAction, DetailSortDirection, DetailSortField, DetailSortSelection, DetailSortView,
};
use std::cmp::Ordering;

struct Key {
    title: Vec<u16>,
    date: Option<(i32, u8, u8)>,
    runtime: Option<i32>,
}
pub(super) struct NativeSortState {
    keys: Vec<Key>,
    pub(super) order: Vec<usize>,
    pending: DetailSortSelection,
    applied: DetailSortSelection,
    visible: bool,
}
impl NativeSortState {
    pub(super) fn new(media: &[MediaSummary], budget: usize) -> Result<Self, ProjectionLimit> {
        let mut keys = Vec::with_capacity(media.len());
        let order: Vec<_> = (0..media.len()).collect();
        let mut bytes = vec_bytes(&keys)
            .checked_add(vec_bytes(&order))
            .ok_or(ProjectionLimit::TooLarge)?;
        if bytes > budget {
            return Err(ProjectionLimit::TooLarge);
        }
        for media in media {
            // Full-string lowercase uses the project's pinned Rust Unicode17 policy.
            // UTF16 comparison preserves Java string order, not universal casing parity.
            let title: Vec<u16> = media.title.to_lowercase().encode_utf16().collect();
            bytes = bytes
                .checked_add(vec_bytes(&title))
                .filter(|bytes| *bytes <= budget)
                .ok_or(ProjectionLimit::TooLarge)?;
            let date = matches!(
                media.kind,
                MediaKind::Film | MediaKind::Supplement | MediaKind::Series
            )
            .then(|| {
                media
                    .release_date
                    .map(|date| (date.year(), date.month() as u8, date.day()))
            })
            .flatten();
            let runtime = matches!(
                media.kind,
                MediaKind::Film | MediaKind::Supplement | MediaKind::Episode
            )
            .then(|| media.duration.map_or(0, |value| value as i32));
            keys.push(Key {
                title,
                date,
                runtime,
            });
        }
        Ok(Self {
            keys,
            order,
            pending: DetailSortSelection::default(),
            applied: DetailSortSelection::default(),
            visible: false,
        })
    }
    pub(super) fn heap_bytes(&self) -> usize {
        vec_bytes(&self.keys)
            + vec_bytes(&self.order)
            + self
                .keys
                .iter()
                .map(|key| vec_bytes(&key.title))
                .sum::<usize>()
    }
    pub(super) fn view(&self) -> DetailSortView {
        DetailSortView {
            pending: self.pending,
            applied: self.applied,
            visible: self.visible,
        }
    }
    pub(super) fn close(&mut self) {
        self.visible = false;
    }
    pub(super) fn action(&mut self, action: DetailSortAction) {
        if action == DetailSortAction::Open {
            self.pending = self.applied;
            self.visible = true;
            return;
        }
        if !self.visible {
            return;
        }
        match action {
            DetailSortAction::Choose(field) => {
                let direction = if field != DetailSortField::Default
                    && field == self.pending.field
                    && self.pending.direction == DetailSortDirection::Ascending
                {
                    DetailSortDirection::Descending
                } else {
                    DetailSortDirection::Ascending
                };
                self.pending = DetailSortSelection { field, direction };
            }
            DetailSortAction::Apply => {
                self.applied = self.pending;
                // Stable ties restart from supplied source order on every application.
                for (index, source) in self.order.iter_mut().enumerate() {
                    *source = index;
                }
                if self.applied.field != DetailSortField::Default {
                    let keys = &self.keys;
                    let selection = self.applied;
                    self.order.sort_by(|a, b| {
                        let ordering = match selection.field {
                            DetailSortField::Title => keys[*a].title.cmp(&keys[*b].title),
                            DetailSortField::ReleaseDate => optional(keys[*a].date, keys[*b].date),
                            DetailSortField::Runtime => {
                                optional(keys[*a].runtime, keys[*b].runtime)
                            }
                            DetailSortField::Default => Ordering::Equal,
                        };
                        if selection.direction == DetailSortDirection::Descending {
                            ordering.reverse()
                        } else {
                            ordering
                        }
                    });
                }
                self.close();
            }
            DetailSortAction::Dismiss => self.close(),
            DetailSortAction::Open => unreachable!("Open was consumed before visibility guard"),
        }
    }
}
fn optional<T: Ord>(a: Option<T>, b: Option<T>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
    }
}
