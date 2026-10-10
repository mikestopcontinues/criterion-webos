// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual application input and paint using captured ordinary public discovery.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

type App = Application<Public, Offline, SystemClock, AccountOffline>;
struct AccountOffline(Arc<AtomicUsize>);
impl criterion_account::Transport for AccountOffline {
    async fn send(
        &self,
        _request: criterion_account::Request,
    ) -> Result<criterion_account::Response, criterion_account::Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(criterion_account::Error::Unavailable)
    }
}

#[derive(Clone, Default)]
struct Public {
    calls: Arc<Mutex<Vec<String>>>,
    new_payload: Arc<Mutex<Option<serde_json::Value>>>,
    account_calls: Arc<AtomicUsize>,
}
impl RequestTransport for Public {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        let path = request.url.path();
        self.calls.lock().unwrap().push(path.to_owned());
        let source = match path {
            "/" => include_str!("../../../../../tests/fixtures/provider/discovery-home.json"),
            "/new" => include_str!("../../../../../tests/fixtures/provider/discovery-new.json"),
            "/discover/newly-added" => {
                include_str!("../../../../../tests/fixtures/provider/discovery-newly-added.json")
            }
            _ => return Err(Error::Unavailable),
        };
        let blocks: serde_json::Value = if path == "/new" {
            self.new_payload.lock().unwrap().clone()
        } else {
            None
        }
        .unwrap_or_else(|| serde_json::from_str(source).unwrap());
        let stream = format!(
            "baf:I[37,[],\"LanderStoryBlocks\"]\nace:{}\n",
            serde_json::json!(["$", "$Lbaf", null, blocks])
        );
        Ok(Response {
            status: 200,
            content_type: "text/html".into(),
            body: format!(
                "<html><script>self.__next_f.push({})</script></html>",
                serde_json::json!([1, stream])
            )
            .into_bytes(),
        })
    }
}
fn fixture_app(runtime: &Runtime, public: Public) -> App {
    let clock = SystemClock::default();
    let session = Arc::new(criterion_session::Session::with_transport(
        criterion_session::Configuration::production(),
        Offline,
        clock.clone(),
    ));
    let account_calls = public.account_calls.clone();
    Application::with_parts(
        surface(),
        Controller::new(Catalog::with_transport(public), runtime.handle()),
        Authentication::with_session(session.clone(), clock),
        Accounts::from_parts(
            Arc::new(criterion_account::AccountClient::with_transport(
                AccountOffline(account_calls),
            )),
            session,
        ),
        Artwork::offline(),
    )
}
fn settle(app: &mut App, runtime: &Runtime) {
    for _ in 0..1000 {
        app.poll(runtime, true);
        if app
            .controller
            .view
            .with_view(app.authentication.view(), |data| {
                data.status == LoadState::Ready
            })
        {
            return;
        }
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    }
    panic!("captured public discovery did not become Ready");
}
fn key(app: &mut App, runtime: &Runtime, scancode: u32) {
    for pressed in [true, false] {
        app.event(
            Event::Key(KeyEvent {
                scancode,
                keycode: 0,
                pressed,
                repeat: false,
            }),
            surface(),
            runtime,
            Duration::ZERO,
        );
        if !pressed && let Some(mut output) = app.take_output() {
            output.textures_delta.clear();
        }
    }
}
fn paint(app: &mut App, runtime: &Runtime) -> Vec<String> {
    app.consume(runtime, Duration::ZERO);
    let mut output = app.take_output().unwrap();
    output.textures_delta.clear();
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}
fn new_page(app: &mut App, runtime: &Runtime) {
    settle(app, runtime);
    for scancode in [80, 81, 40] {
        key(app, runtime, scancode);
    }
    assert_eq!(app.ui.page(), Page::New);
    settle(app, runtime);
}
#[test]
fn captured_new_rail_paints_its_supplied_see_all_action() {
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    app.controller
        .view
        .with_view(app.authentication.view(), |data| {
            let blocks: Vec<_> = data
                .rails
                .iter()
                .filter_map(|rail| rail.action.map(|action| action.block))
                .collect();
            assert_eq!(blocks, [537, 817, 811, 818, 550, 861, 783, 726, 822, 748]);
        });
    new_page(&mut app, &runtime);
    app.controller
        .view
        .with_view(app.authentication.view(), |data| {
            let blocks: Vec<_> = data
                .rails
                .iter()
                .filter_map(|rail| rail.action.map(|action| action.block))
                .collect();
            assert_eq!(blocks, [825, 830, 862, 835, 836, 838]);
        });
    key(&mut app, &runtime, 81);
    key(&mut app, &runtime, 81);
    let text = paint(&mut app, &runtime);
    assert!(text.iter().any(|s| s == "Newly Added Films"));
    assert!(
        text.iter().any(|s| s == "See all"),
        "supplied rail CTA is absent from actual App paint"
    );
    assert_eq!(&*public.calls.lock().unwrap(), &["/", "/new"]);
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn remote_new_see_all_opens_exact_discovery_route_and_back_restores_rail() {
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    new_page(&mut app, &runtime);
    // Newly Added is the second retained rail after October Collections.
    for code in [81, 81, 82, 40] {
        key(&mut app, &runtime, code);
    }
    assert_eq!(app.ui.page(), Page::Discovery);
    settle(&mut app, &runtime);
    assert_eq!(
        &*public.calls.lock().unwrap(),
        &["/", "/new", "/discover/newly-added"]
    );
    key(&mut app, &runtime, 81);
    assert!(
        paint(&mut app, &runtime)
            .iter()
            .any(|s| s == "Barry Lyndon")
    );
    key(&mut app, &runtime, 41);
    assert_eq!(app.ui.page(), Page::New);
    assert_eq!(
        app.ui.focus(),
        Focus::DiscoveryRailAction { row: 1, column: 0 }
    );
    assert!(paint(&mut app, &runtime).iter().any(|s| s == "See all"));
    assert_eq!(
        public.calls.lock().unwrap().len(),
        3,
        "warm Back must retain supplied browsing data"
    );
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn captured_new_and_warm_back_keep_next_rail_headers_below_previous_card_metadata() {
    fn output(app: &mut App, runtime: &Runtime) -> egui::FullOutput {
        app.consume(runtime, Duration::ZERO);
        let mut output = app.take_output().unwrap();
        output.textures_delta.clear();
        output
    }
    fn text_bounds(output: &egui::FullOutput, caption: &str) -> egui::Rect {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == caption => {
                    Some(shape.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("captured public text was not painted: {caption}"))
    }
    fn separated(output: &egui::FullOutput, heading: &str, metadata: [&str; 3]) {
        let next = text_bounds(output, heading);
        for caption in metadata {
            let previous = text_bounds(output, caption);
            assert!(
                previous.bottom() < next.top(),
                "{heading} overlaps previous card {caption}: {previous:?} then {next:?}"
            );
        }
    }
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    new_page(&mut app, &runtime);
    for code in [81, 81, 79, 79, 82] {
        key(&mut app, &runtime, code);
    }
    let focus = Focus::DiscoveryRailAction { row: 1, column: 2 };
    assert_eq!(app.ui.focus(), focus);
    let scroll = app.ui.scroll_y();
    separated(
        &output(&mut app, &runtime),
        "Highway Horror",
        ["Barry Lyndon", "1975", "3 h 5 min"],
    );
    key(&mut app, &runtime, 40);
    assert_eq!(app.ui.page(), Page::Discovery);
    settle(&mut app, &runtime);
    key(&mut app, &runtime, 41);
    assert_eq!(app.ui.page(), Page::New);
    assert_eq!(app.ui.focus(), focus);
    assert_eq!(app.ui.scroll_y(), scroll);
    separated(
        &output(&mut app, &runtime),
        "Highway Horror",
        ["Barry Lyndon", "1975", "3 h 5 min"],
    );
    // Keep the second collision visible at its own focused row rather than
    // requiring offscreen rows to be painted after the layout changes.
    key(&mut app, &runtime, 81);
    key(&mut app, &runtime, 81);
    assert_eq!(app.ui.focus(), Focus::Card { row: 2, column: 2 });
    separated(
        &output(&mut app, &runtime),
        "Possessions",
        ["The Appointment", "1981", "1 h 29 min"],
    );
    assert_eq!(
        &*public.calls.lock().unwrap(),
        &["/", "/new", "/discover/newly-added"]
    );
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}

fn raw_rail() -> serde_json::Value {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/provider/discovery-new.json"
    ))
    .unwrap();
    data["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["id"] == 825)
        .unwrap()
        .clone()
}
fn supplied_new(public: &Public, blocks: Vec<serde_json::Value>) {
    *public.new_payload.lock().unwrap() = Some(serde_json::json!({"blocks": blocks}));
}
fn prepared(app: &mut App, row: usize) -> Command {
    let visit = app.controller.membership_visit();
    app.controller
        .view
        .with_view(app.authentication.view(), |mut data| {
            data.discovery_visit = visit;
            let action = data.rails[row].action.unwrap();
            Command::ActivateRail {
                origin: Page::New,
                from: data.rail_action_cursor(row).unwrap(),
                target: action.target.clone(),
            }
        })
}
fn pointer(app: &mut App, runtime: &Runtime, pressed: bool, pos: [i32; 2]) {
    app.event(
        Event::PointerButton {
            button: 1,
            pressed,
            x: pos[0],
            y: pos[1],
        },
        surface(),
        runtime,
        Duration::ZERO,
    );
    app.consume(runtime, Duration::ZERO);
    if let Some(mut output) = app.take_output() {
        output.textures_delta.clear();
    }
}
#[test]
fn actual_pointer_header_opens_exact_route_and_back_keeps_cards_artwork_and_address() {
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    new_page(&mut app, &runtime);
    for code in [81, 81] {
        key(&mut app, &runtime, code);
    }
    let before = app
        .controller
        .view
        .with_view(app.authentication.view(), |data| {
            data.rails[1]
                .cards
                .iter()
                .map(|card| (card.key.clone(), card.artwork_key.map(str::to_owned)))
                .collect::<Vec<_>>()
        });
    pointer(&mut app, &runtime, true, [1620, 282]);
    pointer(&mut app, &runtime, false, [1620, 282]);
    assert_eq!(app.ui.page(), Page::Discovery);
    settle(&mut app, &runtime);
    assert_eq!(
        &*public.calls.lock().unwrap(),
        &["/", "/new", "/discover/newly-added"]
    );
    key(&mut app, &runtime, 41);
    assert_eq!(
        app.ui.focus(),
        Focus::DiscoveryRailAction { row: 1, column: 0 }
    );
    let after = app
        .controller
        .view
        .with_view(app.authentication.view(), |data| {
            data.rails[1]
                .cards
                .iter()
                .map(|card| (card.key.clone(), card.artwork_key.map(str::to_owned)))
                .collect::<Vec<_>>()
        });
    assert_eq!(before, after);
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}
#[test]
fn stale_visit_wrong_origin_block_row_target_and_background_refuse_before_history() {
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    new_page(&mut app, &runtime);
    for code in [81, 81, 82] {
        key(&mut app, &runtime, code);
    }
    let original = prepared(&mut app, 1);
    let Command::ActivateRail {
        origin,
        from,
        target,
    } = original.clone()
    else {
        unreachable!()
    };
    let wrong = criterion_ui::Target::Content(
        criterion_provider::ContentTarget::parse("/discover/international-cinema").unwrap(),
    );
    for command in [
        Command::ActivateRail {
            origin: Page::Home,
            from,
            target: target.clone(),
        },
        Command::ActivateRail {
            origin,
            from: criterion_ui::RailActionCursor {
                visit: from.visit + 1,
                ..from
            },
            target: target.clone(),
        },
        Command::ActivateRail {
            origin,
            from: criterion_ui::RailActionCursor {
                block: from.block + 1,
                ..from
            },
            target: target.clone(),
        },
        Command::ActivateRail {
            origin,
            from: criterion_ui::RailActionCursor { row: 0, ..from },
            target: target.clone(),
        },
        Command::ActivateRail {
            origin,
            from,
            target: wrong,
        },
    ] {
        app.command(command, runtime.handle());
        assert_eq!(app.ui.page(), Page::New);
        assert_eq!(
            app.ui.focus(),
            Focus::DiscoveryRailAction { row: 1, column: 0 }
        );
        assert_eq!(public.calls.lock().unwrap().len(), 2);
    }
    app.command(original.clone(), runtime.handle());
    settle(&mut app, &runtime);
    key(&mut app, &runtime, 41);
    assert_eq!(app.ui.page(), Page::New);
    app.command(original, runtime.handle());
    assert_eq!(app.ui.page(), Page::New);
    assert_eq!(public.calls.lock().unwrap().len(), 3);
    // New/Home rail navigation itself adds no UI snapshot. After the one accepted
    // action is restored, refused commands must not have left another entry.
    key(&mut app, &runtime, 41);
    assert_eq!(app.ui.page(), Page::New);
    let current = prepared(&mut app, 1);
    app.background();
    app.command(current, runtime.handle());
    assert_eq!(app.ui.page(), Page::New);
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}
#[test]
fn action_only_ordinary_row_is_ready_and_exact_caption_survives_back() {
    let runtime = runtime();
    let public = Public::default();
    let mut raw = raw_rail();
    raw["playlist"] = serde_json::json!([]);
    raw["cta"] = "  See all  ".into();
    supplied_new(&public, vec![raw]);
    let mut app = fixture_app(&runtime, public.clone());
    new_page(&mut app, &runtime);
    app.controller
        .view
        .with_view(app.authentication.view(), |data| {
            assert_eq!(data.status, LoadState::Ready);
            assert!(data.rails[0].cards.is_empty());
            assert_eq!(data.rails[0].action.unwrap().label, "  See all  ");
        });
    key(&mut app, &runtime, 81);
    assert_eq!(
        app.ui.focus(),
        Focus::DiscoveryRailAction { row: 0, column: 0 }
    );
    key(&mut app, &runtime, 40);
    settle(&mut app, &runtime);
    assert_eq!(app.ui.page(), Page::Discovery);
    key(&mut app, &runtime, 41);
    assert_eq!(
        app.ui.focus(),
        Focus::DiscoveryRailAction { row: 0, column: 0 }
    );
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}
#[test]
fn incomplete_blank_new_window_and_account_fed_ctas_remain_unrepresented() {
    let runtime = runtime();
    let public = Public::default();
    let raw = raw_rail();
    let mut blocks = Vec::new();
    for (index, mode) in [
        "missing-cta",
        "blank",
        "missing-link",
        "window",
        "watchlist",
        "continueWatching",
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = raw.clone();
        value["id"] = (825 + index as u32).into();
        match mode {
            "missing-cta" => {
                value.as_object_mut().unwrap().remove("cta");
            }
            "blank" => value["cta"] = "   ".into(),
            "missing-link" => {
                value.as_object_mut().unwrap().remove("link");
            }
            "window" => value["linkTarget"] = 1.into(),
            source => {
                value["playlistType"] = source.into();
                value["playlist"] = serde_json::json!([]);
            }
        }
        blocks.push(value);
    }
    supplied_new(&public, blocks);
    let mut app = fixture_app(&runtime, public.clone());
    new_page(&mut app, &runtime);
    app.controller
        .view
        .with_view(app.authentication.view(), |data| {
            assert_eq!(data.rails.len(), 6);
            assert!(data.rails.iter().all(|rail| rail.action.is_none()));
        });
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}
#[test]
fn supplied_account_targets_cannot_become_public_rail_actions() {
    for (link, caption) in [
        ("/my-list", "Save to My List"),
        ("/subscribe", "Subscribe now"),
    ] {
        let runtime = runtime();
        let public = Public::default();
        let mut raw = raw_rail();
        raw["link"] = link.into();
        raw["cta"] = caption.into();
        supplied_new(&public, vec![raw]);
        let mut app = fixture_app(&runtime, public.clone());
        new_page(&mut app, &runtime);
        // The supplied, same-window Provided rail still has real cards. Its
        // account route must not become a remote header control.
        for code in [81, 82, 40] {
            key(&mut app, &runtime, code);
        }
        assert_eq!(app.ui.page(), Page::New, "account CTA opened {link}");
        let before = app
            .controller
            .view
            .with_view(app.authentication.view(), |data| {
                assert_eq!(data.status, LoadState::Ready);
                assert_eq!(data.rails.len(), 1);
                assert!(data.rails[0].action.is_none());
                assert!(!data.rails[0].cards.is_empty());
                data.rails[0]
                    .cards
                    .iter()
                    .map(|card| (card.key.clone(), card.artwork_key.map(str::to_owned)))
                    .collect::<Vec<_>>()
            });
        key(&mut app, &runtime, 81);
        assert!(!paint(&mut app, &runtime).iter().any(|text| text == caption));
        // A pointer at the normal scrolled row-zero header and a forged exact
        // row address must both refuse the unowned account action before Back.
        pointer(&mut app, &runtime, true, [1620, 282]);
        pointer(&mut app, &runtime, false, [1620, 282]);
        app.command(
            Command::ActivateRail {
                origin: Page::New,
                from: criterion_ui::RailActionCursor {
                    visit: app.controller.membership_visit().unwrap(),
                    block: 825,
                    row: 0,
                },
                target: criterion_ui::Target::Content(
                    criterion_provider::ContentTarget::parse(link).unwrap(),
                ),
            },
            runtime.handle(),
        );
        assert_eq!(app.ui.page(), Page::New);
        key(&mut app, &runtime, 41);
        assert_eq!(app.ui.page(), Page::New);
        let after = app
            .controller
            .view
            .with_view(app.authentication.view(), |data| {
                data.rails[0]
                    .cards
                    .iter()
                    .map(|card| (card.key.clone(), card.artwork_key.map(str::to_owned)))
                    .collect::<Vec<_>>()
            });
        assert_eq!(before, after);
        assert_eq!(&*public.calls.lock().unwrap(), &["/", "/new"]);
        assert!(app.finish(&runtime));
        assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn duplicate_block_and_target_rows_keep_distinct_addresses_and_owned_action_budget() {
    let runtime = runtime();
    let public = Public::default();
    let mut second = raw_rail();
    second["cta"] = "A".repeat(256).into();
    supplied_new(&public, vec![raw_rail(), second]);
    let mut app = fixture_app(&runtime, public.clone());
    new_page(&mut app, &runtime);
    let first = prepared(&mut app, 0);
    let second = prepared(&mut app, 1);
    let (
        Command::ActivateRail {
            from: first,
            target: a,
            ..
        },
        Command::ActivateRail {
            from: second,
            target: b,
            ..
        },
    ) = (first, second)
    else {
        unreachable!()
    };
    assert_eq!(first.block, second.block);
    assert_eq!(a, b);
    assert_ne!(first.row, second.row);
    let supplied = app.controller.view.estimated_bytes();
    let mut raw = raw_rail();
    raw.as_object_mut().unwrap().remove("cta");
    supplied_new(&public, vec![raw.clone(), raw]);
    let page = runtime
        .block_on(
            Catalog::with_transport(public.clone())
                .discovery(criterion_provider::DiscoveryRoute::New),
        )
        .unwrap();
    let without = crate::presentation::Presentation::discovery(page).estimated_bytes();
    assert!(
        supplied >= without + "See all".len() + 256 + 2 * "newly-added".len(),
        "history budget must charge both labels and typed target storage"
    );
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn action_only_row_is_reachable_when_the_single_supplied_hero_is_unavailable() {
    let runtime = runtime();
    let public = Public::default();
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/provider/discovery-new.json"
    ))
    .unwrap();
    let mut hero = source["blocks"][0].clone();
    let mut slide = hero["slides"][0].clone();
    slide.as_object_mut().unwrap().remove("cta");
    hero["slides"] = serde_json::json!([slide]);
    let mut rail = raw_rail();
    rail["playlist"] = serde_json::json!([]);
    supplied_new(&public, vec![hero, rail]);
    let mut app = fixture_app(&runtime, public.clone());
    new_page(&mut app, &runtime);
    paint(&mut app, &runtime);
    // The unavailable Hero is not an actionable dead end: its supplied ordinary
    // rail still has a real control, reachable from the current remote focus.
    if matches!(app.ui.focus(), Focus::Rail(_)) {
        key(&mut app, &runtime, 79);
    }
    if app.ui.focus() != (Focus::DiscoveryRailAction { row: 0, column: 0 }) {
        key(&mut app, &runtime, 81);
    }
    assert_eq!(
        app.ui.focus(),
        Focus::DiscoveryRailAction { row: 0, column: 0 }
    );
    key(&mut app, &runtime, 40);
    assert_eq!(app.ui.page(), Page::Discovery);
    settle(&mut app, &runtime);
    assert_eq!(
        &*public.calls.lock().unwrap(),
        &["/", "/new", "/discover/newly-added"]
    );
    assert!(app.finish(&runtime));
    assert_eq!(public.account_calls.load(Ordering::SeqCst), 0);
}

#[cfg(feature = "sdl")]
#[path = "discovery_actions_render_tests.rs"]
mod discovery_actions_render_tests;
