/// Native grouped requests, independent of the account transport's filter type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MyListGroup {
    #[default]
    All,
    FilmsAndSeries,
    Collections,
    OriginalsAndFranchises,
    Supplements,
    Categories,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MyListChoice {
    pub group: MyListGroup,
    /// None omits an unavailable or unrepresentable upstream count.
    pub count: Option<u64>,
}

#[derive(Clone, Copy)]
pub struct MyListView<'a> {
    pub selected: MyListGroup,
    /// Admitted choices in native order; no local filtering of fetched records.
    pub choices: &'a [MyListChoice],
}

#[derive(Clone, Copy, Default)]
pub(crate) struct MyListState {
    active: bool,
    selected: MyListGroup,
    pending: Option<MyListGroup>,
    anchors: [Option<(usize, f32)>; 6],
}
impl MyListState {
    pub(crate) fn dirty(&mut self) {
        self.active = false;
        self.pending = None;
        self.anchors = [None; 6];
    }
    pub(crate) fn selected(&self) -> MyListGroup {
        self.selected
    }
}

// Project wording/layout: native grouped filter identities are admitted, but
// exact localized control labels and geometry have not been measured.
impl MyListGroup {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::FilmsAndSeries => "Films & Series",
            Self::Collections => "Collections",
            Self::OriginalsAndFranchises => "Originals & Franchises",
            Self::Supplements => "Supplements",
            Self::Categories => "Categories",
        }
    }
    fn width(self) -> f32 {
        match self {
            Self::All => 110.0,
            Self::FilmsAndSeries => 280.0,
            Self::Collections => 240.0,
            Self::OriginalsAndFranchises => 390.0,
            Self::Supplements => 280.0,
            Self::Categories => 260.0,
        }
    }
}

pub(crate) fn group_rects(
    view: MyListView<'_>,
) -> impl Iterator<Item = (MyListChoice, egui::Rect)> {
    view.choices
        .iter()
        .take(6)
        .copied()
        .scan(150.0_f32, |x, choice| {
            let width = choice.group.width().min((1920.0 - *x).max(0.0));
            let rect = egui::Rect::from_min_size(egui::pos2(*x, 134.0), egui::vec2(width, 72.0));
            *x += width + 18.0;
            Some((choice, rect))
        })
}

impl crate::AppUi {
    pub(crate) fn my_list_waiting(&self) -> bool {
        self.page() == crate::Page::MyList && self.my_list.pending.is_some()
    }
    pub(crate) fn remember_my_list_focus(&mut self) {
        if self.page() == crate::Page::MyList {
            let focus = if matches!(self.focus, crate::Focus::Rail(_)) {
                self.return_focus
            } else {
                self.focus
            };
            if let crate::Focus::Card { row, column } = focus {
                self.my_list.anchors[self.my_list.selected as usize] =
                    Some((row * 4 + column, self.scroll_y));
            }
        }
    }
    pub(crate) fn sync_my_list(&mut self, data: &crate::ViewData<'_>) {
        if !matches!(data.login, crate::LoginView::SignedIn) {
            return;
        }
        if self.page() != crate::Page::MyList {
            self.my_list.active = false;
            return;
        }
        let Some(view) = data.my_list else {
            return;
        };
        if !self.my_list.active {
            self.my_list.active = true;
            self.my_list.selected = view.selected;
            if let Some((index, scroll)) = self.my_list_anchor() {
                if data.catalog.is_some_and(|window| {
                    index >= window.first && index < window.first + data.cards.len()
                }) {
                    self.focus = crate::Focus::Card {
                        row: index / 4,
                        column: index % 4,
                    };
                    self.scroll_y = scroll;
                } else {
                    self.focus = crate::Focus::MyListGroup(view.selected);
                    self.scroll_y = 0.0;
                    self.catalog_pending = Some((self.focus, index));
                }
            }
        }
        if view.selected != self.my_list.selected {
            self.remember_my_list_focus();
            self.my_list.selected = view.selected;
            self.catalog_pending = None;
            self.pointer_press = None;
            self.pointer_layout_focus = None;
            let header = crate::Focus::MyListGroup(view.selected);
            if matches!(
                self.focus,
                crate::Focus::Card { .. } | crate::Focus::CatalogRetry
            ) {
                self.focus = header;
                self.scroll_y = 0.0;
            } else if matches!(self.focus, crate::Focus::Rail(_)) {
                self.return_focus = header;
                self.scroll_y = 0.0;
            }
        }
        if self.my_list.pending == Some(view.selected) {
            self.my_list.pending = None;
        }
        let visible = |group| {
            view.choices
                .iter()
                .take(6)
                .any(|choice| choice.group == group)
        };
        if self.my_list.pending.is_some_and(|group| !visible(group)) {
            self.my_list.pending = None;
        }
        let selected = view
            .choices
            .iter()
            .take(6)
            .find(|choice| choice.group == view.selected)
            .or_else(|| view.choices.first())
            .map(|choice| crate::Focus::MyListGroup(choice.group));
        if data.cards.is_empty()
            && matches!(self.focus, crate::Focus::Card { .. })
            && let Some(selected) = selected
        {
            self.focus = selected;
            self.scroll_y = 0.0;
        }
        if let crate::Focus::MyListGroup(group) = self.focus
            && !visible(group)
            && let Some(selected) = selected
        {
            self.focus = selected;
            self.pointer_press = None;
            self.pointer_layout_focus = None;
        }
        self.remember_my_list_focus();
    }
    pub(crate) fn my_list_anchor(&self) -> Option<(usize, f32)> {
        (self.page() == crate::Page::MyList)
            .then(|| self.my_list.anchors[self.my_list.selected as usize])
            .flatten()
    }
    pub(crate) fn my_list_snapshot(&self) -> MyListState {
        MyListState {
            pending: None,
            ..self.my_list
        }
    }
    pub(crate) fn paint_my_list(&self, p: &egui::Painter, view: MyListView<'_>) {
        use crate::view::{GOLD, MUTED, WHITE, label};
        for (choice, rect) in group_rects(view) {
            let focused = self.focus == crate::Focus::MyListGroup(choice.group);
            p.rect_filled(
                rect,
                8,
                if focused {
                    GOLD
                } else {
                    egui::Color32::from_gray(11)
                },
            );
            if choice.group == view.selected {
                p.line_segment(
                    [
                        rect.left_bottom() + egui::vec2(12.0, -4.0),
                        rect.right_bottom() + egui::vec2(-12.0, -4.0),
                    ],
                    egui::Stroke::new(3.0, GOLD),
                );
            }
            let candidate = choice.count.map_or_else(
                || choice.group.label().to_owned(),
                |count| format!("{} {count}", choice.group.label()),
            );
            let text = if p
                .layout_no_wrap(candidate.clone(), egui::FontId::proportional(22.0), WHITE)
                .size()
                .x
                <= rect.width() - 24.0
            {
                candidate
            } else {
                choice.group.label().to_owned()
            };
            label(
                p,
                [rect.left() + 12.0, rect.top() + 22.0],
                &text,
                22.0,
                if choice.group == view.selected || focused {
                    WHITE
                } else {
                    MUTED
                },
                rect.width() - 24.0,
            );
        }
    }
    pub(crate) fn handle_my_list(
        &mut self,
        action: crate::Action,
        data: &crate::ViewData<'_>,
    ) -> Option<Vec<crate::Command>> {
        use crate::{Action, Focus, Page};
        if self.page() != Page::MyList {
            return None;
        }
        let view = data.my_list?;
        let choices = &view.choices[..view.choices.len().min(6)];
        let selected = choices
            .iter()
            .find(|choice| choice.group == view.selected)
            .or_else(|| choices.first())?
            .group;
        if action == Action::Up && matches!(self.focus, Focus::Card { row: 0, .. }) {
            self.focus = Focus::MyListGroup(selected);
            self.scroll_y = 0.0;
            return Some(vec![]);
        }
        let Focus::MyListGroup(group) = self.focus else {
            return None;
        };
        let index = choices.iter().position(|choice| choice.group == group);
        let Some(index) = index else {
            self.focus = Focus::MyListGroup(selected);
            return Some(vec![]);
        };
        match action {
            Action::Right if index + 1 < choices.len() => {
                self.focus = Focus::MyListGroup(choices[index + 1].group);
            }
            Action::Left if index > 0 => {
                self.focus = Focus::MyListGroup(choices[index - 1].group);
            }
            Action::Left => {
                self.return_focus = self.focus;
                self.focus = Focus::Rail(crate::RailItem::MyList);
            }
            Action::Select
                if (group != view.selected || self.my_list.pending.is_some())
                    && self.my_list.pending != Some(group) =>
            {
                self.catalog_pending = None;
                self.my_list.pending = Some(group);
                return Some(vec![crate::Command::MyListGroup(group)]);
            }
            Action::Down if self.my_list_waiting() => (),
            Action::Back | Action::Down => return None,
            _ => (),
        }
        Some(vec![])
    }
}
