// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact current native action context. This grants no entitlement or player capability.
use super::{Controller, Query};
use criterion_account::MediaKind;
use criterion_provider::{MediaId, RequestTransport};
use criterion_session::MonotonicClock;
use criterion_ui::{Focus, LoadState, LoginView, Page, Target};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativePlayTrigger {
    Primary,
    EpisodeCard(Focus),
    DetailAutoPlay,
}
#[derive(PartialEq, Eq)]
pub(crate) struct NativePlaySelection {
    pub(crate) root: MediaId,
    pub(crate) selected: MediaId,
    pub(crate) parent_series: Option<(MediaId, String)>,
    pub(crate) trigger: NativePlayTrigger,
    pub(crate) visit: u64,
}
/// Native saved seconds, independent of catalog duration and UI progress.
/// None refuses checked milliseconds overflow; it never clamps or rewinds.
pub(crate) fn saved_start_ms(position: Option<&criterion_account::Position>) -> Option<i64> {
    let Some(position) = position.filter(|position| position.pos > 0 && position.dur > 0) else {
        return Some(0);
    };
    let cutoff = if position.dur < 300 { 0.95 } else { 0.98 };
    if position.pos as f64 / position.dur as f64 >= cutoff {
        Some(0)
    } else {
        position.pos.checked_mul(1000)
    }
}
impl std::fmt::Debug for NativePlaySelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePlaySelection([redacted])")
    }
}

impl<T: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<T, C> {
    pub(crate) fn native_play_visit(&self) -> Option<u64> {
        if !self.foreground_active || self.suspended || self.page != Page::Detail {
            return None;
        }
        let Query::NativeDetail { id, .. } = self.query.as_ref()? else {
            return None;
        };
        let (root, _) = self.view.native_root()?;
        (id == root
            && self
                .view
                .with_view(LoginView::SignedOut, |view| view.status == LoadState::Ready))
        .then_some(self.membership_visit?)
    }

    pub(super) fn native_play_selection(
        &self,
        selected: &MediaId,
        trigger: NativePlayTrigger,
    ) -> Option<NativePlaySelection> {
        let visit = self.native_play_visit()?;
        let (root, kind) = self.view.native_root()?;
        let admitted = match trigger {
            NativePlayTrigger::EpisodeCard(focus) => matches!(
                self.view.selected_card_action(self.page, focus, &Target::Native(selected.clone())),
                Some(Some(crate::presentation::NativeActivation::Play { id })) if id == *selected
            ),
            NativePlayTrigger::Primary | NativePlayTrigger::DetailAutoPlay => {
                self.view.with_view(LoginView::SignedOut, |view| {
                    view.detail
                        .as_ref()
                        .is_some_and(|detail| detail.primary_playback_target == Some(selected))
                })
            }
        };
        if !admitted {
            return None;
        }
        let parent_series = if kind == MediaKind::Series {
            self.view.with_view(LoginView::SignedOut, |view| {
                view.detail
                    .as_ref()
                    .map(|detail| (root.clone(), detail.card.title.to_owned()))
            })
        } else {
            None
        };
        Some(NativePlaySelection {
            root: root.clone(),
            selected: selected.clone(),
            parent_series,
            trigger,
            visit,
        })
    }

    pub(crate) fn native_play_owns(&self, selection: &NativePlaySelection) -> bool {
        self.native_play_visit() == Some(selection.visit)
            && self
                .view
                .native_root()
                .is_some_and(|(root, _)| *root == selection.root)
            && self
                .native_play_selection(&selection.selected, selection.trigger)
                .is_some_and(|current| current == *selection)
    }

    pub(super) fn native_primary_play(
        &self,
        id: &MediaId,
        trigger: NativePlayTrigger,
    ) -> Option<NativePlaySelection> {
        self.native_play_selection(id, trigger)
    }

    pub(super) fn retire_native_auto_play(&mut self) {
        if let Some(Query::NativeDetail { auto_play, .. }) = &mut self.query {
            *auto_play = false;
        }
    }
}
