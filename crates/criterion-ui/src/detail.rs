use crate::{Action, AppUi, Focus, Intent, Page};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailKind {
    #[default]
    Film,
    Collection,
    Supplement,
    Category,
    Series,
    Original,
    Episode,
    Franchise,
    Live,
}
#[derive(Clone, Copy)]
pub(crate) struct DetailState {
    pub kind: DetailKind,
    pub primary_enabled: bool,
    pub selected_tab: usize,
    pub seasons_count: usize,
    pub selected_season: usize,
    pub featured_count: usize,
    pub featured_height: f32,
    pub information: bool,
    pub return_focus: Focus,
    pub information_page: usize,
    pub information_pages: usize,
}
impl Default for DetailState {
    fn default() -> Self {
        Self {
            kind: DetailKind::Film,
            primary_enabled: true,
            selected_tab: 0,
            seasons_count: 0,
            selected_season: 0,
            featured_count: 0,
            featured_height: 0.0,
            information: false,
            return_focus: Focus::DetailAction(1),
            information_page: 0,
            information_pages: 1,
        }
    }
}
impl AppUi {
    pub(crate) fn set_information_content(&mut self, description: &str) {
        self.detail_state.information_pages =
            description.chars().count().div_ceil(420).clamp(1, 256);
        self.detail_state.information_page = self
            .detail_state
            .information_page
            .min(self.detail_state.information_pages - 1);
    }
    pub fn set_detail_kind(&mut self, kind: DetailKind) {
        self.detail_state.kind = kind;
        self.detail_state.primary_enabled = matches!(
            kind,
            DetailKind::Film
                | DetailKind::Series
                | DetailKind::Original
                | DetailKind::Episode
                | DetailKind::Supplement
        );
        if !self.detail_state.primary_enabled && self.focus == Focus::DetailAction(0) {
            self.focus = Focus::DetailAction(1);
        }
    }
    pub(crate) fn sync_detail(&mut self, detail: &crate::Detail<'_>) {
        self.set_detail_kind(detail.kind);
        self.set_information_content(detail.description);
        self.detail_state.primary_enabled = detail.primary_playback_target.is_some();
        self.detail_state.selected_tab = detail.selected_playlist.unwrap_or(match self.focus {
            Focus::DetailTab(index) | Focus::Card { row: index, .. } => index,
            _ => self.detail_state.selected_tab,
        });
        self.detail_state.seasons_count =
            detail.seasons.as_ref().map_or(0, |view| view.choices.len());
        self.detail_state.selected_season = detail.seasons.as_ref().map_or(0, |view| {
            view.selected.min(view.choices.len().saturating_sub(1))
        });
        self.detail_state.featured_count = detail
            .featured
            .as_ref()
            .map_or(0, |value| value.cards.len());
        self.detail_state.featured_height = crate::view::featured_height(detail);
        if self.page() != Page::Detail {
            return;
        }
        if let Focus::FeaturedCard(column) = self.focus {
            let focus = if self.detail_state.featured_count == 0 {
                Focus::DetailDescription
            } else {
                Focus::FeaturedCard(column.min(self.detail_state.featured_count - 1))
            };
            if focus != self.focus {
                self.focus = focus;
                self.pointer_press = None;
                self.pointer_layout_focus = None;
                self.scroll_y = if matches!(focus, Focus::FeaturedCard(_)) {
                    632.0
                } else {
                    0.0
                };
            }
        }
        if !self.detail_state.primary_enabled {
            if self.focus == Focus::DetailAction(0) {
                self.focus = Focus::DetailAction(1);
            }
            if self.focus == Focus::InformationPrimary {
                self.focus = Focus::InformationClose;
            }
        }
        if matches!(self.focus, Focus::DetailSeason(index) if index >= self.detail_state.seasons_count)
        {
            self.focus = Focus::DetailTab(self.detail_state.selected_tab);
        }
        if let Focus::Card { row, column } = self.focus
            && row == self.detail_state.selected_tab
            && let Some(season) = detail
                .seasons
                .as_ref()
                .and_then(|view| view.choices.get(self.detail_state.selected_season))
        {
            let focus = if season.episode_count == 0 {
                Focus::DetailSeason(self.detail_state.selected_season)
            } else {
                Focus::Card {
                    row,
                    column: column.min(season.episode_count - 1),
                }
            };
            if focus != self.focus {
                self.focus = focus;
                self.pointer_press = None;
                self.pointer_layout_focus = None;
            }
        }
    }
    pub(crate) fn handle_detail(&mut self, action: Action, rows: &[usize]) -> Option<Vec<Intent>> {
        if self.page() != Page::Detail {
            return None;
        }
        if self.detail_state.information {
            match action {
                Action::Back => {
                    self.detail_state.information = false;
                    self.focus = self.detail_state.return_focus;
                }
                Action::Select if self.focus == Focus::InformationClose => {
                    self.detail_state.information = false;
                    self.focus = self.detail_state.return_focus;
                }
                Action::Select
                    if self.focus == Focus::InformationPrimary
                        && self.detail_state.primary_enabled =>
                {
                    return Some(vec![Intent::Play]);
                }
                Action::Up => {
                    self.focus = Focus::InformationClose;
                    self.detail_state.information_page =
                        self.detail_state.information_page.saturating_sub(1);
                }
                Action::Down => {
                    self.focus = if !self.detail_state.primary_enabled {
                        Focus::InformationClose
                    } else {
                        Focus::InformationPrimary
                    };
                    self.detail_state.information_page = (self.detail_state.information_page + 1)
                        .min(self.detail_state.information_pages - 1);
                }
                _ => (),
            }
            return Some(Vec::new());
        }
        match self.focus {
            Focus::DetailAction(index) => match action {
                Action::Right if index < 2 => self.focus = Focus::DetailAction(index + 1),
                Action::Left
                    if index
                        > if !self.detail_state.primary_enabled {
                            1
                        } else {
                            0
                        } =>
                {
                    self.focus = Focus::DetailAction(index - 1)
                }
                Action::Down => self.focus = Focus::DetailDescription,
                Action::Select => match index {
                    0 => return Some(vec![Intent::Play]),
                    1 => self.open_information(),
                    2 => return Some(vec![Intent::ToggleList]),
                    _ => (),
                },
                _ => return None,
            },
            Focus::DetailDescription => match action {
                Action::Up => {
                    self.focus = Focus::DetailAction(if !self.detail_state.primary_enabled {
                        1
                    } else {
                        0
                    })
                }
                Action::Down if self.detail_state.featured_count > 0 => {
                    self.focus = Focus::FeaturedCard(0);
                    self.scroll_y = 632.0;
                }
                Action::Down if !rows.is_empty() => {
                    self.focus = Focus::DetailTab(self.detail_state.selected_tab)
                }
                Action::Select => self.open_information(),
                _ => return None,
            },
            Focus::DetailTab(index) => match action {
                Action::Up => {
                    self.focus = if self.detail_state.featured_count > 0 {
                        Focus::FeaturedCard(0)
                    } else {
                        Focus::DetailDescription
                    };
                    self.scroll_y = if self.detail_state.featured_count > 0 {
                        632.0
                    } else {
                        0.0
                    };
                }
                Action::Right if index + 1 < rows.len() => {
                    self.focus = Focus::DetailTab(index + 1);
                    return Some(vec![Intent::SelectPlaylist(index + 1)]);
                }
                Action::Left if index > 0 => {
                    self.focus = Focus::DetailTab(index - 1);
                    return Some(vec![Intent::SelectPlaylist(index - 1)]);
                }
                Action::Down if self.detail_state.seasons_count > 0 => {
                    self.focus = Focus::DetailSeason(self.detail_state.selected_season);
                    self.scroll_y = 632.0 + self.detail_state.featured_height;
                    return Some(vec![Intent::SelectSeason(
                        self.detail_state.selected_season,
                    )]);
                }
                Action::Down if rows.get(index).is_some_and(|count| *count > 0) => {
                    self.focus = Focus::Card {
                        row: index,
                        column: 0,
                    };
                    self.scroll_y = 632.0 + self.detail_state.featured_height;
                }
                _ => return None,
            },
            Focus::DetailSeason(index) => match action {
                Action::Up => {
                    self.focus = Focus::DetailTab(self.detail_state.selected_tab);
                    self.scroll_y = self.detail_state.featured_height;
                }
                Action::Left if index > 0 => {
                    self.focus = Focus::DetailSeason(index - 1);
                    return Some(vec![Intent::SelectSeason(index - 1)]);
                }
                Action::Right if index + 1 < self.detail_state.seasons_count => {
                    self.focus = Focus::DetailSeason(index + 1);
                    return Some(vec![Intent::SelectSeason(index + 1)]);
                }
                Action::Select => return Some(vec![Intent::SelectSeason(index)]),
                Action::Down
                    if rows
                        .get(self.detail_state.selected_tab)
                        .is_some_and(|count| *count > 0) =>
                {
                    self.focus = Focus::Card {
                        row: self.detail_state.selected_tab,
                        column: 0,
                    };
                }
                _ => return None,
            },
            Focus::Card { row, column }
                if action == Action::Right || (action == Action::Left && column > 0) =>
            {
                let next = match action {
                    Action::Right if column + 1 < rows.get(row).copied().unwrap_or(0) => column + 1,
                    Action::Left => column - 1,
                    _ => column,
                };
                self.focus = Focus::Card { row, column: next };
                self.scroll_y = 632.0 + self.detail_state.featured_height;
            }
            Focus::Card { .. } if action == Action::Down => (),
            Focus::Card { .. } if action == Action::Up && self.detail_state.seasons_count > 0 => {
                self.focus = Focus::DetailSeason(self.detail_state.selected_season);
                return Some(vec![Intent::SelectSeason(
                    self.detail_state.selected_season,
                )]);
            }
            Focus::Card { row, .. } if action == Action::Up => {
                self.focus = Focus::DetailTab(row);
                self.scroll_y = self.detail_state.featured_height;
            }
            Focus::FeaturedCard(column) => match action {
                Action::Up => {
                    self.focus = Focus::DetailDescription;
                    self.scroll_y = 0.0;
                }
                Action::Down if !rows.is_empty() => {
                    self.focus = Focus::DetailTab(self.detail_state.selected_tab)
                }
                Action::Right if column + 1 < self.detail_state.featured_count => {
                    self.focus = Focus::FeaturedCard(column + 1)
                }
                Action::Left if column > 0 => self.focus = Focus::FeaturedCard(column - 1),
                Action::Select => {
                    self.push_history();
                    return Some(vec![Intent::OpenCard {
                        page: Page::Detail,
                        focus: self.focus,
                    }]);
                }
                Action::Down | Action::Right => (),
                _ => return None,
            },
            _ => return None,
        }
        Some(Vec::new())
    }
    fn open_information(&mut self) {
        self.detail_state.return_focus = self.focus;
        self.detail_state.information = true;
        self.detail_state.information_page = 0;
        self.focus = if !self.detail_state.primary_enabled {
            Focus::InformationClose
        } else {
            Focus::InformationPrimary
        };
    }
}
