use crate::{Action, AppUi, FilterSelection, Focus, Intent, Page, ViewData};
use criterion_provider::MediaId;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Navigate(Page),
    Restore(Page),
    Open(crate::Target),
    MoveHero {
        page: Page,
        from: crate::HeroCursor,
        direction: crate::HeroDirection,
    },
    ActivateHero {
        origin: Page,
        from: crate::HeroCursor,
        target: crate::Target,
    },
    ActivateRail {
        origin: Page,
        from: crate::RailActionCursor,
        target: crate::Target,
    },
    ActivateCard {
        target: crate::Target,
        focus: Focus,
    },
    Play(MediaId),
    ToggleList(MediaId),
    SelectPlaylist(usize),
    SelectSeason(usize),
    DetailSort {
        root: MediaId,
        action: crate::DetailSortAction,
    },
    Authenticate,
    RetryAuthentication,
    CancelAuthentication,
    Logout,
    VoiceSearch,
    Search {
        query: String,
        group: crate::SearchGroup,
    },
    Exit,
    ApplyFilters(FilterSelection),
    Catalog {
        anchor: usize,
        target: usize,
    },
    RetryCatalog,
    MyListGroup(crate::MyListGroup),
}
impl AppUi {
    pub(crate) fn sync_discovery_actions(&mut self, data: &ViewData<'_>) {
        if !matches!(self.page(), Page::Home | Page::New | Page::Discovery) {
            return;
        }
        let normalize = |focus| {
            let Focus::DiscoveryRailAction { row, column } = focus else {
                return focus;
            };
            match data.rails.get(row) {
                Some(rail) if rail.action.is_some() => Focus::DiscoveryRailAction {
                    row,
                    column: column.min(rail.cards.len().saturating_sub(1)),
                },
                Some(rail) if !rail.cards.is_empty() => Focus::Card {
                    row,
                    column: column.min(rail.cards.len() - 1),
                },
                _ => Focus::Hero,
            }
        };
        let next = normalize(self.focus);
        if next != self.focus {
            self.focus = next;
            self.pointer_press = None;
            self.pointer_layout_focus = None;
        }
        self.return_focus = normalize(self.return_focus);
    }
    fn handle_discovery_action(
        &mut self,
        action: Action,
        data: &ViewData<'_>,
    ) -> Option<Vec<Command>> {
        if !matches!(self.page(), Page::Home | Page::New | Page::Discovery) {
            return None;
        }
        let row_focus = |row: usize, column: usize| {
            let rail = data.rails.get(row)?;
            if !rail.cards.is_empty() {
                Some(Focus::Card {
                    row,
                    column: column.min(rail.cards.len() - 1),
                })
            } else {
                rail.action
                    .map(|_| Focus::DiscoveryRailAction { row, column: 0 })
            }
        };
        let next = match (self.focus, action) {
            (Focus::DiscoveryRailAction { row, .. }, Action::Select) => {
                return Some(
                    data.rail_action_cursor(row)
                        .map(|from| {
                            vec![Command::ActivateRail {
                                origin: self.page(),
                                from,
                                target: data.rails[row].action.unwrap().target.clone(),
                            }]
                        })
                        .unwrap_or_default(),
                );
            }
            (Focus::Card { row, column }, Action::Up)
                if data
                    .rails
                    .get(row)
                    .is_some_and(|rail| rail.action.is_some()) =>
            {
                Some(Focus::DiscoveryRailAction { row, column })
            }
            (Focus::DiscoveryRailAction { row, column }, Action::Right | Action::Down)
                if !data.rails[row].cards.is_empty() =>
            {
                Some(Focus::Card {
                    row,
                    column: column.min(data.rails[row].cards.len() - 1),
                })
            }
            (Focus::DiscoveryRailAction { row, .. }, Action::Down) => {
                ((row + 1)..data.rails.len()).find_map(|row| row_focus(row, 0))
            }
            (Focus::Card { row, column }, Action::Down) => {
                ((row + 1)..data.rails.len()).find_map(|row| row_focus(row, column))
            }
            (Focus::DiscoveryRailAction { row, column }, Action::Up) => (0..row)
                .rev()
                .find_map(|row| row_focus(row, column))
                .or(Some(if data.hero.is_some() {
                    Focus::Hero
                } else if data
                    .hero_carousel
                    .is_some_and(|carousel| carousel.total > 1)
                {
                    Focus::HeroPrevious
                } else {
                    Focus::Hero
                })),
            (Focus::Card { row, column }, Action::Up)
                if row > 0
                    && data
                        .rails
                        .get(row - 1)
                        .is_some_and(|rail| rail.cards.is_empty()) =>
            {
                (0..row).rev().find_map(|row| row_focus(row, column))
            }
            (Focus::Hero | Focus::HeroPrevious | Focus::HeroNext, Action::Down)
                if data.rails.first().is_some_and(|rail| rail.cards.is_empty()) =>
            {
                (0..data.rails.len()).find_map(|row| row_focus(row, 0))
            }
            (Focus::DiscoveryRailAction { .. }, Action::Right) => return Some(Vec::new()),
            _ => return None,
        };
        if let Some(focus) = next {
            self.focus = focus;
            self.scroll_y = match focus {
                Focus::Card { row, .. } | Focus::DiscoveryRailAction { row, .. } => {
                    632.0 + row as f32 * 397.0
                }
                _ => 0.0,
            };
        }
        Some(Vec::new())
    }

    pub(crate) fn sync_hero(&mut self, data: &ViewData<'_>) {
        if !matches!(self.page(), Page::Home | Page::New | Page::Discovery) {
            return;
        }
        let Some(carousel) = data.hero_carousel else {
            if matches!(self.focus, Focus::HeroPrevious | Focus::HeroNext) {
                self.focus = Focus::Hero;
            }
            if matches!(self.return_focus, Focus::HeroPrevious | Focus::HeroNext) {
                self.return_focus = Focus::Hero;
            }
            return;
        };
        let fallback = if data.hero.is_some() {
            Focus::Hero
        } else if carousel.total > 1 && data.status == crate::LoadState::Ready {
            Focus::HeroPrevious
        } else if let Some(row) = data
            .rails
            .iter()
            .position(|rail| !rail.cards.is_empty() || rail.action.is_some())
        {
            if data.rails[row].cards.is_empty() {
                Focus::DiscoveryRailAction { row, column: 0 }
            } else {
                Focus::Card { row, column: 0 }
            }
        } else {
            Focus::Rail(if self.page() == Page::New {
                crate::RailItem::New
            } else {
                crate::RailItem::Home
            })
        };
        let valid = |focus| match focus {
            Focus::Hero => data.hero.is_some(),
            Focus::HeroPrevious | Focus::HeroNext => {
                carousel.total > 1 && data.status == crate::LoadState::Ready
            }
            _ => true,
        };
        if !valid(self.focus) {
            self.focus = fallback;
            self.pointer_press = None;
            self.pointer_layout_focus = None;
        }
        if !valid(self.return_focus) {
            self.return_focus = fallback;
        }
    }
    fn handle_hero(&mut self, action: Action, data: &ViewData<'_>) -> Option<Vec<Command>> {
        if !matches!(self.page(), Page::Home | Page::New | Page::Discovery) {
            return None;
        }
        let carousel = data.hero_carousel?;
        match (self.focus(), action) {
            (Focus::Hero, Action::Right) if carousel.total > 1 => self.focus = Focus::HeroPrevious,
            (Focus::HeroPrevious, Action::Right) => self.focus = Focus::HeroNext,
            (Focus::HeroNext, Action::Right) => (),
            (Focus::HeroNext, Action::Left) => self.focus = Focus::HeroPrevious,
            (Focus::HeroPrevious, Action::Left) if data.hero.is_some() => self.focus = Focus::Hero,
            (Focus::HeroPrevious | Focus::HeroNext, Action::Select) => {
                return Some(
                    carousel
                        .cursor()
                        .filter(|_| data.status == crate::LoadState::Ready && carousel.total > 1)
                        .map(|from| {
                            vec![Command::MoveHero {
                                page: self.page(),
                                from,
                                direction: if self.focus() == Focus::HeroPrevious {
                                    crate::HeroDirection::Previous
                                } else {
                                    crate::HeroDirection::Next
                                },
                            }]
                        })
                        .unwrap_or_default(),
                );
            }
            (Focus::Hero, Action::Select) => {
                return Some(
                    carousel
                        .cursor()
                        .filter(|_| data.status == crate::LoadState::Ready)
                        .zip(data.hero.as_ref())
                        .map(|(from, hero)| {
                            vec![Command::ActivateHero {
                                origin: self.page(),
                                from,
                                target: hero.card.key.clone(),
                            }]
                        })
                        .unwrap_or_default(),
                );
            }
            (Focus::Card { row: 0, .. }, Action::Up) => {
                self.focus = if data.hero.is_some() {
                    Focus::Hero
                } else if carousel.total > 1 {
                    Focus::HeroPrevious
                } else {
                    self.focus
                };
                self.scroll_y = 0.0;
            }
            _ => return None,
        }
        Some(Vec::new())
    }
    /// Transform one remote action using the current admitted display model.
    /// The caller executes returned commands and owns asynchronous publication.
    pub fn handle(&mut self, action: Action, data: &ViewData<'_>) -> Vec<Command> {
        self.pointer_press = None;
        self.pointer_layout_focus = None;
        self.sync_login(data.login);
        self.sync_rail(data.login);
        self.sync_my_list(data);
        self.sync_hero(data);
        self.sync_discovery_actions(data);
        if let Some(commands) = self.handle_discovery_action(action, data) {
            return commands;
        }
        if let Some(commands) = self.handle_hero(action, data) {
            return commands;
        }
        if let Some(commands) = self.handle_my_list(action, data) {
            return commands;
        }
        if action == Action::Select
            && let Some(card) = card_at_focus(data, self.page(), self.focus())
            && card.action != crate::CardAction::Open
        {
            return if card.action == crate::CardAction::Play {
                vec![Command::ActivateCard {
                    target: card.key.clone(),
                    focus: self.focus(),
                }]
            } else {
                Vec::new()
            };
        }
        if let Some(commands) = self.handle_catalog(action, data) {
            return commands;
        }
        if let Some(commands) = self.handle_login(action, data.login) {
            return commands;
        }
        if let Some(detail) = &data.detail {
            self.sync_detail(detail);
        }
        if action == Action::Select && self.focus() == Focus::Hero && data.hero.is_none() {
            return Vec::new();
        }
        if action == Action::Select
            && self.focus() == Focus::Hero
            && let Some(hero) = &data.hero
            && hero.action_kind == crate::HeroAction::Play
        {
            return hero
                .card
                .key
                .media_id()
                .map(|id| vec![Command::Play(id.clone())])
                .unwrap_or_default();
        }
        let columns = if self.page() == Page::Search { 3 } else { 4 };
        let rows: Vec<_> = if self.page() == Page::Search && self.query().trim().is_empty() {
            Vec::new()
        } else if matches!(
            self.page(),
            Page::Home | Page::New | Page::Discovery | Page::Detail
        ) {
            data.rails.iter().map(|rail| rail.cards.len()).collect()
        } else {
            data.cards.chunks(columns).map(<[_]>::len).collect()
        };
        let catalog_anchor = self.catalog_anchor(data);
        let previous_page = self.page();
        let mut commands: Vec<_> = self
            .handle_navigation(action, &rows, data.login)
            .into_iter()
            .filter_map(|intent| match intent {
                Intent::Navigate(page) => Some(Command::Navigate(page)),
                Intent::Restore(page) => Some(Command::Restore(page)),
                Intent::OpenCard { page, focus } => {
                    let card = card_at_focus(data, page, focus);
                    card.map(|card| {
                        self.activate_target(card.key, data.login);
                        Command::ActivateCard {
                            target: card.key.clone(),
                            focus,
                        }
                    })
                }
                Intent::OpenHero => data.hero.as_ref().map(|hero| {
                    self.activate_target(hero.card.key, data.login);
                    Command::Open(hero.card.key.clone())
                }),
                Intent::Play => data
                    .detail
                    .as_ref()
                    .and_then(|detail| detail.primary_playback_target)
                    .map(|id| Command::Play(id.clone())),
                Intent::ToggleList => data
                    .detail
                    .as_ref()
                    .filter(|detail| {
                        // Membership is display-only; signed-in writes await durable admission.
                        detail.kind != crate::DetailKind::Live
                            && !matches!(
                                data.login,
                                crate::LoginView::SignedIn | crate::LoginView::SigningOut
                            )
                    })
                    .and_then(|detail| detail.card.key.media_id())
                    .map(|id| Command::ToggleList(id.clone())),
                Intent::SelectPlaylist(index) => Some(Command::SelectPlaylist(index)),
                Intent::SelectSeason(index) => Some(Command::SelectSeason(index)),
                Intent::DetailSort(action) => data
                    .detail
                    .as_ref()
                    .and_then(|detail| detail.card.key.media_id())
                    .map(|root| Command::DetailSort {
                        root: root.clone(),
                        action,
                    }),
                Intent::Authenticate => Some(
                    if matches!(
                        data.login,
                        crate::LoginView::SignedIn
                            | crate::LoginView::SigningOut
                            | crate::LoginView::Requesting
                            | crate::LoginView::Awaiting { .. }
                    ) {
                        Command::Navigate(Page::Login)
                    } else {
                        Command::Authenticate
                    },
                ),
                Intent::VoiceSearch => Some(Command::VoiceSearch),
                Intent::Search { query, group } => Some(Command::Search { query, group }),
                Intent::Exit => Some(Command::Exit),
                Intent::ApplyFilters(selection) => Some(Command::ApplyFilters(selection)),
            })
            .collect();
        if let Some(anchor) = catalog_anchor {
            commands.insert(
                0,
                Command::Catalog {
                    anchor,
                    target: anchor,
                },
            );
        }
        if previous_page == Page::Login
            && self.page() != Page::Login
            && matches!(
                data.login,
                crate::LoginView::Requesting | crate::LoginView::Awaiting { .. }
            )
        {
            commands.insert(0, Command::CancelAuthentication);
        }
        self.sync_login(data.login);
        self.sync_rail(data.login);
        if !self.wants_text_input() {
            self.search.composition.clear();
            self.search.select_all = false;
        }
        commands
    }
}

pub(crate) fn card_at_focus<'a, 'b>(
    data: &'b ViewData<'a>,
    page: Page,
    focus: Focus,
) -> Option<&'b crate::Card<'a>> {
    match focus {
        Focus::Card { row, column } => card_at(data, page, row, column),
        Focus::FeaturedCard(column) if page == Page::Detail => {
            data.detail.as_ref()?.featured.as_ref()?.cards.get(column)
        }
        _ => None,
    }
}

pub(crate) fn card_at<'a, 'b>(
    data: &'b ViewData<'a>,
    page: Page,
    row: usize,
    column: usize,
) -> Option<&'b crate::Card<'a>> {
    if matches!(
        page,
        Page::Home | Page::New | Page::Discovery | Page::Detail
    ) {
        data.rails.get(row).and_then(|rail| rail.cards.get(column))
    } else {
        if column >= if page == Page::Search { 3 } else { 4 } {
            return None;
        }
        row.checked_mul(if page == Page::Search { 3 } else { 4 })
            .and_then(|index| index.checked_add(column))
            .and_then(|index| index.checked_sub(data.catalog.map_or(0, |w| w.first)))
            .and_then(|index| data.cards.get(index))
    }
}
