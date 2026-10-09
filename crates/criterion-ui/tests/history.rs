use criterion_ui::{Action, AppUi, Command, FilterSelection, LoginView, Page, ViewData};

fn all_films(ui: &mut AppUi, data: &ViewData<'_>) {
    ui.handle(Action::Left, data);
    ui.handle(Action::Down, data);
    ui.handle(Action::Down, data);
    ui.handle(Action::Select, data);
}
fn select_sort(ui: &mut AppUi, data: &ViewData<'_>, prior: usize, next: usize) -> FilterSelection {
    ui.handle(Action::Up, data);
    ui.handle(Action::Select, data);
    ui.handle(Action::Right, data);
    for _ in prior..next {
        ui.handle(Action::Down, data);
    }
    ui.handle(Action::Select, data);
    ui.handle(Action::Left, data);
    for _ in 0..5 {
        ui.handle(Action::Down, data);
    }
    let commands = ui.handle(Action::Select, data);
    let [Command::ApplyFilters(selection)] = commands.as_slice() else {
        panic!("filter is applied")
    };
    selection.clone()
}
#[test]
fn restored_grid_uses_its_saved_committed_filter_selection() {
    let mut ui = AppUi::new();
    let data = ViewData::default();
    all_films(&mut ui, &data);
    assert_eq!(select_sort(&mut ui, &data, 0, 1).sort_index, 1);
    ui.begin_authentication();
    let login = ViewData {
        login: LoginView::Requesting,
        ..Default::default()
    };
    ui.handle(Action::Left, &login);
    for _ in 0..3 {
        ui.handle(Action::Up, &login);
    }
    ui.handle(Action::Select, &login);
    assert_eq!(ui.page(), Page::Home);
    all_films(&mut ui, &data);
    assert_eq!(select_sort(&mut ui, &data, 1, 2).sort_index, 2);
    assert_eq!(
        ui.handle(Action::Back, &data),
        vec![Command::Restore(Page::AllFilms)]
    );
    let selection = select_sort(&mut ui, &data, 1, 1);
    assert_eq!(
        selection,
        FilterSelection {
            sort_index: 1,
            descending: true,
            options: vec![]
        }
    );
}

#[test]
fn fresh_grid_closes_departed_filter_draft_but_back_restores_it() {
    let mut ui = AppUi::new();
    let data = ViewData::default();
    all_films(&mut ui, &data);
    ui.handle(Action::Up, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.focus(), criterion_ui::Focus::FilterGroup(0));
    ui.begin_authentication();
    let login = ViewData {
        login: LoginView::Requesting,
        ..Default::default()
    };
    ui.handle(Action::Left, &login);
    for _ in 0..3 {
        ui.handle(Action::Up, &login);
    }
    ui.handle(Action::Select, &login);
    all_films(&mut ui, &data);
    ui.handle(Action::Up, &data);
    assert_eq!(ui.focus(), criterion_ui::Focus::FilterButton);
    assert_eq!(
        ui.handle(Action::Back, &data),
        vec![Command::Restore(Page::AllFilms)]
    );
    assert_eq!(ui.focus(), criterion_ui::Focus::FilterGroup(0));
    ui.handle(Action::Right, &data);
    assert_eq!(ui.focus(), criterion_ui::Focus::FilterOption(0));
}
