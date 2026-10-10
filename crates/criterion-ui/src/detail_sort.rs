//! Local native Detail sort choices, independent of remote catalog filters.
use crate::{Action, AppUi, Focus, Intent};
use egui::{Pos2, Rect, Vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailSortField {
    #[default]
    Default,
    Title,
    ReleaseDate,
    Runtime,
}
impl DetailSortField {
    pub const ALL: [Self; 4] = [Self::Default, Self::Title, Self::ReleaseDate, Self::Runtime];
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Title => "Title",
            Self::ReleaseDate => "Release Date",
            Self::Runtime => "Runtime",
        }
    }
    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|field| *field == self)
            .expect("fixed sort choice")
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailSortDirection {
    #[default]
    Ascending,
    Descending,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetailSortSelection {
    pub field: DetailSortField,
    pub direction: DetailSortDirection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetailSortView {
    pub pending: DetailSortSelection,
    pub applied: DetailSortSelection,
    pub visible: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailSortAction {
    Open,
    Choose(DetailSortField),
    Apply,
    Dismiss,
}

impl AppUi {
    pub(crate) fn sync_detail_sort(&mut self, sort: Option<DetailSortView>) {
        let was_visible = self.detail_state.sort.is_some_and(|sort| sort.visible);
        let visible = sort.is_some_and(|sort| sort.visible);
        self.detail_state.sort = sort;
        if visible && !was_visible {
            self.detail_state.information = false;
            self.focus = Focus::DetailSortOption(sort.expect("visible sort").pending.field);
        } else if !visible
            && matches!(
                self.focus,
                Focus::DetailSortOption(_) | Focus::DetailSortApply | Focus::DetailSortClose
            )
        {
            self.focus = Focus::DetailTab(0);
            self.scroll_y = self.detail_state.featured_height;
        }
        if visible != was_visible {
            self.pointer_press = None;
            self.pointer_layout_focus = None;
        }
    }
    pub(crate) fn detail_tab_intent(&self, index: usize) -> Intent {
        if index == 0 && self.detail_state.selected_tab == 0 && self.detail_state.sort.is_some() {
            Intent::DetailSort(DetailSortAction::Open)
        } else {
            Intent::SelectPlaylist(index)
        }
    }
    pub(crate) fn handle_detail_sort(&mut self, action: Action) -> Option<Vec<Intent>> {
        let sort = self.detail_state.sort.filter(|sort| sort.visible)?;
        if action == Action::Back {
            return Some(vec![Intent::DetailSort(DetailSortAction::Dismiss)]);
        }
        match (self.focus, action) {
            (Focus::DetailSortOption(field), Action::Select) => {
                return Some(vec![Intent::DetailSort(DetailSortAction::Choose(field))]);
            }
            (Focus::DetailSortApply, Action::Select) => {
                return Some(vec![Intent::DetailSort(DetailSortAction::Apply)]);
            }
            (Focus::DetailSortClose, Action::Select) => {
                return Some(vec![Intent::DetailSort(DetailSortAction::Dismiss)]);
            }
            (Focus::DetailSortOption(field), Action::Up) => {
                self.focus = field
                    .index()
                    .checked_sub(1)
                    .map_or(Focus::DetailSortClose, |i| {
                        Focus::DetailSortOption(DetailSortField::ALL[i])
                    })
            }
            (Focus::DetailSortOption(field), Action::Down) => {
                self.focus = DetailSortField::ALL
                    .get(field.index() + 1)
                    .copied()
                    .map_or(Focus::DetailSortApply, Focus::DetailSortOption)
            }
            (Focus::DetailSortOption(_), Action::Right) => self.focus = Focus::DetailSortApply,
            (Focus::DetailSortApply, Action::Up | Action::Left) => {
                self.focus = Focus::DetailSortOption(sort.pending.field)
            }
            (Focus::DetailSortApply, Action::Right) => self.focus = Focus::DetailSortClose,
            (Focus::DetailSortClose, Action::Down | Action::Left) => {
                self.focus = Focus::DetailSortOption(sort.pending.field)
            }
            _ => (),
        }
        Some(Vec::new())
    }
}

pub(crate) fn sort_rect(focus: Focus) -> Option<Rect> {
    let (x, y, width, height) = match focus {
        Focus::DetailSortOption(field) => {
            (510.0, 270.0 + field.index() as f32 * 100.0, 900.0, 80.0)
        }
        Focus::DetailSortApply => (510.0, 750.0, 900.0, 82.0),
        Focus::DetailSortClose => (1360.0, 170.0, 82.0, 82.0),
        _ => return None,
    };
    Some(Rect::from_min_size(
        Pos2::new(x, y),
        Vec2::new(width, height),
    ))
}
