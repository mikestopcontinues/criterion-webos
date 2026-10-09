use crate::{Action, AppUi, FilterSelection, Focus, Intent, Page, ViewData};
use criterion_provider::MediaId;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Navigate(Page),
    Restore(Page),
    Open(crate::Target),
    Play(MediaId),
    ToggleList(MediaId),
    SelectPlaylist(usize),
    Authenticate,
    VoiceSearch,
    Search {
        query: String,
        group: crate::SearchGroup,
    },
    Exit,
    ApplyFilters(FilterSelection),
}
impl AppUi {
    /// Transform one remote action using the current admitted display model.
    /// The caller executes returned commands and owns asynchronous publication.
    pub fn handle(&mut self, action: Action, data: &ViewData<'_>) -> Vec<Command> {
        self.pointer_press = None;
        self.pointer_layout_focus = None;
        if let Some(detail) = &data.detail {
            self.set_detail_kind(detail.kind);
            self.set_information_content(detail.description);
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
        self.handle_navigation(action, &rows)
            .into_iter()
            .filter_map(|intent| match intent {
                Intent::Navigate(page) => Some(Command::Navigate(page)),
                Intent::Restore(page) => Some(Command::Restore(page)),
                Intent::OpenCard { page, row, column } => {
                    let card = if matches!(
                        page,
                        Page::Home | Page::New | Page::Discovery | Page::Detail
                    ) {
                        data.rails.get(row).and_then(|rail| rail.cards.get(column))
                    } else {
                        row.checked_mul(if page == Page::Search { 3 } else { 4 })
                            .and_then(|index| index.checked_add(column))
                            .and_then(|index| data.cards.get(index))
                    };
                    card.map(|card| {
                        self.activate_target(card.key);
                        Command::Open(card.key.clone())
                    })
                }
                Intent::OpenHero => data.hero.as_ref().map(|hero| {
                    self.activate_target(hero.card.key);
                    Command::Open(hero.card.key.clone())
                }),
                Intent::Play => data
                    .detail
                    .as_ref()
                    .and_then(|detail| detail.card.key.media_id())
                    .map(|id| Command::Play(id.clone())),
                Intent::ToggleList => data
                    .detail
                    .as_ref()
                    .and_then(|detail| detail.card.key.media_id())
                    .map(|id| Command::ToggleList(id.clone())),
                Intent::SelectPlaylist(index) => Some(Command::SelectPlaylist(index)),
                Intent::Authenticate => Some(Command::Authenticate),
                Intent::VoiceSearch => Some(Command::VoiceSearch),
                Intent::Search { query, group } => Some(Command::Search { query, group }),
                Intent::Exit => Some(Command::Exit),
                Intent::ApplyFilters(selection) => Some(Command::ApplyFilters(selection)),
            })
            .collect()
    }
}
