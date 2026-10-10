// SPDX-License-Identifier: GPL-3.0-or-later
//! Current Detail intent grants one held write, never an authorization replay.
use super::Application;
use crate::{account::WriteRequest, controller::MembershipScope};
use criterion_account::{MediaKind, WatchListContentType};
use criterion_provider::{MediaId, RequestTransport};
use criterion_session::{MonotonicClock, Transport};
use criterion_ui::ListMembership;
use std::time::Duration;
use tokio::runtime::{Handle, Runtime};

pub(super) struct Intent {
    scope: MembershipScope,
    present: bool,
    deadline: Duration,
}

impl<
    P: RequestTransport + Send + Sync + 'static,
    T: Transport + 'static,
    C: MonotonicClock + Clone + 'static,
    A: criterion_account::Transport + 'static,
    D: MonotonicClock,
> Application<P, T, C, A, D>
{
    pub(super) fn start_list_write(
        &mut self,
        root: MediaId,
        visit: Option<u64>,
        present: Option<bool>,
        runtime: &Handle,
    ) {
        if !self.active
            || self.exiting
            || !self.authentication.write_ready()
            || !self.accounts.write_ready()
            || self.list_write.is_some()
        {
            return;
        }
        let Some((scope, observed)) = self
            .list_membership
            .as_ref()
            .and_then(super::list_membership::Observation::known)
        else {
            return;
        };
        if self.account_epoch != Some(scope.epoch)
            || scope.root != root
            || visit != Some(scope.visit)
            || present != Some(observed)
            || !self.controller.membership_owns(&scope)
        {
            return;
        }
        let Some(deadline) = self.controller.membership_deadline() else {
            return;
        };
        let content_type = match scope.kind {
            MediaKind::Film => WatchListContentType::Film,
            MediaKind::Series => WatchListContentType::Series,
            MediaKind::Collection => WatchListContentType::Collection,
            MediaKind::Episode => WatchListContentType::Episode,
            MediaKind::Supplement => WatchListContentType::Supplement,
            MediaKind::Category => WatchListContentType::Category,
            MediaKind::Franchise => WatchListContentType::Franchise,
            MediaKind::Original => WatchListContentType::Original,
            MediaKind::Live => return,
        };
        if self
            .accounts
            .write(
                runtime,
                WriteRequest {
                    epoch: scope.epoch,
                    root,
                    content_type,
                    remove: observed,
                },
            )
            .is_err()
        {
            return;
        }
        self.list_write = Some(Intent {
            scope,
            present: observed,
            deadline,
        });
        self.authentication.hold_for_account_write(true);
        if let Some(observation) = &mut self.list_membership {
            observation.set_state(ListMembership::Updating);
        }
        if let Some(output) = &mut self.output {
            output.shapes.clear();
        }
    }

    pub(super) fn retire_departed_list_write(&mut self) {
        if self.list_write.as_ref().is_some_and(|intent| {
            !self.active
                || self.exiting
                || self.account_epoch != Some(intent.scope.epoch)
                || !self.authentication.signed_in()
                || !self.controller.membership_owns(&intent.scope)
        }) {
            // Keep settlement ownership; only the permission to begin provider I/O retires.
            self.accounts.retire_write();
        }
    }

    pub(super) fn complete_list_write(&mut self, runtime: &Runtime) {
        let Some(outcome) = self.accounts.poll_write(runtime) else {
            return;
        };
        self.authentication.hold_for_account_write(false);
        let Some(intent) = self.list_write.take() else {
            return;
        };
        if outcome.possibly_issued && self.account_epoch == Some(outcome.epoch) {
            self.shelf_pending = None;
            self.shelf_generation = None;
            if let Some(read) = self.controller.dirty_shelf(outcome.epoch) {
                self.stage_shelf(read);
            }
            self.ui.dirty_my_list();
            if let Some(output) = &mut self.output {
                output.shapes.clear();
            }
        }
        if outcome.epoch == intent.scope.epoch
            && self.account_epoch == Some(outcome.epoch)
            && self.controller.membership_owns(&intent.scope)
            && let Some(observation) = &mut self.list_membership
        {
            if outcome.acknowledged && !self.controller.membership_expired(intent.deadline) {
                observation.refresh_after_write(intent.deadline);
            } else {
                observation.set_state(if !outcome.possibly_issued && outcome.fence_ready {
                    ListMembership::Known {
                        present: intent.present,
                    }
                } else {
                    ListMembership::Unavailable
                });
            }
        }
    }
}
