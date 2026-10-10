// SPDX-License-Identifier: GPL-3.0-or-later
//! One read-only root membership observation for the current foreground visit.
use super::Application;
use crate::{
    account::{Loaded, LoadedAccount, ReadRequest},
    controller::MembershipScope,
};
use criterion_account::Error;
use criterion_provider::RequestTransport;
use criterion_session::{MonotonicClock, Transport};
use criterion_ui::{DetailKind, ListMembership, ViewData};
use std::time::Duration;
use tokio::runtime::Runtime;

pub(super) struct Observation {
    scope: MembershipScope,
    deadline: Option<Duration>,
    generation: Option<u64>,
    state: ListMembership,
}
impl Observation {
    pub(super) fn is_issued(&self) -> bool {
        self.generation.is_some()
    }
    pub(super) fn known(&self) -> Option<(MembershipScope, bool)> {
        match self.state {
            ListMembership::Known { present } => Some((self.scope.clone(), present)),
            _ => None,
        }
    }
    pub(super) fn set_state(&mut self, state: ListMembership) {
        self.state = state;
    }
    pub(super) fn refresh_after_write(&mut self, deadline: Duration) {
        self.deadline = Some(deadline);
        self.generation = None;
        self.state = ListMembership::Pending;
    }
}

pub(super) fn overlay(data: &mut ViewData<'_>, membership: ListMembership) {
    if let Some(detail) = &mut data.detail {
        detail.membership = if detail.kind == DetailKind::Live {
            ListMembership::Unavailable
        } else {
            membership
        };
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
    pub(super) fn membership_view(&self) -> ListMembership {
        if !self.authentication.signed_in() {
            return ListMembership::SignedOut;
        }
        if !self.active || self.exiting {
            return ListMembership::Unavailable;
        }
        if let Some(observation) = &self.list_membership
            && self.controller.membership_owns(&observation.scope)
        {
            return observation.state;
        }
        if self
            .account_epoch
            .is_some_and(|epoch| self.controller.membership_available(epoch))
        {
            ListMembership::Pending
        } else {
            ListMembership::Unavailable
        }
    }

    pub(super) fn poll_membership(&mut self, runtime: &Runtime) {
        // Stage the logical deadline even while credentials or an older read
        // are retiring. Waiting never extends the original demand budget.
        if self.list_membership.is_none()
            && self.authentication.signed_in()
            && let Some(epoch) = self.account_epoch
            && let Some(scope) = self.controller.membership_scope(epoch)
        {
            let deadline = self.controller.membership_deadline();
            self.list_membership = Some(Observation {
                scope,
                deadline,
                generation: None,
                state: if deadline.is_some() {
                    ListMembership::Pending
                } else {
                    ListMembership::Unavailable
                },
            });
        }
        let Some(observation) = &mut self.list_membership else {
            return;
        };
        let Some(deadline) = observation.deadline else {
            return;
        };
        if self.controller.membership_expired(deadline) {
            observation.deadline = None;
            observation.state = ListMembership::Unavailable;
            if observation.generation.take().is_some() {
                self.accounts.background();
            }
            return;
        }
        if self.accounts.write_active()
            || observation.generation.is_some()
            || self.shelf_pending.is_some()
            || self.shelf_generation.is_some()
            || self.continue_watching_pending.is_some()
            || self.continue_watching_generation.is_some()
            || self.native_detail_pending.is_some()
            || self.native_detail_generation.is_some()
            || !self.authentication.access_ready()
        {
            return;
        }
        match self.accounts.request(
            runtime.handle(),
            observation.scope.epoch,
            ReadRequest::MyListIds,
        ) {
            Ok(generation) => observation.generation = Some(generation),
            Err(_) => {
                observation.deadline = None;
                observation.state = ListMembership::Unavailable;
            }
        }
    }

    pub(super) fn complete_membership(&mut self, result: Result<LoadedAccount, Error>) {
        let Some(observation) = &mut self.list_membership else {
            return;
        };
        let Some(generation) = observation.generation.take() else {
            return;
        };
        let admitted = self.active
            && !self.exiting
            && self.account_epoch == Some(observation.scope.epoch)
            && self.authentication.access_ready()
            && self.controller.membership_owns(&observation.scope)
            && observation
                .deadline
                .is_some_and(|due| !self.controller.membership_expired(due));
        observation.deadline = None;
        observation.state = match result {
            Ok(loaded)
                if admitted
                    && generation == loaded.generation()
                    && observation.scope.epoch == loaded.session_generation()
                    && loaded.matches_request(&ReadRequest::MyListIds) =>
            {
                let Loaded::MyListIds(ids) = loaded.data else {
                    unreachable!("typed MyListIds read matched");
                };
                // Drop every unrelated ID and position with the result. This
                // endpoint does not supply the Series resume snapshot.
                ListMembership::Known {
                    present: ids.watchlist.contains(&observation.scope.root),
                }
            }
            _ => ListMembership::Unavailable,
        };
    }

    pub(super) fn retire_departed_membership(&mut self) {
        self.retire_departed_list_write();
        if self.list_membership.as_ref().is_some_and(|observation| {
            !self.active
                || self.exiting
                || !self.authentication.signed_in()
                || self.account_epoch != Some(observation.scope.epoch)
                || !self.controller.membership_owns(&observation.scope)
        }) {
            self.cancel_membership();
        }
    }

    pub(super) fn cancel_membership(&mut self) {
        if let Some(observation) = self.list_membership.take() {
            if observation.is_issued() {
                self.accounts.background();
            }
            if let Some(output) = &mut self.output {
                output.shapes.clear();
            }
        }
    }
}
