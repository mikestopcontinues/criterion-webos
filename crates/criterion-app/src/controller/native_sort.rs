//! Current route-owned local sorting; no request or subscriber authority.
use super::Controller;
use criterion_provider::{MediaId, RequestTransport};
use criterion_session::MonotonicClock;
use criterion_ui::{DetailSortAction, LoadState, LoginView, Page};

impl<T: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<T, C> {
    pub(super) fn sort_detail(&mut self, root: &MediaId, action: DetailSortAction) {
        if self.page != Page::Detail
            || !self.foreground_active
            || self.suspended
            || self.view.native_root().is_none_or(|(id, _)| id != root)
            || !self.view.with_view(LoginView::SignedOut, |data| {
                data.status == LoadState::Ready
                    && data
                        .detail
                        .as_ref()
                        .is_some_and(|detail| detail.selected_playlist == Some(0))
            })
        {
            return;
        }
        self.view.native_sort_action(action);
    }
}
