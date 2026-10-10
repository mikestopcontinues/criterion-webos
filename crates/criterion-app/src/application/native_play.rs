// SPDX-License-Identifier: GPL-3.0-or-later
//! Current native attempts terminate visibly without a licensed platform player.
use super::Application;
use crate::controller::NativePlaySelection;
use criterion_provider::RequestTransport;
use criterion_session::{MonotonicClock, Transport};
use criterion_ui::PlaybackFeedback;
use tokio::runtime::Handle;

pub(super) struct UnavailableNotice {
    epoch: u64,
    visit: u64,
}
/// One bounded synthetic-only observer at the actual consumer, never a runtime log.
#[cfg(test)]
pub(super) struct AttemptObservation {
    pub(super) selection: NativePlaySelection,
    pub(super) start_ms: Option<i64>,
    pub(super) commentary: Option<String>,
    pub(super) count: u32,
}
#[cfg(test)]
impl std::fmt::Debug for AttemptObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AttemptObservation([redacted])")
    }
}

impl<
    P: RequestTransport + Send + Sync + 'static,
    T: Transport + 'static,
    C: MonotonicClock + Clone + 'static,
    A: criterion_account::Transport + 'static,
    D: MonotonicClock,
> Application<P, T, C, A, D>
{
    pub(super) fn consume_native_play(&mut self, selection: NativePlaySelection, runtime: &Handle) {
        if !self.active || self.exiting || !self.controller.native_play_owns(&selection) {
            return;
        }
        if !self.authentication.signed_in() {
            for command in self.ui.begin_authentication() {
                self.command(command, runtime);
            }
            return;
        }
        let Some(epoch) = self.account_epoch else {
            return;
        };
        let positions = self
            .positions
            .as_ref()
            .filter(|positions| positions.epoch == epoch)
            .map(|positions| &positions.snapshot);
        let start_ms = crate::controller::saved_start_ms(
            positions.and_then(|positions| positions.position(&selection.selected)),
        );
        let commentary = if matches!(
            selection.trigger,
            crate::controller::NativePlayTrigger::EpisodeCard(_)
        ) {
            None
        } else {
            positions
                .and_then(|positions| positions.position(&selection.root))
                .and_then(|position| position.commentary_track.as_deref())
        };
        // Private preparation is synchronous and borrowed. The unavailable
        // consumer drops it; no request or pending player intent is retained.
        #[cfg(not(test))]
        let _ = (start_ms, commentary);
        // No stream/policy/entitlement request: this is an unavailable attempt,
        // not an unlocked gate or a fulfilled native playback action.
        self.native_play_notice = Some(UnavailableNotice {
            epoch,
            visit: selection.visit,
        });
        #[cfg(test)]
        {
            let count = self.native_attempt.as_ref().map_or(1, |last| {
                last.count
                    .checked_add(1)
                    .expect("bounded synthetic attempts")
            });
            self.native_attempt = Some(AttemptObservation {
                selection,
                start_ms,
                commentary: commentary.map(str::to_owned),
                count,
            });
        }
    }

    pub(super) fn sync_native_play_feedback(&mut self) {
        let current = self.native_play_notice.as_ref().is_some_and(|notice| {
            self.active
                && !self.exiting
                && self.authentication.signed_in()
                && self.account_epoch == Some(notice.epoch)
                && self.controller.native_play_visit() == Some(notice.visit)
        });
        if !current {
            self.native_play_notice = None;
            #[cfg(test)]
            {
                self.native_attempt = None;
            }
        }
        self.ui.set_playback_feedback(if current {
            PlaybackFeedback::Unavailable
        } else {
            PlaybackFeedback::None
        });
    }

    pub(super) fn clear_native_play_feedback(&mut self) {
        self.native_play_notice = None;
        #[cfg(test)]
        {
            self.native_attempt = None;
        }
        self.ui.set_playback_feedback(PlaybackFeedback::None);
    }
}
