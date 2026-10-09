use crate::{Action, AppUi, Focus, Intent, Page};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailKind {
    #[default]
    Film,
    Collection,
    Supplement,
}
#[derive(Clone, Copy)]
pub(crate) struct DetailState {
    pub kind: DetailKind,
    pub information: bool,
    pub return_focus: Focus,
    pub information_page: usize,
    pub information_pages: usize,
}
impl Default for DetailState {
    fn default() -> Self {
        Self {
            kind: DetailKind::Film,
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
        if kind == DetailKind::Collection && self.focus == Focus::DetailAction(0) {
            self.focus = Focus::DetailAction(1);
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
                        && self.detail_state.kind != DetailKind::Collection =>
                {
                    return Some(vec![Intent::Play]);
                }
                Action::Up => {
                    self.focus = Focus::InformationClose;
                    self.detail_state.information_page =
                        self.detail_state.information_page.saturating_sub(1);
                }
                Action::Down => {
                    self.focus = if self.detail_state.kind == DetailKind::Collection {
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
                        > if self.detail_state.kind == DetailKind::Collection {
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
                    self.focus =
                        Focus::DetailAction(if self.detail_state.kind == DetailKind::Collection {
                            1
                        } else {
                            0
                        })
                }
                Action::Down => self.focus = Focus::DetailTab(0),
                Action::Select => self.open_information(),
                _ => return None,
            },
            Focus::DetailTab(index) => match action {
                Action::Up => self.focus = Focus::DetailDescription,
                Action::Right if index + 1 < rows.len() => {
                    self.focus = Focus::DetailTab(index + 1);
                    return Some(vec![Intent::SelectPlaylist(index + 1)]);
                }
                Action::Left if index > 0 => {
                    self.focus = Focus::DetailTab(index - 1);
                    return Some(vec![Intent::SelectPlaylist(index - 1)]);
                }
                Action::Down if rows.get(index).is_some_and(|count| *count > 0) => {
                    self.focus = Focus::Card {
                        row: index,
                        column: 0,
                    };
                    self.scroll_y = 632.0;
                }
                _ => return None,
            },
            Focus::Card { row, .. } if action == Action::Up => {
                self.focus = Focus::DetailTab(row);
                self.scroll_y = 0.0;
            }
            _ => return None,
        }
        Some(Vec::new())
    }
    fn open_information(&mut self) {
        self.detail_state.return_focus = self.focus;
        self.detail_state.information = true;
        self.detail_state.information_page = 0;
        self.focus = if self.detail_state.kind == DetailKind::Collection {
            Focus::InformationClose
        } else {
            Focus::InformationPrimary
        };
    }
}
