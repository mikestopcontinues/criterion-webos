// SPDX-License-Identifier: GPL-3.0-or-later
//! One foreground demand for a supplied account gallery. The shared account
//! worker owns execution; presentation owns private rows and their epoch.
use super::{Controller, Query};
use crate::continue_watching::ContinueWatchingShelf;
use criterion_account::ContinueWatching;
use criterion_provider::RequestTransport;
use criterion_session::MonotonicClock;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContinueWatchingRead {
    pub(crate) epoch: u64,
    operation: u64,
}
pub(super) struct Demand {
    read: ContinueWatchingRead,
    deadline: Duration,
}

impl<T: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<T, C> {
    pub(crate) fn begin_continue_watching(&mut self, epoch: u64) -> Option<ContinueWatchingRead> {
        if !self.foreground_active
            || self.suspended
            || self.account_session != Some(epoch)
            || !matches!(self.query, Some(Query::Discovery(_)))
            || self.continue_watching_demand.is_some()
            || !self.view.continue_watching_needs_read(epoch)
        {
            return None;
        }
        self.view.mark_continue_watching_pending(epoch);
        let Some(operation) = self.continue_watching_sequence.checked_add(1) else {
            self.view.fail_continue_watching(epoch);
            return None;
        };
        let Some(deadline) = self.clock.now().checked_add(DEADLINE) else {
            self.view.fail_continue_watching(epoch);
            return None;
        };
        let read = ContinueWatchingRead { epoch, operation };
        self.continue_watching_sequence = operation;
        self.continue_watching_demand = Some(Demand { read, deadline });
        Some(read)
    }
    pub(crate) fn continue_watching_owns(&self, read: &ContinueWatchingRead) -> bool {
        self.foreground_active
            && !self.suspended
            && self.account_session == Some(read.epoch)
            && matches!(self.query, Some(Query::Discovery(_)))
            && self
                .continue_watching_demand
                .as_ref()
                .is_some_and(|demand| demand.read == *read)
    }
    pub(crate) fn continue_watching_expired(&self) -> bool {
        self.continue_watching_demand
            .as_ref()
            .is_some_and(|demand| self.clock.now() >= demand.deadline)
    }
    pub(crate) fn renew_continue_watching(&self) -> Option<ContinueWatchingRead> {
        self.continue_watching_demand
            .as_ref()
            .filter(|demand| {
                self.continue_watching_owns(&demand.read) && !self.continue_watching_expired()
            })
            .map(|demand| demand.read)
    }
    pub(crate) fn admit_continue_watching(
        &mut self,
        read: &ContinueWatchingRead,
        data: ContinueWatching,
    ) {
        if !self.continue_watching_owns(read) {
            return;
        }
        if self.continue_watching_expired() {
            self.fail_continue_watching(read);
            return;
        }
        let admitted = ContinueWatchingShelf::from_admitted(data)
            .ok()
            .is_some_and(|shelf| {
                self.view
                    .admit_continue_watching(read.epoch, &shelf)
                    .is_ok_and(|admitted| admitted)
            });
        if admitted {
            self.continue_watching_demand = None;
            self.trim_history();
        } else {
            self.fail_continue_watching(read);
        }
    }
    pub(crate) fn fail_continue_watching(&mut self, read: &ContinueWatchingRead) {
        if self.continue_watching_owns(read) {
            self.view.fail_continue_watching(read.epoch);
            self.continue_watching_demand = None;
        }
    }
    pub(crate) fn cancel_continue_watching(&mut self) {
        self.continue_watching_demand = None;
        self.view.cancel_continue_watching();
    }
}
