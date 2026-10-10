// SPDX-License-Identifier: GPL-3.0-or-later
//! Main-thread catalog publication and bounded navigation snapshots.
mod catalog;
mod continue_watching;
mod list_membership;
mod native_detail;
mod native_sort;
pub(crate) use list_membership::MembershipScope;
pub(crate) use native_detail::NativeDetailRead;
#[cfg(test)]
mod continue_watching_tests;
pub(crate) use continue_watching::ContinueWatchingRead;
mod my_list;
use crate::{
    jobs::Jobs,
    my_list::{MyListState, Read},
    presentation::Presentation,
};
use criterion_provider::{
    BrowseOptions, BrowseRequest, Catalog, CatalogPage, ContentTarget, DiscoveryPage,
    DiscoveryRoute, Error, Filter, HttpTransport, MediaDetail, MediaId, RequestTransport,
    SearchResults, SortDirection,
};
use criterion_session::{MonotonicClock, SystemClock};
use criterion_ui::{Command, FilterSelection, Page, SearchGroup, Target};
use std::{sync::Arc, time::Duration};
use tokio::runtime::{Handle, Runtime};

const MAX_HISTORY: usize = 16;
const HISTORY_BYTES: usize = 8 * 1024 * 1024;
const SEARCH_DELAY: Duration = Duration::from_millis(250);
const MAX_SEARCH_BYTES: usize = 256;

#[derive(Clone)]
enum Query {
    Discovery(DiscoveryRoute),
    Browse {
        request: Arc<BrowseRequest>,
        options: Option<Arc<BrowseOptions>>,
    },
    Detail(MediaId),
    NativeDetail {
        id: MediaId,
        auto_play: bool,
    },
    Search(String, SearchGroup),
}
enum Loaded {
    Discovery(DiscoveryPage),
    Browse(CatalogPage, Arc<BrowseOptions>),
    Detail(Box<MediaDetail>),
    Search(SearchResults, String),
}
struct Snapshot {
    page: Page,
    view: Option<Presentation>,
    query: Option<Query>,
    browse: Option<Arc<BrowseRequest>>,
    private_epoch: Option<u64>,
    search_loaded: bool,
    pager: Option<catalog::Pager>,
    my_list: Option<MyListState>,
}

pub(crate) enum Effect {
    None,
    Authenticate,
    AccountShelf,
    AccountRead(Read),
    RetryAuthentication,
    CancelAuthentication,
    Logout,
    Play(MediaId),
    ToggleList(MediaId),
    VoiceSearch,
    Exit,
}

pub(crate) struct Controller<T = HttpTransport, C: MonotonicClock = SystemClock> {
    clock: C,
    search_due: Option<Duration>,
    search_loaded: bool,
    catalog: Arc<Catalog<T>>,
    jobs: Jobs<Result<Loaded, Error>>,
    pub(crate) view: Presentation,
    page: Page,
    query: Option<Query>,
    history: Vec<Snapshot>,
    suspended: bool,
    foreground_active: bool,
    options: Option<Arc<BrowseOptions>>,
    browse: Option<Arc<BrowseRequest>>,
    pager: Option<catalog::Pager>,
    account_session: Option<u64>,
    private_epoch: Option<u64>,
    continue_watching_demand: Option<continue_watching::Demand>,
    continue_watching_sequence: u64,
    native_detail_demand: Option<native_detail::Demand>,
    native_detail_sequence: u64,
    membership_visit: Option<u64>,
    my_list: Option<MyListState>,
    shelf_read: Option<Read>,
    shelf_deadline: Option<Duration>,
}

impl<T: RequestTransport + Send + Sync + 'static> Controller<T> {
    pub(crate) fn new(catalog: Catalog<T>, runtime: &Handle) -> Self {
        Self::with_clock(catalog, runtime, SystemClock::default())
    }
}
impl<T: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<T, C> {
    pub(crate) fn with_clock(catalog: Catalog<T>, runtime: &Handle, clock: C) -> Self {
        let mut owner = Self {
            clock,
            search_due: None,
            search_loaded: false,
            catalog: Arc::new(catalog),
            jobs: Jobs::new(),
            view: Presentation::loading("Home"),
            page: Page::Home,
            query: None,
            history: Vec::new(),
            suspended: false,
            foreground_active: true,
            options: None,
            browse: None,
            pager: None,
            account_session: None,
            private_epoch: None,
            continue_watching_demand: None,
            continue_watching_sequence: 0,
            native_detail_demand: None,
            native_detail_sequence: 0,
            membership_visit: Some(0),
            my_list: None,
            shelf_read: None,
            shelf_deadline: None,
        };
        owner.start(Query::Discovery(DiscoveryRoute::Home), runtime);
        owner
    }

    pub(crate) fn command(&mut self, command: Command, page: Page, runtime: &Handle) -> Effect {
        let card_action = if let Command::ActivateCard { target, focus } = &command {
            let Some(action) = self.view.selected_card_action(self.page, *focus, target) else {
                return Effect::None;
            };
            if matches!(
                action,
                Some(crate::presentation::NativeActivation::Unsupported)
            ) {
                return Effect::None;
            }
            Some(action)
        } else {
            None
        };
        match command {
            Command::Navigate(destination) => {
                if (destination == Page::Search && self.page != Page::Search)
                    || (destination != Page::Search && self.page == Page::Search)
                    || (destination == Page::Login && self.page != Page::Login)
                    || ((destination == Page::MyList) != (self.page == Page::MyList))
                {
                    self.remember();
                }
                self.page = destination;
                return self.navigate(destination, runtime);
            }
            Command::Open(target) | Command::ActivateCard { target, .. } => {
                let native = card_action.unwrap_or_else(|| self.view.native_activation(&target));
                if let Some(crate::presentation::NativeActivation::Play { id }) = &native {
                    return Effect::Play(id.clone());
                }
                self.remember();
                self.page = page;
                match target {
                    Target::Native(_) => match native {
                        Some(crate::presentation::NativeActivation::Detail { id, auto_play }) => {
                            self.start(Query::NativeDetail { id, auto_play }, runtime);
                        }
                        Some(crate::presentation::NativeActivation::Play { .. }) => {
                            unreachable!("native Play intent was consumed before navigation")
                        }
                        Some(crate::presentation::NativeActivation::Unsupported) | None => {
                            self.query = None;
                            self.jobs.cancel();
                            self.view.set_status(criterion_ui::LoadState::Error);
                        }
                    },
                    Target::Media(id) | Target::Content(ContentTarget::Media { id, .. }) => {
                        self.start(Query::Detail(id), runtime)
                    }
                    Target::Content(ContentTarget::Home) => {
                        self.start(Query::Discovery(DiscoveryRoute::Home), runtime)
                    }
                    Target::Content(ContentTarget::New) => {
                        self.start(Query::Discovery(DiscoveryRoute::New), runtime)
                    }
                    Target::Content(ContentTarget::AllFilms) => {
                        return self.navigate(Page::AllFilms, runtime);
                    }
                    Target::Content(ContentTarget::MyList) if page == Page::MyList => {
                        return self.navigate(Page::MyList, runtime);
                    }
                    Target::Content(ContentTarget::MyList | ContentTarget::Subscribe) => {
                        self.jobs.cancel();
                        self.search_due = None;
                        self.search_loaded = false;
                        self.query = None;
                        self.clear_shelf();
                        self.view = Presentation::loading(title(page));
                        return Effect::Authenticate;
                    }
                    Target::Content(ContentTarget::Discover(slug)) => {
                        self.start(Query::Discovery(DiscoveryRoute::Discover(slug)), runtime)
                    }
                }
            }
            Command::Restore(destination) => {
                self.retire_membership_visit();
                self.cancel_native_detail();
                self.cancel_continue_watching();
                self.jobs.cancel();
                self.search_due = None;
                self.search_loaded = false;
                if let Some(snapshot) = self.history.pop() {
                    self.page = destination;
                    self.query = snapshot.query;
                    self.browse = snapshot.browse;
                    self.private_epoch = snapshot.private_epoch;
                    self.search_loaded = snapshot.search_loaded;
                    self.pager = snapshot.pager;
                    self.my_list = snapshot.my_list;
                    if snapshot.page == destination
                        && destination == Page::MyList
                        && self.private_epoch.is_some()
                        && self.private_epoch == self.account_session
                        && self.my_list.is_some()
                    {
                        return self
                            .resume_shelf()
                            .map_or(Effect::None, Effect::AccountRead);
                    }
                    if snapshot.page == destination {
                        if let Some(view) = snapshot.view {
                            self.view = view;
                        } else if let Some(pager) = &mut self.pager {
                            self.view = Presentation::loading("All Films");
                            if let Some(request) = pager.rehydrate(&mut self.view, self.clock.now())
                            {
                                self.issue(
                                    Query::Browse {
                                        request: Arc::new(request),
                                        options: self.options.clone(),
                                    },
                                    runtime,
                                );
                            } else if let Some(query) = self.query.clone() {
                                self.start(query, runtime);
                            }
                        } else if let Some(query) = self.query.clone() {
                            self.start(query, runtime);
                        } else {
                            return self.navigate(destination, runtime);
                        }
                    } else {
                        return self.navigate(destination, runtime);
                    }
                } else {
                    self.page = destination;
                    return self.navigate(destination, runtime);
                }
            }
            Command::Search { query, group } => {
                let query = query.trim();
                if query.is_empty()
                    || query.len() > MAX_SEARCH_BYTES
                    || query.chars().any(char::is_control)
                {
                    self.jobs.cancel();
                    self.search_due = None;
                    self.search_loaded = false;
                    self.query = None;
                    self.view = Presentation::loading("Search");
                    self.view.set_status(if query.is_empty() {
                        criterion_ui::LoadState::Empty
                    } else {
                        criterion_ui::LoadState::Error
                    });
                } else {
                    if let Some(Query::Search(current, selected)) = &mut self.query
                        && self.page == Page::Search
                        && current == query
                    {
                        *selected = group;
                        if self.search_loaded
                            && self
                                .view
                                .with_view(criterion_ui::LoginView::SignedOut, |view| {
                                    matches!(
                                        view.status,
                                        criterion_ui::LoadState::Ready
                                            | criterion_ui::LoadState::Empty
                                    )
                                })
                        {
                            self.view.set_group(group);
                            return Effect::None;
                        }
                        if self.search_due.is_some() || self.jobs.is_active() {
                            return Effect::None;
                        }
                    }
                    self.start(Query::Search(query.to_owned(), group), runtime);
                }
            }
            Command::ApplyFilters(selection) => {
                let request = self
                    .options
                    .as_ref()
                    .ok_or(Error::InvalidRequest)
                    .and_then(|options| browse_request(&selection, options));
                match request {
                    Ok(request) => self.start(
                        Query::Browse {
                            request: Arc::new(request),
                            options: self.options.clone(),
                        },
                        runtime,
                    ),
                    Err(_) => self.view.set_status(criterion_ui::LoadState::Error),
                }
            }
            Command::Catalog { anchor, target } if self.page == Page::AllFilms => {
                if let Some(pager) = &mut self.pager
                    && pager.retire_obsolete(target, &mut self.view)
                {
                    self.jobs.cancel();
                }
                if let Some(pager) = &mut self.pager
                    && let Some(request) =
                        pager.demand(anchor, target, &mut self.view, self.clock.now())
                {
                    self.issue(
                        Query::Browse {
                            request: Arc::new(request),
                            options: self.options.clone(),
                        },
                        runtime,
                    );
                }
            }
            Command::RetryCatalog if self.page == Page::AllFilms => {
                if let Some(pager) = &mut self.pager
                    && let Some(request) = pager.retry(&mut self.view, self.clock.now())
                {
                    self.issue(
                        Query::Browse {
                            request: Arc::new(request),
                            options: self.options.clone(),
                        },
                        runtime,
                    );
                }
            }
            Command::MyListGroup(group) if self.page == Page::MyList => {
                return self
                    .select_shelf_group(group)
                    .map_or(Effect::None, Effect::AccountRead);
            }
            Command::Catalog { anchor, target } if self.page == Page::MyList => {
                return self
                    .demand_shelf(anchor, target)
                    .map_or(Effect::None, Effect::AccountRead);
            }
            Command::RetryCatalog if self.page == Page::MyList => {
                return self.retry_shelf().map_or(Effect::None, Effect::AccountRead);
            }
            Command::MyListGroup(_) | Command::Catalog { .. } | Command::RetryCatalog => {}
            Command::SelectPlaylist(index) => {
                self.view.close_native_sort();
                self.view.select_playlist(index);
            }
            Command::DetailSort { root, action } => self.sort_detail(&root, action),
            Command::SelectSeason(index) => self.view.select_native_season(index),
            Command::Authenticate => {
                if self.page != Page::Login {
                    self.remember();
                }
                self.jobs.cancel();
                self.search_due = None;
                self.search_loaded = false;
                self.page = Page::Login;
                self.query = None;
                self.clear_shelf();
                self.view = Presentation::loading("Sign in");
                return Effect::Authenticate;
            }
            Command::RetryAuthentication => return Effect::RetryAuthentication,
            Command::CancelAuthentication => return Effect::CancelAuthentication,
            Command::Logout => return Effect::Logout,
            Command::Play(id) => return Effect::Play(id),
            Command::ToggleList(id) => return Effect::ToggleList(id),
            Command::VoiceSearch => return Effect::VoiceSearch,
            Command::Exit => return Effect::Exit,
        }
        Effect::None
    }

    pub(crate) fn poll(&mut self, runtime: &Runtime) {
        if self.page == Page::AllFilms
            && self
                .pager
                .as_ref()
                .is_some_and(|p| p.expired(self.clock.now()))
        {
            self.jobs.cancel();
            self.pager.as_mut().unwrap().fail(&mut self.view);
        }
        if self.search_due.is_some_and(|due| self.clock.now() >= due) {
            self.search_due = None;
            if let Some(query @ Query::Search(_, _)) = self.query.clone() {
                self.issue(query, runtime.handle());
            }
        }
        if let Some(result) = runtime.block_on(self.jobs.take_ready()) {
            match result {
                Ok(Ok(Loaded::Discovery(page))) => self.view = Presentation::discovery(page),
                Ok(Ok(Loaded::Browse(page, options))) => {
                    let next = if let Some(pager) = &mut self.pager {
                        pager.loaded(page, &mut self.view)
                    } else {
                        self.view = Presentation::catalog("All Films", page);
                        None
                    };
                    if next.is_none() {
                        self.view.set_options(&options);
                    }
                    self.options = Some(options.clone());
                    self.trim_history();
                    if let Some(request) = next {
                        self.issue(
                            Query::Browse {
                                request: Arc::new(request),
                                options: Some(options),
                            },
                            runtime.handle(),
                        );
                    }
                }
                Ok(Ok(Loaded::Detail(detail))) => self.view = Presentation::detail(*detail),
                Ok(Ok(Loaded::Search(results, loaded_query))) => {
                    if let Some(Query::Search(current, group)) = &self.query
                        && self.page == Page::Search
                        && *current == loaded_query
                    {
                        self.view = Presentation::search(results, *group);
                        self.search_loaded = true;
                    }
                }
                _ if self.page == Page::AllFilms && self.pager.is_some() => {
                    self.pager.as_mut().unwrap().fail(&mut self.view);
                }
                Ok(Err(Error::Unavailable | Error::Deadline)) => {
                    self.view.set_status(criterion_ui::LoadState::Offline)
                }
                _ => self.view.set_status(criterion_ui::LoadState::Error),
            }
        }
    }

    pub(crate) fn set_account_session(&mut self, epoch: Option<u64>) {
        if epoch.is_none() {
            // Mirror UI retirement: Back may retain public origins, but must not
            // reopen an inaccessible shelf and start another authorization.
            self.history
                .retain(|snapshot| snapshot.page != Page::MyList);
        }
        if self.account_session == epoch {
            return;
        }
        self.retire_membership_visit();
        self.cancel_continue_watching();
        for snapshot in &mut self.history {
            if let Some(view) = &mut snapshot.view {
                view.clear_private_rows();
                view.close_native_sort();
            }
            if snapshot.private_epoch.take().is_some() {
                snapshot.view = None;
                if let Some(mut state) = snapshot.my_list.take() {
                    state.retire();
                }
            }
        }
        if self.private_epoch.take().is_some() {
            self.view = Presentation::loading("My List");
            self.view.set_status(criterion_ui::LoadState::Empty);
        }
        self.view.clear_private_rows();
        self.view.close_native_sort();
        if let Some(mut state) = self.my_list.take() {
            state.retire();
        }
        self.shelf_read = None;
        self.shelf_deadline = None;
        self.account_session = epoch;
    }
    pub(crate) fn is_shelf(&self) -> bool {
        self.page == Page::MyList
    }

    pub(crate) fn background(&mut self) {
        self.retire_membership_visit();
        self.foreground_active = false;
        self.view.close_native_sort();
        self.view.clear_native_resume();
        for snapshot in &mut self.history {
            if let Some(view) = &mut snapshot.view {
                view.clear_native_resume();
                view.close_native_sort();
            }
        }
        self.cancel_continue_watching();
        self.cancel_shelf();
        self.suspended |= self.jobs.is_active()
            || self.search_due.is_some()
            || self.native_detail_demand.is_some();
        self.cancel_native_detail();
        self.search_due = None;
        self.jobs.cancel();
        if let Some(pager) = &mut self.pager {
            pager.fail(&mut self.view);
        }
    }
    pub(crate) fn foreground(&mut self, runtime: &Handle) {
        self.foreground_active = true;
        if std::mem::take(&mut self.suspended)
            && let Some(query) = self.query.clone()
        {
            if let Some(pager) = &mut self.pager {
                if let Some(request) = pager.retry(&mut self.view, self.clock.now()) {
                    self.issue(
                        Query::Browse {
                            request: Arc::new(request),
                            options: self.options.clone(),
                        },
                        runtime,
                    );
                }
            } else {
                self.start(query, runtime);
            }
        }
    }

    fn remember(&mut self) {
        self.retire_membership_visit();
        self.view.clear_native_resume();
        self.view.close_native_sort();
        let interrupted_native = self.native_detail_demand.is_some();
        self.cancel_native_detail();
        self.cancel_continue_watching();
        if self.history.len() == MAX_HISTORY {
            self.history.remove(0);
        }
        let mut view = std::mem::replace(&mut self.view, Presentation::loading(""));
        let mut pager = self.pager.take();
        let committed_catalog = pager.as_ref().is_some_and(catalog::Pager::has_window);
        if let Some(p) = &mut pager {
            p.cancel(&mut view);
        }
        if let Some(state) = &mut self.my_list {
            state.cancel();
            view = Presentation::my_list(state);
        }
        self.shelf_read = None;
        self.shelf_deadline = None;
        let my_list = self.my_list.take();
        let interrupted_shelf = self.private_epoch.is_some()
            && my_list.is_none()
            && view.with_view(criterion_ui::LoginView::SignedOut, |data| {
                data.status == criterion_ui::LoadState::Loading
            });
        let interrupted_search = self.page == Page::Search
            && view.with_view(criterion_ui::LoginView::SignedOut, |data| {
                data.status == criterion_ui::LoadState::Loading
            });
        self.history.push(Snapshot {
            page: self.page,
            view: if (self.jobs.is_active() && !committed_catalog)
                || self.search_due.is_some()
                || interrupted_shelf
                || interrupted_search
                || interrupted_native
            {
                None
            } else {
                Some(view)
            },
            query: self.query.clone(),
            browse: self.browse.clone(),
            private_epoch: self.private_epoch.take(),
            search_loaded: self.search_loaded,
            pager,
            my_list,
        });
        self.trim_history();
    }
    fn trim_history(&mut self) {
        let fixed = self
            .options
            .as_ref()
            .map_or(0, |options| options_bytes(options));
        let query_bytes: usize = self
            .history
            .iter()
            .map(|snapshot| {
                snapshot
                    .pager
                    .as_ref()
                    .map_or(0, catalog::Pager::estimated_bytes)
                    + snapshot
                        .my_list
                        .as_ref()
                        .map_or(0, MyListState::retained_bytes)
                    + query_bytes(snapshot.query.as_ref())
                    + snapshot
                        .browse
                        .as_ref()
                        .map_or(0, |request| request_bytes(request))
            })
            .sum();
        let mut bytes: usize = fixed
            + query_bytes
            + self
                .history
                .iter()
                .filter_map(|s| s.view.as_ref())
                .map(Presentation::estimated_bytes)
                .sum::<usize>();
        for snapshot in &mut self.history {
            if bytes <= HISTORY_BYTES {
                break;
            }
            if let Some(view) = snapshot.view.take() {
                bytes = bytes.saturating_sub(view.estimated_bytes());
            }
            if let Some(state) = &mut snapshot.my_list {
                let before = state.retained_bytes();
                state.evict_windows();
                bytes = bytes.saturating_sub(before.saturating_sub(state.retained_bytes()));
            }
        }
    }

    fn navigate(&mut self, page: Page, runtime: &Handle) -> Effect {
        self.retire_membership_visit();
        self.cancel_native_detail();
        self.cancel_continue_watching();
        self.search_due = None;
        self.search_loaded = false;
        if page != Page::MyList {
            self.clear_shelf();
        }
        match page {
            Page::Home => self.start(Query::Discovery(DiscoveryRoute::Home), runtime),
            Page::New => self.start(Query::Discovery(DiscoveryRoute::New), runtime),
            Page::AllFilms => self.start(
                Query::Browse {
                    request: self
                        .browse
                        .clone()
                        .unwrap_or_else(|| Arc::new(BrowseRequest::default())),
                    options: self.options.clone(),
                },
                runtime,
            ),
            _ => {
                self.jobs.cancel();
                self.query = None;
                self.view = Presentation::loading(title(page));
                self.view.set_status(criterion_ui::LoadState::Empty);
            }
        }
        if page == Page::MyList {
            Effect::AccountShelf
        } else {
            Effect::None
        }
    }

    fn start(&mut self, mut query: Query, runtime: &Handle) {
        self.retire_membership_visit();
        self.cancel_native_detail();
        self.cancel_continue_watching();
        self.search_due = None;
        self.search_loaded = false;
        self.clear_shelf();
        if let Query::Browse { options, .. } = &mut query {
            *options = self.options.clone();
        }
        self.suspended = false;
        self.query = Some(query.clone());
        self.pager = None;
        if let Query::Browse { request, .. } = &query {
            self.browse = Some(request.clone());
            self.pager = Some(catalog::Pager::new(request.clone(), self.clock.now()));
        }
        self.view = Presentation::loading(title(self.page));
        if matches!(query, Query::NativeDetail { .. }) {
            // Native middleware reads belong to the shared Accounts owner.
            self.jobs.cancel();
            return;
        }
        if matches!(query, Query::Search(_, _)) {
            self.jobs.cancel();
            self.search_due = self.clock.now().checked_add(SEARCH_DELAY);
            if self.search_due.is_none() {
                self.view.set_status(criterion_ui::LoadState::Error);
            }
            return;
        }
        self.issue(query, runtime);
    }
    fn issue(&mut self, query: Query, runtime: &Handle) {
        let catalog = self.catalog.clone();
        self.jobs.replace(runtime, async move {
            match query {
                Query::NativeDetail { .. } => Err(Error::InvalidRequest),
                Query::Discovery(route) => catalog.discovery(route).await.map(Loaded::Discovery),
                Query::Detail(id) => catalog
                    .detail(&id)
                    .await
                    .map(|detail| Loaded::Detail(Box::new(detail))),
                Query::Search(query, _) => catalog
                    .search(&query)
                    .await
                    .map(|results| Loaded::Search(results, query)),
                Query::Browse { request, options } => {
                    let options = match options {
                        Some(options) => options,
                        None => Arc::new(catalog.options().await?),
                    };
                    let page = catalog.browse(&request).await?;
                    Ok(Loaded::Browse(page, options))
                }
            }
        });
    }
}

#[cfg(test)]
#[path = "controller/my_list_tests.rs"]
mod my_list_tests;

#[cfg(test)]
#[path = "controller/search_tests.rs"]
mod search_tests;

// The validated options are admitted once per process and shared across snapshots.
// Charge them once, including retained vector/string storage, inside the history
// estimate. Query/request charges are conservative when snapshots share an Arc.
fn options_bytes(options: &BrowseOptions) -> usize {
    std::mem::size_of::<BrowseOptions>()
        + options.filter_groups.capacity()
            * std::mem::size_of::<criterion_provider::FilterOptions>()
        + options.sort_options.capacity() * std::mem::size_of::<criterion_provider::SortOption>()
        + options
            .sort_options
            .iter()
            .map(|option| option.label.capacity())
            .sum::<usize>()
        + options
            .filter_groups
            .iter()
            .map(|group| {
                group.label.capacity()
                    + group.options.capacity()
                        * std::mem::size_of::<criterion_provider::FilterOption>()
                    + group
                        .options
                        .iter()
                        .map(|option| option.label.capacity() + option.value.as_str().len())
                        .sum::<usize>()
            })
            .sum::<usize>()
}
fn request_bytes(request: &BrowseRequest) -> usize {
    std::mem::size_of::<BrowseRequest>()
        + request.filters.capacity() * std::mem::size_of::<Filter>()
        + request
            .filters
            .iter()
            .map(|filter| filter.value.as_str().len())
            .sum::<usize>()
        + request.cursor.as_ref().map_or(0, |_| 512)
}
fn query_bytes(query: Option<&Query>) -> usize {
    std::mem::size_of::<Query>()
        + match query {
            Some(Query::Search(query, _)) => query.capacity(),
            Some(Query::Detail(id) | Query::NativeDetail { id, .. }) => id.as_str().len(),
            Some(Query::Discovery(DiscoveryRoute::Discover(slug))) => slug.as_str().len(),
            Some(Query::Browse { request, .. }) => request_bytes(request),
            _ => 0,
        }
}

fn browse_request(
    selection: &FilterSelection,
    options: &BrowseOptions,
) -> Result<BrowseRequest, Error> {
    let sort = options
        .sort_options
        .get(selection.sort_index)
        .ok_or(Error::InvalidRequest)?
        .sort;
    let filters = selection
        .options
        .iter()
        .map(|(group, option)| {
            let group = options
                .filter_groups
                .get(*group)
                .ok_or(Error::InvalidRequest)?;
            Ok(Filter {
                group: group.group,
                value: group
                    .options
                    .get(*option)
                    .ok_or(Error::InvalidRequest)?
                    .value
                    .clone(),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(BrowseRequest {
        sort,
        direction: if selection.descending {
            SortDirection::Descending
        } else {
            SortDirection::Ascending
        },
        filters,
        ..BrowseRequest::default()
    })
}

fn title(page: Page) -> &'static str {
    match page {
        Page::Home => "Home",
        Page::New => "New",
        Page::AllFilms => "All Films",
        Page::Search => "Search",
        Page::Detail => "",
        Page::Login => "Sign in",
        Page::Discovery => "Criterion Channel",
        Page::MyList => "My List",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use criterion_provider::{Request, Response};
    struct Offline;
    impl RequestTransport for Offline {
        async fn get(&self, _request: Request) -> Result<Response, Error> {
            Err(Error::Unavailable)
        }
    }
    fn runtime() -> Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }
    fn detail(runtime: &Runtime) -> Controller<Offline> {
        let mut owner = Controller::new(Catalog::with_transport(Offline), runtime.handle());
        owner.jobs.cancel();
        runtime.block_on(owner.jobs.finish());
        owner.page = Page::Detail;
        owner.query = Some(Query::Detail(MediaId::new("fixtureA").unwrap()));
        owner.view = Presentation::loading("Fixture detail");
        owner.view.set_status(criterion_ui::LoadState::Ready);
        owner
    }
    #[test]
    fn detail_login_cancel_restores_exact_previous_model() {
        let runtime = runtime();
        let mut owner = detail(&runtime);
        owner.command(Command::Authenticate, Page::Login, runtime.handle());
        assert_eq!(
            owner.history.len(),
            1,
            "Login must mirror the UI push from Detail"
        );
        owner.command(
            Command::Restore(Page::Detail),
            Page::Detail,
            runtime.handle(),
        );
        assert_eq!(owner.view.title(), "Fixture detail");
        assert!(matches!(owner.query, Some(Query::Detail(_))));
        assert!(
            !owner.jobs.is_active(),
            "restoring a completed detail need not refetch"
        );
    }
    #[test]
    fn account_login_navigation_preserves_detail_history() {
        let runtime = runtime();
        let mut owner = detail(&runtime);
        owner.command(
            Command::Navigate(Page::Login),
            Page::Login,
            runtime.handle(),
        );
        assert_eq!(
            owner.history.len(),
            1,
            "signed-in Account opens the same Login history boundary"
        );
        owner.command(
            Command::Restore(Page::Detail),
            Page::Detail,
            runtime.handle(),
        );
        assert_eq!(owner.view.title(), "Fixture detail");
    }
    fn private_list() -> criterion_account::WatchList {
        criterion_account::WatchList {
            playlist: vec![criterion_account::MediaSummary {
                id: MediaId::new("fixtureA").unwrap(),
                title: "Synthetic private selection".into(),
                kind: criterion_account::MediaKind::Film,
                duration: None,
                release_date: None,
                series_id: None,
                series_title: None,
            }],
            paging: criterion_account::PagingInfo {
                page_limit: 50,
                next_pagination_key: None,
            },
            type_counts: Vec::new(),
        }
    }
    #[test]
    fn native_my_list_boundary_requests_continuation_without_discarding_committed_cards() {
        let runtime = runtime();
        let mut owner = detail(&runtime);
        owner.set_account_session(Some(1));
        owner.command(
            Command::Navigate(Page::MyList),
            Page::MyList,
            runtime.handle(),
        );
        let first_read = owner.begin_shelf(1).unwrap();
        let page = criterion_account::WatchList {
            playlist: (0..50)
                .map(|i| criterion_account::MediaSummary {
                    id: MediaId::new(&format!("F{i:07X}")).unwrap(),
                    title: format!("Synthetic private film {i}"),
                    kind: criterion_account::MediaKind::Film,
                    duration: None,
                    release_date: None,
                    series_id: None,
                    series_title: None,
                })
                .collect(),
            paging: criterion_account::PagingInfo {
                page_limit: 50,
                next_pagination_key: Some(
                    criterion_provider::PageCursor::new("opaque continuation").unwrap(),
                ),
            },
            type_counts: Vec::new(),
        };
        assert!(owner.admit_shelf(1, &first_read, page).is_none());
        let effect = owner.command(
            Command::Catalog {
                anchor: 48,
                target: 52,
            },
            Page::MyList,
            runtime.handle(),
        );
        let Effect::AccountRead(read) = effect else {
            panic!("the native 50-card boundary must issue account continuation work")
        };
        assert_eq!(read.request.filter, criterion_account::WatchListFilter::All);
        assert_eq!(read.request.cursor.unwrap().as_str(), "opaque continuation");
        owner
            .view
            .with_view(criterion_ui::LoginView::SignedIn, |view| {
                assert_eq!(
                    view.cards.len(),
                    50,
                    "in-flight continuation retains the committed page"
                );
            });
    }
    #[test]
    fn logout_retires_private_history_without_destroying_public_navigation() {
        let runtime = runtime();
        let mut owner = detail(&runtime);
        owner.set_account_session(Some(1));
        owner.command(
            Command::Navigate(Page::MyList),
            Page::MyList,
            runtime.handle(),
        );
        let first_read = owner.begin_shelf(1).unwrap();
        assert!(owner.admit_shelf(1, &first_read, private_list()).is_none());
        owner.command(
            Command::Navigate(Page::Login),
            Page::Login,
            runtime.handle(),
        );
        owner.set_account_session(None);
        owner
            .view
            .with_view(criterion_ui::LoginView::SignedOut, |view| {
                assert!(view.cards.is_empty())
            });
        owner.command(
            Command::Restore(Page::Detail),
            Page::Detail,
            runtime.handle(),
        );
        assert_eq!(owner.view.title(), "Fixture detail");
        assert!(!owner.jobs.is_active());
    }
    #[test]
    fn reauthentication_rejects_old_shelf_and_departed_navigation_publication() {
        let runtime = runtime();
        let mut owner = detail(&runtime);
        owner.set_account_session(Some(1));
        owner.command(
            Command::Navigate(Page::MyList),
            Page::MyList,
            runtime.handle(),
        );
        let first_read = owner.begin_shelf(1).unwrap();
        assert!(owner.admit_shelf(1, &first_read, private_list()).is_none());
        owner.set_account_session(Some(2));
        owner
            .view
            .with_view(criterion_ui::LoginView::SignedIn, |view| {
                assert!(
                    view.cards.is_empty(),
                    "a new subscriber intent cannot retain the prior selection"
                )
            });
        let new_read = owner.begin_shelf(2).unwrap();
        assert!(owner.admit_shelf(1, &first_read, private_list()).is_none());
        assert!(owner.shelf_owns(2, &new_read));
        owner.command(
            Command::Navigate(Page::Search),
            Page::Search,
            runtime.handle(),
        );
        assert!(owner.admit_shelf(2, &new_read, private_list()).is_none());
        assert!(!owner.shelf_owns(2, &new_read));
        owner.set_account_session(None);
        owner
            .view
            .with_view(criterion_ui::LoginView::SignedOut, |view| {
                assert!(view.cards.is_empty());
            });
        owner.command(
            Command::Restore(Page::Detail),
            Page::Detail,
            runtime.handle(),
        );
        assert_eq!(owner.view.title(), "Fixture detail");
    }
    #[test]
    fn all_films_reentry_keeps_exact_applied_request_and_options() {
        let runtime = runtime();
        let mut owner = detail(&runtime);
        let options = Arc::new(BrowseOptions {
            filter_groups: Vec::new(),
            sort_options: Vec::new(),
        });
        let request = Arc::new(BrowseRequest {
            sort: criterion_provider::Sort::Year,
            direction: SortDirection::Descending,
            ..BrowseRequest::default()
        });
        owner.options = Some(options.clone());
        owner.browse = Some(request.clone());
        owner.command(
            Command::Navigate(Page::AllFilms),
            Page::AllFilms,
            runtime.handle(),
        );
        let Some(Query::Browse {
            request: issued,
            options: Some(issued_options),
        }) = &owner.query
        else {
            panic!("browse request missing");
        };
        assert!(Arc::ptr_eq(issued, &request));
        assert!(Arc::ptr_eq(issued_options, &options));
        assert_eq!(issued.sort, criterion_provider::Sort::Year);
        assert_eq!(issued.direction, SortDirection::Descending);
    }
    #[test]
    fn options_storage_is_charged_and_shared_across_history() {
        let runtime = runtime();
        let mut owner = detail(&runtime);
        let options = Arc::new(BrowseOptions {
            filter_groups: vec![criterion_provider::FilterOptions {
                group: criterion_provider::FilterGroup::Genres,
                label: String::with_capacity(1024),
                options: vec![criterion_provider::FilterOption {
                    label: String::with_capacity(2048),
                    value: criterion_provider::FilterValue::new("fixture").unwrap(),
                }],
            }],
            sort_options: Vec::new(),
        });
        assert!(options_bytes(&options) >= 3072);
        owner.options = Some(options.clone());
        owner.remember();
        owner.command(
            Command::Restore(Page::Detail),
            Page::Detail,
            runtime.handle(),
        );
        assert!(Arc::ptr_eq(owner.options.as_ref().unwrap(), &options));
        assert_eq!(
            Arc::strong_count(&options),
            2,
            "one options admission is shared, never cloned into historical sets"
        );
    }
    #[test]
    fn first_options_publication_retrims_retained_history_immediately() {
        let runtime = runtime();
        let mut owner = detail(&runtime);
        owner.view = Presentation::loading(String::with_capacity(HISTORY_BYTES - 4096));
        owner.remember();
        assert!(owner.history[0].view.is_some());
        let options = Arc::new(BrowseOptions {
            filter_groups: vec![criterion_provider::FilterOptions {
                group: criterion_provider::FilterGroup::Genres,
                label: "g".repeat(512),
                options: (0..16)
                    .map(|_| criterion_provider::FilterOption {
                        label: "l".repeat(512),
                        value: criterion_provider::FilterValue::new("fixture").unwrap(),
                    })
                    .collect(),
            }],
            sort_options: Vec::new(),
        });
        owner.jobs.replace(runtime.handle(), async move {
            Ok(Loaded::Browse(
                CatalogPage {
                    total: 0,
                    next_cursor: None,
                    items: Vec::new(),
                },
                options,
            ))
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while owner.jobs.is_active() {
            owner.poll(&runtime);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            owner.history[0].view.is_none(),
            "new options storage must immediately evict the older near-budget model"
        );
        assert!(
            matches!(owner.history[0].query, Some(Query::Detail(_))),
            "evicted view remains reloadable"
        );
    }
}

#[cfg(test)]
#[path = "controller/catalog_tests.rs"]
mod catalog_tests;
