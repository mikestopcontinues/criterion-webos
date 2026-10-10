#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailItem {
    Search,
    Home,
    New,
    MyList,
    AllFilms,
    Login,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    New,
    AllFilms,
    Search,
    Detail,
    Login,
    Discovery,
    MyList,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Hero,
    Card { row: usize, column: usize },
    FeaturedCard(usize),
    Rail(RailItem),
    FilterButton,
    LoginPrimary,
    LoginCancel,
    SearchKey(usize),
    SearchField,
    SearchVoice,
    SearchGroup(usize),
    DetailAction(usize),
    DetailDescription,
    DetailTab(usize),
    DetailSeason(usize),
    InformationPrimary,
    InformationClose,
    FilterGroup(usize),
    FilterOption(usize),
    FilterApply,
    FilterReset,
    FilterClose,
    CatalogRetry,
    MyListGroup(crate::MyListGroup),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Intent {
    Navigate(Page),
    Restore(Page),
    OpenCard {
        page: Page,
        focus: Focus,
    },
    OpenHero,
    Play,
    ToggleList,
    SelectPlaylist(usize),
    SelectSeason(usize),
    Authenticate,
    VoiceSearch,
    Search {
        query: String,
        group: crate::SearchGroup,
    },
    Exit,
    ApplyFilters(crate::FilterSelection),
}

#[derive(Clone)]
struct Snapshot {
    page: Page,
    focus: Focus,
    return_focus: Focus,
    scroll_y: f32,
    detail_state: crate::detail::DetailState,
    search_query: String,
    search_group: crate::SearchGroup,
    filters: crate::filter::FilterState,
    my_list: crate::my_list::MyListState,
}
pub struct AppUi {
    pub(crate) detail_state: crate::detail::DetailState,
    pub(crate) filters: crate::filter::FilterState,
    history: Vec<Snapshot>,
    pub(crate) context: egui::Context,
    pub(crate) images: crate::images::ImageCache,
    pub(crate) focus: Focus,
    pub(crate) scroll_y: f32,
    pub(crate) return_focus: Focus,
    page: Page,
    pub(crate) search: crate::search::SearchState,
    pub(crate) login: crate::login::LoginState,
    pub(crate) pointer_press: Option<crate::input::PointerTarget>,
    pub(crate) pointer_layout_focus: Option<Focus>,
    pub(crate) catalog_pending: Option<(Focus, usize)>,
    pub(crate) my_list: crate::my_list::MyListState,
}
impl Default for AppUi {
    fn default() -> Self {
        Self::new()
    }
}
impl AppUi {
    pub fn new() -> Self {
        Self {
            detail_state: crate::detail::DetailState::default(),
            filters: crate::filter::FilterState::default(),
            history: Vec::new(),
            context: egui::Context::default(),
            images: crate::images::ImageCache::default(),
            focus: Focus::Hero,
            scroll_y: 0.0,
            return_focus: Focus::Hero,
            page: Page::Home,
            search: crate::search::SearchState::default(),
            login: crate::login::LoginState::default(),
            pointer_press: None,
            pointer_layout_focus: None,
            catalog_pending: None,
            my_list: crate::my_list::MyListState::default(),
        }
    }
    pub(crate) fn sync_rail(&mut self, login: crate::LoginView<'_>) {
        if !matches!(login, crate::LoginView::SignedIn) {
            self.my_list = crate::my_list::MyListState::default();
            if self.page == Page::MyList {
                self.catalog_pending = None;
                if matches!(
                    self.focus,
                    Focus::Card { .. } | Focus::MyListGroup(_) | Focus::CatalogRetry
                ) {
                    self.pointer_press = None;
                    self.pointer_layout_focus = None;
                }
                if matches!(self.focus, Focus::MyListGroup(_) | Focus::CatalogRetry) {
                    self.focus = Focus::Card { row: 0, column: 0 };
                    self.scroll_y = 0.0;
                }
                if matches!(
                    self.return_focus,
                    Focus::MyListGroup(_) | Focus::CatalogRetry
                ) {
                    self.return_focus = Focus::Card { row: 0, column: 0 };
                }
            }
            // A departed subscriber's shelf must never become an automatic
            // authorization origin. Keep the public history in its exact order.
            self.history
                .retain(|snapshot| snapshot.page != Page::MyList);
            for snapshot in &mut self.history {
                snapshot.my_list = crate::my_list::MyListState::default();
            }
            if self.focus == Focus::Rail(RailItem::MyList) {
                self.focus = Focus::Rail(RailItem::Login);
                self.pointer_press = None;
                self.pointer_layout_focus = None;
            }
        }
    }
    /// Enter activation while retaining the exact current focus and scroll.
    /// The runtime mirrors this display history and executes the returned command.
    pub fn begin_authentication(&mut self) -> Vec<crate::Command> {
        if self.page != Page::Login {
            self.push_history();
        }
        self.page = Page::Login;
        self.focus = Focus::LoginCancel;
        self.scroll_y = 0.0;
        self.login = crate::login::LoginState::default();
        self.search.composition.clear();
        self.search.select_all = false;
        self.pointer_press = None;
        self.pointer_layout_focus = None;
        vec![crate::Command::Authenticate]
    }
    pub(crate) fn restore_previous(&mut self) -> Option<Page> {
        let previous = self.history.pop()?;
        self.page = previous.page;
        self.focus = previous.focus;
        self.return_focus = previous.return_focus;
        self.scroll_y = previous.scroll_y;
        self.detail_state = previous.detail_state;
        self.search.query = previous.search_query;
        self.search.group = previous.search_group;
        self.search.composition.clear();
        self.search.select_all = false;
        self.filters = previous.filters;
        if self.page == Page::MyList {
            self.my_list = previous.my_list;
        }
        Some(self.page)
    }
    pub fn page(&self) -> Page {
        self.page
    }
    pub fn focus(&self) -> Focus {
        self.focus
    }
    pub(crate) fn activate_target(&mut self, target: &crate::Target, login: crate::LoginView<'_>) {
        self.page = if matches!(
            target,
            crate::Target::Content(criterion_provider::ContentTarget::MyList)
        ) && !matches!(login, crate::LoginView::SignedIn)
        {
            Page::Login
        } else {
            target.page()
        };
        if self.page == Page::AllFilms {
            self.filters.open = false;
        }
        if self.page == Page::Detail {
            self.detail_state = crate::detail::DetailState::default();
        }
        self.focus = match self.page {
            Page::Detail => Focus::DetailAction(0),
            Page::Home | Page::New | Page::Discovery => Focus::Hero,
            _ => Focus::Card { row: 0, column: 0 },
        };
        self.scroll_y = 0.0;
    }
    pub(crate) fn layout_focus(&self) -> Focus {
        self.pointer_layout_focus.unwrap_or(self.focus)
    }
    pub(crate) fn handle_navigation(
        &mut self,
        action: Action,
        rows: &[usize],
        login: crate::LoginView<'_>,
    ) -> Vec<Intent> {
        if let Some(commands) = self.handle_filter(action) {
            return commands;
        }
        if let Some(commands) = self.handle_detail(action, rows) {
            return commands;
        }
        if let Some(commands) = self.handle_search(action, rows) {
            return commands;
        }
        if action == Action::Back
            && !matches!(self.focus, Focus::Rail(_))
            && let Some(page) = self.restore_previous()
        {
            return vec![Intent::Restore(page)];
        }
        if action == Action::Select && self.focus == Focus::Hero {
            self.push_history();
            self.page = Page::Detail;
            self.focus = Focus::DetailAction(0);
            self.scroll_y = 0.0;
            return vec![Intent::OpenHero];
        }
        if action == Action::Select
            && let Focus::Card { row, column } = self.focus
            && rows.get(row).is_some_and(|count| column < *count)
        {
            let page = self.page;
            self.push_history();
            self.page = Page::Detail;
            self.focus = Focus::DetailAction(0);
            self.scroll_y = 0.0;
            return vec![Intent::OpenCard {
                page,
                focus: Focus::Card { row, column },
            }];
        }
        if action == Action::Left
            && matches!(
                self.focus,
                Focus::Hero | Focus::Card { column: 0, .. } | Focus::FeaturedCard(0)
            )
        {
            self.return_focus = self.focus;
            self.focus = Focus::Rail(match self.page {
                Page::AllFilms => RailItem::AllFilms,
                Page::New => RailItem::New,
                Page::Search => RailItem::Search,
                Page::Login => RailItem::Login,
                Page::MyList if matches!(login, crate::LoginView::SignedIn) => RailItem::MyList,
                Page::MyList => RailItem::Login,
                _ => RailItem::Home,
            });
        } else if matches!(self.focus, Focus::Rail(_))
            && matches!(action, Action::Back | Action::Right)
        {
            self.focus = self.return_focus;
            return Vec::new();
        } else if let Focus::Rail(item) = self.focus {
            let mut items = crate::rail::entries(login);
            let index = items
                .clone()
                .position(|candidate| candidate.item == item)
                .unwrap_or(1);
            match action {
                Action::Down => {
                    if let Some(next) = items.clone().nth(index + 1) {
                        self.focus = Focus::Rail(next.item);
                    }
                }
                Action::Up if index > 0 => {
                    if let Some(previous) = items.clone().nth(index - 1) {
                        self.focus = Focus::Rail(previous.item);
                    }
                }
                Action::Select => {
                    if (item == RailItem::Search && self.page != Page::Search)
                        || (item != RailItem::Search && self.page == Page::Search)
                        || (item == RailItem::Login && self.page != Page::Login)
                        || ((item == RailItem::MyList) != (self.page == Page::MyList))
                    {
                        self.push_history();
                    }
                    let Some(selected) = items.find(|entry| entry.item == item) else {
                        return Vec::new();
                    };
                    self.page = selected.page;
                    if self.page == Page::AllFilms {
                        self.filters.open = false;
                    }
                    self.focus = if self.page == Page::Login {
                        Focus::LoginCancel
                    } else if self.page == Page::Search {
                        Focus::SearchKey(0)
                    } else if matches!(self.page, Page::Home | Page::New) {
                        Focus::Hero
                    } else {
                        Focus::Card { row: 0, column: 0 }
                    };
                    self.scroll_y = 0.0;
                    return vec![if self.page == Page::Login {
                        Intent::Authenticate
                    } else {
                        Intent::Navigate(self.page)
                    }];
                }
                _ => (),
            }
        } else if action == Action::Down
            && self.focus == Focus::FilterButton
            && rows.first().is_some_and(|count| *count > 0)
        {
            self.focus = Focus::Card { row: 0, column: 0 };
        } else if action == Action::Down
            && self.focus == Focus::Hero
            && rows.first().is_some_and(|count| *count > 0)
        {
            self.focus = Focus::Card { row: 0, column: 0 };
            self.scroll_y = 632.0;
        } else if action == Action::Up && matches!(self.focus, Focus::Card { row: 0, .. }) {
            self.focus = if self.page == Page::MyList {
                self.focus
            } else if self.page == Page::AllFilms {
                Focus::FilterButton
            } else {
                Focus::Hero
            };
            self.scroll_y = 0.0;
        } else if let Focus::Card { row, column } = self.focus {
            let next = match action {
                Action::Right if column + 1 < rows.get(row).copied().unwrap_or(0) => {
                    Some((row, column + 1))
                }
                Action::Left if column > 0 => Some((row, column - 1)),
                Action::Down if rows.get(row + 1).is_some_and(|count| *count > 0) => {
                    Some((row + 1, column.min(rows[row + 1] - 1)))
                }
                Action::Up if row > 0 && rows.get(row - 1).is_some_and(|count| *count > 0) => {
                    Some((row - 1, column.min(rows[row - 1] - 1)))
                }
                _ => None,
            };
            if let Some((row, column)) = next {
                self.focus = Focus::Card { row, column };
                self.scroll_y = if self.page == Page::Search {
                    (row as f32 * 321.0 - 415.0).max(0.0)
                } else if matches!(self.page, Page::AllFilms | Page::MyList) {
                    (row as f32 * 321.0 - 172.0).max(0.0)
                } else {
                    632.0 + row as f32 * 397.0
                };
            }
        }
        if action == Action::Back
            && self.page == Page::Home
            && !matches!(self.focus, Focus::Rail(_))
        {
            return vec![Intent::Exit];
        }
        Vec::new()
    }
    pub(crate) fn push_history(&mut self) {
        let catalog_restore = self.catalog_pending.map(|(focus, _)| focus);
        self.catalog_pending = None;
        let restore_focus = |focus| {
            if focus == Focus::CatalogRetry {
                catalog_restore.unwrap_or(Focus::Card { row: 0, column: 0 })
            } else {
                focus
            }
        };
        if self.history.len() == 16 {
            self.history.remove(0);
        }
        self.history.push(Snapshot {
            page: self.page,
            focus: if matches!(self.focus, Focus::Rail(_)) {
                restore_focus(self.return_focus)
            } else {
                restore_focus(self.focus)
            },
            return_focus: restore_focus(self.return_focus),
            scroll_y: self.scroll_y,
            detail_state: self.detail_state,
            search_query: self.search.query.clone(),
            search_group: self.search.group,
            filters: self.filters.clone(),
            my_list: self.my_list_snapshot(),
        });
    }
    pub fn scroll_y(&self) -> f32 {
        self.scroll_y
    }
}
