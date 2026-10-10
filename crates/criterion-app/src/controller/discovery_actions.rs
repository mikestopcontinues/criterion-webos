// SPDX-License-Identifier: GPL-3.0-or-later
//! Supplied public row actions are admitted before either navigation history changes.
use super::{Controller, Query};
use criterion_provider::RequestTransport;
use criterion_session::MonotonicClock;
use criterion_ui::{Page, RailActionCursor, Target};
impl<P: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<P, C> {
    pub(crate) fn admit_rail_activation(
        &self,
        page: Page,
        from: RailActionCursor,
        target: &Target,
    ) -> Option<Target> {
        (self.foreground_active
            && !self.suspended
            && self.page == page
            && matches!(page, Page::Home | Page::New | Page::Discovery)
            && matches!(self.query, Some(Query::Discovery(_)))
            && self.membership_visit() == Some(from.visit))
        .then(|| self.view.rail_action_target(from))
        .flatten()
        .filter(|current| *current == target)
        .cloned()
    }
}
