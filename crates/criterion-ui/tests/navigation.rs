use criterion_ui::{Action, AppUi, Focus};
#[test]
fn home_down_focuses_first_popular_film_and_reveals_it() {
    let mut ui = AppUi::new();
    assert_eq!(ui.focus(), Focus::Hero);
    nav(&mut ui, Action::Down, &[6, 5]);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.scroll_y(), 632.0);
}
#[test]
fn left_opens_home_rail_and_back_restores_film_without_scroll_change() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Down, &[6, 5]);
    nav(&mut ui, Action::Left, &[6, 5]);
    assert_eq!(ui.focus(), Focus::Rail(criterion_ui::RailItem::Home));
    assert!(nav(&mut ui, Action::Back, &[6, 5]).is_empty());
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.scroll_y(), 632.0);
}
#[test]
fn selecting_all_films_enters_grid_and_up_reaches_filter() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[6]);
    nav(&mut ui, Action::Down, &[6]);
    nav(&mut ui, Action::Down, &[6]);
    let commands = nav(&mut ui, Action::Select, &[6]);
    assert_eq!(
        commands,
        vec![criterion_ui::Command::Navigate(
            criterion_ui::Page::AllFilms
        )]
    );
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    nav(&mut ui, Action::Up, &[4, 4, 2]);
    assert_eq!(ui.focus(), Focus::FilterButton);
}
#[test]
fn grid_down_preserves_column_then_clamps_at_short_final_row() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[6]);
    nav(&mut ui, Action::Down, &[6]);
    nav(&mut ui, Action::Down, &[6]);
    nav(&mut ui, Action::Select, &[6]);
    nav(&mut ui, Action::Right, &[4, 4, 2]);
    nav(&mut ui, Action::Right, &[4, 4, 2]);
    nav(&mut ui, Action::Right, &[4, 4, 2]);
    nav(&mut ui, Action::Down, &[4, 4, 2]);
    assert_eq!(ui.focus(), Focus::Card { row: 1, column: 3 });
    nav(&mut ui, Action::Down, &[4, 4, 2]);
    assert_eq!(ui.focus(), Focus::Card { row: 2, column: 1 });
    assert_eq!(ui.scroll_y(), 470.0);
}
#[test]
fn opening_film_then_back_restores_exact_grid_focus_and_scroll() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[6]);
    nav(&mut ui, Action::Down, &[6]);
    nav(&mut ui, Action::Down, &[6]);
    nav(&mut ui, Action::Select, &[6]);
    nav(&mut ui, Action::Right, &[4, 4, 4]);
    nav(&mut ui, Action::Down, &[4, 4, 4]);
    nav(&mut ui, Action::Down, &[4, 4, 4]);
    let commands = nav(&mut ui, Action::Select, &[4, 4, 4]);
    assert_eq!(
        commands,
        vec![criterion_ui::Command::Open(criterion_ui::Target::Media(
            criterion_provider::MediaId::new("qvwT6mJ4").unwrap()
        ))]
    );
    assert_eq!(ui.page(), criterion_ui::Page::Detail);
    nav(&mut ui, Action::Back, &[]);
    assert_eq!(ui.page(), criterion_ui::Page::AllFilms);
    assert_eq!(ui.focus(), Focus::Card { row: 2, column: 1 });
    assert_eq!(ui.scroll_y(), 470.0);
}
#[test]
fn filter_draft_applies_once_and_back_discards_unapplied_changes() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    ui.set_filter_option_counts([2, 1, 1, 1]);
    nav(&mut ui, Action::Up, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    assert_eq!(ui.focus(), Focus::FilterGroup(0));
    nav(&mut ui, Action::Right, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Left, &[1]);
    for _ in 0..5 {
        nav(&mut ui, Action::Down, &[1]);
    }
    let commands = nav(&mut ui, Action::Select, &[1]);
    assert_eq!(
        commands,
        vec![criterion_ui::Command::ApplyFilters(
            criterion_ui::FilterSelection {
                sort_index: 1,
                descending: false,
                options: vec![]
            }
        )]
    );
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    nav(&mut ui, Action::Up, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Right, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Back, &[1]);
    nav(&mut ui, Action::Up, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    let commands = nav(&mut ui, Action::Select, &[1]);
    assert_eq!(
        commands,
        vec![criterion_ui::Command::ApplyFilters(
            criterion_ui::FilterSelection {
                sort_index: 1,
                descending: false,
                options: vec![]
            }
        )]
    );
}
#[test]
fn cancelling_sort_modal_returns_to_first_grid_card_as_observed() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Up, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Back, &[1]);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
}
#[test]
fn two_column_genres_can_select_adjacent_options() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    ui.set_filter_option_counts([18, 13, 20, 50]);
    nav(&mut ui, Action::Up, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Right, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Right, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    assert_eq!(ui.focus(), Focus::FilterOption(1));
    nav(&mut ui, Action::Left, &[1]);
    nav(&mut ui, Action::Left, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    assert_eq!(
        nav(&mut ui, Action::Select, &[1]),
        vec![criterion_ui::Command::ApplyFilters(
            criterion_ui::FilterSelection {
                sort_index: 0,
                descending: false,
                options: vec![(0, 0), (0, 1)]
            }
        )]
    );
}
#[test]
fn film_information_back_preserves_action_then_supplements_open() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Right, &[2]);
    nav(&mut ui, Action::Select, &[2]);
    assert_eq!(ui.focus(), Focus::InformationPrimary);
    nav(&mut ui, Action::Back, &[2]);
    assert_eq!(ui.focus(), Focus::DetailAction(1));
    nav(&mut ui, Action::Down, &[2]);
    nav(&mut ui, Action::Down, &[2]);
    nav(&mut ui, Action::Down, &[2]);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
}

fn nav(ui: &mut AppUi, action: Action, rows: &[usize]) -> Vec<criterion_ui::Command> {
    let id = criterion_ui::Target::Media(criterion_provider::MediaId::new("qvwT6mJ4").unwrap());
    let card = criterion_ui::Card {
        key: &id,
        artwork_key: None,
        title: "Fixture film",
        year: "1986",
        duration_label: Some("1 h 37 min"),
        saved_fraction: None,
        action: criterion_ui::CardAction::Open,
    };
    let lists: Vec<Vec<_>> = rows.iter().map(|count| vec![card; *count]).collect();
    let rails: Vec<_> = lists
        .iter()
        .map(|cards| criterion_ui::Rail {
            title: "Fixture rail",
            cards,
        })
        .collect();
    let flat: Vec<_> = lists.iter().flatten().copied().collect();
    let data = criterion_ui::ViewData {
        cards: &flat,
        rails: &rails,
        status: criterion_ui::LoadState::Ready,
        ..criterion_ui::ViewData::default()
    };
    ui.handle(action, &data)
}
#[test]
fn every_supplement_column_can_return_to_the_owning_tab() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    for _ in 0..3 {
        nav(&mut ui, Action::Down, &[3]);
    }
    nav(&mut ui, Action::Right, &[3]);
    nav(&mut ui, Action::Up, &[3]);
    assert_eq!(ui.focus(), Focus::DetailTab(0));
}
#[test]
fn filter_button_down_restores_first_grid_card() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[8]);
    nav(&mut ui, Action::Down, &[8]);
    nav(&mut ui, Action::Down, &[8]);
    nav(&mut ui, Action::Select, &[8]);
    nav(&mut ui, Action::Up, &[4, 4]);
    nav(&mut ui, Action::Down, &[4, 4]);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
}
#[test]
fn new_page_opens_on_hero() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[8]);
    nav(&mut ui, Action::Down, &[8]);
    nav(&mut ui, Action::Select, &[8]);
    assert_eq!(ui.page(), criterion_ui::Page::New);
    assert_eq!(ui.focus(), Focus::Hero);
}
#[test]
fn collection_information_has_a_visible_close_focus_and_never_plays() {
    let mut ui = AppUi::new();
    nav(&mut ui, Action::Left, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Down, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    nav(&mut ui, Action::Select, &[1]);
    ui.set_detail_kind(criterion_ui::DetailKind::Collection);
    nav(&mut ui, Action::Select, &[]);
    assert_eq!(ui.focus(), Focus::InformationClose);
    assert!(nav(&mut ui, Action::Select, &[]).is_empty());
    assert_eq!(ui.focus(), Focus::DetailAction(1));
}
