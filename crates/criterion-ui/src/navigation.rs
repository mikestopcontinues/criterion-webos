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
    Rail(RailItem),
    FilterButton,
    SearchKey(usize),
    SearchField,
    SearchVoice,
    SearchGroup(usize),
    DetailAction(usize),
    DetailDescription,
    DetailTab(usize),
    InformationPrimary,
    InformationClose,
    FilterGroup(usize),
    FilterOption(usize),
    FilterApply,
    FilterReset,
    FilterClose,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Intent {
    Navigate(Page),
    Restore(Page),
    OpenCard {
        page: Page,
        row: usize,
        column: usize,
    },
    OpenHero,
    Play,
    ToggleList,
    SelectPlaylist(usize),
    Authenticate,
    VoiceSearch,
    Search {
        query: String,
        group: crate::SearchGroup,
    },
    Exit,
    ApplyFilters(crate::FilterSelection),
}

#[derive(Clone, Copy)]
struct Snapshot {
    page: Page,
    focus: Focus,
    return_focus: Focus,
    scroll_y: f32,
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
    pub(crate) pointer_press: Option<crate::input::PointerTarget>,
    pub(crate) pointer_layout_focus: Option<Focus>,
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
            pointer_press: None,
            pointer_layout_focus: None,
        }
    }
    pub fn page(&self) -> Page {
        self.page
    }
    pub fn focus(&self) -> Focus {
        self.focus
    }
    pub(crate) fn activate_target(&mut self, target: &crate::Target) {
        self.page = target.page();
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
    pub(crate) fn handle_navigation(&mut self, action: Action, rows: &[usize]) -> Vec<Intent> {
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
            && let Some(previous) = self.history.pop()
        {
            self.page = previous.page;
            self.focus = previous.focus;
            self.return_focus = previous.return_focus;
            self.scroll_y = previous.scroll_y;
            return vec![Intent::Restore(self.page)];
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
            return vec![Intent::OpenCard { page, row, column }];
        }
        if action == Action::Left
            && matches!(self.focus, Focus::Hero | Focus::Card { column: 0, .. })
        {
            self.return_focus = self.focus;
            self.focus = Focus::Rail(match self.page {
                Page::AllFilms => RailItem::AllFilms,
                Page::New => RailItem::New,
                Page::Search => RailItem::Search,
                Page::Login => RailItem::Login,
                _ => RailItem::Home,
            });
        } else if matches!(self.focus, Focus::Rail(_))
            && matches!(action, Action::Back | Action::Right)
        {
            self.focus = self.return_focus;
            return Vec::new();
        } else if let Focus::Rail(item) = self.focus {
            const ITEMS: [RailItem; 5] = [
                RailItem::Search,
                RailItem::Home,
                RailItem::New,
                RailItem::AllFilms,
                RailItem::Login,
            ];
            let index = ITEMS
                .iter()
                .position(|candidate| *candidate == item)
                .unwrap_or(1);
            match action {
                Action::Down if index < 4 => self.focus = Focus::Rail(ITEMS[index + 1]),
                Action::Up if index > 0 => self.focus = Focus::Rail(ITEMS[index - 1]),
                Action::Select => {
                    if (item == RailItem::Search && self.page != Page::Search)
                        || (item != RailItem::Search && self.page == Page::Search)
                    {
                        self.push_history();
                    }
                    self.page = match item {
                        RailItem::Search => Page::Search,
                        RailItem::Home => Page::Home,
                        RailItem::New => Page::New,
                        RailItem::AllFilms => Page::AllFilms,
                        RailItem::Login => Page::Login,
                    };
                    self.focus = if self.page == Page::Search {
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
    fn push_history(&mut self) {
        if self.history.len() == 16 {
            self.history.remove(0);
        }
        self.history.push(Snapshot {
            page: self.page,
            focus: if matches!(self.focus, Focus::Rail(_)) {
                self.return_focus
            } else {
                self.focus
            },
            return_focus: self.return_focus,
            scroll_y: self.scroll_y,
        });
    }
    pub fn scroll_y(&self) -> f32 {
        self.scroll_y
    }
}
