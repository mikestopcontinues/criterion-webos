// SPDX-License-Identifier: GPL-3.0-or-later
//! CPU application journeys through real Session/Account adapters and synthetic transports.
use super::*;
use crate::presentation::Presentation;
use criterion_account::{AccountClient, Region, SecretBody, Target};
use criterion_platform::{KeyEvent, Size};
use criterion_provider::{
    ContentTarget, DiscoveryArtwork, DiscoveryBlock, DiscoveryNavItem, DiscoveryPage,
    GalleryLayout, GalleryPresentation,
};
use criterion_session::{Configuration, Endpoint, Session};
use criterion_ui::{Action, LoginView, Page};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

struct Offline;
impl RequestTransport for Offline {
    async fn get(
        &self,
        _: criterion_provider::Request,
    ) -> Result<criterion_provider::Response, criterion_provider::Error> {
        Err(criterion_provider::Error::Unavailable)
    }
}
#[derive(Clone)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Clone, Default)]
struct Issuer {
    fail: Arc<AtomicBool>,
    revokes: Arc<AtomicUsize>,
    tokens: Arc<AtomicUsize>,
    hold_refresh: Arc<AtomicBool>,
    release: Arc<Notify>,
}
impl Transport for Issuer {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        let body = match request.endpoint {
            Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
            Endpoint::Token => {
                self.tokens.fetch_add(1, Ordering::SeqCst);
                if self.hold_refresh.swap(false, Ordering::SeqCst) {
                    self.release.notified().await;
                }
                if self.fail.load(Ordering::SeqCst) { return Err(criterion_session::Error::Unavailable); }
                br#"{"access_token":"synthetic-same-token","refresh_token":"synthetic-refresh","expires_in":3600}"#.to_vec()
            },
            Endpoint::Revoke => { self.revokes.fetch_add(1, Ordering::SeqCst); b"{}".to_vec() },
        };
        Ok(criterion_session::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
#[derive(Clone, Default)]
struct Middleware {
    calls: Arc<Mutex<Vec<Target>>>,
    hold: Arc<AtomicBool>,
    retired: Arc<AtomicUsize>,
    release: Arc<Notify>,
}
struct Retire(Arc<AtomicUsize>);
impl Drop for Retire {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl criterion_account::Transport for Middleware {
    async fn send(
        &self,
        request: criterion_account::Request,
    ) -> Result<criterion_account::Response, criterion_account::Error> {
        self.calls.lock().unwrap().push(request.target.clone());
        let body = match request.target {
            Target::Bootstrap => {
                assert!(request.credentials.is_none());
                br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()
            }
            Target::WatchList {
                region: Region::Us, ..
            } => {
                if self.hold.swap(false, Ordering::SeqCst) {
                    let _retire = Retire(self.retired.clone());
                    self.release.notified().await;
                }
                br#"{"paging":{"page_limit":60},"type_counts":{"film":1},"playlist":[{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic private selection","duration":90.5}]}"#.to_vec()
            }
            _ => panic!("read-only application must not issue a mutation"),
        };
        if let Some(credentials) = request.credentials {
            assert_eq!(
                credentials.subscriber().to_str().unwrap(),
                "synthetic-same-token"
            );
        }
        Ok(criterion_account::Response {
            status: 200,
            body: SecretBody::new(body),
        })
    }
}
type App = Application<Offline, Issuer, Clock, Middleware>;
fn fixture(signed_in: bool) -> (App, Runtime, Clock, Issuer, Middleware) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let clock = Clock(Arc::new(AtomicU64::new(0)));
    let issuer = Issuer::default();
    let session = Arc::new(Session::with_transport(
        Configuration::production(),
        issuer.clone(),
        clock.clone(),
    ));
    if signed_in {
        runtime.block_on(session.start_link()).unwrap();
        clock.0.store(5, Ordering::SeqCst);
        runtime.block_on(session.poll_once()).unwrap();
    }
    let middleware = Middleware::default();
    let authentication = Authentication::with_session(session.clone(), clock.clone());
    let accounts = Accounts::from_parts(
        Arc::new(AccountClient::with_transport(middleware.clone())),
        session,
    );
    let surface = Surface {
        window: Size {
            width: 1920,
            height: 1080,
        },
        drawable: Size {
            width: 1920,
            height: 1080,
        },
    };
    let mut app = Application::with_parts(
        surface,
        Controller::new(Catalog::with_transport(Offline), runtime.handle()),
        authentication,
        accounts,
        Artwork::offline(),
    );
    // Drain the real initial public read before installing the synthetic Home projection.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !app
        .controller
        .view
        .with_view(LoginView::SignedOut, |v| v.status == LoadState::Offline)
    {
        app.controller.poll(&runtime);
        runtime.block_on(tokio::task::yield_now());
        assert!(std::time::Instant::now() < deadline);
    }
    app.controller.background();
    app.controller.view = Presentation::discovery(DiscoveryPage {
        blocks: vec![DiscoveryBlock::Navigation {
            id: 1,
            header: None,
            presentation: GalleryPresentation {
                aspect_ratio_percent: 56.25,
                cards_per_view: 4,
                layout: GalleryLayout::Rail,
                variant: 0,
            },
            items: vec![DiscoveryNavItem {
                id: 1,
                label: "My List".into(),
                target: ContentTarget::MyList,
                opens_new_window: false,
                artwork: DiscoveryArtwork {
                    desktop: vec![],
                    mobile: vec![],
                    logo: None,
                },
            }],
        }],
    });
    (app, runtime, clock, issuer, middleware)
}
fn action(app: &mut App, runtime: &Runtime, action: Action) {
    let commands = app
        .controller
        .view
        .with_view(app.authentication.view(), |data| {
            app.ui.handle(action, data)
        });
    for command in commands {
        app.command(command, runtime.handle());
    }
}
fn pump_until(app: &mut App, runtime: &Runtime, predicate: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !predicate(app) {
        app.poll(runtime, true);
        runtime.block_on(tokio::task::yield_now());
        assert!(
            std::time::Instant::now() < deadline,
            "bounded CPU journey did not settle"
        );
    }
}
fn open_list(app: &mut App, runtime: &Runtime) {
    action(app, runtime, Action::Down);
    action(app, runtime, Action::Select);
}
fn native_key(app: &mut App, runtime: &Runtime, key: (u32, i32)) {
    let surface = Surface {
        window: Size {
            width: 1920,
            height: 1080,
        },
        drawable: Size {
            width: 1920,
            height: 1080,
        },
    };
    for pressed in [true, false] {
        app.event(
            Event::Key(KeyEvent {
                scancode: key.0,
                keycode: key.1,
                pressed,
                repeat: false,
            }),
            surface,
            runtime,
            Duration::ZERO,
        );
        if let Some(mut output) = app.output.take() {
            output.textures_delta.clear();
        }
    }
}

#[test]
fn native_subscriber_rail_reads_list_without_home_cards_and_back_restores_display() {
    for status in [LoadState::Empty, LoadState::Offline] {
        let (mut app, runtime, _, issuer, middleware) = fixture(true);
        app.controller.view = Presentation::loading("Fixture Home without navigation");
        app.controller.view.set_status(status);
        let origin_focus = app.ui.focus();
        for key in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
        ] {
            native_key(&mut app, &runtime, key);
        }
        assert_eq!(app.ui.page(), Page::MyList);
        pump_until(&mut app, &runtime, |app| {
            app.controller
                .view
                .with_view(LoginView::SignedIn, |view| view.status == LoadState::Ready)
        });
        assert_eq!(
            *middleware.calls.lock().unwrap(),
            vec![
                Target::Bootstrap,
                Target::WatchList {
                    region: Region::Us,
                    request: criterion_account::WatchListRequest::default()
                }
            ]
        );
        native_key(&mut app, &runtime, (41, 27));
        assert_eq!(app.ui.page(), Page::Home);
        assert_eq!(app.ui.focus(), origin_focus);
        app.controller.view.with_view(LoginView::SignedIn, |view| {
            assert_eq!(view.title, "Fixture Home without navigation");
            assert_eq!(view.status, status);
            assert!(view.cards.is_empty() && view.rails.is_empty());
        });
        assert!(app.authentication.signed_in());
        app.command(Command::Logout, runtime.handle());
        app.background();
        assert!(app.finish(&runtime));
        assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn native_account_to_my_list_to_explicit_logout_ignores_loading_home() {
    let (mut app, runtime, _, issuer, middleware) = fixture(true);
    app.controller.view = Presentation::loading("Unresolved Home without navigation");
    // The same admitted native-key route as the live subscriber test.
    for key in [
        (80, 1_073_741_904),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (40, 13),
    ] {
        native_key(&mut app, &runtime, key);
    }
    assert_eq!(app.ui.page(), Page::Login);
    assert_eq!(app.ui.focus(), criterion_ui::Focus::LoginPrimary);
    for key in [
        (80, 1_073_741_904),
        (82, 1_073_741_906),
        (82, 1_073_741_906),
        (40, 13),
    ] {
        native_key(&mut app, &runtime, key);
    }
    assert_eq!(app.ui.page(), Page::MyList);
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |view| view.status == LoadState::Ready)
    });
    assert_eq!(
        *middleware.calls.lock().unwrap(),
        vec![
            Target::Bootstrap,
            Target::WatchList {
                region: Region::Us,
                request: criterion_account::WatchListRequest::default()
            }
        ]
    );
    for key in [
        (80, 1_073_741_904),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (40, 13),
        (40, 13),
    ] {
        native_key(&mut app, &runtime, key);
    }
    assert!(!app.authentication.signed_in());
    app.background();
    assert!(app.finish(&runtime));
    assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
    native_key(&mut app, &runtime, (41, 27));
    assert_eq!(app.ui.page(), Page::Login);
    app.controller
        .view
        .with_view(app.authentication.view(), |view| {
            assert!(view.cards.is_empty() && view.rails.is_empty());
        });
    assert_eq!(middleware.calls.lock().unwrap().len(), 2);
    assert!(matches!(app.authentication.view(), LoginView::SignedOut));
    native_key(&mut app, &runtime, (41, 27));
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(
        app.controller.view.title(),
        "Unresolved Home without navigation"
    );
    assert!(app.finish(&runtime));
    assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
}

#[test]
fn native_logout_back_cannot_relink_a_departed_list_or_trap_cancel() {
    let (mut app, runtime, _, issuer, middleware) = fixture(true);
    app.controller.view = Presentation::loading("Public origin after list logout");
    app.controller.view.set_status(LoadState::Empty);
    let origin = app.ui.focus();
    for key in [
        (80, 1_073_741_904),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (40, 13),
    ] {
        native_key(&mut app, &runtime, key);
    }
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |view| view.status == LoadState::Ready)
    });
    for key in [
        (80, 1_073_741_904),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (40, 13),
        (40, 13),
    ] {
        native_key(&mut app, &runtime, key);
    }
    pump_until(&mut app, &runtime, |app| {
        matches!(app.authentication.view(), LoginView::SignedOut)
    });
    native_key(&mut app, &runtime, (41, 27));
    let relinked = matches!(
        app.authentication.view(),
        LoginView::Requesting | LoginView::Awaiting { .. }
    );
    // Continue cancellation through the real UI if an inaccessible restore
    // started activation; this exposed the former repeated My List loop.
    if app.ui.page() == Page::Login {
        native_key(&mut app, &runtime, (41, 27));
    }
    assert_eq!(app.ui.page(), Page::Home);
    assert!(!relinked, "Back must not begin another authorization");
    assert_eq!(app.ui.focus(), origin);
    assert!(matches!(app.authentication.view(), LoginView::SignedOut));
    app.controller
        .view
        .with_view(app.authentication.view(), |view| {
            assert_eq!(view.title, "Public origin after list logout");
            assert_eq!(view.status, LoadState::Empty);
            assert!(view.cards.is_empty() && view.rails.is_empty());
        });
    assert_eq!(middleware.calls.lock().unwrap().len(), 2);
    assert_eq!(issuer.tokens.load(Ordering::SeqCst), 1);
    assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
    app.background();
    assert!(app.finish(&runtime));
}

#[test]
fn native_logout_preserves_public_home_history_across_departed_list() {
    let (mut app, runtime, _, issuer, middleware) = fixture(true);
    app.controller.view = Presentation::loading("Original public Home");
    app.controller.view.set_status(LoadState::Empty);
    let origin = app.ui.focus();
    for key in [
        (80, 1_073_741_904),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (40, 13),
    ] {
        native_key(&mut app, &runtime, key);
    }
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |view| view.status == LoadState::Ready)
    });
    for key in [
        (80, 1_073_741_904),
        (82, 1_073_741_906),
        (82, 1_073_741_906),
        (40, 13),
    ] {
        native_key(&mut app, &runtime, key);
    }
    assert_eq!(app.ui.page(), Page::Home);
    pump_until(&mut app, &runtime, |app| {
        app.controller.view.status() == LoadState::Offline
    });
    for key in [
        (80, 1_073_741_904),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (81, 1_073_741_905),
        (40, 13),
        (40, 13),
    ] {
        native_key(&mut app, &runtime, key);
    }
    pump_until(&mut app, &runtime, |app| {
        matches!(app.authentication.view(), LoginView::SignedOut)
    });
    native_key(&mut app, &runtime, (41, 27));
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(app.controller.view.title(), "Home");
    assert_eq!(app.controller.view.status(), LoadState::Offline);
    native_key(&mut app, &runtime, (41, 27));
    assert_eq!(app.ui.page(), Page::Home);
    assert_eq!(app.ui.focus(), origin);
    assert_eq!(app.controller.view.title(), "Original public Home");
    assert_eq!(app.controller.view.status(), LoadState::Empty);
    assert!(matches!(app.authentication.view(), LoginView::SignedOut));
    assert_eq!(middleware.calls.lock().unwrap().len(), 2);
    assert_eq!(issuer.tokens.load(Ordering::SeqCst), 1);
    assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
    app.background();
    assert!(app.finish(&runtime));
}

#[test]
fn native_failed_refresh_then_login_cancel_or_home_back_retains_public_origin() {
    for via_login in [true, false] {
        let (mut app, runtime, clock, issuer, middleware) = fixture(true);
        app.controller.view = Presentation::loading("Public origin before failed refresh");
        app.controller.view.set_status(LoadState::Empty);
        let origin = app.ui.focus();
        for key in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
        ] {
            native_key(&mut app, &runtime, key);
        }
        pump_until(&mut app, &runtime, |app| {
            app.controller.view.status() == LoadState::Ready
        });
        issuer.fail.store(true, Ordering::SeqCst);
        clock.0.store(4000, Ordering::SeqCst);
        pump_until(&mut app, &runtime, |app| {
            matches!(app.authentication.view(), LoginView::Error)
        });
        assert_eq!(app.ui.page(), Page::MyList);
        if via_login {
            for key in [(80, 1_073_741_904), (40, 13)] {
                native_key(&mut app, &runtime, key);
            }
            pump_until(&mut app, &runtime, |app| {
                matches!(app.authentication.view(), LoginView::Awaiting { .. })
            });
        } else {
            for key in [
                (80, 1_073_741_904),
                (82, 1_073_741_906),
                (82, 1_073_741_906),
                (82, 1_073_741_906),
                (40, 13),
            ] {
                native_key(&mut app, &runtime, key);
            }
            pump_until(&mut app, &runtime, |app| {
                app.controller.view.status() == LoadState::Offline
            });
        }
        native_key(&mut app, &runtime, (41, 27));
        assert_eq!(app.ui.page(), Page::Home);
        assert_eq!(app.ui.focus(), origin);
        assert!(if via_login {
            matches!(app.authentication.view(), LoginView::SignedOut)
        } else {
            matches!(app.authentication.view(), LoginView::Error)
        });
        app.controller
            .view
            .with_view(app.authentication.view(), |view| {
                assert_eq!(view.title, "Public origin before failed refresh");
                assert_eq!(view.status, LoadState::Empty);
                assert!(view.cards.is_empty() && view.rails.is_empty());
            });
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        assert_eq!(issuer.tokens.load(Ordering::SeqCst), 2);
        assert_eq!(issuer.revokes.load(Ordering::SeqCst), 0);
        app.background();
        assert!(app.finish(&runtime));
    }
}

#[test]
fn signed_subscriber_reads_native_shelf_and_renders_real_adapter_projection() {
    let (mut app, runtime, _, _, middleware) = fixture(true);
    open_list(&mut app, &runtime);
    assert_eq!(app.ui.page(), Page::MyList);
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    app.controller
        .view
        .with_view(app.authentication.view(), |view| {
            assert_eq!(view.cards[0].key.media_id().unwrap().as_str(), "AbCd1234");
            assert_eq!(view.cards[0].title, "Synthetic private selection");
            assert_eq!(view.cards[0].duration_seconds, 0);
        });
    assert_eq!(
        *middleware.calls.lock().unwrap(),
        vec![
            Target::Bootstrap,
            Target::WatchList {
                region: Region::Us,
                request: criterion_account::WatchListRequest::default()
            }
        ]
    );
    assert!(app.finish(&runtime));
}
#[test]
fn unsigned_my_list_enters_linking_without_any_account_contact() {
    let (mut app, runtime, _, _, middleware) = fixture(false);
    open_list(&mut app, &runtime);
    assert_eq!(app.ui.page(), Page::Login);
    assert!(app.shelf_pending.is_none());
    app.poll(&runtime, true);
    assert!(middleware.calls.lock().unwrap().is_empty());
    app.authentication.cancel();
    assert!(app.finish(&runtime));
}
#[test]
fn logout_clears_private_views_before_revoke_and_retains_safe_back_navigation() {
    let (mut app, runtime, _, issuer, _) = fixture(true);
    open_list(&mut app, &runtime);
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    app.consume(&runtime, Duration::ZERO);
    let contains_private = |shapes: &[egui::epaint::ClippedShape]| {
        shapes.iter().any(|shape|
        matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains("Synthetic private selection")))
    };
    assert!(
        contains_private(&app.output.as_ref().unwrap().shapes),
        "the fixture must first paint the private card"
    );
    for key in [
        Action::Left,
        Action::Down,
        Action::Down,
        Action::Down,
        Action::Select,
    ] {
        action(&mut app, &runtime, key);
    }
    assert_eq!(app.ui.page(), Page::Login);
    action(&mut app, &runtime, Action::Select);
    assert!(!app.authentication.signed_in());
    assert!(
        !contains_private(&app.output.as_ref().unwrap().shapes),
        "logout must also discard an unpainted private frame"
    );
    assert!(app.finish(&runtime));
    assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
    action(&mut app, &runtime, Action::Back);
    app.controller
        .view
        .with_view(app.authentication.view(), |view| {
            assert!(view.cards.is_empty())
        });
}
#[test]
fn background_retires_shelf_and_foreground_restarts_only_current_intent() {
    let (mut app, runtime, _, _, middleware) = fixture(true);
    middleware.hold.store(true, Ordering::SeqCst);
    open_list(&mut app, &runtime);
    pump_until(&mut app, &runtime, |_| {
        middleware.calls.lock().unwrap().len() == 2
    });
    app.background();
    pump_until(&mut app, &runtime, |_| {
        middleware.retired.load(Ordering::SeqCst) == 1
    });
    assert_eq!(middleware.calls.lock().unwrap().len(), 2);
    app.foreground(runtime.handle());
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    assert_eq!(middleware.calls.lock().unwrap().len(), 3);
    assert!(app.finish(&runtime));
}
#[test]
fn failed_refresh_invalidates_visible_private_state() {
    let (mut app, runtime, clock, issuer, _) = fixture(true);
    open_list(&mut app, &runtime);
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    let epoch = app.account_epoch;
    issuer.fail.store(true, Ordering::SeqCst);
    clock.0.store(4000, Ordering::SeqCst);
    pump_until(&mut app, &runtime, |app| app.account_epoch != epoch);
    assert_ne!(app.account_epoch, epoch);
    app.controller
        .view
        .with_view(app.authentication.view(), |view| {
            assert!(view.cards.is_empty())
        });
    assert!(app.finish(&runtime));
}
#[test]
fn expiring_shelf_read_waits_for_refresh_then_resumes_the_current_view() {
    let (mut app, runtime, clock, issuer, middleware) = fixture(true);
    middleware.hold.store(true, Ordering::SeqCst);
    open_list(&mut app, &runtime);
    pump_until(&mut app, &runtime, |_| {
        middleware.calls.lock().unwrap().len() == 2
    });
    issuer.hold_refresh.store(true, Ordering::SeqCst);
    clock.0.store(4000, Ordering::SeqCst);
    pump_until(&mut app, &runtime, |_| {
        issuer.tokens.load(Ordering::SeqCst) == 2
    });
    assert_eq!(middleware.calls.lock().unwrap().len(), 2);
    issuer.release.notify_one();
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    assert_eq!(middleware.calls.lock().unwrap().len(), 3);
    assert!(app.finish(&runtime));
}
#[test]
fn identical_token_reauthentication_requires_a_fresh_list_intent_in_the_new_epoch() {
    let (mut app, runtime, clock, issuer, middleware) = fixture(true);
    open_list(&mut app, &runtime);
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    let original = app.account_epoch;
    for key in [
        Action::Left,
        Action::Down,
        Action::Down,
        Action::Down,
        Action::Select,
        Action::Select,
    ] {
        action(&mut app, &runtime, key);
    }
    pump_until(&mut app, &runtime, |app| {
        matches!(app.authentication.view(), LoginView::SignedOut)
    });
    assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
    action(&mut app, &runtime, Action::Select);
    pump_until(&mut app, &runtime, |app| {
        matches!(app.authentication.view(), LoginView::Awaiting { .. })
    });
    clock.0.store(10, Ordering::SeqCst);
    pump_until(&mut app, &runtime, |app| {
        matches!(app.authentication.view(), LoginView::SignedIn)
    });
    assert_ne!(app.account_epoch, original);
    action(&mut app, &runtime, Action::Back);
    assert_eq!(app.ui.page(), Page::Home);
    app.controller.view.with_view(LoginView::SignedIn, |view| {
        assert!(
            view.cards.is_empty(),
            "identical credentials cannot restore the old private snapshot"
        )
    });
    for key in [Action::Left, Action::Down, Action::Down, Action::Select] {
        action(&mut app, &runtime, key);
    }
    assert_eq!(app.ui.page(), Page::MyList);
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    assert_eq!(
        middleware
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|target| matches!(target, Target::WatchList { .. }))
            .count(),
        2
    );
    assert!(app.finish(&runtime));
}

#[test]
fn departing_loading_shelf_then_back_restarts_the_retired_read() {
    let (mut app, runtime, _, _, middleware) = fixture(true);
    middleware.hold.store(true, Ordering::SeqCst);
    open_list(&mut app, &runtime);
    pump_until(&mut app, &runtime, |_| {
        middleware.calls.lock().unwrap().len() == 2
    });
    for key in [
        Action::Left,
        Action::Down,
        Action::Down,
        Action::Down,
        Action::Select,
    ] {
        action(&mut app, &runtime, key);
    }
    assert_eq!(app.ui.page(), Page::Login);
    action(&mut app, &runtime, Action::Back);
    assert_eq!(app.ui.page(), Page::MyList);
    assert!(
        app.shelf_pending.is_some(),
        "returning to an interrupted shelf must issue a fresh intent"
    );
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    assert_eq!(middleware.calls.lock().unwrap().len(), 3);
    assert_eq!(middleware.retired.load(Ordering::SeqCst), 1);
    assert!(app.finish(&runtime));
}
#[test]
fn cancelling_activation_discards_the_departed_frame_shapes() {
    let (mut app, runtime, _, _, _) = fixture(false);
    open_list(&mut app, &runtime);
    pump_until(&mut app, &runtime, |app| {
        matches!(app.authentication.view(), LoginView::Awaiting { .. })
    });
    app.consume(&runtime, Duration::ZERO);
    assert!(!take_cpu_output(&mut app).shapes.is_empty());
    let epoch = app.account_epoch;
    app.event(
        Event::Key(KeyEvent {
            scancode: 41,
            keycode: 27,
            pressed: true,
            repeat: false,
        }),
        Surface {
            window: Size {
                width: 1920,
                height: 1080,
            },
            drawable: Size {
                width: 1920,
                height: 1080,
            },
        },
        &runtime,
        Duration::ZERO,
    );
    assert_ne!(app.account_epoch, epoch);
    assert!(
        take_cpu_output(&mut app).shapes.is_empty(),
        "a cancelled code/QR frame must not be re-admitted"
    );
    assert!(app.finish(&runtime));
}
#[test]
fn failed_refresh_between_poll_and_consume_cannot_paint_private_shelf() {
    let (mut app, runtime, clock, issuer, _) = fixture(true);
    open_list(&mut app, &runtime);
    pump_until(&mut app, &runtime, |app| {
        app.controller
            .view
            .with_view(LoginView::SignedIn, |v| v.status == LoadState::Ready)
    });
    app.consume(&runtime, Duration::ZERO);
    take_cpu_output(&mut app);
    let epoch = app.account_epoch;
    issuer.fail.store(true, Ordering::SeqCst);
    clock.0.store(4000, Ordering::SeqCst);
    issuer.hold_refresh.store(true, Ordering::SeqCst);
    app.poll(&runtime, true);
    pump_until(&mut app, &runtime, |_| {
        issuer.tokens.load(Ordering::SeqCst) == 2
    });
    issuer.release.notify_one();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while app.authentication.session().status()
        != criterion_session::Status::ReauthenticationRequired
    {
        assert!(std::time::Instant::now() < deadline);
        runtime.block_on(tokio::task::yield_now());
    }
    app.consume(&runtime, Duration::ZERO);
    assert_ne!(
        app.account_epoch, epoch,
        "render must reconcile completed session invalidation"
    );
    let output = take_cpu_output(&mut app);
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("Synthetic private selection"))));
    assert!(app.finish(&runtime));
}

fn take_cpu_output(app: &mut App) -> egui::FullOutput {
    let mut output = app.take_output().unwrap();
    // CPU assertions intentionally do not upload fonts/textures to a GPU.
    output.textures_delta.clear();
    output
}
