use crate::icons::{Icon, centered};
use crate::{AppUi, Focus, Page};
use egui::{Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};

pub const LOGICAL_SIZE: [f32; 2] = [1920.0, 1080.0];
pub const MAX_VISIBLE_CARDS: usize = 15;
pub(crate) const GOLD: Color32 = Color32::from_rgb(181, 138, 22);
pub(crate) const WHITE: Color32 = Color32::from_rgb(239, 239, 239);
pub(crate) const MUTED: Color32 = Color32::from_rgb(151, 151, 151);

/// Provider-admitted metadata. Media identity and optional public artwork identity are distinct.
#[derive(Clone, Copy)]
pub struct Card<'a> {
    pub key: &'a crate::Target,
    pub artwork_key: Option<&'a str>,
    pub title: &'a str,
    pub year: &'a str,
    pub duration_seconds: u32,
}
pub struct Rail<'a> {
    pub title: &'a str,
    pub cards: &'a [Card<'a>],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroAction {
    Open,
    Play,
}
pub struct Hero<'a> {
    pub card: Card<'a>,
    pub description: &'a str,
    pub action: &'a str,
    pub action_kind: HeroAction,
    pub background_key: Option<&'a str>,
    pub title_logo_key: Option<&'a str>,
}
pub struct FilterGroup<'a> {
    pub label: &'a str,
    pub options: &'a [&'a str],
}
pub struct FilterMenu<'a> {
    pub groups: &'a [FilterGroup<'a>],
}
pub struct Detail<'a> {
    pub card: Card<'a>,
    pub directors: &'a str,
    pub description: &'a str,
    pub starring: &'a str,
    pub countries: &'a str,
    pub languages: &'a str,
    pub primary_action: &'a str,
    pub kind: crate::DetailKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    Ready,
    Loading,
    Empty,
    Offline,
    Error,
}
/// Global positions in the bounded All Films window. Opaque cursors stay with the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogTail {
    More,
    Loading,
    Error,
    End,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogWindow {
    pub first: usize,
    pub tail: CatalogTail,
}
pub struct ViewData<'a> {
    pub title: &'a str,
    pub hero: Option<Hero<'a>>,
    pub rails: &'a [Rail<'a>],
    pub cards: &'a [Card<'a>],
    pub total: u32,
    pub catalog: Option<CatalogWindow>,
    pub status: LoadState,
    pub filters: Option<FilterMenu<'a>>,
    pub detail: Option<Detail<'a>>,
    pub search_counts: [u32; 4],
    pub login: crate::LoginView<'a>,
}
impl Default for ViewData<'_> {
    fn default() -> Self {
        Self {
            title: "",
            hero: None,
            rails: &[],
            cards: &[],
            total: 0,
            catalog: None,
            status: LoadState::Loading,
            filters: None,
            detail: None,
            search_counts: [0; 4],
            login: crate::LoginView::SignedOut,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct CardLayout {
    pub row: usize,
    pub column: usize,
    pub key: crate::Target,
    pub image: Rect,
}
pub struct UiFrame {
    pub output: egui::FullOutput,
    pub visible_cards: Vec<CardLayout>,
    pub visible_artwork: Vec<String>,
    pub commands: Vec<crate::Command>,
    pub wants_text_input: bool,
}

impl AppUi {
    pub fn render(&mut self, mut input: egui::RawInput, data: &ViewData<'_>) -> UiFrame {
        self.sync_login(data.login);
        self.sync_rail(data.login);
        self.sync_catalog(data);
        if !self.wants_text_input() {
            self.search.composition.clear();
            self.search.select_all = false;
        }
        if let Some(menu) = &data.filters {
            let mut counts = [0; 4];
            for (index, group) in menu.groups.iter().take(4).enumerate() {
                counts[index] = group.options.len();
            }
            self.set_filter_option_counts(counts);
        }
        if let Some(detail) = &data.detail {
            self.set_detail_kind(detail.kind);
            self.set_information_content(detail.description);
        }
        let events = if input.focused {
            input.events.clone()
        } else {
            self.pointer_press = None;
            self.pointer_layout_focus = None;
            self.search.composition.clear();
            self.search.select_all = false;
            Vec::new()
        };
        let mut commands = Vec::new();
        self.flush_images();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0)));
        let context = self.context.clone();
        let mut visible_cards = Vec::new();
        let mut visible_artwork = Vec::new();
        let output = context.run_ui(input, |ui| {
            let p = ui
                .painter()
                .with_clip_rect(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0)));
            p.rect_filled(
                Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0)),
                0,
                Color32::from_rgb(17, 17, 17),
            );
            match self.page() {
                Page::Login => self.paint_login(&p, data.login),
                Page::Search => self.paint_search(&p, data, &mut visible_cards),
                Page::Detail => {
                    if let Some(detail) = &data.detail {
                        if let Some(key) = detail.card.artwork_key {
                            visible_artwork.push(key.to_owned());
                        }
                        self.paint_detail(&p, detail, data.rails, &mut visible_cards);
                    }
                }
                Page::Home | Page::New | Page::Discovery => {
                    if let Some(hero) = &data.hero
                        && self.scroll_y() < 1080.0
                    {
                        let y = -self.scroll_y();
                        if let Some(key) = hero.background_key {
                            visible_artwork.push(key.to_owned());
                        }
                        if let Some(key) = hero.title_logo_key {
                            visible_artwork.push(key.to_owned());
                        }
                        p.rect_filled(
                            Rect::from_min_size(Pos2::new(150.0, y), Vec2::new(1770.0, 900.0)),
                            0,
                            Color32::from_rgb(28, 28, 28),
                        );
                        if let Some(texture) = hero.background_key.and_then(|key| self.image(key)) {
                            p.image(
                                texture,
                                Rect::from_min_size(Pos2::new(0.0, y), Vec2::new(1920.0, 1080.0)),
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                WHITE,
                            );
                            gradient(
                                &p,
                                Rect::from_min_size(Pos2::new(0.0, y), Vec2::new(1400.0, 1080.0)),
                                Color32::BLACK,
                                Color32::TRANSPARENT,
                            );
                        }
                        if let Some((texture, size)) =
                            hero.title_logo_key.and_then(|key| self.image_info(key))
                        {
                            let scale = (600.0 / size[0] as f32).min(240.0 / size[1] as f32);
                            p.image(
                                texture,
                                Rect::from_min_size(
                                    Pos2::new(150.0, y + 300.0),
                                    Vec2::new(size[0] as f32 * scale, size[1] as f32 * scale),
                                ),
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                WHITE,
                            );
                        } else {
                            label(&p, [150.0, y + 300.0], hero.card.title, 72.0, WHITE, 850.0);
                        }
                        paragraph(
                            &p,
                            [150.0, y + 600.0],
                            hero.description,
                            27.0,
                            WHITE,
                            900.0,
                            (4, 360),
                        );
                        button(
                            &p,
                            Rect::from_min_size(
                                Pos2::new(150.0, y + 740.0),
                                Vec2::new(214.0, 80.0),
                            ),
                            hero.action,
                            self.focus() == Focus::Hero,
                        );
                    }
                    for (row, rail) in data.rails.iter().enumerate() {
                        let y = 896.0 + row as f32 * 397.0 - self.scroll_y();
                        if !(-380.0..1080.0).contains(&y) {
                            continue;
                        }
                        label(&p, [150.0, y], rail.title, 34.0, WHITE, 1620.0);
                        let selected = match self.layout_focus() {
                            Focus::Card { row: r, column } if r == row => column,
                            _ => 0,
                        };
                        let width = if self.page() == Page::New {
                            516.0
                        } else {
                            378.0
                        };
                        let first =
                            selected.saturating_sub(if self.page() == Page::New { 2 } else { 3 });
                        for (column, card) in rail
                            .cards
                            .iter()
                            .enumerate()
                            .skip(first)
                            .take(if self.page() == Page::New { 4 } else { 5 })
                        {
                            let x = 150.0 + (column - first) as f32 * (width + 36.0);
                            let rect = Rect::from_min_size(
                                Pos2::new(x, y + 60.0),
                                Vec2::new(width, width * 9.0 / 16.0),
                            );
                            if visible_cards.len() < MAX_VISIBLE_CARDS {
                                paint_card(
                                    &p,
                                    card,
                                    rect,
                                    self.focus() == Focus::Card { row, column },
                                    card.artwork_key.and_then(|key| self.image(key)),
                                );
                                visible_cards.push(CardLayout {
                                    row,
                                    column,
                                    key: card.key.clone(),
                                    image: rect,
                                });
                            }
                        }
                    }
                }
                _ => {
                    label(
                        &p,
                        [150.0, 115.0],
                        if data.title.is_empty() {
                            page_label(self.page())
                        } else {
                            data.title
                        },
                        60.0,
                        WHITE,
                        620.0,
                    );
                    if self.page() == Page::AllFilms {
                        button(
                            &p,
                            Rect::from_min_size(Pos2::new(410.0, 112.0), Vec2::new(330.0, 82.0)),
                            "SORT + FILTER",
                            self.focus() == Focus::FilterButton,
                        );
                        label(
                            &p,
                            [774.0, 138.0],
                            &format!("{} Films", data.total),
                            30.0,
                            WHITE,
                            700.0,
                        );
                    }
                    let columns = if self.page() == Page::Search { 3 } else { 4 };
                    let content = p.with_clip_rect(Rect::from_min_max(
                        Pos2::new(150.0, 228.0),
                        Pos2::new(1920.0, 1080.0),
                    ));
                    let first_row = (self.scroll_y() / 321.0).floor().max(0.0) as usize;
                    for (index, card) in data
                        .cards
                        .iter()
                        .enumerate()
                        .skip(
                            (first_row * columns)
                                .saturating_sub(data.catalog.map_or(0, |w| w.first)),
                        )
                        .take(16)
                    {
                        let global = index + data.catalog.map_or(0, |w| w.first);
                        let row = global / columns;
                        let column = global % columns;
                        let y = 248.0 + row as f32 * 321.0 - self.scroll_y();
                        if y + 285.0 < 0.0
                            || y >= 1080.0
                            || visible_cards.len() >= MAX_VISIBLE_CARDS
                        {
                            continue;
                        }
                        let rect = Rect::from_min_size(
                            Pos2::new(150.0 + column as f32 * 414.0, y),
                            Vec2::new(378.0, 213.0),
                        );
                        paint_card(
                            &content,
                            card,
                            rect,
                            self.focus() == Focus::Card { row, column },
                            card.artwork_key.and_then(|key| self.image(key)),
                        );
                        visible_cards.push(CardLayout {
                            row,
                            column,
                            key: card.key.clone(),
                            image: rect,
                        });
                    }
                }
            }
            if self.page() != Page::Login
                && data.status != LoadState::Ready
                && (self.page() != Page::Search || !self.query().is_empty())
            {
                label(
                    &p,
                    if self.page() == Page::Search {
                        [565.0, 360.0]
                    } else {
                        [150.0, 620.0]
                    },
                    match data.status {
                        LoadState::Loading => "Loading…",
                        LoadState::Empty => "No films found",
                        LoadState::Offline => "Offline — reconnect and try again",
                        LoadState::Error => "Unable to load films — try again",
                        LoadState::Ready => "",
                    },
                    32.0,
                    WHITE,
                    1400.0,
                );
            }
            if self.page() == Page::AllFilms
                && let Some(window) = data.catalog
            {
                let end = window.first + data.cards.len();
                let end_y = 248.0 + end.div_ceil(4) as f32 * 321.0 - self.scroll_y();
                let y = if window.tail == CatalogTail::End {
                    end_y
                } else {
                    end_y.clamp(600.0, 970.0)
                };
                match window.tail {
                    CatalogTail::Loading => {
                        label(&p, [150.0, y], "Loading more films…", 28.0, WHITE, 1200.0)
                    }
                    CatalogTail::Error => button(
                        &p,
                        catalog_retry_rect(),
                        "Unable to load — retry",
                        self.focus() == Focus::CatalogRetry,
                    ),
                    CatalogTail::End if data.cards.is_empty() => (),
                    CatalogTail::End => label(&p, [150.0, y], "End of films", 28.0, MUTED, 1200.0),
                    CatalogTail::More => (),
                }
            }
            paint_rail(&p, self.focus(), self.page(), data.login);
            if self.page() == Page::AllFilms && self.filters.open {
                self.paint_filters(&p, data);
            }
            if self.page() == Page::Detail
                && self.detail_state.information
                && let Some(detail) = &data.detail
            {
                self.paint_information(&p, detail);
            }
        });
        for card in &visible_cards {
            let source = if matches!(
                self.page(),
                Page::Home | Page::New | Page::Discovery | Page::Detail
            ) {
                data.rails
                    .get(card.row)
                    .and_then(|rail| rail.cards.get(card.column))
            } else {
                data.cards.get(
                    (card.row * if self.page() == Page::Search { 3 } else { 4 } + card.column)
                        .saturating_sub(data.catalog.map_or(0, |w| w.first)),
                )
            };
            if let Some(key) = source.and_then(|card| card.artwork_key)
                && !visible_artwork.iter().any(|candidate| candidate == key)
            {
                visible_artwork.push(key.to_owned());
            }
        }
        for event in &events {
            let old_page = self.page();
            let command_start = commands.len();
            let allow_results = !commands
                .iter()
                .any(|command| matches!(command, crate::Command::Search { .. }));
            commands.extend(self.pointer_events(
                std::slice::from_ref(event),
                data,
                &visible_cards,
                allow_results,
            ));
            commands.extend(self.text_events(std::slice::from_ref(event)));
            if commands[command_start..]
                .iter()
                .any(|command| matches!(command, crate::Command::Search { .. }))
            {
                self.pointer_press = None;
                self.pointer_layout_focus = None;
            }
            if self.page() != old_page
                || commands[command_start..]
                    .iter()
                    .any(|command| matches!(command, crate::Command::Open(_)))
            {
                break;
            }
        }
        if let Some(demand) = self.catalog_demand(data)
            && !commands
                .iter()
                .any(|c| matches!(c, crate::Command::Catalog { .. }))
        {
            commands.push(demand);
        }
        UiFrame {
            commands,
            wants_text_input: self.wants_text_input(),
            output,
            visible_cards,
            visible_artwork,
        }
    }
}
fn page_label(page: Page) -> &'static str {
    match page {
        Page::Home => "Home",
        Page::New => "New",
        Page::AllFilms => "All Films",
        Page::Search => "Search",
        Page::Detail => "Film",
        Page::Login => "Log In",
        Page::Discovery => "Explore",
        Page::MyList => "My List",
    }
}
pub(crate) fn label(
    p: &egui::Painter,
    pos: [f32; 2],
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
) {
    paragraph(p, pos, text, size, color, width, (1, 256));
}
pub(crate) fn paragraph(
    p: &egui::Painter,
    pos: [f32; 2],
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
    bounds: (usize, usize),
) {
    let (rows, limit) = bounds;
    if pos[1] > p.clip_rect().bottom() || pos[1] + size * rows as f32 * 1.5 < p.clip_rect().top() {
        return;
    }
    let text: String = text
        .chars()
        .take(limit)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let mut job = egui::text::LayoutJob::simple(text, FontId::proportional(size), color, width);
    job.wrap.max_rows = rows;
    let galley = p.layout_job(job);
    p.galley(Pos2::new(pos[0], pos[1]), galley, color);
}
pub(crate) fn button(p: &egui::Painter, rect: Rect, text: &str, focused: bool) {
    button_background(p, rect, focused);
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text.chars().take(128).collect::<String>(),
        FontId::proportional(26.0),
        WHITE,
    );
}
pub(crate) fn icon_button(p: &egui::Painter, rect: Rect, icon: crate::icons::Icon, focused: bool) {
    button_background(p, rect, focused);
    icon.paint(p, crate::icons::centered(rect.center(), 32.0), WHITE);
}
fn button_background(p: &egui::Painter, rect: Rect, focused: bool) {
    p.rect_filled(
        rect,
        40,
        if focused {
            GOLD
        } else {
            Color32::from_rgb(11, 11, 11)
        },
    );
    p.rect_stroke(
        rect,
        40,
        Stroke::new(2.0, Color32::from_rgb(37, 37, 37)),
        StrokeKind::Inside,
    );
}
fn marked_button(p: &egui::Painter, rect: Rect, text: &str, checked: bool, focused: bool) {
    button_background(p, rect, focused);
    let mut job = egui::text::LayoutJob::simple(
        text.chars().take(128).collect(),
        FontId::proportional(26.0),
        WHITE,
        rect.width() - 84.0,
    );
    job.wrap.max_rows = 1;
    let galley = p.layout_job(job);
    let prefix = if checked { 40.0 } else { 0.0 };
    let left = rect.center().x - (galley.size().x + prefix) * 0.5;
    if checked {
        Icon::Check.paint(
            p,
            centered(Pos2::new(left + 16.0, rect.center().y), 32.0),
            WHITE,
        );
    }
    p.galley(
        Pos2::new(left + prefix, rect.center().y - galley.size().y * 0.5),
        galley,
        WHITE,
    );
}
pub(crate) fn paint_card(
    p: &egui::Painter,
    card: &Card<'_>,
    rect: Rect,
    focused: bool,
    image: Option<egui::TextureId>,
) {
    p.rect_filled(rect, 0, Color32::from_rgb(39, 39, 39));
    if let Some(texture) = image {
        p.image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    if focused {
        p.rect_stroke(
            rect.expand(8.0),
            0,
            Stroke::new(8.0, GOLD),
            StrokeKind::Inside,
        );
    }
    let title = if card.title.chars().count() > 28 {
        format!("{}…", card.title.chars().take(27).collect::<String>())
    } else {
        card.title.to_owned()
    };
    label(
        p,
        [rect.left(), rect.bottom() + 8.0],
        &title,
        28.0,
        if focused {
            WHITE
        } else {
            Color32::from_rgb(192, 192, 192)
        },
        346.0,
    );
    Icon::More.paint(
        p,
        centered(Pos2::new(rect.right() - 14.0, rect.bottom() + 26.0), 24.0),
        MUTED,
    );
    if card.key.media_id().is_none() {
        return;
    }
    label(
        p,
        [rect.left(), rect.bottom() + 46.0],
        card.year,
        22.0,
        MUTED,
        120.0,
    );
    if card.duration_seconds == 0 {
        return;
    }
    let duration = if card.duration_seconds >= 3600 {
        format!(
            "{} h {} min",
            card.duration_seconds / 3600,
            (card.duration_seconds % 3600) / 60
        )
    } else {
        format!("{} min", card.duration_seconds / 60)
    };
    p.text(
        Pos2::new(rect.right(), rect.bottom() + 46.0),
        egui::Align2::RIGHT_TOP,
        duration,
        FontId::proportional(22.0),
        MUTED,
    );
}
fn paint_rail(p: &egui::Painter, focus: Focus, page: Page, login: crate::LoginView<'_>) {
    let expanded = matches!(focus, Focus::Rail(_));
    if expanded {
        p.rect_filled(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0)),
            0,
            Color32::from_black_alpha(190),
        );
    }
    p.rect_filled(
        Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(if expanded { 340.0 } else { 130.0 }, 1080.0),
        ),
        0,
        Color32::BLACK,
    );
    p.text(
        Pos2::new(75.0, 92.0),
        egui::Align2::CENTER_CENTER,
        "CU",
        FontId::proportional(46.0),
        WHITE,
    );
    if expanded {
        label(
            p,
            [126.0, 62.0],
            "CRITERION\nUNOFFICIAL",
            23.0,
            WHITE,
            175.0,
        );
    }
    for entry in crate::rail::entries(login) {
        let y = entry.y(login);
        let color = if focus == Focus::Rail(entry.item) || (!expanded && page == entry.page) {
            GOLD
        } else {
            MUTED
        };
        entry
            .icon
            .paint(p, centered(Pos2::new(75.0, y), 32.0), color);
        if expanded {
            label(p, [126.0, y - 15.0], entry.label(login), 26.0, color, 200.0);
            if focus == Focus::Rail(entry.item) {
                p.line_segment(
                    [Pos2::new(108.0, y - 15.0), Pos2::new(108.0, y + 15.0)],
                    Stroke::new(2.0, GOLD),
                );
            }
        }
    }
    for y in [
        282.0,
        if matches!(login, crate::LoginView::SignedIn) {
            653.0
        } else {
            579.0
        },
    ] {
        p.line_segment(
            [
                Pos2::new(58.0, y),
                Pos2::new(if expanded { 282.0 } else { 92.0 }, y),
            ],
            Stroke::new(1.0, Color32::from_gray(45)),
        );
    }
}
fn gradient(p: &egui::Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = egui::Mesh::default();
    for (pos, color) in [
        (rect.left_top(), left),
        (rect.right_top(), right),
        (rect.right_bottom(), right),
        (rect.left_bottom(), left),
    ] {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color,
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    p.add(egui::Shape::mesh(mesh));
}
impl AppUi {
    fn paint_filters(&self, p: &egui::Painter, data: &ViewData<'_>) {
        p.rect_filled(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0)),
            0,
            Color32::from_black_alpha(155),
        );
        let rect = Rect::from_min_size(Pos2::new(150.0, 80.0), Vec2::new(1620.0, 920.0));
        p.rect_filled(rect, 36, Color32::from_rgb(24, 24, 24));
        p.rect_stroke(
            rect,
            36,
            Stroke::new(2.0, Color32::from_rgb(40, 40, 40)),
            StrokeKind::Inside,
        );
        let p = p.with_clip_rect(rect.shrink(2.0));
        label(&p, [210.0, 150.0], "All Films", 60.0, WHITE, 460.0);
        icon_button(
            &p,
            Rect::from_min_size(Pos2::new(1630.0, 139.0), Vec2::splat(82.0)),
            Icon::Close,
            self.focus() == Focus::FilterClose,
        );
        p.line_segment(
            [Pos2::new(210.0, 345.0), Pos2::new(678.0, 345.0)],
            Stroke::new(2.0, Color32::from_gray(40)),
        );
        label(&p, [210.0, 399.0], "FILTERS", 24.0, MUTED, 450.0);
        for (group, label_text, y) in [
            (0, "Sort", 256.0),
            (1, "Genres", 444.0),
            (2, "Decades", 532.0),
            (3, "Countries", 620.0),
            (4, "Directors", 708.0),
        ] {
            let color = if self.filters.group == group {
                GOLD
            } else {
                Color32::from_gray(190)
            };
            let selected = if group == 0 {
                0
            } else {
                self.filters
                    .draft
                    .options
                    .iter()
                    .filter(|(g, _)| *g == group - 1)
                    .count()
            };
            let text = if selected > 0 {
                format!("{label_text} ({selected})")
            } else {
                label_text.to_owned()
            };
            label(&p, [210.0, y], &text, 34.0, color, 430.0);
            Icon::ChevronRight.paint(&p, centered(Pos2::new(666.0, y + 16.0), 24.0), color);
        }
        let selected = match self.layout_focus() {
            Focus::FilterOption(index) => index,
            _ => 0,
        };
        if self.filters.group == 0 {
            for (index, title) in ["Title", "Director", "Year", "Country", "Duration"]
                .iter()
                .enumerate()
            {
                let rect = Rect::from_min_size(
                    Pos2::new(714.0, 255.0 + index as f32 * 96.0),
                    Vec2::new(500.0, 80.0),
                );
                button(&p, rect, title, self.focus() == Focus::FilterOption(index));
                if self.filters.draft.sort_index == index {
                    let icon = if self.filters.draft.descending {
                        Icon::ArrowDown
                    } else {
                        Icon::ArrowUp
                    };
                    icon.paint(&p, centered(Pos2::new(1156.0, rect.center().y), 32.0), GOLD);
                }
            }
        } else if let Some(group) = data
            .filters
            .as_ref()
            .and_then(|menu| menu.groups.get(self.filters.group - 1))
        {
            let first_row = (selected / 2).saturating_sub(3);
            for (index, title) in group
                .options
                .iter()
                .enumerate()
                .skip(first_row * 2)
                .take(16)
            {
                let rect = Rect::from_min_size(
                    Pos2::new(
                        714.0 + (index % 2) as f32 * 506.0,
                        255.0 + (index / 2 - first_row) as f32 * 96.0,
                    ),
                    Vec2::new(490.0, 80.0),
                );
                let checked = self
                    .filters
                    .draft
                    .options
                    .contains(&(self.filters.group - 1, index));
                marked_button(
                    &p,
                    rect,
                    title,
                    checked,
                    self.focus() == Focus::FilterOption(index),
                );
            }
        } else {
            label(&p, [754.0, 280.0], "Loading filters…", 28.0, MUTED, 700.0);
        }
        icon_button(
            &p,
            Rect::from_min_size(Pos2::new(210.0, 868.0), Vec2::splat(82.0)),
            Icon::Reset,
            self.focus() == Focus::FilterReset,
        );
        button(
            &p,
            Rect::from_min_size(Pos2::new(310.0, 868.0), Vec2::new(370.0, 82.0)),
            &format!("APPLY ({} FILMS)", data.total),
            self.focus() == Focus::FilterApply,
        );
    }
}
impl AppUi {
    fn paint_detail(
        &mut self,
        p: &egui::Painter,
        detail: &Detail<'_>,
        rails: &[Rail<'_>],
        visible: &mut Vec<CardLayout>,
    ) {
        let y = -self.scroll_y();
        if let Some(texture) = detail.card.artwork_key.and_then(|key| self.image(key)) {
            p.image(
                texture,
                Rect::from_min_size(Pos2::new(0.0, y), Vec2::new(1920.0, 1080.0)),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                WHITE,
            );
            gradient(
                p,
                Rect::from_min_size(Pos2::new(0.0, y), Vec2::new(1600.0, 1080.0)),
                Color32::from_black_alpha(235),
                Color32::from_black_alpha(30),
            );
        }
        label(p, [169.0, y + 445.0], detail.directors, 24.0, GOLD, 1250.0);
        label(
            p,
            [169.0, y + 487.0],
            detail.card.title,
            60.0,
            WHITE,
            1400.0,
        );
        label(
            p,
            [169.0, y + 563.0],
            &format!(
                "{}   {}",
                detail.card.year,
                duration(detail.card.duration_seconds)
            ),
            24.0,
            WHITE,
            1200.0,
        );
        if detail.kind != crate::DetailKind::Collection {
            button(
                p,
                Rect::from_min_size(Pos2::new(150.0, y + 620.0), Vec2::new(460.0, 80.0)),
                detail.primary_action,
                self.focus() == Focus::DetailAction(0),
            );
        }
        let info_x = if detail.kind == crate::DetailKind::Collection {
            150.0
        } else {
            630.0
        };
        button(
            p,
            Rect::from_min_size(Pos2::new(info_x, y + 620.0), Vec2::splat(82.0)),
            "i",
            self.focus() == Focus::DetailAction(1),
        );
        button(
            p,
            Rect::from_min_size(Pos2::new(info_x + 102.0, y + 620.0), Vec2::splat(82.0)),
            "+",
            self.focus() == Focus::DetailAction(2),
        );
        paragraph(
            p,
            [150.0, y + 805.0],
            &detail.description.chars().take(360).collect::<String>(),
            27.0,
            WHITE,
            1250.0,
            (2, 360),
        );
        label(
            p,
            [150.0, y + 895.0],
            &format!(
                "Starring: {}",
                detail.starring.chars().take(128).collect::<String>()
            ),
            25.0,
            WHITE,
            1250.0,
        );
        for (index, rail) in rails.iter().take(8).enumerate() {
            label(
                p,
                [150.0 + index as f32 * 280.0, y + 966.0],
                rail.title,
                26.0,
                if matches!(self.focus(),Focus::DetailTab(i)|Focus::Card{row:i,..} if i==index) {
                    GOLD
                } else {
                    MUTED
                },
                260.0,
            );
        }
        p.line_segment(
            [Pos2::new(150.0, y + 1018.0), Pos2::new(1770.0, y + 1018.0)],
            Stroke::new(1.0, Color32::from_gray(45)),
        );
        let selected_row = match self.focus() {
            Focus::Card { row, .. } | Focus::DetailTab(row) => row,
            _ => 0,
        };
        if let Some(rail) = rails.get(selected_row) {
            let selected = match self.layout_focus() {
                Focus::Card { column, .. } => column,
                _ => 0,
            };
            let first = selected.saturating_sub(3);
            for (column, card) in rail.cards.iter().enumerate().skip(first).take(5) {
                let rect = Rect::from_min_size(
                    Pos2::new(150.0 + (column - first) as f32 * 414.0, y + 1067.0),
                    Vec2::new(378.0, 213.0),
                );
                if rect.bottom() > 0.0 && rect.top() < 1080.0 {
                    paint_card(
                        p,
                        card,
                        rect,
                        self.focus()
                            == Focus::Card {
                                row: selected_row,
                                column,
                            },
                        card.artwork_key.and_then(|key| self.image(key)),
                    );
                    visible.push(CardLayout {
                        row: selected_row,
                        column,
                        key: card.key.clone(),
                        image: rect,
                    });
                }
            }
        }
    }
    fn paint_information(&self, p: &egui::Painter, detail: &Detail<'_>) {
        p.rect_filled(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0)),
            0,
            Color32::from_black_alpha(170),
        );
        let rect = Rect::from_min_size(Pos2::new(290.0, 50.0), Vec2::new(1340.0, 980.0));
        p.rect_filled(rect, 36, Color32::from_rgb(24, 24, 24));
        let p = p.with_clip_rect(rect.shrink(2.0));
        label(&p, [348.0, 114.0], detail.directors, 24.0, GOLD, 1150.0);
        label(&p, [348.0, 159.0], detail.card.title, 58.0, WHITE, 1140.0);
        let description = detail
            .description
            .chars()
            .skip(self.detail_state.information_page * 420)
            .take(420)
            .collect::<String>();
        let content = p.with_clip_rect(Rect::from_min_max(
            Pos2::new(348.0, 250.0),
            Pos2::new(1570.0, 852.0),
        ));
        let synopsis = content.with_clip_rect(Rect::from_min_max(
            Pos2::new(348.0, 250.0),
            Pos2::new(1570.0, 575.0),
        ));
        paragraph(
            &synopsis,
            [348.0, 250.0],
            &description,
            27.0,
            WHITE,
            1220.0,
            (10, 420),
        );
        for (index, (name, value)) in [
            ("Starring", detail.starring),
            ("Countries", detail.countries),
            ("Languages", detail.languages),
        ]
        .iter()
        .enumerate()
        {
            let y = 588.0 + index as f32 * 75.0;
            label(&content, [348.0, y], name, 25.0, GOLD, 1200.0);
            label(&content, [348.0, y + 34.0], value, 25.0, WHITE, 1200.0);
        }
        label(
            &content,
            [348.0, 826.0],
            &format!(
                "{}   {}",
                detail.card.year,
                duration(detail.card.duration_seconds)
            ),
            24.0,
            WHITE,
            1200.0,
        );
        icon_button(
            &p,
            Rect::from_min_size(Pos2::new(1490.0, 108.0), Vec2::splat(82.0)),
            Icon::Close,
            self.focus() == Focus::InformationClose,
        );
        if detail.kind != crate::DetailKind::Collection {
            button(
                &p,
                Rect::from_min_size(Pos2::new(348.0, 903.0), Vec2::new(1224.0, 80.0)),
                detail.primary_action,
                self.focus() == Focus::InformationPrimary,
            );
        }
    }
}
fn duration(seconds: u32) -> String {
    if seconds == 0 {
        String::new()
    } else if seconds >= 3600 {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{} min", seconds / 60)
    }
}

pub(crate) fn catalog_retry_rect() -> Rect {
    Rect::from_min_size(Pos2::new(150.0, 970.0), Vec2::new(650.0, 80.0))
}
