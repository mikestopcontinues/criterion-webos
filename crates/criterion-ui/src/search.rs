use crate::{Action, AppUi, Focus, Intent, Page};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchGroup {
    #[default]
    All,
    Films,
    Collections,
    Supplements,
}
impl SearchGroup {
    pub(crate) const ALL: [Self; 4] =
        [Self::All, Self::Films, Self::Collections, Self::Supplements];
    pub(crate) fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|value| *value == self)
            .unwrap_or(0)
    }
}
#[derive(Default)]
pub(crate) struct SearchState {
    pub query: String,
    pub group: SearchGroup,
    pub composition: String,
    pub select_all: bool,
}
impl AppUi {
    pub fn query(&self) -> &str {
        &self.search.query
    }
    pub fn search_group(&self) -> SearchGroup {
        self.search.group
    }
    pub fn wants_text_input(&self) -> bool {
        self.page() == Page::Search
            && matches!(self.focus(), Focus::SearchField | Focus::SearchKey(_))
    }
    pub(crate) fn search_intent(&self) -> Intent {
        Intent::Search {
            query: self.search.query.clone(),
            group: self.search.group,
        }
    }
    pub(crate) fn append_query(&mut self, text: &str) -> bool {
        let before = self.search.query.clone();
        for character in text
            .chars()
            .filter(|c| !c.is_control())
            .flat_map(char::to_lowercase)
        {
            if self.search.query.len() + character.len_utf8() > 256 {
                break;
            }
            self.search.query.push(character);
        }
        before != self.search.query
    }
    pub(crate) fn handle_search(&mut self, action: Action, rows: &[usize]) -> Option<Vec<Intent>> {
        if self.page() != Page::Search {
            return None;
        }
        match self.focus() {
            Focus::SearchKey(index) => match action {
                Action::Select => {
                    let changed = match index {
                        0..=35 => self.append_query(
                            &"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
                                .chars()
                                .nth(index)
                                .unwrap_or(' ')
                                .to_string(),
                        ),
                        36 => self.append_query(" "),
                        37 => self.search.query.pop().is_some(),
                        _ => false,
                    };
                    return Some(if changed {
                        vec![self.search_intent()]
                    } else {
                        vec![]
                    });
                }
                Action::Left if index < 36 && index % 6 > 0 => {
                    self.focus = Focus::SearchKey(index - 1)
                }
                Action::Left if index == 37 => self.focus = Focus::SearchKey(36),
                Action::Left => {
                    self.return_focus = self.focus;
                    self.focus = Focus::Rail(crate::RailItem::Search);
                }
                Action::Right if index < 36 && index % 6 < 5 => {
                    self.focus = Focus::SearchKey(index + 1)
                }
                Action::Right if index == 36 => self.focus = Focus::SearchKey(37),
                Action::Right => self.focus = Focus::SearchVoice,
                Action::Up if index >= 36 => {
                    self.focus = Focus::SearchKey(if index == 36 { 30 } else { 33 })
                }
                Action::Up if index >= 6 => self.focus = Focus::SearchKey(index - 6),
                Action::Down if index < 30 => self.focus = Focus::SearchKey(index + 6),
                Action::Down if index < 36 => {
                    self.focus = Focus::SearchKey(if index % 6 < 3 { 36 } else { 37 })
                }
                Action::Back => return None,
                _ => (),
            },
            Focus::SearchVoice => match action {
                Action::Left => self.focus = Focus::SearchKey(5),
                Action::Right => self.focus = Focus::SearchField,
                Action::Down => self.focus = Focus::SearchGroup(0),
                Action::Select => return Some(vec![Intent::VoiceSearch]),
                Action::Back => return None,
                _ => (),
            },
            Focus::SearchField => match action {
                Action::Left => self.focus = Focus::SearchVoice,
                Action::Down => self.focus = Focus::SearchGroup(self.search.group.index()),
                Action::Back => return None,
                _ => (),
            },
            Focus::SearchGroup(index) => match action {
                Action::Left if index > 0 => self.focus = Focus::SearchGroup(index - 1),
                Action::Left => self.focus = Focus::SearchKey(0),
                Action::Right if index < 3 => self.focus = Focus::SearchGroup(index + 1),
                Action::Up => self.focus = Focus::SearchField,
                Action::Down if rows.first().is_some_and(|count| *count > 0) => {
                    self.focus = Focus::Card { row: 0, column: 0 };
                    self.scroll_y = 0.0;
                }
                Action::Select => {
                    self.search.group = SearchGroup::ALL[index.min(3)];
                    self.scroll_y = 0.0;
                    return Some(vec![self.search_intent()]);
                }
                Action::Back => return None,
                _ => (),
            },
            Focus::Card { row: 0, .. } if action == Action::Up => {
                self.focus = Focus::SearchGroup(self.search.group.index());
                self.scroll_y = 0.0;
            }
            Focus::Card { column: 0, .. } if action == Action::Left => {
                self.focus = Focus::SearchKey(0)
            }
            _ => return None,
        }
        Some(vec![])
    }
}

pub(crate) fn group_rects() -> [egui::Rect; 4] {
    [
        (565.0, 156.0),
        (759.0, 180.0),
        (971.0, 265.0),
        (1269.0, 280.0),
    ]
    .map(|(x, width)| egui::Rect::from_min_size(egui::pos2(x, 254.0), egui::vec2(width, 60.0)))
}

impl AppUi {
    pub(crate) fn paint_search(
        &mut self,
        p: &egui::Painter,
        data: &crate::ViewData<'_>,
        visible: &mut Vec<crate::CardLayout>,
    ) {
        use crate::icons::{Icon, centered};
        use crate::view::{GOLD, MUTED, WHITE, icon_button, label, paint_card};
        use egui::{Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};
        for (index, key) in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(
                    150.0 + (index % 6) as f32 * 64.0,
                    114.0 + (index / 6) as f32 * 77.0,
                ),
                Vec2::new(59.0, 72.0),
            );
            p.rect_filled(
                rect,
                8,
                if self.focus() == Focus::SearchKey(index) {
                    GOLD
                } else {
                    Color32::BLACK
                },
            );
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                key,
                FontId::proportional(28.0),
                WHITE,
            );
        }
        for (index, x, icon) in [(36, 150.0, Icon::Space), (37, 342.0, Icon::Backspace)] {
            let rect = Rect::from_min_size(Pos2::new(x, 576.0), Vec2::new(187.0, 60.0));
            p.rect_filled(
                rect,
                8,
                if self.focus() == Focus::SearchKey(index) {
                    GOLD
                } else {
                    Color32::BLACK
                },
            );
            icon.paint(p, centered(rect.center(), 32.0), WHITE);
        }
        icon_button(
            p,
            Rect::from_min_size(Pos2::new(565.0, 115.0), Vec2::splat(100.0)),
            Icon::Microphone,
            self.focus() == Focus::SearchVoice,
        );
        let field = Rect::from_min_size(Pos2::new(691.0, 115.0), Vec2::new(1078.0, 100.0));
        p.rect_filled(field, 12, Color32::from_gray(11));
        p.rect_stroke(
            field,
            12,
            Stroke::new(
                2.0,
                if self.focus() == Focus::SearchField {
                    GOLD
                } else {
                    Color32::from_gray(33)
                },
            ),
            StrokeKind::Inside,
        );
        Icon::Search.paint(p, centered(Pos2::new(749.0, 165.0), 36.0), WHITE);
        label(
            p,
            [783.0, 147.0],
            &format!("{}{}", self.query(), self.search.composition),
            36.0,
            WHITE,
            940.0,
        );
        for (index, (rect, title)) in group_rects()
            .into_iter()
            .zip(["ALL", "FILMS", "COLLECTIONS", "SUPPLEMENTS"])
            .enumerate()
        {
            if self.search.group.index() == index || self.focus() == Focus::SearchGroup(index) {
                p.rect_filled(
                    rect,
                    40,
                    if self.focus() == Focus::SearchGroup(index) {
                        GOLD
                    } else {
                        Color32::from_gray(33)
                    },
                );
            }
            label(
                p,
                [rect.left() + 32.0, rect.top() + 17.0],
                &format!("{title}  {}", data.search_counts[index]),
                26.0,
                if self.search.group.index() == index {
                    WHITE
                } else {
                    MUTED
                },
                rect.width() - 40.0,
            );
        }
        if self.query().trim().is_empty() {
            return;
        }
        let content = p.with_clip_rect(Rect::from_min_max(
            Pos2::new(557.0, 348.0),
            Pos2::new(1920.0, 1080.0),
        ));
        let first = (self.scroll_y() / 321.0).floor().max(0.0) as usize;
        for (index, card) in data.cards.iter().enumerate().skip(first * 3).take(12) {
            let row = index / 3;
            let column = index % 3;
            let y = 360.0 + row as f32 * 321.0 - self.scroll_y();
            if y + 285.0 < 348.0 || y >= 1080.0 || visible.len() >= crate::MAX_VISIBLE_CARDS {
                continue;
            }
            let rect = Rect::from_min_size(
                Pos2::new(565.0 + column as f32 * 414.0, y),
                Vec2::new(378.0, 213.0),
            );
            paint_card(
                &content,
                card,
                rect,
                self.focus() == Focus::Card { row, column },
                card.artwork_key.and_then(|key| self.image(key)),
            );
            visible.push(crate::CardLayout {
                focus: Focus::Card { row, column },
                key: card.key.clone(),
                image: rect,
            });
        }
    }
}
