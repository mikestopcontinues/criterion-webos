// SPDX-License-Identifier: GPL-3.0-or-later
//! Main-thread catalog publication and bounded navigation snapshots.
use crate::{jobs::Jobs, presentation::Presentation};
use criterion_provider::{
    BrowseOptions, BrowseRequest, Catalog, CatalogPage, ContentTarget, DiscoveryPage,
    DiscoveryRoute, Error, Filter, HttpTransport, MediaDetail, MediaId, RequestTransport,
    SearchResults, SortDirection,
};
use criterion_ui::{Command, FilterSelection, Page, SearchGroup, Target};
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime};

const MAX_HISTORY: usize = 16;
const HISTORY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
enum Query {
    Discovery(DiscoveryRoute),
    Browse {
        request: Arc<BrowseRequest>,
        options: Option<Arc<BrowseOptions>>,
    },
    Detail(MediaId),
    Search(String, SearchGroup),
}
enum Loaded {
    Discovery(DiscoveryPage),
    Browse(CatalogPage, Arc<BrowseOptions>),
    Detail(Box<MediaDetail>),
    Search(SearchResults, SearchGroup),
}
struct Snapshot {
    page: Page,
    view: Option<Presentation>,
    query: Option<Query>,
    browse: Option<Arc<BrowseRequest>>,
}

pub(crate) enum Effect {
    None,
    Authenticate,
    RetryAuthentication,
    CancelAuthentication,
    Logout,
    Play(MediaId),
    ToggleList(MediaId),
    VoiceSearch,
    Exit,
}

pub(crate) struct Controller<T = HttpTransport> {
    catalog: Arc<Catalog<T>>,
    jobs: Jobs<Result<Loaded, Error>>,
    pub(crate) view: Presentation,
    page: Page,
    query: Option<Query>,
    history: Vec<Snapshot>,
    suspended: bool,
    options: Option<Arc<BrowseOptions>>,
    browse: Option<Arc<BrowseRequest>>,
}

impl<T: RequestTransport + Send + Sync + 'static> Controller<T> {
    pub(crate) fn new(catalog: Catalog<T>, runtime: &Handle) -> Self {
        let mut owner = Self {
            catalog: Arc::new(catalog),
            jobs: Jobs::new(),
            view: Presentation::loading("Home"),
            page: Page::Home,
            query: None,
            history: Vec::new(),
            suspended: false,
            options: None,
            browse: None,
        };
        owner.start(Query::Discovery(DiscoveryRoute::Home), runtime);
        owner
    }

    pub(crate) fn command(&mut self, command: Command, page: Page, runtime: &Handle) -> Effect {
        match command {
            Command::Navigate(destination) => {
                if (destination == Page::Search && self.page != Page::Search)
                    || (destination != Page::Search && self.page == Page::Search)
                    || (destination == Page::Login && self.page != Page::Login)
                {
                    self.remember();
                }
                self.page = destination;
                self.navigate(destination, runtime);
            }
            Command::Open(target) => {
                self.remember();
                self.page = page;
                match target {
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
                        self.navigate(Page::AllFilms, runtime)
                    }
                    Target::Content(ContentTarget::MyList | ContentTarget::Subscribe) => {
                        self.jobs.cancel();
                        self.query = None;
                        self.view = Presentation::loading(title(page));
                        return Effect::Authenticate;
                    }
                    Target::Content(ContentTarget::Discover(slug)) => {
                        self.start(Query::Discovery(DiscoveryRoute::Discover(slug)), runtime)
                    }
                }
            }
            Command::Restore(destination) => {
                self.jobs.cancel();
                if let Some(snapshot) = self.history.pop() {
                    self.page = destination;
                    self.query = snapshot.query;
                    self.browse = snapshot.browse;
                    if snapshot.page == destination {
                        if let Some(view) = snapshot.view {
                            self.view = view;
                        } else if let Some(query) = self.query.clone() {
                            self.start(query, runtime);
                        } else {
                            self.navigate(destination, runtime);
                        }
                    } else {
                        self.navigate(destination, runtime);
                    }
                } else {
                    self.page = destination;
                    self.navigate(destination, runtime);
                }
            }
            Command::Search { query, group } => {
                if query.trim().is_empty() {
                    self.jobs.cancel();
                    self.query = None;
                    self.view = Presentation::loading("Search");
                    self.view.set_status(criterion_ui::LoadState::Empty);
                } else {
                    self.start(Query::Search(query, group), runtime);
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
            Command::SelectPlaylist(index) => self.view.select_playlist(index),
            Command::Authenticate => {
                if self.page != Page::Login {
                    self.remember();
                }
                self.jobs.cancel();
                self.page = Page::Login;
                self.query = None;
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
        if let Some(result) = runtime.block_on(self.jobs.take_ready()) {
            match result {
                Ok(Ok(Loaded::Discovery(page))) => self.view = Presentation::discovery(page),
                Ok(Ok(Loaded::Browse(page, options))) => {
                    self.view = Presentation::catalog("All Films", page);
                    self.view.set_options(&options);
                    self.options = Some(options);
                    self.trim_history();
                }
                Ok(Ok(Loaded::Detail(detail))) => self.view = Presentation::detail(*detail),
                Ok(Ok(Loaded::Search(results, group))) => {
                    self.view = Presentation::search(results, group)
                }
                Ok(Err(Error::Unavailable | Error::Deadline)) => {
                    self.view.set_status(criterion_ui::LoadState::Offline)
                }
                _ => self.view.set_status(criterion_ui::LoadState::Error),
            }
        }
    }

    pub(crate) fn background(&mut self) {
        self.suspended |= self.jobs.is_active();
        self.jobs.cancel();
    }
    pub(crate) fn foreground(&mut self, runtime: &Handle) {
        if std::mem::take(&mut self.suspended)
            && let Some(query) = self.query.clone()
        {
            self.start(query, runtime);
        }
    }

    fn remember(&mut self) {
        if self.history.len() == MAX_HISTORY {
            self.history.remove(0);
        }
        let view = std::mem::replace(&mut self.view, Presentation::loading(""));
        self.history.push(Snapshot {
            page: self.page,
            view: if self.jobs.is_active() {
                None
            } else {
                Some(view)
            },
            query: self.query.clone(),
            browse: self.browse.clone(),
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
                query_bytes(snapshot.query.as_ref())
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
        }
    }

    fn navigate(&mut self, page: Page, runtime: &Handle) {
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
    }

    fn start(&mut self, mut query: Query, runtime: &Handle) {
        if let Query::Browse { options, .. } = &mut query {
            *options = self.options.clone();
        }
        self.suspended = false;
        self.query = Some(query.clone());
        if let Query::Browse { request, .. } = &query {
            self.browse = Some(request.clone());
        }
        self.view = Presentation::loading(title(self.page));
        let catalog = self.catalog.clone();
        self.jobs.replace(runtime, async move {
            match query {
                Query::Discovery(route) => catalog.discovery(route).await.map(Loaded::Discovery),
                Query::Detail(id) => catalog
                    .detail(&id)
                    .await
                    .map(|detail| Loaded::Detail(Box::new(detail))),
                Query::Search(query, group) => catalog
                    .search(&query)
                    .await
                    .map(|results| Loaded::Search(results, group)),
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
            Some(Query::Detail(id)) => id.as_str().len(),
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
