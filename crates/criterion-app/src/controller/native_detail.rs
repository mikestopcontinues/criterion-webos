// SPDX-License-Identifier: GPL-3.0-or-later
//! Anonymous native metadata demands share the Accounts worker with private reads.
use super::{Controller, Query};
use criterion_account::NativeDetail;
use criterion_provider::{MediaId, RequestTransport};
use criterion_session::MonotonicClock;
use criterion_ui::{LoadState, LoginView};
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeDetailRead {
    pub(crate) epoch: u64,
    pub(crate) id: MediaId,
    pub(crate) auto_play: bool,
    operation: u64,
}
pub(super) struct Demand {
    read: NativeDetailRead,
    deadline: Duration,
}

impl<T: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<T, C> {
    pub(crate) fn begin_native_detail(&mut self, epoch: u64) -> Option<NativeDetailRead> {
        if !self.foreground_active
            || self.suspended
            || self.native_detail_demand.is_some()
            || !self.view.with_view(LoginView::SignedOut, |view| {
                view.status == LoadState::Loading
            })
        {
            return None;
        }
        let Some(Query::NativeDetail { id, auto_play }) = &self.query else {
            return None;
        };
        let Some(operation) = self.native_detail_sequence.checked_add(1) else {
            self.view.set_status(LoadState::Error);
            return None;
        };
        let Some(deadline) = self.clock.now().checked_add(DEADLINE) else {
            self.view.set_status(LoadState::Error);
            return None;
        };
        let read = NativeDetailRead {
            epoch,
            id: id.clone(),
            auto_play: *auto_play,
            operation,
        };
        self.native_detail_sequence = operation;
        self.native_detail_demand = Some(Demand {
            read: read.clone(),
            deadline,
        });
        Some(read)
    }
    pub(crate) fn native_detail_owns(&self, read: &NativeDetailRead) -> bool {
        self.foreground_active
            && !self.suspended
            && matches!(&self.query, Some(Query::NativeDetail { id, auto_play }) if *id == read.id && *auto_play == read.auto_play)
            && self
                .native_detail_demand
                .as_ref()
                .is_some_and(|demand| demand.read == *read)
    }
    pub(crate) fn native_detail_expired(&self) -> bool {
        self.native_detail_demand
            .as_ref()
            .is_some_and(|demand| self.clock.now() >= demand.deadline)
    }
    pub(crate) fn admit_native_detail(&mut self, read: &NativeDetailRead, detail: NativeDetail) {
        if !self.native_detail_owns(read) {
            return;
        }
        if self.native_detail_expired() || detail.media.id != read.id {
            self.fail_native_detail(read);
            return;
        }
        match crate::presentation::Presentation::native_detail(detail) {
            Ok(view) => {
                self.view = view;
                self.native_detail_demand = None;
                self.trim_history();
            }
            Err(_) => self.fail_native_detail(read),
        }
    }
    pub(crate) fn fail_native_detail(&mut self, read: &NativeDetailRead) {
        if self.native_detail_owns(read) {
            self.view.set_status(LoadState::Error);
            self.native_detail_demand = None;
        }
    }
    pub(crate) fn cancel_native_detail(&mut self) {
        self.native_detail_demand = None;
    }
    pub(crate) fn abandon_native_detail(&mut self) {
        if self.native_detail_demand.is_some() {
            self.view.set_status(LoadState::Error);
        }
        self.cancel_native_detail();
    }
}
