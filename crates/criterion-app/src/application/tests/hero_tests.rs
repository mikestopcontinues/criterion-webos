// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual CPU application input using captured public discovery; transports are offline.
use super::*;
use std::sync::Mutex;

#[derive(Clone)]
struct Public {
    calls: Arc<Mutex<Vec<String>>>,
    payload: Arc<Mutex<serde_json::Value>>,
}
impl Default for Public {
    fn default() -> Self {
        Self {
            calls: Arc::default(),
            payload: Arc::new(Mutex::new(
                serde_json::from_str(include_str!(
                    "../../../../../tests/fixtures/provider/discovery-home.json"
                ))
                .unwrap(),
            )),
        }
    }
}
impl RequestTransport for Public {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        self.calls
            .lock()
            .unwrap()
            .push(request.url.path().to_owned());
        if request.url.path() != "/" {
            return Err(Error::Unavailable);
        }
        let blocks = self.payload.lock().unwrap().clone();
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
fn fixture_app(runtime: &Runtime, public: Public) -> Application<Public, Offline> {
    let clock = SystemClock::default();
    let session = Arc::new(criterion_session::Session::with_transport(
        criterion_session::Configuration::production(),
        Offline,
        clock.clone(),
    ));
    Application::with_parts(
        surface(),
        Controller::new(Catalog::with_transport(public), runtime.handle()),
        Authentication::with_session(session.clone(), clock),
        Accounts::from_parts(
            Arc::new(criterion_account::AccountClient::with_transport(
                criterion_account::HttpTransport::new().unwrap(),
            )),
            session,
        ),
        Artwork::offline(),
    )
}
fn settle(app: &mut Application<Public, Offline>, runtime: &Runtime) {
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
    panic!("supplied Home did not become Ready");
}
fn key(app: &mut Application<Public, Offline>, runtime: &Runtime, scancode: u32) {
    app.event(
        Event::Key(KeyEvent {
            scancode,
            keycode: 0,
            pressed: true,
            repeat: false,
        }),
        surface(),
        runtime,
        Duration::ZERO,
    );
    if let Some(mut output) = app.take_output() {
        output.textures_delta.clear();
    }
    app.event(
        Event::Key(KeyEvent {
            scancode,
            keycode: 0,
            pressed: false,
            repeat: false,
        }),
        surface(),
        runtime,
        Duration::ZERO,
    );
    if let Some(mut output) = app.take_output() {
        output.textures_delta.clear();
    }
}
#[test]
fn manual_next_stays_on_home_and_opens_the_second_supplied_slide() {
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    key(&mut app, &runtime, 79); // Right: CTA -> Previous.
    key(&mut app, &runtime, 79); // Right: Previous -> Next.
    key(&mut app, &runtime, 40); // Select: advance once, keep arrow focus.
    assert_eq!(app.ui.page(), Page::Home);
    app.controller
        .view
        .with_view(app.authentication.view(), |data| {
            assert_eq!(
                data.hero.as_ref().unwrap().card.key,
                &criterion_ui::Target::Content(
                    criterion_provider::ContentTarget::parse(
                        "/collections/p753ts71/made-for-tv-terror"
                    )
                    .unwrap()
                )
            );
        });
    assert_eq!(&*public.calls.lock().unwrap(), &["/"]);
    let visit = app.controller.membership_visit().unwrap();
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 40);
    assert_eq!(app.ui.page(), Page::Detail);
    for _ in 0..1000 {
        if public.calls.lock().unwrap().len() == 2 {
            break;
        }
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    }
    assert_eq!(
        &*public.calls.lock().unwrap(),
        &["/", "/api/media/p753ts71"]
    );
    key(&mut app, &runtime, 41);
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(app.ui.focus(), Focus::Hero);
    assert_ne!(app.controller.membership_visit(), Some(visit));
    app.controller
        .view
        .with_view(app.authentication.view(), |data| {
            let carousel = data.hero_carousel.unwrap();
            assert_eq!(
                (carousel.index, carousel.slide, carousel.caption),
                (1, 1599, "Slide 2 of 7")
            );
        });
    assert_eq!(
        public.calls.lock().unwrap().len(),
        2,
        "warm Back performs no Home read"
    );
    assert!(app.finish(&runtime));
}

fn cursor(app: &Application<Public, Offline>) -> criterion_ui::HeroCursor {
    app.controller
        .view
        .hero_cursor(app.controller.membership_visit().unwrap())
        .unwrap()
}
#[test]
fn website_upper_bound_identity_survives_move_activation_and_warm_back() {
    let runtime = runtime();
    let public = Public::default();
    {
        let mut payload = public.payload.lock().unwrap();
        let slides = payload["blocks"][0]["slides"].as_array_mut().unwrap();
        slides.truncate(2);
        for slide in slides {
            slide["id"] = 4_294_967_295_u32.into();
        }
    }
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    let from = cursor(&app);
    assert_eq!((from.index, from.slide), (0, 4_294_967_295));
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 40);
    assert_eq!((cursor(&app).index, cursor(&app).slide), (1, 4_294_967_295));
    let target = app.controller.view.hero_target().unwrap().clone();
    app.command(
        Command::ActivateHero {
            origin: Page::Home,
            from,
            target,
        },
        runtime.handle(),
    );
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(&*public.calls.lock().unwrap(), &["/"]);
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 40);
    assert_eq!(app.ui.page(), Page::Detail);
    for _ in 0..1000 {
        if public.calls.lock().unwrap().len() == 2 {
            break;
        }
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    }
    assert_eq!(
        &*public.calls.lock().unwrap(),
        &["/", "/api/media/p753ts71"]
    );
    key(&mut app, &runtime, 41);
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(app.ui.focus(), Focus::Hero);
    assert_eq!((cursor(&app).index, cursor(&app).slide), (1, 4_294_967_295));
    assert_eq!(public.calls.lock().unwrap().len(), 2);
    assert!(app.finish(&runtime));
}
fn unavailable(public: &Public, count: usize) {
    let mut data = public.payload.lock().unwrap();
    let mut first = data["blocks"][0].clone();
    let source = first["slides"][0].clone();
    first["slides"] = serde_json::Value::Array(
        (0..count)
            .map(|index| {
                let mut slide = source.clone();
                if index == 0 {
                    slide["linkTarget"] = 1.into();
                } else {
                    slide.as_object_mut().unwrap().remove("cta");
                }
                slide
            })
            .collect(),
    );
    data["blocks"] = serde_json::json!([first, {"type":20,"id":2,"header":"Saved","playlistType":"continueWatching","imageJWLabel":"default_16x9","imageAspectRatio":56.25,"galleryPageNum":4,"galleryWrap":0}]);
}
#[test]
fn unavailable_raw_slots_wrap_without_targets_and_remain_ready_after_private_retirement() {
    let runtime = runtime();
    let public = Public::default();
    unavailable(&public, 2);
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    app.consume(&runtime, Duration::ZERO);
    if let Some(mut output) = app.take_output() {
        output.textures_delta.clear();
    }
    assert_eq!(app.ui.focus(), Focus::HeroPrevious);
    assert!(app.controller.view.artwork_bindings().is_empty());
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 40);
    assert_eq!(cursor(&app).index, 1);
    key(&mut app, &runtime, 40);
    assert_eq!(cursor(&app).index, 0);
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 40);
    assert_eq!(cursor(&app).index, 1);
    app.controller.set_account_session(Some(0));
    app.controller.view.mark_continue_watching_pending(0);
    let (shelf, _) = crate::continue_watching::ContinueWatchingShelf::from_admitted(
        criterion_account::ContinueWatching {
            playlist: Vec::new(),
            positions: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(
        app.controller.view.admit_continue_watching(0, &shelf),
        Ok(true)
    );
    app.command(Command::Logout, runtime.handle());
    app.controller
        .view
        .with_view(app.authentication.view(), |data| {
            assert_eq!(data.status, LoadState::Ready);
            assert_eq!(data.total, 0);
            assert!(data.hero.is_none());
            assert_eq!(data.hero_carousel.unwrap().caption, "Slide 2 of 2");
        });
    key(&mut app, &runtime, 40);
    assert_eq!(cursor(&app).index, 0);
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(&*public.calls.lock().unwrap(), &["/"]);
    assert!(app.finish(&runtime));
}
fn pointer(app: &mut Application<Public, Offline>, runtime: &Runtime, pressed: bool, x: i32) {
    app.event(
        Event::PointerButton {
            button: 1,
            pressed,
            x,
            y: 780,
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
fn same_identity_stale_pointer_and_typed_activation_leave_origin_and_history_unchanged() {
    let runtime = runtime();
    let public = Public::default();
    {
        let mut payload = public.payload.lock().unwrap();
        let first = payload["blocks"][0]["slides"][0].clone();
        payload["blocks"][0]["slides"] = serde_json::json!([first.clone(), first]);
    }
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    let from = cursor(&app);
    let target = app.controller.view.hero_target().unwrap().clone();
    pointer(&mut app, &runtime, true, 180);
    app.command(
        Command::MoveHero {
            page: Page::Home,
            from,
            direction: criterion_ui::HeroDirection::Next,
        },
        runtime.handle(),
    );
    pointer(&mut app, &runtime, false, 180);
    app.command(
        Command::ActivateHero {
            origin: Page::Home,
            from,
            target: target.clone(),
        },
        runtime.handle(),
    );
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(cursor(&app).index, 1);
    pointer(&mut app, &runtime, true, 180);
    let old = cursor(&app);
    app.command(Command::Navigate(Page::Home), runtime.handle());
    settle(&mut app, &runtime);
    assert_eq!(cursor(&app).index, 0);
    assert_ne!(cursor(&app).visit, old.visit);
    pointer(&mut app, &runtime, false, 180);
    app.command(
        Command::ActivateHero {
            origin: Page::Home,
            from: old,
            target,
        },
        runtime.handle(),
    );
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(
        public.calls.lock().unwrap().len(),
        2,
        "only the explicit Home reload reads"
    );
    key(&mut app, &runtime, 41);
    assert!(
        app.exiting(),
        "rejected activation must not create an invisible Back origin"
    );
    assert!(app.finish(&runtime));
}

#[test]
fn every_duplicate_raw_ordinal_survives_and_wraps_at_the_admitted_32_slide_limit() {
    let runtime = runtime();
    let public = Public::default();
    {
        let mut payload = public.payload.lock().unwrap();
        let source = payload["blocks"][0]["slides"][0].clone();
        payload["blocks"][0]["slides"] = serde_json::Value::Array(vec![source; 32]);
    }
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 40);
    assert_eq!((cursor(&app).index, cursor(&app).slide), (31, 1595));
    app.controller
        .view
        .with_view(app.authentication.view(), |data| {
            assert_eq!(data.hero_carousel.unwrap().caption, "Slide 32 of 32")
        });
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 40);
    assert_eq!(cursor(&app).index, 0);
    assert_eq!(&*public.calls.lock().unwrap(), &["/"]);
    assert!(app.finish(&runtime));
}

#[test]
fn private_retirement_retains_inactive_public_sources_and_current_selection() {
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    let sources: Vec<_> = app
        .controller
        .view
        .artwork_bindings()
        .iter()
        .map(|b| (b.key.clone(), b.source.clone()))
        .collect();
    assert!(sources.len() > 2);
    app.controller.set_account_session(Some(0));
    app.controller.view.mark_continue_watching_pending(0);
    let (shelf, _) = crate::continue_watching::ContinueWatchingShelf::from_admitted(
        criterion_account::ContinueWatching {
            playlist: Vec::new(),
            positions: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(
        app.controller.view.admit_continue_watching(0, &shelf),
        Ok(true)
    );
    app.command(Command::Logout, runtime.handle());
    assert_eq!(
        app.controller
            .view
            .artwork_bindings()
            .iter()
            .map(|b| (b.key.clone(), b.source.clone()))
            .collect::<Vec<_>>(),
        sources
    );
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 40);
    assert_eq!((cursor(&app).index, cursor(&app).slide), (1, 1599));
    assert_eq!(&*public.calls.lock().unwrap(), &["/"]);
    assert!(app.finish(&runtime));
}

#[test]
fn old_pre_input_artwork_demand_is_retired_before_poll_and_never_prefetches_hidden_slides() {
    shared_artwork_demand(false);
}

#[test]
fn a_current_visible_card_can_keep_its_shared_source_when_the_hero_moves() {
    shared_artwork_demand(true);
}

fn shared_artwork_demand(visible: bool) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Flight(Arc<AtomicUsize>);
    impl Drop for Flight {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let runtime = runtime();
    let public = Public::default();
    {
        let mut payload = public.payload.lock().unwrap();
        payload["blocks"] = serde_json::json!([payload["blocks"][0].clone()]);
    }
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    // A supplied but offscreen navigation card shares the old hero image.
    // Retained source presence must not turn it into current artwork demand.
    let mut page = runtime
        .block_on(
            Catalog::with_transport(public).discovery(criterion_provider::DiscoveryRoute::Home),
        )
        .unwrap();
    let criterion_provider::DiscoveryBlock::Slideshow { slides, .. } = &page.blocks[0] else {
        panic!("first supplied slideshow")
    };
    let shared = slides[0].artwork.clone();
    let gallery = criterion_provider::GalleryPresentation {
        aspect_ratio_percent: 56.25,
        cards_per_view: 4,
        layout: criterion_provider::GalleryLayout::Rail,
        variant: 0,
    };
    if !visible {
        page.blocks
            .push(criterion_provider::DiscoveryBlock::Navigation {
                id: 800,
                header: Some("Empty supplied row".into()),
                items: vec![],
                presentation: gallery,
            });
    }
    page.blocks
        .push(criterion_provider::DiscoveryBlock::Navigation {
            id: 801,
            header: Some("Offscreen supplied row".into()),
            items: vec![criterion_provider::DiscoveryNavItem {
                id: 802,
                label: "Shared source".into(),
                target: criterion_provider::ContentTarget::New,
                opens_new_window: false,
                artwork: shared,
            }],
            presentation: gallery,
        });
    app.controller.view = crate::presentation::Presentation::discovery(page);
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let dropped = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Notify::new());
    app.artwork = Artwork::with_loader(Arc::new({
        let calls = calls.clone();
        let dropped = dropped.clone();
        let release = release.clone();
        move |source| {
            let calls = calls.clone();
            let dropped = dropped.clone();
            let release = release.clone();
            Box::pin(async move {
                let criterion_artwork::ArtworkSource::Editorial(image) = &source else {
                    panic!("hero-only source must be editorial")
                };
                let url = image.url().to_string();
                calls.lock().unwrap().push(url);
                let _flight = Flight(dropped);
                release.notified().await;
                criterion_artwork::decode_artwork(
                    source.role(),
                    "image/png",
                    include_bytes!("../../../../criterion-artwork/tests/fixtures/two-pixels.png"),
                )
            })
        }
    }));
    app.consume(&runtime, Duration::ZERO);
    if let Some(mut output) = app.take_output() {
        output.textures_delta.clear();
    }
    for _ in 0..1000 {
        if calls.lock().unwrap().len() == 2 {
            break;
        }
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    }
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "only current backdrop/logo start"
    );
    let old: Vec<_> = app
        .controller
        .view
        .with_view(app.authentication.view(), |data| {
            let hero = data.hero.unwrap();
            [hero.background_key.unwrap(), hero.title_logo_key.unwrap()]
                .map(str::to_owned)
                .to_vec()
        });
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 79);
    app.event(
        Event::Key(KeyEvent {
            scancode: 40,
            keycode: 0,
            pressed: true,
            repeat: false,
        }),
        surface(),
        &runtime,
        Duration::ZERO,
    );
    if let Some(mut output) = app.take_output() {
        output.textures_delta.clear();
    }
    assert_eq!(cursor(&app).index, 1);
    for _ in 0..1000 {
        if dropped.load(Ordering::SeqCst) == if visible { 1 } else { 2 } {
            break;
        }
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    }
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        if visible { 1 } else { 2 },
        "old hero demand retires; an actual visible shared card still owns its source"
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "old pre-input keys cannot restart or preload next source"
    );
    release.notify_waiters();
    for _ in 0..1000 {
        app.consume(&runtime, Duration::ZERO);
        if let Some(mut output) = app.take_output() {
            output.textures_delta.clear();
        }
        if calls.lock().unwrap().len() == 4 {
            break;
        }
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    }
    assert!(
        !app.ui.has_image(&old[1]),
        "the previous logo has no current owner"
    );
    if !visible {
        assert!(!app.ui.has_image(&old[0]));
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 4);
    assert!(
        calls[2..]
            .iter()
            .all(|url| url.contains("MADE-FOR-TV_TERROR")),
        "only newly current supplied images start"
    );
    drop(calls);
    assert!(app.finish(&runtime));
}

#[test]
fn reserved_inactive_source_capacity_evicts_history_and_cold_back_reloads_ordinal_zero() {
    use criterion_provider::{
        DiscoveryArtwork, DiscoveryBlock, DiscoveryPage, DiscoverySlide, EditorialImage,
        ResponsiveImage,
    };
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    let mut reserved = String::with_capacity(9 * 1024 * 1024);
    reserved.push_str("Supplied title");
    let artwork = DiscoveryArtwork {
        desktop: vec![ResponsiveImage {
            width: 1920,
            image: EditorialImage::new(
                "https://cc.criterion.com/uploads/storyBlocks/493/thumbnails/",
                "POSSESSIONS_collection_hero_wide_1920x0.webp",
            )
            .unwrap(),
        }],
        mobile: Vec::new(),
        logo: None,
    };
    let mut slides = Vec::new();
    for (id, title, link) in [
        (1595, reserved, "/collections/fhQRpxw4/possessions"),
        (
            1599,
            "Second title".into(),
            "/collections/p753ts71/made-for-tv-terror",
        ),
    ] {
        slides.push(DiscoverySlide {
            id,
            title: Some(title),
            title_prefix: None,
            cta: Some("See more".into()),
            target: Some(criterion_provider::ContentTarget::parse(link).unwrap()),
            opens_new_window: false,
            artwork: artwork.clone(),
        });
    }
    app.controller.view = crate::presentation::Presentation::discovery(DiscoveryPage {
        blocks: vec![DiscoveryBlock::Slideshow { id: 493, slides }],
    });
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 40);
    assert_eq!(cursor(&app).index, 1);
    assert!(
        app.controller.view.estimated_bytes() > 8 * 1024 * 1024,
        "inactive owned capacity must count against the actual history budget"
    );
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 40);
    for _ in 0..1000 {
        if public.calls.lock().unwrap().len() == 2 {
            break;
        }
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    }
    key(&mut app, &runtime, 41);
    settle(&mut app, &runtime);
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!((cursor(&app).index, cursor(&app).slide), (0, 1595));
    assert_eq!(
        &*public.calls.lock().unwrap(),
        &["/", "/api/media/p753ts71", "/"]
    );
    assert!(app.finish(&runtime));
}

#[test]
fn a_single_unavailable_slide_has_no_ghost_arrow_or_cta() {
    let runtime = runtime();
    let public = Public::default();
    unavailable(&public, 1);
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    pointer(&mut app, &runtime, true, 561);
    pointer(&mut app, &runtime, false, 561);
    assert!(matches!(app.ui.focus(), Focus::Rail(_)));
    assert_eq!(cursor(&app).index, 0);
    app.command(
        Command::MoveHero {
            page: Page::Home,
            from: cursor(&app),
            direction: criterion_ui::HeroDirection::Next,
        },
        runtime.handle(),
    );
    assert_eq!(cursor(&app).index, 0);
    assert_eq!(&*public.calls.lock().unwrap(), &["/"]);
    assert!(app.finish(&runtime));
}

#[test]
fn background_retires_pointer_and_typed_commands_but_preserves_the_selected_public_slot() {
    let runtime = runtime();
    let public = Public::default();
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 40);
    let from = cursor(&app);
    let target = app.controller.view.hero_target().unwrap().clone();
    pointer(&mut app, &runtime, true, 180);
    app.background();
    app.command(
        Command::MoveHero {
            page: Page::Home,
            from,
            direction: criterion_ui::HeroDirection::Next,
        },
        runtime.handle(),
    );
    app.command(
        Command::ActivateHero {
            origin: Page::Home,
            from,
            target: target.clone(),
        },
        runtime.handle(),
    );
    assert_eq!(app.ui.page(), Page::Home);
    app.foreground(runtime.handle());
    assert_ne!(cursor(&app).visit, from.visit);
    pointer(&mut app, &runtime, false, 180);
    app.command(
        Command::ActivateHero {
            origin: Page::Home,
            from,
            target,
        },
        runtime.handle(),
    );
    assert_eq!(
        (app.ui.page(), cursor(&app).index, cursor(&app).slide),
        (Page::Home, 1, 1599)
    );
    assert_eq!(&*public.calls.lock().unwrap(), &["/"]);
    key(&mut app, &runtime, 41);
    assert!(
        app.exiting(),
        "retired activation cannot create a history entry"
    );
    assert!(app.finish(&runtime));
}

#[test]
fn supplied_nonmedia_cta_commits_the_destination_then_warm_back_restores_its_origin() {
    let runtime = runtime();
    let public = Public::default();
    {
        let mut payload = public.payload.lock().unwrap();
        payload["blocks"][0]["slides"][1]["link"] = "/new".into();
    }
    let mut app = fixture_app(&runtime, public.clone());
    settle(&mut app, &runtime);
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 79);
    key(&mut app, &runtime, 40);
    let from = cursor(&app);
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 80);
    key(&mut app, &runtime, 40);
    assert_eq!(app.ui.page(), Page::New);
    for _ in 0..1000 {
        if public.calls.lock().unwrap().len() == 2 {
            break;
        }
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    }
    assert_eq!(&*public.calls.lock().unwrap(), &["/", "/new"]);
    key(&mut app, &runtime, 41);
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(
        (cursor(&app).index, cursor(&app).slide),
        (from.index, from.slide)
    );
    assert_ne!(cursor(&app).visit, from.visit);
    assert_eq!(&*public.calls.lock().unwrap(), &["/", "/new"]);
    assert!(app.finish(&runtime));
}

#[cfg(feature = "sdl")]
#[path = "hero_render_tests.rs"]
mod hero_render_tests;
