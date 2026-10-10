// SPDX-License-Identifier: GPL-3.0-or-later
//! Local carousel commands share the existing current navigation visit.
use super::{Controller, Query};
use criterion_provider::RequestTransport;
use criterion_session::MonotonicClock;
use criterion_ui::{HeroCursor, HeroDirection, Page, Target};

impl<P: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<P, C> {
    fn current_hero(&self, page: Page, from: HeroCursor) -> bool {
        self.foreground_active
            && !self.suspended
            && self.page == page
            && matches!(page, Page::Home | Page::New | Page::Discovery)
            && matches!(self.query, Some(Query::Discovery(_)))
            && self.membership_visit() == Some(from.visit)
            && self.view.hero_cursor(from.visit) == Some(from)
    }
    pub(crate) fn move_hero(&mut self, page: Page, from: HeroCursor, direction: HeroDirection) {
        if self.current_hero(page, from) {
            self.view.move_hero(direction);
        }
    }
    pub(crate) fn admit_hero_activation(
        &self,
        page: Page,
        from: HeroCursor,
        target: &Target,
    ) -> Option<Target> {
        self.current_hero(page, from)
            .then(|| self.view.hero_target())
            .flatten()
            .filter(|current| *current == target)
            .cloned()
    }
}
