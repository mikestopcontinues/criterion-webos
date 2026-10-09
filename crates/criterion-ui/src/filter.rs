use crate::{Action, AppUi, Focus, Intent};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterSelection {
    pub sort_index: usize,
    pub descending: bool,
    pub options: Vec<(usize, usize)>,
}
#[derive(Default)]
pub(crate) struct FilterState {
    pub open: bool,
    pub group: usize,
    pub draft: FilterSelection,
    pub committed: FilterSelection,
    pub counts: [usize; 4],
}
impl AppUi {
    pub fn set_filter_option_counts(&mut self, counts: [usize; 4]) {
        self.filters.counts = counts.map(|count| count.min(2048));
    }
    pub(crate) fn handle_filter(&mut self, action: Action) -> Option<Vec<Intent>> {
        if !self.filters.open {
            if self.focus == Focus::FilterButton && action == Action::Select {
                self.filters.open = true;
                self.filters.draft = self.filters.committed.clone();
                self.filters.group = 0;
                self.focus = Focus::FilterGroup(0);
                return Some(Vec::new());
            }
            return None;
        }
        if action == Action::Back {
            self.filters.open = false;
            self.focus = Focus::Card { row: 0, column: 0 };
            return Some(Vec::new());
        }
        match self.focus {
            Focus::FilterGroup(group) => match action {
                Action::Down => {
                    self.focus = if group < 4 {
                        self.filters.group = group + 1;
                        Focus::FilterGroup(group + 1)
                    } else {
                        Focus::FilterApply
                    };
                }
                Action::Up => {
                    self.focus = if group > 0 {
                        self.filters.group = group - 1;
                        Focus::FilterGroup(group - 1)
                    } else {
                        Focus::FilterClose
                    };
                }
                Action::Right | Action::Select
                    if group == 0 || self.filters.counts[group - 1] > 0 =>
                {
                    self.focus = Focus::FilterOption(if group == 0 {
                        self.filters.draft.sort_index
                    } else {
                        0
                    });
                }
                _ => (),
            },
            Focus::FilterOption(option) => match action {
                Action::Left if self.filters.group > 0 && option % 2 == 1 => {
                    self.focus = Focus::FilterOption(option - 1)
                }
                Action::Left => self.focus = Focus::FilterGroup(self.filters.group),
                Action::Right
                    if self.filters.group > 0
                        && option % 2 == 0
                        && option + 1 < self.filters.counts[self.filters.group - 1] =>
                {
                    self.focus = Focus::FilterOption(option + 1)
                }
                Action::Up if self.filters.group > 0 && option >= 2 => {
                    self.focus = Focus::FilterOption(option - 2)
                }
                Action::Up if self.filters.group == 0 && option > 0 => {
                    self.focus = Focus::FilterOption(option - 1)
                }
                Action::Down
                    if option + if self.filters.group == 0 { 1 } else { 2 }
                        < if self.filters.group == 0 {
                            5
                        } else {
                            self.filters.counts[self.filters.group - 1]
                        } =>
                {
                    self.focus =
                        Focus::FilterOption(option + if self.filters.group == 0 { 1 } else { 2 })
                }
                Action::Down => self.focus = Focus::FilterApply,
                Action::Select => {
                    if self.filters.group == 0 {
                        if self.filters.draft.sort_index == option {
                            self.filters.draft.descending = !self.filters.draft.descending;
                        } else {
                            self.filters.draft.sort_index = option;
                            self.filters.draft.descending = false;
                        }
                    } else {
                        let selected = (self.filters.group - 1, option);
                        if let Some(index) = self
                            .filters
                            .draft
                            .options
                            .iter()
                            .position(|value| *value == selected)
                        {
                            self.filters.draft.options.remove(index);
                        } else if self.filters.draft.options.len() < 64 {
                            self.filters.draft.options.push(selected);
                        }
                    }
                }
                _ => (),
            },
            Focus::FilterApply => match action {
                Action::Up => self.focus = Focus::FilterGroup(4),
                Action::Left => self.focus = Focus::FilterReset,
                Action::Right => self.focus = Focus::FilterClose,
                Action::Select => {
                    self.filters.committed = self.filters.draft.clone();
                    self.filters.open = false;
                    self.focus = Focus::Card { row: 0, column: 0 };
                    return Some(vec![Intent::ApplyFilters(self.filters.committed.clone())]);
                }
                _ => (),
            },
            Focus::FilterReset => match action {
                Action::Right => self.focus = Focus::FilterApply,
                Action::Up => self.focus = Focus::FilterGroup(4),
                Action::Select => self.filters.draft = FilterSelection::default(),
                _ => (),
            },
            Focus::FilterClose => match action {
                Action::Down | Action::Left => self.focus = Focus::FilterGroup(0),
                Action::Select => {
                    self.filters.open = false;
                    self.focus = Focus::Card { row: 0, column: 0 };
                }
                _ => (),
            },
            _ => (),
        }
        Some(Vec::new())
    }
}
