use criterion_provider::MediaId;
use criterion_ui::{
    Action, AppUi, Card, CardAction, Command, Focus, LoadState, Page, Rail, Target, ViewData,
};

#[test]
fn episode_play_keeps_its_real_origin_focus_and_history() {
    let id = MediaId::new("Episode1").unwrap();
    let target = Target::Native(id.clone());
    let cards = [Card {
        key: &target,
        artwork_key: None,
        title: "Episode",
        year: "",
        duration_seconds: 0,
        saved_fraction: None,
        action: CardAction::Play,
    }];
    let rails = [Rail {
        title: "Episodes",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &data);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.handle(Action::Select, &data), [Command::Play(id)]);
    assert_eq!(ui.page(), Page::Home);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.handle(Action::Back, &data), [Command::Exit]);
}

fn card(target: &Target) -> Card<'_> {
    Card {
        key: target,
        artwork_key: None,
        title: "Native Series",
        year: "",
        duration_seconds: 0,
        saved_fraction: None,
        action: CardAction::Open,
    }
}
fn detail<'a>(target: &'a Target, primary: Option<&'a MediaId>) -> criterion_ui::Detail<'a> {
    criterion_ui::Detail {
        card: card(target),
        header_metadata: "",
        information_metadata: "",
        directors: "",
        description: "Series synopsis",
        starring: None,
        countries: None,
        languages: None,
        content_warnings: None,
        primary_action: "WATCH FIRST EPISODE",
        primary_playback_target: primary,
        selected_playlist: None,
        seasons: None,
        kind: criterion_ui::DetailKind::Series,
    }
}
fn open_detail(ui: &mut AppUi, target: &Target) {
    let cards = [card(target)];
    let rails = [Rail {
        title: "Native",
        cards: &cards,
    }];
    let data = ViewData {
        rails: &rails,
        status: LoadState::Ready,
        ..ViewData::default()
    };
    ui.handle(Action::Down, &data);
    assert_eq!(
        ui.handle(Action::Select, &data),
        [Command::Open(target.clone())]
    );
}
#[test]
fn series_primary_and_information_play_the_separate_episode_id() {
    let target = Target::Native(MediaId::new("Series01").unwrap());
    let episode = MediaId::new("Episode1").unwrap();
    let data = ViewData {
        detail: Some(detail(&target, Some(&episode))),
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    open_detail(&mut ui, &target);
    assert_eq!(
        ui.handle(Action::Select, &data),
        [Command::Play(episode.clone())]
    );
    assert_eq!(ui.page(), Page::Detail);
    assert_eq!(ui.focus(), Focus::DetailAction(0));
    ui.handle(Action::Right, &data);
    ui.handle(Action::Select, &data);
    assert_eq!(ui.handle(Action::Select, &data), [Command::Play(episode)]);
}

fn text(frame: &criterion_ui::UiFrame) -> String {
    frame
        .output
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some(t.galley.job.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn complete_series_resume_caption_fits_primary_buttons() {
    let target = Target::Native(MediaId::new("Series01").unwrap());
    let episode = MediaId::new("Episode1").unwrap();
    let caption = "RESUME SEASON -2147483648, EPISODE 510";
    let mut native = detail(&target, Some(&episode));
    native.primary_action = caption;
    let data = ViewData {
        detail: Some(native),
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut ui = AppUi::new();
    open_detail(&mut ui, &target);
    for (information, button) in [
        (
            false,
            egui::Rect::from_min_size(egui::pos2(150.0, 620.0), egui::vec2(460.0, 80.0)),
        ),
        (
            true,
            egui::Rect::from_min_size(egui::pos2(348.0, 903.0), egui::vec2(1224.0, 80.0)),
        ),
    ] {
        if information {
            ui.handle(Action::Right, &data);
            ui.handle(Action::Select, &data);
            assert_eq!(ui.focus(), Focus::InformationPrimary);
        }
        let mut frame = ui.render(egui::RawInput::default(), &data);
        frame.output.textures_delta.clear();
        let (painted, bounds) = frame
            .output
            .shapes
            .iter()
            .rev()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == caption => {
                    Some((text, shape.shape.visual_bounding_rect()))
                }
                _ => None,
            })
            .expect("complete season and episode caption is painted");
        assert!(
            !painted.galley.elided,
            "numeric operands must remain visible"
        );
        assert!(
            button.contains_rect(bounds),
            "complete caption {bounds:?} must fit primary button {button:?}"
        );
    }
}

#[test]
fn no_primary_target_hides_play_for_containers_live_and_missing_series_episode() {
    let target = Target::Native(MediaId::new("Native01").unwrap());
    for kind in [
        criterion_ui::DetailKind::Category,
        criterion_ui::DetailKind::Collection,
        criterion_ui::DetailKind::Franchise,
        criterion_ui::DetailKind::Live,
        criterion_ui::DetailKind::Series,
    ] {
        let mut d = detail(&target, None);
        d.kind = kind;
        let data = ViewData {
            detail: Some(d),
            status: LoadState::Ready,
            ..Default::default()
        };
        let mut ui = AppUi::new();
        open_detail(&mut ui, &target);
        let mut f = ui.render(egui::RawInput::default(), &data);
        let painted = text(&f);
        f.output.textures_delta.clear();
        assert!(!painted.contains("WATCH FIRST EPISODE"));
        assert_eq!(ui.focus(), Focus::DetailAction(1));
        assert!(ui.handle(Action::Select, &data).is_empty());
        assert_eq!(ui.focus(), Focus::InformationClose);
        assert!(ui.handle(Action::Select, &data).is_empty());
    }
}

#[test]
fn episodes_tab_selects_source_seasons_and_plays_clicked_episode_without_departure() {
    let target = Target::Native(MediaId::new("Series01").unwrap());
    let episode = Target::Native(MediaId::new("Episode2").unwrap());
    let choices = [
        criterion_ui::SeasonChoice {
            number: 7,
            title: "First supplied season",
            episode_count: 1,
        },
        criterion_ui::SeasonChoice {
            number: 3,
            title: "Second supplied season",
            episode_count: 2,
        },
    ];
    let mut episode_card = card(&episode);
    episode_card.action = CardAction::Play;
    let episodes = [episode_card];
    let rails = [
        Rail {
            title: "Episodes",
            cards: &episodes,
        },
        Rail {
            title: "Related",
            cards: &[],
        },
    ];
    let data = |selected| {
        let mut d = detail(&target, None);
        d.selected_playlist = Some(0);
        d.seasons = Some(criterion_ui::SeasonView {
            selected,
            choices: &choices,
        });
        ViewData {
            detail: Some(d),
            rails: &rails,
            status: LoadState::Ready,
            ..Default::default()
        }
    };
    let mut ui = AppUi::new();
    open_detail(&mut ui, &target);
    ui.handle(Action::Down, &data(0));
    ui.handle(Action::Down, &data(0));
    assert_eq!(ui.focus(), Focus::DetailTab(0));
    assert_eq!(
        ui.handle(Action::Down, &data(0)),
        [Command::SelectSeason(0)]
    );
    assert_eq!(ui.focus(), Focus::DetailSeason(0));
    assert_eq!(
        ui.handle(Action::Right, &data(0)),
        [Command::SelectSeason(1)]
    );
    assert_eq!(ui.focus(), Focus::DetailSeason(1));
    let mut frame = ui.render(egui::RawInput::default(), &data(1));
    let painted = text(&frame);
    frame.output.textures_delta.clear();
    assert!(painted.contains("First supplied season"));
    assert!(painted.contains("Second supplied season"));
    ui.handle(Action::Down, &data(1));
    assert_eq!(
        ui.handle(Action::Select, &data(1)),
        [Command::Play(episode.media_id().unwrap().clone())]
    );
    assert_eq!(ui.page(), Page::Detail);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert_eq!(ui.handle(Action::Up, &data(1)), [Command::SelectSeason(1)]);
    assert_eq!(ui.focus(), Focus::DetailSeason(1));
    ui.handle(Action::Up, &data(1));
    assert_eq!(
        ui.handle(Action::Back, &data(1)),
        [Command::Restore(Page::Home)]
    );
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
}

#[test]
fn changed_selected_season_restores_visible_episode_focus() {
    let target = Target::Native(MediaId::new("Series01").unwrap());
    let first = Target::Native(MediaId::new("Episode1").unwrap());
    let second = Target::Native(MediaId::new("Episode2").unwrap());
    let choices = [
        criterion_ui::SeasonChoice {
            number: 1,
            title: "First supplied season",
            episode_count: 1,
        },
        criterion_ui::SeasonChoice {
            number: 2,
            title: "Second supplied season",
            episode_count: 2,
        },
    ];
    let mut first_card = card(&first);
    first_card.action = CardAction::Play;
    let mut second_card = card(&second);
    second_card.action = CardAction::Play;
    let episodes = [first_card, second_card];
    let data = |selected| {
        let mut native = detail(&target, first.media_id());
        native.selected_playlist = Some(0);
        native.seasons = Some(criterion_ui::SeasonView {
            selected,
            choices: &choices,
        });
        ViewData {
            detail: Some(native),
            status: LoadState::Ready,
            ..ViewData::default()
        }
    };
    let rails = [Rail {
        title: "Episodes",
        cards: &episodes,
    }];
    let mut initial = data(1);
    initial.rails = &rails;
    let mut ui = AppUi::new();
    open_detail(&mut ui, &target);
    for _ in 0..4 {
        ui.handle(Action::Down, &initial);
    }
    ui.handle(Action::Right, &initial);
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 1 });

    let restored_rails = [Rail {
        title: "Episodes",
        cards: &episodes[..1],
    }];
    let mut restored = data(0);
    restored.rails = &restored_rails;
    let mut frame = ui.render(egui::RawInput::default(), &restored);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::Card { row: 0, column: 0 });
    assert!(
        frame
            .visible_cards
            .iter()
            .any(|card| { card.row == 0 && card.column == 0 && card.key == first })
    );
    assert_eq!(
        ui.handle(Action::Select, &restored),
        [Command::Play(first.media_id().unwrap().clone())]
    );

    let empty_choices = [criterion_ui::SeasonChoice {
        number: 1,
        title: "Empty season",
        episode_count: 0,
    }];
    let mut empty_detail = detail(&target, None);
    empty_detail.selected_playlist = Some(0);
    empty_detail.seasons = Some(criterion_ui::SeasonView {
        selected: 0,
        choices: &empty_choices,
    });
    let empty_rails = [Rail {
        title: "Episodes",
        cards: &[],
    }];
    let empty = ViewData {
        rails: &empty_rails,
        detail: Some(empty_detail),
        status: LoadState::Ready,
        ..ViewData::default()
    };
    let mut frame = ui.render(egui::RawInput::default(), &empty);
    frame.output.textures_delta.clear();
    assert_eq!(ui.focus(), Focus::DetailSeason(0));
    assert!(frame.visible_cards.is_empty());
    let control = frame
        .output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Empty season" => {
                Some((text, shape.shape.visual_bounding_rect()))
            }
            _ => None,
        })
        .expect("empty season selector remains painted");
    assert!(!control.0.galley.elided);
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0))
            .contains_rect(control.1)
    );
    assert_eq!(
        ui.handle(Action::Select, &empty),
        [Command::SelectSeason(0)]
    );
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}
#[test]
fn season_pointer_focus_selects_then_episode_pointer_play_preserves_series_origin() {
    let target = Target::Native(MediaId::new("Series01").unwrap());
    let episode = Target::Native(MediaId::new("Episode2").unwrap());
    let choices = [
        criterion_ui::SeasonChoice {
            number: 1,
            title: "First",
            episode_count: 1,
        },
        criterion_ui::SeasonChoice {
            number: 2,
            title: "Second",
            episode_count: 1,
        },
    ];
    let mut c = card(&episode);
    c.action = CardAction::Play;
    let cards = [c];
    let rails = [Rail {
        title: "Episodes",
        cards: &cards,
    }];
    let data = |selected| {
        let mut d = detail(&target, None);
        d.selected_playlist = Some(0);
        d.seasons = Some(criterion_ui::SeasonView {
            selected,
            choices: &choices,
        });
        ViewData {
            detail: Some(d),
            rails: &rails,
            status: LoadState::Ready,
            ..Default::default()
        }
    };
    let mut ui = AppUi::new();
    open_detail(&mut ui, &target);
    for _ in 0..3 {
        ui.handle(Action::Down, &data(0));
    }
    let mut f = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(500.0, 470.0), true)],
            ..Default::default()
        },
        &data(0),
    );
    f.output.textures_delta.clear();
    assert_eq!(f.commands, [Command::SelectSeason(1)]);
    assert_eq!(ui.focus(), Focus::DetailSeason(1));
    let mut f = ui.render(
        egui::RawInput {
            events: vec![pointer(egui::pos2(500.0, 470.0), false)],
            ..Default::default()
        },
        &data(1),
    );
    f.output.textures_delta.clear();
    assert_eq!(f.commands, [Command::SelectSeason(1)]);
    let mut frame = ui.render(egui::RawInput::default(), &data(1));
    let pos = frame.visible_cards[0].image.center();
    frame.output.textures_delta.clear();
    let mut f = ui.render(
        egui::RawInput {
            events: vec![pointer(pos, true), pointer(pos, false)],
            ..Default::default()
        },
        &data(1),
    );
    f.output.textures_delta.clear();
    assert_eq!(
        f.commands,
        [Command::Play(episode.media_id().unwrap().clone())]
    );
    assert_eq!(ui.page(), Page::Detail);
    assert_eq!(
        ui.handle(Action::Back, &data(1)),
        [Command::Restore(Page::Home)]
    );
}

#[test]
fn pointer_release_cannot_change_an_open_card_into_play() {
    let target = Target::Native(MediaId::new("Episode1").unwrap());
    let original = [card(&target)];
    let mut changed = original;
    changed[0].action = CardAction::Play;
    let first_rails = [Rail {
        title: "Original",
        cards: &original,
    }];
    let changed_rails = [Rail {
        title: "Original",
        cards: &changed,
    }];
    let first = ViewData {
        rails: &first_rails,
        status: LoadState::Ready,
        ..Default::default()
    };
    let changed = ViewData {
        rails: &changed_rails,
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    ui.handle(Action::Down, &first);
    let mut frame = ui.render(egui::RawInput::default(), &first);
    let pos = frame.visible_cards[0].image.center();
    frame.output.textures_delta.clear();
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(pos, true)],
            ..Default::default()
        },
        &first,
    );
    frame.output.textures_delta.clear();
    assert!(frame.commands.is_empty());
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(pos, false)],
            ..Default::default()
        },
        &changed,
    );
    frame.output.textures_delta.clear();
    assert!(frame.commands.is_empty());
    assert_eq!(ui.page(), Page::Home);
}

#[test]
fn pointer_release_cannot_play_a_replaced_primary_episode() {
    let target = Target::Native(MediaId::new("Series01").unwrap());
    let first = MediaId::new("Episode1").unwrap();
    let changed = MediaId::new("Episode2").unwrap();
    let data = |id| ViewData {
        detail: Some(detail(&target, Some(id))),
        status: LoadState::Ready,
        ..Default::default()
    };
    let mut ui = AppUi::new();
    open_detail(&mut ui, &target);
    let pos = egui::pos2(350.0, 650.0);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(pos, true)],
            ..Default::default()
        },
        &data(&first),
    );
    frame.output.textures_delta.clear();
    assert!(frame.commands.is_empty());
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(pos, false)],
            ..Default::default()
        },
        &data(&changed),
    );
    frame.output.textures_delta.clear();
    assert!(frame.commands.is_empty());
    assert_eq!(ui.page(), Page::Detail);
}

#[test]
fn late_generic_tab_remains_visible_and_selected_after_focus_leaves_tabs() {
    let target = Target::Native(MediaId::new("Series01").unwrap());
    let child = Target::Native(MediaId::new("Child001").unwrap());
    let cards = [card(&child)];
    let titles: Vec<_> = (0..20).map(|index| format!("Generic {index:02}")).collect();
    let rails: Vec<_> = titles
        .iter()
        .map(|title| Rail {
            title,
            cards: &cards,
        })
        .collect();
    let data = |selected| {
        let mut d = detail(&target, None);
        d.selected_playlist = Some(selected);
        ViewData {
            detail: Some(d),
            rails: &rails,
            status: LoadState::Ready,
            ..Default::default()
        }
    };
    let mut ui = AppUi::new();
    open_detail(&mut ui, &target);
    ui.handle(Action::Down, &data(0));
    ui.handle(Action::Down, &data(0));
    for index in 0..9 {
        assert_eq!(
            ui.handle(Action::Right, &data(index)),
            [Command::SelectPlaylist(index + 1)]
        );
    }
    let mut frame = ui.render(egui::RawInput::default(), &data(9));
    let painted = text(&frame);
    frame.output.textures_delta.clear();
    assert!(painted.contains("Generic 09"));
    assert!(!painted.contains("Generic 00"));
    assert!(!painted.contains("Generic 19"));
    let pos = egui::pos2(1280.0, 985.0);
    let mut frame = ui.render(
        egui::RawInput {
            events: vec![pointer(pos, true), pointer(pos, false)],
            ..Default::default()
        },
        &data(9),
    );
    frame.output.textures_delta.clear();
    assert_eq!(frame.commands, [Command::SelectPlaylist(10)]);
    assert_eq!(ui.focus(), Focus::DetailTab(10));
    ui.handle(Action::Down, &data(10));
    assert_eq!(ui.focus(), Focus::Card { row: 10, column: 0 });
    ui.handle(Action::Up, &data(10));
    ui.handle(Action::Up, &data(10));
    ui.handle(Action::Up, &data(10));
    let mut frame = ui.render(egui::RawInput::default(), &data(10));
    frame.output.textures_delta.clear();
    assert_eq!(frame.visible_cards[0].row, 10);
    ui.handle(Action::Down, &data(10));
    ui.handle(Action::Down, &data(10));
    assert_eq!(ui.focus(), Focus::DetailTab(10));
}

#[test]
fn late_detail_horizontal_card_movement_keeps_active_rail_visible_and_origin_stable() {
    let root = Target::Native(MediaId::new("Series01").unwrap());
    let child = Target::Native(MediaId::new("Child001").unwrap());
    let episode = Target::Native(MediaId::new("Episode2").unwrap());
    let mut play = card(&episode);
    play.action = CardAction::Play;
    let cards = [card(&child), play];
    let titles: Vec<_> = (0..11).map(|index| format!("Generic {index:02}")).collect();
    let rails: Vec<_> = titles
        .iter()
        .map(|title| Rail {
            title,
            cards: &cards,
        })
        .collect();
    let data = |selected| {
        let mut d = detail(&root, None);
        d.selected_playlist = Some(selected);
        ViewData {
            detail: Some(d),
            rails: &rails,
            status: LoadState::Ready,
            ..Default::default()
        }
    };
    let mut ui = AppUi::new();
    open_detail(&mut ui, &root);
    ui.handle(Action::Down, &data(0));
    ui.handle(Action::Down, &data(0));
    for index in 0..9 {
        ui.handle(Action::Right, &data(index));
    }
    ui.handle(Action::Down, &data(9));
    assert!(ui.handle(Action::Right, &data(9)).is_empty());
    assert_eq!(ui.focus(), Focus::Card { row: 9, column: 1 });
    assert_eq!(ui.scroll_y(), 632.0);
    let mut frame = ui.render(egui::RawInput::default(), &data(9));
    frame.output.textures_delta.clear();
    let visible = frame
        .visible_cards
        .iter()
        .find(|card| card.key == episode)
        .expect("selected episode remains painted");
    assert!(visible.image.min.y >= 0.0 && visible.image.max.y <= 1080.0);
    assert!(ui.handle(Action::Down, &data(9)).is_empty());
    assert_eq!(ui.focus(), Focus::Card { row: 9, column: 1 });
    assert_eq!(
        ui.handle(Action::Select, &data(9)),
        [Command::Play(episode.media_id().unwrap().clone())]
    );
    assert_eq!(ui.page(), Page::Detail);
    assert_eq!(
        ui.handle(Action::Back, &data(9)),
        [Command::Restore(Page::Home)]
    );
}
