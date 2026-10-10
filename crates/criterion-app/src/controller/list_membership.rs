// SPDX-License-Identifier: GPL-3.0-or-later
//! Membership belongs to a foreground native Detail visit, never its cached snapshot.
use super::{Controller, Query};
use criterion_account::MediaKind;
use criterion_provider::{MediaId, RequestTransport};
use criterion_session::MonotonicClock;
use criterion_ui::Page;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct MembershipScope {
    pub(crate) epoch: u64,
    pub(crate) root: MediaId,
    kind: MediaKind,
    visit: u64,
}

impl<T: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<T, C> {
    pub(crate) fn membership_scope(&self, epoch: u64) -> Option<MembershipScope> {
        let (root, kind, visit) = self.membership_root(epoch)?;
        Some(MembershipScope {
            epoch,
            root: root.clone(),
            kind,
            visit,
        })
    }

    pub(crate) fn membership_owns(&self, scope: &MembershipScope) -> bool {
        self.membership_root(scope.epoch)
            .is_some_and(|(root, kind, visit)| {
                *root == scope.root && kind == scope.kind && visit == scope.visit
            })
    }

    pub(crate) fn membership_available(&self, epoch: u64) -> bool {
        self.membership_root(epoch).is_some()
    }

    fn membership_root(&self, epoch: u64) -> Option<(&MediaId, MediaKind, u64)> {
        if !self.foreground_active
            || self.suspended
            || self.page != Page::Detail
            || self.account_session != Some(epoch)
        {
            return None;
        }
        let Some(Query::NativeDetail { id, .. }) = &self.query else {
            return None;
        };
        let (root, kind) = self.view.native_root()?;
        (*id == *root).then_some((root, kind, self.membership_visit?))
    }

    pub(crate) fn membership_deadline(&self) -> Option<Duration> {
        self.clock.now().checked_add(DEADLINE)
    }

    pub(crate) fn membership_expired(&self, deadline: Duration) -> bool {
        self.clock.now() >= deadline
    }

    pub(crate) fn membership_visit(&self) -> Option<u64> {
        self.membership_visit
    }

    pub(super) fn retire_membership_visit(&mut self) {
        // Exhaustion closes membership admission permanently; a fresh visit
        // cannot share an old operation identity even for the same root/epoch.
        self.membership_visit = self.membership_visit.and_then(|visit| visit.checked_add(1));
    }
}
