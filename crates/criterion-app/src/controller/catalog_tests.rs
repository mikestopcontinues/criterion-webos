// SPDX-License-Identifier: GPL-3.0-or-later
//! Public catalog journeys with literal opaque cursors and synthetic HTTP data.
use super::*;
use criterion_provider::{Request, Response};
use criterion_ui::{CatalogTail, LoadState, LoginView};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
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
    fail: Arc<AtomicBool>,
    hold: Arc<AtomicBool>,
    invalid_next: Arc<AtomicBool>,
    empty: Arc<AtomicBool>,
    terminal_empty: Arc<AtomicBool>,
    size: Arc<AtomicU64>,
    tail_size: Arc<AtomicU64>,
    pages: Arc<AtomicU64>,
    release: Arc<tokio::sync::Notify>,
}
impl RequestTransport for Fixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        let path = request.url.path();
        if path == "/api/all-films/filters" {
            return Ok(Response {
                status: 200,
                content_type: "application/json".into(),
                body: br#"{"sortOptions":[{"label":"Year","value":"year"}],"filterGroups":[]}"#
                    .to_vec(),
            });
        }
        if path != "/api/all-films/results" {
            return Err(Error::Unavailable);
        }
        let cursor = request
            .url
            .query_pairs()
            .find(|(key, _)| key == "pagination_key")
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default();
        self.calls
            .lock()
            .unwrap()
            .push(request.url.query().unwrap().to_owned());
        if self.hold.swap(false, Ordering::SeqCst) {
            self.release.notified().await;
        }
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(Error::Unavailable);
        }
        let pages = self.pages.load(Ordering::SeqCst).max(3);
        let page = match cursor.as_str() {
            "" => 0,
            "opaque /+? one" => 1,
            "opaque = two" => 2,
            _ => cursor
                .strip_prefix("page:")
                .and_then(|p| p.parse::<u64>().ok())
                .ok_or(Error::InvalidResponse)?,
        };
        let size = self.size.load(Ordering::SeqCst);
        let size = if size == 0 { 60 } else { size };
        let tail = self.tail_size.load(Ordering::SeqCst);
        let tail = if tail == 0 { size } else { tail };
        let first = if page == 0 {
            0
        } else {
            size + (page - 1) * tail
        };
        let size = if page == 0 { size } else { tail };
        let next = if page + 1 >= pages {
            String::new()
        } else {
            match page {
                0 => "opaque /+? one".into(),
                1 => "opaque = two".into(),
                _ => format!("page:{}", page + 1),
            }
        };
        if self.terminal_empty.swap(false, Ordering::SeqCst) {
            return Ok(Response {
                status: 200,
                content_type: "application/json".into(),
                body: br#"{"items":[],"total":180,"paging":{"page_limit":60}}"#.to_vec(),
            });
        }
        if self.empty.swap(false, Ordering::SeqCst) {
            return Ok(Response{status:200,content_type:"application/json".into(),body:br#"{"items":[],"total":180,"paging":{"next_pagination_key":"still-next","page_limit":60}}"#.to_vec()});
        }
        let next = if self.invalid_next.swap(false, Ordering::SeqCst) {
            cursor.clone()
        } else {
            next
        };
        let items=(first..first+size).map(|i|format!(r#"{{"mediaid":"Film{i:04X}","title":"Synthetic {i}","contentType":"film","duration":5400}}"#)).collect::<Vec<_>>().join(",");
        Ok(Response{status:200,content_type:"application/json".into(),body:format!(r#"{{"items":[{items}],"total":{},"paging":{{"next_pagination_key":"{next}","page_limit":60}}}}"#,pages*size+777).into_bytes()})
    }
}
type Owner = Controller<Fixture, Clock>;
fn until(owner: &mut Owner, runtime: &Runtime, done: impl Fn(&Owner) -> bool) {
    let limit = std::time::Instant::now() + Duration::from_secs(3);
    while !done(owner) {
        owner.poll(runtime);
        runtime.block_on(tokio::task::yield_now());
        assert!(std::time::Instant::now() < limit, "catalog did not settle");
    }
}
fn fixture() -> (Owner, Runtime, Clock, Fixture) {
    fixture_transport(Fixture::default())
}
fn fixture_transport(transport: Fixture) -> (Owner, Runtime, Clock, Fixture) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let clock = Clock::default();
    let mut owner = Controller::with_clock(
        Catalog::with_transport(transport.clone()),
        runtime.handle(),
        clock.clone(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.command(
        Command::Navigate(Page::AllFilms),
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    (owner, runtime, clock, transport)
}
fn len(owner: &Owner) -> usize {
    owner
        .view
        .with_view(LoginView::SignedOut, |v| v.cards.len())
}
#[test]
fn catalog_continuation_uses_the_exact_opaque_cursor_and_keeps_the_committed_grid() {
    let (mut owner, runtime, _, transport) = fixture();
    assert_eq!(len(&owner), 60);
    transport.hold.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 48,
            target: 48,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    runtime.block_on(tokio::task::yield_now());
    assert_eq!(
        len(&owner),
        60,
        "pending continuation retains committed cards"
    );
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.status, LoadState::Ready)
    });
    assert_eq!(
        transport.calls.lock().unwrap().len(),
        2,
        "one continuation must issue"
    );
    transport.release.notify_one();
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(len(&owner), 120);
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.cards[60].key.media_id().unwrap().as_str(), "Film003C")
    });
    let calls = transport.calls.lock().unwrap();
    assert!(calls[1].contains("pagination_key=opaque+%2F%2B%3F+one"));
    assert_eq!(owner.history.len(), 0, "paging does not push navigation");
}
#[test]
fn failed_continuation_has_explicit_retry_and_preserves_the_last_good_grid() {
    let (mut owner, runtime, _, transport) = fixture();
    transport.fail.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 48,
            target: 48,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(len(&owner), 60);
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(
            v.status,
            LoadState::Ready,
            "a paging error must not replace committed cards"
        );
        assert_eq!(v.catalog.unwrap().tail, CatalogTail::Error);
    });
    owner.command(Command::RetryCatalog, Page::AllFilms, runtime.handle());
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(len(&owner), 120);
    let calls = transport.calls.lock().unwrap();
    assert_eq!(
        calls[1], calls[2],
        "retry reissues the same opaque input cursor"
    );
}

#[test]
fn catalog_window_evicts_old_cards_and_backward_uses_a_recorded_input_cursor() {
    let (mut owner, runtime, _, transport) = fixture();
    transport.pages.store(4, Ordering::SeqCst);
    for anchor in [48, 108, 168] {
        owner.command(
            Command::Catalog {
                anchor,
                target: anchor,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
    }
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.cards.len(), 180, "the active window must stay bounded");
        assert_eq!(v.catalog.unwrap().first, 60);
        assert_eq!(v.cards[0].key.media_id().unwrap().as_str(), "Film003C");
    });
    owner.command(
        Command::Catalog {
            anchor: 60,
            target: 56,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.catalog.unwrap().first, 0);
        assert_eq!(v.cards[56].key.media_id().unwrap().as_str(), "Film0038");
    });
    assert!(
        !transport
            .calls
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .contains("pagination_key"),
        "first page reload must use its recorded None input cursor"
    );
}
#[test]
fn detail_back_during_continuation_restores_the_committed_window_and_query() {
    let (mut owner, runtime, _, transport) = fixture();
    owner.command(
        Command::Catalog {
            anchor: 48,
            target: 48,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    transport.hold.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 108,
            target: 108,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    runtime.block_on(tokio::task::yield_now());
    owner.command(
        Command::Open(Target::Media(MediaId::new("Film006C").unwrap())),
        Page::Detail,
        runtime.handle(),
    );
    owner.command(
        Command::Restore(Page::AllFilms),
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(
        len(&owner),
        120,
        "Back must restore committed pages even when an append was interrupted"
    );
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.cards[108].key.media_id().unwrap().as_str(), "Film006C")
    });
    owner.command(
        Command::Catalog {
            anchor: 108,
            target: 108,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(len(&owner), 180);
}

#[test]
fn cold_backward_traversal_replays_forward_only_with_bounded_retained_state() {
    let (mut owner, runtime, _, transport) = fixture();
    transport.pages.store(280, Ordering::SeqCst);
    for _ in 1..280 {
        let anchor = owner.view.with_view(LoginView::SignedOut, |v| {
            v.catalog.unwrap().first + v.cards.len() - 12
        });
        owner.command(
            Command::Catalog {
                anchor,
                target: anchor,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
        assert!(len(&owner) <= 180);
        assert!(owner.view.estimated_bytes() <= 512 * 1024);
        assert!(owner.pager.as_ref().unwrap().estimated_bytes() <= 192 * 1024 + 512 * 1024);
    }
    let before = transport.calls.lock().unwrap().len();
    owner.command(
        Command::Catalog {
            anchor: 16620,
            target: 60,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(
            v.catalog.unwrap().first,
            60,
            "an evicted bookmark must replay to the requested position"
        );
        assert_eq!(v.cards[0].key.media_id().unwrap().as_str(), "Film003C");
    });
    let calls = transport.calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        before + 2,
        "only first then its observed next cursor are needed"
    );
    assert!(!calls[before].contains("pagination_key"));
    assert!(calls[before + 1].contains("pagination_key=opaque+%2F%2B%3F+one"));
}
#[test]
fn history_budget_eviction_reloads_the_saved_anchor_page_instead_of_resetting_to_page_one() {
    let (mut owner, runtime, _, _) = fixture();
    owner.command(
        Command::Catalog {
            anchor: 48,
            target: 48,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.command(
        Command::Catalog {
            anchor: 80,
            target: 80,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    owner.command(
        Command::Open(Target::Media(MediaId::new("Film0050").unwrap())),
        Page::Detail,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view = Presentation::loading(String::with_capacity(HISTORY_BYTES));
    owner.remember();
    assert!(
        owner.history[0].view.is_none(),
        "history fixture must actually evict its catalog view"
    );
    owner.command(
        Command::Restore(Page::Detail),
        Page::Detail,
        runtime.handle(),
    );
    owner.command(
        Command::Restore(Page::AllFilms),
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(
            v.catalog.unwrap().first,
            60,
            "saved global focus must reload its observed input-cursor page"
        );
        assert_eq!(v.cards[20].key.media_id().unwrap().as_str(), "Film0050");
    });
}
#[test]
fn foreground_resumes_a_retired_append_without_discarding_committed_pages() {
    let (mut owner, runtime, _, transport) = fixture();
    owner.command(
        Command::Catalog {
            anchor: 48,
            target: 48,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    transport.hold.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 108,
            target: 108,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    runtime.block_on(tokio::task::yield_now());
    owner.background();
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.foreground(runtime.handle());
    assert_eq!(
        len(&owner),
        120,
        "foreground must keep the committed window while its retired append resumes"
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(len(&owner), 180);
}
#[test]
fn moving_away_from_a_pending_backward_seek_retires_it_without_shifting_focus() {
    let (mut owner, runtime, _, transport) = fixture();
    transport.pages.store(4, Ordering::SeqCst);
    for anchor in [48, 108, 168] {
        owner.command(
            Command::Catalog {
                anchor,
                target: anchor,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
    }
    transport.hold.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 60,
            target: 56,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    runtime.block_on(tokio::task::yield_now());
    owner.command(
        Command::Catalog {
            anchor: 64,
            target: 64,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    transport.release.notify_one();
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(
            v.catalog.unwrap().first,
            60,
            "departed seek must not publish a window that omits the current focus"
        );
        assert_eq!(v.cards[4].key.media_id().unwrap().as_str(), "Film0040");
    });
}

#[test]
fn traversal_deadline_retires_the_issued_read_and_explicit_retry_keeps_its_cursor() {
    let (mut owner, runtime, clock, transport) = fixture();
    transport.hold.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 48,
            target: 48,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    runtime.block_on(tokio::task::yield_now());
    clock.0.store(60_000, Ordering::SeqCst);
    owner.poll(&runtime);
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.catalog.unwrap().tail, CatalogTail::Error);
        assert_eq!(v.cards.len(), 60);
    });
    owner.command(Command::RetryCatalog, Page::AllFilms, runtime.handle());
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(len(&owner), 120);
    let calls = transport.calls.lock().unwrap();
    assert_eq!(calls[1], calls[2]);
}
#[test]
fn filter_apply_resets_the_cursor_and_retires_the_old_continuation() {
    let (mut owner, runtime, _, transport) = fixture();
    transport.hold.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 48,
            target: 48,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    runtime.block_on(tokio::task::yield_now());
    owner.command(
        Command::ApplyFilters(FilterSelection {
            sort_index: 0,
            descending: true,
            options: Vec::new(),
        }),
        Page::AllFilms,
        runtime.handle(),
    );
    transport.release.notify_one();
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.catalog.unwrap().first, 0);
        assert_eq!(v.cards.len(), 60);
    });
    let calls = transport.calls.lock().unwrap();
    let latest = calls.last().unwrap();
    assert!(latest.contains("sort=year"));
    assert!(latest.contains("sortDir=desc"));
    assert!(!latest.contains("pagination_key"));
}
#[test]
fn repeated_cursor_and_empty_continuation_fail_without_a_request_loop() {
    for empty in [false, true] {
        let (mut owner, runtime, _, transport) = fixture();
        if empty {
            transport.empty.store(true, Ordering::SeqCst);
        } else {
            transport.invalid_next.store(true, Ordering::SeqCst);
        }
        owner.command(
            Command::Catalog {
                anchor: 48,
                target: 48,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
        owner.view.with_view(LoginView::SignedOut, |v| {
            assert_eq!(v.cards.len(), 60);
            assert_eq!(v.catalog.unwrap().tail, CatalogTail::Error);
        });
        for _ in 0..20 {
            owner.command(
                Command::Catalog {
                    anchor: 48,
                    target: 60,
                },
                Page::AllFilms,
                runtime.handle(),
            );
            owner.poll(&runtime);
        }
        assert_eq!(transport.calls.lock().unwrap().len(), 2);
    }
}
#[test]
fn final_cursor_none_stops_reads_even_when_the_reported_total_is_larger() {
    let (mut owner, runtime, _, transport) = fixture();
    for anchor in [48, 108] {
        owner.command(
            Command::Catalog {
                anchor,
                target: anchor,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
    }
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.catalog.unwrap().tail, CatalogTail::End);
        assert_eq!(v.total, 957);
        assert_eq!(v.cards.len(), 180);
    });
    for _ in 0..20 {
        owner.command(
            Command::Catalog {
                anchor: 176,
                target: 180,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        owner.poll(&runtime);
    }
    assert_eq!(transport.calls.lock().unwrap().len(), 3);
}

#[test]
fn short_pages_preserve_exact_global_offsets_and_continue_until_cursor_none() {
    let transport = Fixture::default();
    transport.size.store(5, Ordering::SeqCst);
    let (mut owner, runtime, _, transport) = fixture_transport(transport);
    assert_eq!(len(&owner), 5);
    for anchor in [4, 8] {
        owner.command(
            Command::Catalog {
                anchor,
                target: anchor,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
    }
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(v.cards.len(), 15);
        assert_eq!(v.cards[5].key.media_id().unwrap().as_str(), "Film0005");
        assert_eq!(v.catalog.unwrap().tail, CatalogTail::End);
        assert_eq!(v.total, 792);
    });
    assert_eq!(transport.calls.lock().unwrap().len(), 3);
}

#[test]
fn a_missing_backward_position_preserves_the_committed_window_and_exposes_retry() {
    let (mut owner, runtime, _, transport) = fixture();
    transport.pages.store(4, Ordering::SeqCst);
    for anchor in [48, 108, 168] {
        owner.command(
            Command::Catalog {
                anchor,
                target: anchor,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
    }
    transport.terminal_empty.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 60,
            target: 56,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(
            v.cards.len(),
            180,
            "a missing requested position must not erase the last good grid"
        );
        assert_eq!(v.catalog.unwrap().first, 60);
        assert_eq!(v.catalog.unwrap().tail, CatalogTail::Error);
    });
}

#[test]
fn pending_down_across_short_pages_keeps_every_card_in_provider_order() {
    let (mut owner, runtime, _, transport) = fixture();
    transport.tail_size.store(1, Ordering::SeqCst);
    transport.pages.store(5, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 59,
            target: 63,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(
            v.cards.len(),
            64,
            "forward traversal must retain each short intermediate page"
        );
        assert_eq!(v.cards[60].key.media_id().unwrap().as_str(), "Film003C");
        assert_eq!(v.cards[63].key.media_id().unwrap().as_str(), "Film003F");
        assert_eq!(v.catalog.unwrap().tail, CatalogTail::End);
    });
}
#[test]
fn leaving_a_failed_backward_intent_prevents_retry_from_publishing_its_old_target() {
    let (mut owner, runtime, _, transport) = fixture();
    transport.pages.store(4, Ordering::SeqCst);
    for anchor in [48, 108, 168] {
        owner.command(
            Command::Catalog {
                anchor,
                target: anchor,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
    }
    transport.fail.store(true, Ordering::SeqCst);
    owner.command(
        Command::Catalog {
            anchor: 60,
            target: 56,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.command(
        Command::Catalog {
            anchor: 64,
            target: 64,
        },
        Page::AllFilms,
        runtime.handle(),
    );
    owner.command(Command::RetryCatalog, Page::AllFilms, runtime.handle());
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(
            v.catalog.unwrap().first,
            60,
            "a departed failed seek cannot later replace the focused window"
        );
        assert_eq!(v.cards[4].key.media_id().unwrap().as_str(), "Film0040");
    });
    assert_eq!(transport.calls.lock().unwrap().len(), 5);
}
#[test]
fn remote_catalog_continuation_detail_and_back_keep_the_paired_display_and_focus() {
    let (mut owner, runtime, _, _) = fixture();
    // The transport has already admitted page one; enter the UI's matching rail.
    let mut ui = criterion_ui::AppUi::new();
    for action in [
        criterion_ui::Action::Left,
        criterion_ui::Action::Down,
        criterion_ui::Action::Down,
        criterion_ui::Action::Select,
    ] {
        let commands = owner
            .view
            .with_view(LoginView::SignedOut, |v| ui.handle(action, &v));
        for command in commands {
            owner.command(command, ui.page(), runtime.handle());
        }
    }
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    for _ in 0..14 {
        let commands = owner.view.with_view(LoginView::SignedOut, |v| {
            ui.handle(criterion_ui::Action::Down, &v)
        });
        for command in commands {
            owner.command(command, ui.page(), runtime.handle());
        }
    }
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(len(&owner), 120);
    let selected = owner.view.with_view(LoginView::SignedOut, |v| {
        ui.handle(criterion_ui::Action::Select, &v)
    });
    assert!(selected.contains(&Command::Open(Target::Media(
        MediaId::new("Film0038").unwrap()
    ))));
    for command in selected {
        owner.command(command, ui.page(), runtime.handle());
    }
    let back = owner.view.with_view(LoginView::SignedOut, |v| {
        ui.handle(criterion_ui::Action::Back, &v)
    });
    for command in back {
        owner.command(command, ui.page(), runtime.handle());
    }
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(ui.focus(), criterion_ui::Focus::Card { row: 14, column: 0 });
    assert_eq!(len(&owner), 120);
    assert_eq!(owner.history.len(), 0);
}
#[test]
fn rail_departure_after_pending_down_fulfillment_records_the_exact_history_anchor() {
    let transport = Fixture::default();
    transport.pages.store(4, Ordering::SeqCst);
    let (mut owner, runtime, _, transport) = fixture_transport(transport);
    for anchor in [48, 108] {
        owner.command(
            Command::Catalog {
                anchor,
                target: anchor,
            },
            Page::AllFilms,
            runtime.handle(),
        );
        until(&mut owner, &runtime, |o| !o.jobs.is_active());
    }
    let mut ui = criterion_ui::AppUi::new();
    for action in [
        criterion_ui::Action::Left,
        criterion_ui::Action::Down,
        criterion_ui::Action::Down,
        criterion_ui::Action::Select,
    ] {
        owner
            .view
            .with_view(LoginView::SignedOut, |v| ui.handle(action, &v));
    }
    transport.hold.store(true, Ordering::SeqCst);
    for _ in 0..45 {
        let commands = owner.view.with_view(LoginView::SignedOut, |v| {
            ui.handle(criterion_ui::Action::Down, &v)
        });
        for command in commands {
            owner.command(command, ui.page(), runtime.handle());
        }
    }
    runtime.block_on(tokio::task::yield_now());
    transport.release.notify_one();
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    for action in [
        criterion_ui::Action::Left,
        criterion_ui::Action::Up,
        criterion_ui::Action::Up,
        criterion_ui::Action::Up,
        criterion_ui::Action::Select,
    ] {
        let commands = owner
            .view
            .with_view(LoginView::SignedOut, |v| ui.handle(action, &v));
        for command in commands {
            owner.command(command, ui.page(), runtime.handle());
        }
    }
    assert_eq!(ui.page(), Page::Search);
    // Budget eviction itself is covered above; this isolates paired anchor ownership.
    owner.history[0].view = None;
    let commands = owner.view.with_view(LoginView::SignedOut, |v| {
        ui.handle(criterion_ui::Action::Back, &v)
    });
    for command in commands {
        owner.command(command, ui.page(), runtime.handle());
    }
    until(&mut owner, &runtime, |o| !o.jobs.is_active());
    assert_eq!(ui.focus(), criterion_ui::Focus::Card { row: 45, column: 0 });
    owner.view.with_view(LoginView::SignedOut, |v| {
        assert_eq!(
            v.catalog.unwrap().first,
            180,
            "rail departure must preserve the newly fulfilled global card"
        );
        assert_eq!(v.cards[0].key.media_id().unwrap().as_str(), "Film00B4");
    });
}
