// SPDX-License-Identifier: GPL-3.0-or-later
//! Controlled-clock journeys through the production catalog and display seams.
use super::*;
use criterion_provider::{Request, Response};
use criterion_ui::{LoadState, LoginView};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Default)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Clone, Default)]
struct Fixture {
    calls: Arc<Mutex<Vec<String>>>,
    hold: Arc<AtomicBool>,
    release: Arc<tokio::sync::Notify>,
    retired: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
}
struct Retire(Arc<AtomicUsize>);
impl Drop for Retire {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl RequestTransport for Fixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        if request.url.path() != "/api/search" {
            return Err(Error::Unavailable);
        }
        let query = request
            .url
            .query_pairs()
            .find(|(key, _)| key == "q")
            .unwrap()
            .1
            .into_owned();
        self.calls.lock().unwrap().push(query.clone());
        if self.hold.swap(false, Ordering::SeqCst) {
            let _guard = Retire(self.retired.clone());
            self.release.notified().await;
        }
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(Error::Unavailable);
        }
        let body = br#"{"playlist":[{"contentType":"film","mediaid":"Film0001","title":"Synthetic film","duration":5400},{"contentType":"collection","mediaid":"Collect1","title":"Synthetic collection","duration":0}],"type_counts":{"film":11,"collection":4}}"#;
        let body = if query == "new" {
            std::str::from_utf8(body)
                .unwrap()
                .replace("Synthetic film", "Synthetic film new")
                .into_bytes()
        } else {
            body.to_vec()
        };
        Ok(Response {
            status: 200,
            content_type: "application/json".into(),
            body,
        })
    }
}
type Owner = Controller<Fixture, Clock>;
fn fixture() -> (Owner, Runtime, Clock, Fixture) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let clock = Clock::default();
    let transport = Fixture::default();
    let mut owner = Controller::with_clock(
        Catalog::with_transport(transport.clone()),
        runtime.handle(),
        clock.clone(),
    );
    until(&mut owner, &runtime, |owner| {
        owner.view.with_view(LoginView::SignedOut, |view| {
            view.status == LoadState::Offline
        })
    });
    owner.command(
        Command::Navigate(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    (owner, runtime, clock, transport)
}
fn until(owner: &mut Owner, runtime: &Runtime, predicate: impl Fn(&Owner) -> bool) {
    let limit = std::time::Instant::now() + Duration::from_secs(2);
    while !predicate(owner) {
        owner.poll(runtime);
        runtime.block_on(tokio::task::yield_now());
        assert!(
            std::time::Instant::now() < limit,
            "controlled Search journey did not settle"
        );
    }
}
fn search(owner: &mut Owner, runtime: &Runtime, query: &str, group: SearchGroup) {
    owner.command(
        Command::Search {
            query: query.into(),
            group,
        },
        Page::Search,
        runtime.handle(),
    );
}
fn ticks(owner: &mut Owner, runtime: &Runtime) {
    for _ in 0..16 {
        owner.poll(runtime);
        runtime.block_on(tokio::task::yield_now());
    }
}
#[test]
fn trimmed_search_waits_until_the_exact_250ms_boundary() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(
        &mut owner,
        &runtime,
        "  synthetic query  ",
        SearchGroup::All,
    );
    clock.0.store(249, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert!(
        transport.calls.lock().unwrap().is_empty(),
        "249ms must issue no Search request"
    );
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Loading)
    });
    clock.0.store(250, Ordering::SeqCst);
    until(&mut owner, &runtime, |owner| {
        owner
            .view
            .with_view(LoginView::SignedOut, |view| view.status == LoadState::Ready)
    });
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic query"]);
}

#[test]
fn same_query_groups_keep_the_deadline_and_reuse_the_admitted_projection() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "synthetic", SearchGroup::All);
    clock.0.store(100, Ordering::SeqCst);
    search(&mut owner, &runtime, " synthetic ", SearchGroup::Films);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
        assert_eq!(view.cards.len(), 1);
        assert_eq!(view.cards[0].title, "Synthetic film");
    });
    search(&mut owner, &runtime, "synthetic", SearchGroup::Collections);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 4);
        assert_eq!(view.cards[0].title, "Synthetic collection");
    });
    search(&mut owner, &runtime, "synthetic", SearchGroup::Supplements);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Empty);
        assert_eq!(view.total, 0);
        assert!(view.cards.is_empty());
    });
    search(&mut owner, &runtime, "synthetic", SearchGroup::All);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 15);
        assert_eq!(view.cards.len(), 2);
    });
    clock.0.store(1000, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
}

#[test]
fn background_cancels_pending_search_and_foreground_stages_a_fresh_delay() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    clock.0.store(100, Ordering::SeqCst);
    owner.background();
    clock.0.store(1000, Ordering::SeqCst);
    owner.foreground(runtime.handle());
    ticks(&mut owner, &runtime);
    assert!(
        transport.calls.lock().unwrap().is_empty(),
        "the departed deadline cannot issue on foreground"
    );
    clock.0.store(1249, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert!(transport.calls.lock().unwrap().is_empty());
    clock.0.store(1250, Ordering::SeqCst);
    until(&mut owner, &runtime, |owner| {
        owner
            .view
            .with_view(LoginView::SignedOut, |view| view.status == LoadState::Ready)
    });
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    owner
        .view
        .with_view(LoginView::SignedOut, |view| assert_eq!(view.total, 11));
}

#[test]
fn pending_search_navigation_restores_a_fresh_intent_instead_of_loading_forever() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    clock.0.store(100, Ordering::SeqCst);
    owner.command(
        Command::Navigate(Page::Login),
        Page::Login,
        runtime.handle(),
    );
    clock.0.store(1000, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert!(transport.calls.lock().unwrap().is_empty());
    owner.command(
        Command::Restore(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    ticks(&mut owner, &runtime);
    assert!(transport.calls.lock().unwrap().is_empty());
    clock.0.store(1249, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert!(transport.calls.lock().unwrap().is_empty());
    clock.0.store(1250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
    });
}

#[test]
fn background_navigation_does_not_retain_an_interrupted_search_loading_display() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    clock.0.store(100, Ordering::SeqCst);
    owner.background();
    owner.command(
        Command::Navigate(Page::Login),
        Page::Login,
        runtime.handle(),
    );
    clock.0.store(1000, Ordering::SeqCst);
    owner.foreground(runtime.handle());
    owner.command(
        Command::Restore(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    clock.0.store(1249, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert!(transport.calls.lock().unwrap().is_empty());
    clock.0.store(1250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
    });
}

#[test]
fn typing_burst_issues_only_the_latest_query_after_its_full_delay() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "s", SearchGroup::All);
    clock.0.store(100, Ordering::SeqCst);
    search(&mut owner, &runtime, "sy", SearchGroup::All);
    clock.0.store(200, Ordering::SeqCst);
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    clock.0.store(449, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert!(transport.calls.lock().unwrap().is_empty());
    clock.0.store(450, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    owner
        .view
        .with_view(LoginView::SignedOut, |view| assert_eq!(view.total, 11));
}

#[test]
fn in_flight_group_changes_publish_only_the_latest_group_without_another_get() {
    let (mut owner, runtime, clock, transport) = fixture();
    transport.hold.store(true, Ordering::SeqCst);
    search(&mut owner, &runtime, "synthetic", SearchGroup::All);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    search(
        &mut owner,
        &runtime,
        " synthetic ",
        SearchGroup::Collections,
    );
    transport.release.notify_one();
    until(&mut owner, &runtime, |owner| {
        owner
            .view
            .with_view(LoginView::SignedOut, |view| view.status == LoadState::Ready)
    });
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.total, 4);
        assert_eq!(view.cards.len(), 1);
        assert_eq!(view.cards[0].title, "Synthetic collection");
    });
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    assert_eq!(transport.retired.load(Ordering::SeqCst), 1);
}

#[test]
fn completed_unpublished_search_uses_the_latest_group() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "synthetic", SearchGroup::All);
    clock.0.store(250, Ordering::SeqCst);
    owner.poll(&runtime);
    runtime.block_on(async {
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    });
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Loading)
    });
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    ticks(&mut owner, &runtime);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
        assert_eq!(view.cards.len(), 1);
    });
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
}

#[test]
fn new_query_retires_a_held_transport_and_prevents_its_publication() {
    let (mut owner, runtime, clock, transport) = fixture();
    transport.hold.store(true, Ordering::SeqCst);
    search(&mut owner, &runtime, "old", SearchGroup::All);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["old"]);
    clock.0.store(300, Ordering::SeqCst);
    search(&mut owner, &runtime, "new", SearchGroup::Films);
    ticks(&mut owner, &runtime);
    assert_eq!(transport.retired.load(Ordering::SeqCst), 1);
    transport.release.notify_one();
    clock.0.store(549, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["old"]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Loading);
        assert!(view.cards.is_empty());
    });
    clock.0.store(550, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
        assert_eq!(view.cards[0].title, "Synthetic film new");
    });
    assert_eq!(*transport.calls.lock().unwrap(), ["old", "new"]);
}

#[test]
fn empty_search_clears_pending_in_flight_and_admitted_results_immediately() {
    for stage in 0..3 {
        let (mut owner, runtime, clock, transport) = fixture();
        transport.hold.store(stage == 1, Ordering::SeqCst);
        search(&mut owner, &runtime, "synthetic", SearchGroup::All);
        if stage > 0 {
            clock.0.store(250, Ordering::SeqCst);
            ticks(&mut owner, &runtime);
            assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
        }
        search(&mut owner, &runtime, " \t ", SearchGroup::Films);
        owner.view.with_view(LoginView::SignedOut, |view| {
            assert_eq!(view.status, LoadState::Empty);
            assert!(view.cards.is_empty());
            assert_eq!(view.total, 0);
        });
        transport.release.notify_one();
        clock.0.store(1000, Ordering::SeqCst);
        ticks(&mut owner, &runtime);
        assert_eq!(
            transport.calls.lock().unwrap().len(),
            usize::from(stage > 0)
        );
        assert_eq!(
            transport.retired.load(Ordering::SeqCst),
            usize::from(stage == 1)
        );
        owner.view.with_view(LoginView::SignedOut, |view| {
            assert_eq!(view.status, LoadState::Empty)
        });
    }
}

#[test]
fn background_retires_an_issued_search_and_foreground_reloads_after_a_fresh_delay() {
    let (mut owner, runtime, clock, transport) = fixture();
    transport.hold.store(true, Ordering::SeqCst);
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    owner.background();
    ticks(&mut owner, &runtime);
    assert_eq!(transport.retired.load(Ordering::SeqCst), 1);
    clock.0.store(1000, Ordering::SeqCst);
    owner.foreground(runtime.handle());
    clock.0.store(1249, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    clock.0.store(1250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic", "synthetic"]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
    });
}

#[test]
fn issued_search_navigation_retires_before_restoring_a_fresh_delay() {
    let (mut owner, runtime, clock, transport) = fixture();
    transport.hold.store(true, Ordering::SeqCst);
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    owner.command(
        Command::Navigate(Page::Login),
        Page::Login,
        runtime.handle(),
    );
    ticks(&mut owner, &runtime);
    assert_eq!(transport.retired.load(Ordering::SeqCst), 1);
    clock.0.store(500, Ordering::SeqCst);
    owner.command(
        Command::Restore(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    clock.0.store(749, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    clock.0.store(750, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic", "synthetic"]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
    });
}

#[test]
fn admitted_search_restores_its_exact_group_without_an_extra_get() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "synthetic", SearchGroup::Collections);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    owner.command(
        Command::Navigate(Page::Login),
        Page::Login,
        runtime.handle(),
    );
    owner.command(
        Command::Restore(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 4);
        assert_eq!(view.cards[0].title, "Synthetic collection");
    });
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    clock.0.store(1000, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    owner
        .view
        .with_view(LoginView::SignedOut, |view| assert_eq!(view.total, 11));
    owner.background();
    owner.foreground(runtime.handle());
    clock.0.store(2000, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
}

#[test]
fn changed_query_discards_an_already_finished_unpublished_response() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "old", SearchGroup::All);
    clock.0.store(250, Ordering::SeqCst);
    owner.poll(&runtime);
    runtime.block_on(async {
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    });
    assert_eq!(*transport.calls.lock().unwrap(), ["old"]);
    search(&mut owner, &runtime, "new", SearchGroup::Films);
    ticks(&mut owner, &runtime);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Loading);
        assert!(view.cards.is_empty());
    });
    clock.0.store(500, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.cards[0].title, "Synthetic film new")
    });
    assert_eq!(*transport.calls.lock().unwrap(), ["old", "new"]);
}

#[test]
fn restore_cancels_a_departed_deadline_before_reusing_an_admitted_snapshot() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "synthetic", SearchGroup::Collections);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    owner.command(
        Command::Navigate(Page::Login),
        Page::Login,
        runtime.handle(),
    );
    owner.command(
        Command::Navigate(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    search(&mut owner, &runtime, "departed", SearchGroup::Films);
    owner.command(Command::Restore(Page::Login), Page::Login, runtime.handle());
    owner.command(
        Command::Restore(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    clock.0.store(1000, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 4);
        assert_eq!(view.cards[0].title, "Synthetic collection");
    });
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    owner
        .view
        .with_view(LoginView::SignedOut, |view| assert_eq!(view.total, 11));
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
}

#[test]
fn failed_search_reloads_only_after_an_explicit_group_action_and_a_fresh_delay() {
    let (mut owner, runtime, clock, transport) = fixture();
    transport.fail.store(true, Ordering::SeqCst);
    search(&mut owner, &runtime, "synthetic", SearchGroup::All);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Offline)
    });
    clock.0.store(1000, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    clock.0.store(1249, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    clock.0.store(1250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic", "synthetic"]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
    });
}

#[test]
fn an_error_display_cannot_be_reused_as_an_admitted_search() {
    let (mut owner, runtime, clock, transport) = fixture();
    search(&mut owner, &runtime, "synthetic", SearchGroup::All);
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert!(matches!(
        owner.command(Command::VoiceSearch, Page::Search, runtime.handle()),
        Effect::VoiceSearch
    ));
    // Application maps the unavailable voice action onto this display status.
    owner.view.set_status(LoadState::Error);
    search(&mut owner, &runtime, "synthetic", SearchGroup::Films);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(
            view.status,
            LoadState::Loading,
            "an error display must stage a fresh read"
        );
        assert!(view.cards.is_empty());
    });
    clock.0.store(499, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
    clock.0.store(500, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), ["synthetic", "synthetic"]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready);
        assert_eq!(view.total, 11);
    });
}

#[test]
fn invalid_search_is_refused_immediately_and_retires_the_previous_intent() {
    for invalid in ["a".repeat(257), "é".repeat(129), "syn\nthetic".to_owned()] {
        for issued in [false, true] {
            let (mut owner, runtime, clock, transport) = fixture();
            transport.hold.store(issued, Ordering::SeqCst);
            search(&mut owner, &runtime, "synthetic", SearchGroup::All);
            if issued {
                clock.0.store(250, Ordering::SeqCst);
                ticks(&mut owner, &runtime);
                assert_eq!(*transport.calls.lock().unwrap(), ["synthetic"]);
            }
            search(&mut owner, &runtime, &invalid, SearchGroup::All);
            owner.view.with_view(LoginView::SignedOut, |view| {
                assert_eq!(
                    view.status,
                    LoadState::Error,
                    "invalid input must be refused before the debounce starts"
                );
                assert!(view.cards.is_empty());
            });
            clock.0.store(1000, Ordering::SeqCst);
            ticks(&mut owner, &runtime);
            assert_eq!(transport.calls.lock().unwrap().len(), usize::from(issued));
            assert_eq!(
                transport.retired.load(Ordering::SeqCst),
                usize::from(issued)
            );
        }
    }
}

#[test]
fn a_256_byte_utf8_search_is_admitted_after_trimming() {
    let (mut owner, runtime, clock, transport) = fixture();
    let query = "é".repeat(128);
    search(
        &mut owner,
        &runtime,
        &format!("  {query}  "),
        SearchGroup::Films,
    );
    clock.0.store(250, Ordering::SeqCst);
    ticks(&mut owner, &runtime);
    assert_eq!(*transport.calls.lock().unwrap(), [query]);
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Ready)
    });
}

#[test]
fn clock_overflow_refuses_search_without_issuing_a_request() {
    struct MaximumClock;
    impl MonotonicClock for MaximumClock {
        fn now(&self) -> Duration {
            Duration::MAX
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let transport = Fixture::default();
    let mut owner = Controller::with_clock(
        Catalog::with_transport(transport.clone()),
        runtime.handle(),
        MaximumClock,
    );
    owner.command(
        Command::Navigate(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    owner.command(
        Command::Search {
            query: "synthetic".into(),
            group: SearchGroup::All,
        },
        Page::Search,
        runtime.handle(),
    );
    owner.view.with_view(LoginView::SignedOut, |view| {
        assert_eq!(view.status, LoadState::Error)
    });
    for _ in 0..16 {
        owner.poll(&runtime);
        runtime.block_on(tokio::task::yield_now());
    }
    assert!(transport.calls.lock().unwrap().is_empty());
}
