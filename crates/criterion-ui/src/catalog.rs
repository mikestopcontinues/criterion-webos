//! Remote navigation over global positions in a bounded catalog window.
use crate::{Action, AppUi, CatalogTail, Command, Focus, Page, ViewData};
impl AppUi {
    pub(crate) fn sync_catalog(&mut self, data: &ViewData<'_>) {
        if self.page() != Page::AllFilms {
            self.catalog_pending = None;
            return;
        }
        let Some(window) = data.catalog else {
            return;
        };
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
    }
    pub(crate) fn handle_catalog(
        &mut self,
        action: Action,
        data: &ViewData<'_>,
    ) -> Option<Vec<Command>> {
        self.sync_catalog(data);
        if self.page() != Page::AllFilms || self.filters.open {
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
                    let focus = self.catalog_pending.map_or(
                        Focus::Card {
                            row: (window.first + data.cards.len().saturating_sub(1)) / 4,
                            column: (window.first + data.cards.len().saturating_sub(1)) % 4,
                        },
                        |(anchor, _)| anchor,
                    );
                    self.focus = focus;
                    vec![Command::RetryCatalog]
                }
                Action::Up | Action::Back => {
                    let focus = self.catalog_pending.map_or(
                        Focus::Card {
                            row: (window.first + data.cards.len().saturating_sub(1)) / 4,
                            column: (window.first + data.cards.len().saturating_sub(1)) % 4,
                        },
                        |(anchor, _)| anchor,
                    );
                    self.focus = focus;
                    vec![]
                }
                Action::Left => {
                    self.return_focus = self
                        .catalog_pending
                        .map_or(Focus::Card { row: 0, column: 0 }, |(focus, _)| focus);
                    self.focus = Focus::Rail(crate::RailItem::AllFilms);
                    vec![]
                }
                _ => vec![],
            });
        }
        if self.focus == Focus::FilterButton && action == Action::Down && !data.cards.is_empty() {
            if window.first > 0 {
                self.catalog_pending = Some((Focus::FilterButton, 0));
            } else {
                self.catalog_focus(0);
            }
            return Some(vec![Command::Catalog {
                anchor: window.first,
                target: 0,
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
            self.focus = Focus::FilterButton;
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
        if self.page() != Page::AllFilms {
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
        if self.page() != Page::AllFilms || self.filters.open {
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
