use criterion_provider::MediaId;
use criterion_ui::{
    Action, AppUi, Card, CardAction, Command, Detail, DetailKind, DetailSortAction,
    DetailSortField, DetailSortSelection, DetailSortView, Focus, Hero, HeroAction, ListMembership,
    LoadState, Page, Rail, Target, ViewData,
};

fn card(target: &Target) -> Card<'_> {
    Card {
        key: target,
        artwork_key: None,
        title: "Root",
        year: "",
        duration_label: None,
        saved_fraction: None,
        action: CardAction::Open,
    }
}
fn data<'a>(
    root: &'a Target,
    rails: &'a [Rail<'a>],
    selected: usize,
    sort: Option<DetailSortView>,
) -> ViewData<'a> {
    ViewData {
        rails,
        status: LoadState::Ready,
        detail: Some(Detail {
            card: card(root),
            header_metadata: "",
            information_metadata: "",
            directors: "",
            description: "Synopsis",
            starring: None,
            countries: None,
            languages: None,
            content_warnings: None,
            primary_action: "WATCH NOW",
            primary_playback_target: None,
            selected_playlist: Some(selected),
            featured: None,
            seasons: None,
            kind: DetailKind::Collection,
            membership: ListMembership::SignedOut,
            sort,
        }),
        ..Default::default()
    }
}
fn open(ui: &mut AppUi, root: &Target) {
    let view = ViewData {
        hero: Some(Hero {
            card: card(root),
            description: "",
            action: "SEE MORE",
            action_kind: HeroAction::Open,
            background_key: None,
            title_logo_key: None,
        }),
        ..Default::default()
    };
    ui.handle(Action::Select, &view);
    assert_eq!(ui.page(), Page::Detail);
}
fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Default::default(),
    }
}
fn frame(ui: &mut AppUi, data: &ViewData<'_>, events: Vec<egui::Event>) -> criterion_ui::UiFrame {
    let mut frame = ui.render(
        egui::RawInput {
            events,
            ..Default::default()
        },
        data,
    );
    // These CPU tests inspect shapes/commands without a GPU texture consumer.
    frame.output.textures_delta.clear();
    frame
}
fn closed() -> DetailSortView {
    DetailSortView {
        pending: DetailSortSelection::default(),
        applied: DetailSortSelection::default(),
        visible: false,
    }
}
fn text_rect(frame: &criterion_ui::UiFrame, caption: &str) -> egui::Rect {
    frame
        .output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == caption => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .expect("literal caption must be painted")
}

#[test]
fn pointer_first_selection_is_normal_and_repeated_selected_first_tab_opens_root_bound_sheet() {
    let root = Target::Native(MediaId::new("Root0001").unwrap());
    let rails = [
        Rail {
            title: "First",
            cards: &[],
        },
        Rail {
            title: "Second",
            cards: &[],
        },
    ];
    let mut ui = AppUi::new();
    open(&mut ui, &root);
    let mut view = data(&root, &rails, 1, Some(closed()));
    ui.handle(Action::Down, &view);
    ui.handle(Action::Down, &view);
    assert_eq!(ui.focus(), Focus::DetailTab(1));
    let pos = text_rect(&frame(&mut ui, &view, vec![]), "First").center();
    assert_eq!(
        frame(
            &mut ui,
            &view,
            vec![pointer(pos, true), pointer(pos, false)]
        )
        .commands,
        [Command::SelectPlaylist(0)]
    );
    view.detail.as_mut().unwrap().selected_playlist = Some(0);
    assert_eq!(
        frame(
            &mut ui,
            &view,
            vec![pointer(pos, true), pointer(pos, false)]
        )
        .commands,
        [Command::DetailSort {
            root: root.media_id().unwrap().clone(),
            action: DetailSortAction::Open
        }]
    );
    view.detail.as_mut().unwrap().sort = None;
    assert_eq!(
        ui.handle(Action::Select, &view),
        [Command::SelectPlaylist(0)]
    );
}

#[test]
fn visible_sheet_paints_bounded_controls_blocks_backdrop_and_returns_to_first_tab_on_owner_close() {
    let root = Target::Native(MediaId::new("Root0001").unwrap());
    let rails = [Rail {
        title: "First",
        cards: &[],
    }];
    let mut ui = AppUi::new();
    open(&mut ui, &root);
    let mut sort = closed();
    sort.visible = true;
    let mut view = data(&root, &rails, 0, Some(sort));
    let painted = frame(&mut ui, &view, vec![]);
    assert_eq!(
        ui.focus(),
        Focus::DetailSortOption(DetailSortField::Default)
    );
    let screen = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1920.0, 1080.0));
    for caption in [
        "Sort by",
        "Default",
        "Title",
        "Release Date",
        "Runtime",
        "APPLY",
    ] {
        let area = text_rect(&painted, caption);
        assert!(
            screen.contains(area.min) && screen.contains(area.max),
            "{caption}"
        );
    }
    assert!(
        frame(
            &mut ui,
            &view,
            vec![
                pointer(egui::pos2(60.0, 270.0), true),
                pointer(egui::pos2(60.0, 270.0), false)
            ]
        )
        .commands
        .is_empty(),
        "the modal cannot activate the backdrop navigation rail"
    );
    let pos = text_rect(&painted, "Title").center();
    assert_eq!(
        frame(
            &mut ui,
            &view,
            vec![pointer(pos, true), pointer(pos, false)]
        )
        .commands,
        [Command::DetailSort {
            root: root.media_id().unwrap().clone(),
            action: DetailSortAction::Choose(DetailSortField::Title)
        }]
    );
    assert_eq!(
        ui.handle(Action::Back, &view),
        [Command::DetailSort {
            root: root.media_id().unwrap().clone(),
            action: DetailSortAction::Dismiss
        }]
    );
    sort.visible = false;
    view.detail.as_mut().unwrap().sort = Some(sort);
    let painted = frame(&mut ui, &view, vec![]);
    assert_eq!(ui.focus(), Focus::DetailTab(0));
    assert!(painted.output.shapes.iter().all(|shape| !matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Sort by")));
}

#[test]
fn closing_or_replacing_root_retires_a_pressed_modal_pointer() {
    let root = Target::Native(MediaId::new("Root0001").unwrap());
    let next = Target::Native(MediaId::new("Root0002").unwrap());
    let rails = [Rail {
        title: "First",
        cards: &[],
    }];
    for replacement in [false, true] {
        let mut ui = AppUi::new();
        open(&mut ui, &root);
        let mut sort = closed();
        sort.visible = true;
        let mut view = data(&root, &rails, 0, Some(sort));
        let pos = text_rect(&frame(&mut ui, &view, vec![]), "APPLY").center();
        assert!(
            frame(&mut ui, &view, vec![pointer(pos, true)])
                .commands
                .is_empty()
        );
        if replacement {
            view.detail.as_mut().unwrap().card = card(&next);
        } else {
            view.detail.as_mut().unwrap().sort = None;
        }
        assert!(
            frame(&mut ui, &view, vec![pointer(pos, false)])
                .commands
                .is_empty()
        );
        assert_eq!(ui.page(), Page::Detail);
    }
}
