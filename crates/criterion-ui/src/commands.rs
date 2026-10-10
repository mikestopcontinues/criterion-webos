use crate::{Action, AppUi, FilterSelection, Focus, Intent, Page, ViewData};
use criterion_provider::MediaId;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Navigate(Page),
    Restore(Page),
    Open(crate::Target),
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
    /// Transform one remote action using the current admitted display model.
    /// The caller executes returned commands and owns asynchronous publication.
    pub fn handle(&mut self, action: Action, data: &ViewData<'_>) -> Vec<Command> {
        self.pointer_press = None;
        self.pointer_layout_focus = None;
        self.sync_login(data.login);
        self.sync_rail(data.login);
        self.sync_my_list(data);
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
