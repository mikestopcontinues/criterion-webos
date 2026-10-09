// SPDX-License-Identifier: GPL-3.0-or-later
//! Private history and bounded demand through the real controller publication seam.
use super::*;
use criterion_account::{MediaKind, MediaSummary, PagingInfo, WatchList, WatchListFilter};
use criterion_provider::{PageCursor, Request, Response};
use criterion_ui::{CatalogTail, LoadState, LoginView, MyListGroup};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Default)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
struct Offline;
impl RequestTransport for Offline {
    async fn get(&self, _: Request) -> Result<Response, Error> {
        Err(Error::Unavailable)
    }
}
fn fixture() -> (Controller<Offline, Clock>, Runtime, Clock) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let clock = Clock::default();
    let mut owner = Controller::with_clock(
        Catalog::with_transport(Offline),
        runtime.handle(),
        clock.clone(),
    );
    owner.set_account_session(Some(1));
    owner.command(
        Command::Navigate(Page::MyList),
        Page::MyList,
        runtime.handle(),
    );
    (owner, runtime, clock)
}
fn page(start: usize, count: usize, next: Option<&str>) -> WatchList {
    WatchList {
        playlist: (start..start + count)
            .map(|i| MediaSummary {
                id: MediaId::new(&format!("F{i:07X}")).unwrap(),
                title: format!("Synthetic private {i}"),
                kind: MediaKind::Film,
                duration: None,
                release_date: None,
            })
            .collect(),
        paging: PagingInfo {
            page_limit: 50,
            next_pagination_key: next.map(|cursor| PageCursor::new(cursor).unwrap()),
        },
        type_counts: Vec::new(),
    }
}
fn read(effect: Effect) -> Read {
    let Effect::AccountRead(read) = effect else {
        panic!("expected private account read")
    };
    read
}
#[test]
fn empty_continuations_and_credential_replacement_keep_one_demand_deadline() {
    let (mut owner, runtime, clock) = fixture();
    let first = owner.begin_shelf(1).unwrap();
    assert!(
        owner
            .admit_shelf(1, &first, page(0, 50, Some("second")))
            .is_none()
    );
    clock.0.store(10, Ordering::SeqCst);
    let next = read(owner.command(
        Command::Catalog {
            anchor: 48,
            target: 52,
        },
        Page::MyList,
        runtime.handle(),
    ));
    clock.0.store(59, Ordering::SeqCst);
    let successor = owner
        .admit_shelf(1, &next, page(0, 0, Some("third")))
        .unwrap();
    assert_eq!(successor.request.cursor.as_ref().unwrap().as_str(), "third");
    clock.0.store(69, Ordering::SeqCst);
    let renewed = owner.renew_shelf().unwrap();
    assert!(renewed.operation > successor.operation);
    assert_eq!(renewed.request, successor.request);
    assert!(!owner.shelf_expired());
    clock.0.store(70, Ordering::SeqCst);
    assert!(
        owner.shelf_expired(),
        "same demand expires at original deadline after replacements"
    );
    assert!(owner.admit_shelf(1, &renewed, page(50, 50, None)).is_none());
    owner.view.with_view(LoginView::SignedIn, |v| {
        assert_eq!(v.cards.len(), 50);
        assert_eq!(v.cards[0].title, "Synthetic private 0");
        assert_eq!(v.catalog.unwrap().tail, CatalogTail::Error);
        assert_eq!(v.status, LoadState::Ready);
    });
    let retry = read(owner.command(Command::RetryCatalog, Page::MyList, runtime.handle()));
    assert_eq!(retry.request, renewed.request);
    assert!(
        !owner.shelf_expired(),
        "explicit retry starts a new bounded demand"
    );
}
#[test]
fn warm_group_history_restores_selection_and_window_without_an_account_read() {
    let (mut owner, runtime, _) = fixture();
    let first = owner.begin_shelf(1).unwrap();
    owner.admit_shelf(1, &first, page(0, 50, None));
    let group = read(owner.command(
        Command::MyListGroup(MyListGroup::Collections),
        Page::MyList,
        runtime.handle(),
    ));
    assert_eq!(group.request.filter, WatchListFilter::Collection);
    owner.admit_shelf(1, &group, page(100, 20, None));
    owner.command(
        Command::Catalog {
            anchor: 8,
            target: 9,
        },
        Page::MyList,
        runtime.handle(),
    );
    owner.command(
        Command::Open(criterion_ui::Target::Media(
            MediaId::new("F0000068").unwrap(),
        )),
        Page::Detail,
        runtime.handle(),
    );
    let restored = owner.command(
        Command::Restore(Page::MyList),
        Page::MyList,
        runtime.handle(),
    );
    assert!(matches!(restored, Effect::None));
    owner.view.with_view(LoginView::SignedIn, |v| {
        assert_eq!(v.my_list.unwrap().selected, MyListGroup::Collections);
        assert_eq!(v.cards.len(), 20);
        assert_eq!(v.cards[8].key.media_id().unwrap().as_str(), "F000006C");
    });
}
#[test]
fn evicted_private_history_rehydrates_the_exact_saved_input_before_tail() {
    let (mut owner, runtime, _) = fixture();
    let first = owner.begin_shelf(1).unwrap();
    owner.admit_shelf(1, &first, page(0, 50, Some("second")));
    let second = read(owner.command(
        Command::Catalog {
            anchor: 48,
            target: 52,
        },
        Page::MyList,
        runtime.handle(),
    ));
    owner.admit_shelf(1, &second, page(50, 50, Some("third")));
    owner.command(
        Command::Catalog {
            anchor: 70,
            target: 71,
        },
        Page::MyList,
        runtime.handle(),
    );
    owner.command(
        Command::Open(criterion_ui::Target::Media(
            MediaId::new("F0000046").unwrap(),
        )),
        Page::Detail,
        runtime.handle(),
    );
    // Simulate the controller's actual memory-budget eviction at its owning seam.
    let snapshot = owner.history.last_mut().unwrap();
    snapshot.view = None;
    snapshot.my_list.as_mut().unwrap().evict_windows();
    let restored = read(owner.command(
        Command::Restore(Page::MyList),
        Page::MyList,
        runtime.handle(),
    ));
    assert_eq!(restored.request.cursor.as_ref().unwrap().as_str(), "second");
    assert!(
        owner
            .admit_shelf(1, &restored, page(50, 50, Some("third")))
            .is_none()
    );
    owner.view.with_view(LoginView::SignedIn, |v| {
        assert_eq!(v.catalog.unwrap().first, 50);
        assert_eq!(v.cards[20].key.media_id().unwrap().as_str(), "F0000046");
    });
}

#[test]
fn fresh_my_list_rail_visit_reuses_latest_private_group_and_retains_older_back_origin() {
    let (mut owner, runtime, _) = fixture();
    let first = owner.begin_shelf(1).unwrap();
    owner.admit_shelf(1, &first, page(0, 50, None));
    let group = read(owner.command(
        Command::MyListGroup(MyListGroup::Collections),
        Page::MyList,
        runtime.handle(),
    ));
    owner.admit_shelf(1, &group, page(100, 50, None));
    owner.command(
        Command::Catalog {
            anchor: 30,
            target: 31,
        },
        Page::MyList,
        runtime.handle(),
    );
    owner.command(Command::Navigate(Page::Home), Page::Home, runtime.handle());
    owner.command(
        Command::Navigate(Page::MyList),
        Page::MyList,
        runtime.handle(),
    );
    assert!(
        owner.begin_shelf(1).is_none(),
        "a warm rail visit must not request a new All page"
    );
    owner.view.with_view(LoginView::SignedIn, |v| {
        assert_eq!(v.my_list.unwrap().selected, MyListGroup::Collections);
        assert_eq!(v.cards[30].key.media_id().unwrap().as_str(), "F0000082");
        assert_eq!(v.catalog.unwrap().tail, CatalogTail::End);
    });
    owner.command(Command::Restore(Page::Home), Page::Home, runtime.handle());
    owner.command(
        Command::Restore(Page::MyList),
        Page::MyList,
        runtime.handle(),
    );
    owner.view.with_view(LoginView::SignedIn, |v| {
        assert_eq!(v.my_list.unwrap().selected, MyListGroup::Collections);
        assert_eq!(v.cards[30].key.media_id().unwrap().as_str(), "F0000082");
    });
}
