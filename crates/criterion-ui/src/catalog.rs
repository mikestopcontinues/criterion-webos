//! Remote navigation over global positions in a bounded catalog window.
use crate::{Action, AppUi, CatalogTail, Command, Focus, Page, ViewData};
impl AppUi {
    fn retry_origin(&self, data: &ViewData<'_>, window: crate::CatalogWindow) -> Focus {
        self.catalog_pending.map_or_else(
            || {
                if data.cards.is_empty() && self.page() == Page::MyList {
                    self.catalog_header(data)
                        .unwrap_or(Focus::Card { row: 0, column: 0 })
                } else {
                    Focus::Card {
                        row: (window.first + data.cards.len().saturating_sub(1)) / 4,
                        column: (window.first + data.cards.len().saturating_sub(1)) % 4,
                    }
                }
            },
            |(anchor, _)| anchor,
        )
    }
    fn catalog_header(&self, data: &ViewData<'_>) -> Option<Focus> {
        match self.page() {
            Page::AllFilms => Some(Focus::FilterButton),
            Page::MyList => data.my_list.and_then(|view| {
                view.choices
                    .iter()
                    .take(6)
                    .find(|c| c.group == view.selected)
                    .or_else(|| view.choices.first())
                    .map(|choice| Focus::MyListGroup(choice.group))
            }),
            _ => None,
        }
    }
    pub(crate) fn sync_catalog(&mut self, data: &ViewData<'_>) {
        if self.my_list_waiting() {
            return;
        }
        if !matches!(self.page(), Page::AllFilms | Page::MyList) {
            self.catalog_pending = None;
            return;
        }
        let Some(window) = data.catalog else {
            return;
        };
        if self.page() == Page::MyList
            && window.tail == CatalogTail::End
            && !data.cards.is_empty()
            && let Focus::Card { row, column } = self.focus
            && row * 4 + column >= window.first + data.cards.len()
        {
            self.catalog_focus(window.first + data.cards.len() - 1);
        }
        if data.cards.is_empty() && window.tail == CatalogTail::Error {
            if self.catalog_pending.is_none()
                && let Focus::Card { row, column } = self.focus
            {
                self.catalog_pending = Some((self.focus, row * 4 + column));
            }
            if self
                .catalog_pending
                .is_some_and(|(anchor, _)| anchor == self.focus)
            {
                self.focus = Focus::CatalogRetry;
            }
        }
        if let Some((anchor, _)) = self.catalog_pending
            && self.focus != anchor
            && self.focus != Focus::CatalogRetry
        {
            self.catalog_pending = None;
        }
        if let Some((anchor, target)) = self.catalog_pending
            && anchor == self.focus
            && window.tail == CatalogTail::End
            && target >= window.first + data.cards.len()
        {
            let end = window.first + data.cards.len();
            if end > window.first
                && let Focus::Card { row, .. } = anchor
                && target / 4 == (end - 1) / 4
                && row < (end - 1) / 4
            {
                self.catalog_focus(end - 1);
            }
            self.catalog_pending = None;
        }
        if let Some((anchor, target)) = self.catalog_pending
            && self.focus == anchor
            && target >= window.first
            && target < window.first + data.cards.len()
        {
            self.catalog_focus(target);
            self.catalog_pending = None;
        }
    }
    fn catalog_focus(&mut self, index: usize) {
        self.focus = Focus::Card {
            row: index / 4,
            column: index % 4,
        };
        self.scroll_y = (index as f32 / 4.0).floor() * 321.0 - 172.0;
        self.scroll_y = self.scroll_y.max(0.0);
        if let Some((saved, scroll)) = self.my_list_anchor()
            && saved == index
        {
            self.scroll_y = scroll;
        }
    }
    pub(crate) fn handle_catalog(
        &mut self,
        action: Action,
        data: &ViewData<'_>,
    ) -> Option<Vec<Command>> {
        if self.my_list_waiting() && !matches!(action, Action::Back | Action::Left) {
            return Some(vec![]);
        }
        self.sync_catalog(data);
        if !matches!(self.page(), Page::AllFilms | Page::MyList)
            || (self.page() == Page::AllFilms && self.filters.open)
        {
            return None;
        }
        let window = data.catalog?;
        if action == Action::Back && data.cards.is_empty() {
            self.catalog_pending = None;
            return None;
        }
        if self.focus == Focus::CatalogRetry {
            return Some(match action {
                Action::Select => {
                    let focus = self.retry_origin(data, window);
                    self.focus = focus;
                    vec![Command::RetryCatalog]
                }
                Action::Up | Action::Back => {
                    let focus = self.retry_origin(data, window);
                    self.focus = focus;
                    vec![]
                }
                Action::Left => {
                    self.return_focus = self
                        .catalog_pending
                        .map_or(Focus::Card { row: 0, column: 0 }, |(focus, _)| focus);
                    self.focus = Focus::Rail(if self.page() == Page::MyList {
                        if matches!(data.login, crate::LoginView::SignedIn) {
                            crate::RailItem::MyList
                        } else {
                            crate::RailItem::Login
                        }
                    } else {
                        crate::RailItem::AllFilms
                    });
                    vec![]
                }
                _ => vec![],
            });
        }
        if self.page() == Page::MyList
            && Some(self.focus) == self.catalog_header(data)
            && action == Action::Down
            && data.cards.is_empty()
            && window.tail == CatalogTail::Error
        {
            self.focus = Focus::CatalogRetry;
            return Some(vec![]);
        }
        if Some(self.focus) == self.catalog_header(data)
            && action == Action::Down
            && !data.cards.is_empty()
        {
            let target = self.my_list_anchor().map_or(0, |(index, _)| index);
            let target = if self.page() == Page::MyList && window.tail == CatalogTail::End {
                target.min(window.first + data.cards.len() - 1)
            } else {
                target
            };
            if target < window.first || target >= window.first + data.cards.len() {
                self.catalog_pending = Some((self.focus, target));
            } else {
                self.catalog_focus(target);
            }
            return Some(vec![Command::Catalog {
                anchor: if self.page() == Page::MyList {
                    target
                } else {
                    window.first
                },
                target,
            }]);
        }
        let Focus::Card { row, column } = self.focus else {
            return None;
        };
        let anchor = row * 4 + column;
        if action == Action::Select
            && let Some(card) = anchor
                .checked_sub(window.first)
                .and_then(|i| data.cards.get(i))
        {
            self.push_history();
            self.activate_target(card.key, data.login);
            return Some(vec![
                Command::Catalog {
                    anchor,
                    target: anchor,
                },
                Command::Open(card.key.clone()),
            ]);
        }
        // Select/Back/rail retain the existing navigation stack and card identity.
        if !matches!(
            action,
            Action::Up | Action::Down | Action::Left | Action::Right
        ) {
            return None;
        }
        self.catalog_pending = None;
        if action == Action::Up && row == 0 {
            self.focus = self.catalog_header(data).unwrap_or(self.focus);
            self.scroll_y = 0.0;
            return Some(vec![]);
        }
        if action == Action::Left && column == 0 {
            return None;
        }
        let end = window.first + data.cards.len();
        let mut target = match action {
            Action::Down => anchor + 4,
            Action::Up => anchor - 4,
            Action::Right if column < 3 => anchor + 1,
            Action::Left => anchor - 1,
            _ => anchor,
        };
        if window.tail == CatalogTail::End && target >= end {
            if action == Action::Down && (row + 1) * 4 < end {
                target = end - 1;
            } else {
                return Some(vec![]);
            }
        }
        if target < window.first || target >= end {
            self.catalog_pending = Some((self.focus, target));
            if window.tail == CatalogTail::Error {
                self.focus = Focus::CatalogRetry;
                return Some(vec![]);
            }
            return Some(vec![Command::Catalog { anchor, target }]);
        }
        self.catalog_focus(target);
        Some(vec![Command::Catalog {
            anchor: target,
            target,
        }])
    }
    pub(crate) fn catalog_anchor(&self, data: &ViewData<'_>) -> Option<usize> {
        if self.my_list_waiting() {
            return None;
        }
        if !matches!(self.page(), Page::AllFilms | Page::MyList) {
            return None;
        }
        let window = data.catalog?;
        let focus = if matches!(self.focus, Focus::Rail(_)) {
            self.return_focus
        } else {
            self.focus
        };
        let Focus::Card { row, column } = focus else {
            return None;
        };
        let anchor = row * 4 + column;
        (anchor >= window.first && anchor < window.first + data.cards.len()).then_some(anchor)
    }
    pub(crate) fn catalog_demand(&self, data: &ViewData<'_>) -> Option<Command> {
        if self.my_list_waiting() {
            return None;
        }
        if !matches!(self.page(), Page::AllFilms | Page::MyList)
            || (self.page() == Page::AllFilms && self.filters.open)
        {
            return None;
        }
        let window = data.catalog?;
        let Focus::Card { row, column } = self.focus else {
            return None;
        };
        if let Some((focus, target)) = self.catalog_pending
            && focus == self.focus
            && window.tail == CatalogTail::More
            && (target < window.first || target >= window.first + data.cards.len())
        {
            return Some(Command::Catalog {
                anchor: row * 4 + column,
                target,
            });
        }
        let anchor = row * 4 + column;
        if self.catalog_pending.is_none()
            && anchor >= window.first
            && anchor < window.first + data.cards.len()
        {
            Some(Command::Catalog {
                anchor,
                target: anchor,
            })
        } else {
            None
        }
    }
}
