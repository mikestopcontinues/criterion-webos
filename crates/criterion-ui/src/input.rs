use crate::{Action, AppUi, CardLayout, Command, Focus, Page, ViewData};
use egui::{Event, Pos2, Rect, Vec2};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PointerTarget {
    page: Page,
    focus: Focus,
    identity: Option<crate::Target>,
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
}
impl AppUi {
    pub(crate) fn text_events(&mut self, events: &[Event]) -> Vec<Command> {
        if !self.wants_text_input() {
            self.search.composition.clear();
            return vec![];
        }
        let before = self.search.query.clone();
        for event in events {
            match event {
                Event::Text(text)
                | Event::Paste(text)
                | Event::Ime(egui::ImeEvent::Commit(text)) => {
                    if self.search.select_all {
                        self.search.query.clear();
                        self.search.select_all = false;
                    }
                    self.append_query(text);
                    self.search.composition.clear();
                }
                Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    self.search.composition =
                        text.chars().filter(|c| !c.is_control()).take(256).collect()
                }
                Event::Ime(egui::ImeEvent::DeleteSurrounding { before_chars, .. }) => {
                    for _ in 0..(*before_chars).min(256) {
                        self.search.query.pop();
                    }
                }
                Event::Key {
                    key: egui::Key::A,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command || modifiers.ctrl => self.search.select_all = true,
                Event::Key {
                    key: egui::Key::Backspace | egui::Key::Delete,
                    pressed: true,
                    ..
                } => {
                    if self.search.select_all {
                        self.search.query.clear();
                        self.search.select_all = false;
                    } else if matches!(
                        event,
                        Event::Key {
                            key: egui::Key::Backspace,
                            ..
                        }
                    ) {
                        self.search.query.pop();
                    }
                }
                _ => (),
            }
        }
        if before == self.search.query {
            vec![]
        } else {
            vec![Command::Search {
                query: self.search.query.clone(),
                group: self.search.group,
            }]
        }
    }
    pub(crate) fn pointer_events(
        &mut self,
        events: &[Event],
        data: &ViewData<'_>,
        cards: &[CardLayout],
        allow_results: bool,
    ) -> Vec<Command> {
        let mut commands = vec![];
        for event in events {
            match event {
                Event::PointerGone => {
                    self.pointer_press = None;
                    self.pointer_layout_focus = None;
                }
                Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    ..
                } => {
                    let target = self.hit_target(*pos, data, cards).filter(|target| {
                        allow_results || !matches!(target.focus, Focus::Card { .. })
                    });
                    if *pressed {
                        self.pointer_layout_focus = Some(self.focus());
                        if let Some(target) = &target {
                            self.pointer_focus(target.focus);
                        }
                        self.pointer_press = target;
                    } else if let Some(prior) = self.pointer_press.take()
                        && Some(&prior) == target.as_ref()
                    {
                        self.pointer_focus(prior.focus);
                        if let Focus::DetailTab(index) = prior.focus {
                            commands.push(Command::SelectPlaylist(index));
                        } else if prior.focus != Focus::SearchField {
                            commands.extend(self.handle(Action::Select, data));
                        }
                        if self.page() != prior.page {
                            break;
                        }
                    }
                    if !*pressed {
                        self.pointer_layout_focus = None;
                    }
                }
                _ => (),
            }
        }
        if !self.wants_text_input() {
            self.search.composition.clear();
            self.search.select_all = false;
        }
        commands
    }
    fn pointer_focus(&mut self, focus: Focus) {
        if matches!(focus, Focus::Rail(_)) && !matches!(self.focus, Focus::Rail(_)) {
            self.return_focus = self.focus;
        }
        if let Focus::FilterGroup(group) = focus {
            self.filters.group = group;
        }
        self.focus = focus;
    }
    fn hit_target(
        &self,
        pos: Pos2,
        data: &ViewData<'_>,
        cards: &[CardLayout],
    ) -> Option<PointerTarget> {
        if !rect(0.0, 0.0, 1920.0, 1080.0).contains(pos) {
            return None;
        }
        let target = |focus| {
            Some(PointerTarget {
                page: self.page(),
                focus,
                identity: match focus {
                    Focus::Hero => data.hero.as_ref().map(|hero| hero.card.key.clone()),
                    Focus::DetailAction(_)
                    | Focus::InformationPrimary
                    | Focus::DetailTab(_)
                    | Focus::DetailDescription => {
                        data.detail.as_ref().map(|detail| detail.card.key.clone())
                    }
                    _ => None,
                },
            })
        };
        if self.filters.open {
            for (focus, area) in [
                (Focus::FilterClose, rect(1630.0, 139.0, 82.0, 82.0)),
                (Focus::FilterReset, rect(210.0, 868.0, 82.0, 82.0)),
                (Focus::FilterApply, rect(310.0, 868.0, 370.0, 82.0)),
            ] {
                if area.contains(pos) {
                    return target(focus);
                }
            }
            for (group, y) in [(0, 256.0), (1, 444.0), (2, 532.0), (3, 620.0), (4, 708.0)] {
                if rect(210.0, y - 8.0, 468.0, 70.0).contains(pos) {
                    return target(Focus::FilterGroup(group));
                }
            }
            let selected = if let Focus::FilterOption(i) = self.layout_focus() {
                i
            } else {
                0
            };
            let count = if self.filters.group == 0 {
                5
            } else {
                self.filters.counts[self.filters.group - 1]
            };
            let first = if self.filters.group == 0 {
                0
            } else {
                (selected / 2).saturating_sub(3) * 2
            };
            for index in first..(first + 16).min(count) {
                let area = if self.filters.group == 0 {
                    rect(714.0, 255.0 + index as f32 * 96.0, 500.0, 80.0)
                } else {
                    rect(
                        714.0 + (index % 2) as f32 * 506.0,
                        255.0 + ((index - first) / 2) as f32 * 96.0,
                        490.0,
                        80.0,
                    )
                };
                if area.contains(pos) && pos.y < 868.0 {
                    return target(Focus::FilterOption(index));
                }
            }
            return None;
        }
        if self.detail_state.information {
            if rect(1490.0, 108.0, 82.0, 82.0).contains(pos) {
                return target(Focus::InformationClose);
            }
            if self.detail_state.kind != crate::DetailKind::Collection
                && rect(348.0, 903.0, 1224.0, 80.0).contains(pos)
            {
                return target(Focus::InformationPrimary);
            }
            return None;
        }
        if pos.x
            < if matches!(self.focus, Focus::Rail(_)) {
                340.0
            } else {
                130.0
            }
        {
            for (item, y) in [
                (crate::RailItem::Search, 208.0),
                (crate::RailItem::Home, 356.0),
                (crate::RailItem::New, 430.0),
                (crate::RailItem::AllFilms, 504.0),
                (crate::RailItem::Login, 649.0),
            ] {
                if (pos.y - y).abs() < 34.0 {
                    return target(Focus::Rail(item));
                }
            }
            return None;
        }
        if matches!(self.focus, Focus::Rail(_)) {
            return None;
        }
        if self.page() == Page::Search {
            for index in 0..36 {
                if rect(
                    150.0 + (index % 6) as f32 * 64.0,
                    114.0 + (index / 6) as f32 * 77.0,
                    59.0,
                    72.0,
                )
                .contains(pos)
                {
                    return target(Focus::SearchKey(index));
                }
            }
            for (focus, area) in [
                (Focus::SearchKey(36), rect(150.0, 576.0, 187.0, 60.0)),
                (Focus::SearchKey(37), rect(342.0, 576.0, 187.0, 60.0)),
                (Focus::SearchVoice, rect(565.0, 115.0, 100.0, 100.0)),
                (Focus::SearchField, rect(691.0, 115.0, 1078.0, 100.0)),
            ] {
                if area.contains(pos) {
                    return target(focus);
                }
            }
            for (index, area) in crate::search::group_rects().into_iter().enumerate() {
                if area.contains(pos) {
                    return target(Focus::SearchGroup(index));
                }
            }
        }
        if self.page() == Page::AllFilms && rect(410.0, 112.0, 330.0, 82.0).contains(pos) {
            return target(Focus::FilterButton);
        }
        if matches!(self.page(), Page::Home | Page::New | Page::Discovery)
            && data.hero.is_some()
            && rect(150.0, 740.0 - self.scroll_y(), 214.0, 80.0).contains(pos)
        {
            return target(Focus::Hero);
        }
        if self.page() == Page::Detail
            && let Some(detail) = &data.detail
        {
            let y = 620.0 - self.scroll_y();
            let collection = detail.kind == crate::DetailKind::Collection;
            let x = if collection { 150.0 } else { 630.0 };
            if !collection && rect(150.0, y, 460.0, 80.0).contains(pos) {
                return target(Focus::DetailAction(0));
            }
            for (index, x) in [(1, x), (2, x + 102.0)] {
                if rect(x, y, 82.0, 82.0).contains(pos) {
                    return target(Focus::DetailAction(index));
                }
            }
            if rect(150.0, 805.0 - self.scroll_y(), 1250.0, 100.0).contains(pos) {
                return target(Focus::DetailDescription);
            }
            for index in 0..data.rails.len().min(8) {
                if rect(
                    150.0 + index as f32 * 280.0,
                    966.0 - self.scroll_y(),
                    260.0,
                    52.0,
                )
                .contains(pos)
                {
                    return target(Focus::DetailTab(index));
                }
            }
        }
        if self.page() == Page::Search && (pos.y < 348.0 || self.query().trim().is_empty()) {
            return None;
        }
        if matches!(self.page(), Page::AllFilms | Page::MyList) && pos.y < 228.0 {
            return None;
        }
        cards
            .iter()
            .find(|card| {
                Rect::from_min_max(card.image.min, card.image.max + egui::vec2(0.0, 72.0))
                    .contains(pos)
            })
            .map(|card| PointerTarget {
                page: self.page(),
                focus: Focus::Card {
                    row: card.row,
                    column: card.column,
                },
                identity: Some(card.key.clone()),
            })
    }
}
