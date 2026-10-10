// SPDX-License-Identifier: GPL-3.0-or-later
//! Supplied-gallery demands through Controller and the real public decoder.
//! All catalog/account data is synthetic. Time is explicitly controlled.
use super::*;
use criterion_account::{ContinueWatching, Position};
use criterion_provider::{Request, Response};
use criterion_ui::{LoadState, LoginView};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[derive(Clone, Default)]
struct Clock(Arc<AtomicU64>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
struct Public(Arc<AtomicUsize>);
impl RequestTransport for Public {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        if request.url.path() != "/" {
            return Err(Error::Unavailable);
        }
        self.0.fetch_add(1, Ordering::SeqCst);
        let blocks = serde_json::json!([
            {"type":20,"id":1,"header":"Public before","playlistType":"playlist",
             "imageJWLabel":"default_16x9","imageAspectRatio":56.25,"galleryPageNum":4,"galleryWrap":0,
             "playlist":[{"contentType":"film","mediaid":"Public01","title":"Public film",
                          "duration":90,"deeplink":"/films/Public01/public-film"}]},
            {"type":20,"id":2,"header":"Supplied saved films","playlistType":"continueWatching",
             "imageJWLabel":"default_16x9","imageAspectRatio":56.25,"galleryPageNum":4,"galleryWrap":0},
            {"type":20,"id":3,"header":"Public after","playlistType":"playlist",
             "imageJWLabel":"default_16x9","imageAspectRatio":56.25,"galleryPageNum":4,"galleryWrap":0,
             "playlist":[]}
        ]);
        let stream = format!(
            "baf:I[37,[],\"LanderStoryBlocks\"]\nace:{}\n",
            serde_json::json!(["$","$Lbaf",null,{"blocks":blocks}])
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
fn fixture() -> (Controller<Public, Clock>, Runtime, Clock, Arc<AtomicUsize>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let clock = Clock::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut owner = Controller::with_clock(
        Catalog::with_transport(Public(calls.clone())),
        runtime.handle(),
        clock.clone(),
    );
    for _ in 0..128 {
        owner.poll(&runtime);
        if owner
            .view
            .with_view(LoginView::SignedOut, |view| view.status == LoadState::Ready)
        {
            owner.set_account_session(Some(1));
            return (owner, runtime, clock, calls);
        }
        runtime.block_on(tokio::task::yield_now());
    }
    panic!("synthetic supplied Home must be admitted through the public decoder");
}
fn saved(title: &str) -> ContinueWatching {
    let id = MediaId::new("Private1").unwrap();
    ContinueWatching {
        playlist: vec![criterion_account::MediaSummary {
            id: id.clone(),
            title: title.into(),
            kind: criterion_account::MediaKind::Film,
            duration: Some(90.0),
            release_date: None,
            series_id: None,
            series_title: None,
        }],
        positions: vec![Position {
            media_id: id,
            pos: 98,
            dur: 100,
            commentary_track: None,
            series_id: None,
            series_title: None,
        }],
    }
}
type Rows = Vec<(String, Vec<(String, Option<f32>)>)>;
fn rows(owner: &Controller<Public, Clock>) -> Rows {
    owner.view.with_view(LoginView::SignedIn, |view| {
        view.rails
            .iter()
            .map(|rail| {
                (
                    rail.title.to_owned(),
                    rail.cards
                        .iter()
                        .map(|card| (card.title.to_owned(), card.saved_fraction))
                        .collect(),
                )
            })
            .collect()
    })
}
fn private_titles(owner: &Controller<Public, Clock>) -> Vec<String> {
    rows(owner)
        .into_iter()
        .filter(|(title, _)| title == "Supplied saved films")
        .flat_map(|(_, cards)| cards.into_iter().map(|(title, _)| title))
        .collect()
}
#[test]
fn refresh_keeps_original_deadline_and_failure_preserves_public_home() {
    let (mut owner, runtime, clock, _) = fixture();
    let first = owner.begin_continue_watching(1).unwrap();
    clock.0.store(59, Ordering::SeqCst);
    let renewed = owner.renew_continue_watching().unwrap();
    assert_eq!(first, renewed);
    clock.0.store(60, Ordering::SeqCst);
    assert!(owner.continue_watching_expired());
    owner.admit_continue_watching(&renewed, saved("Late private film"));
    assert!(private_titles(&owner).is_empty());
    assert!(
        owner.begin_continue_watching(1).is_none(),
        "ordinary polls must not retry a failed gallery"
    );
    assert!(
        rows(&owner)
            .iter()
            .any(|(title, cards)| title == "Public before"
                && cards.iter().any(|(title, _)| title == "Public film"))
    );
    owner.command(Command::Navigate(Page::Home), Page::Home, runtime.handle());
    for _ in 0..128 {
        owner.poll(&runtime);
        runtime.block_on(tokio::task::yield_now());
    }
    let next = owner.begin_continue_watching(1).unwrap();
    assert_ne!(first, next);
    owner.admit_continue_watching(&first, saved("Wrong demand"));
    assert!(private_titles(&owner).is_empty());
    owner.admit_continue_watching(&next, saved("Current private film"));
    assert_eq!(private_titles(&owner), ["Current private film"]);
}
#[test]
fn back_resumes_cancelled_gallery_but_never_publishes_departed_response() {
    let (mut owner, runtime, _, _) = fixture();
    let departed = owner.begin_continue_watching(1).unwrap();
    owner.command(
        Command::Navigate(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    assert!(!owner.continue_watching_owns(&departed));
    owner.command(Command::Restore(Page::Home), Page::Home, runtime.handle());
    let current = owner.begin_continue_watching(1).unwrap();
    assert_ne!(
        departed, current,
        "history must not lower the operation counter"
    );
    owner.admit_continue_watching(&departed, saved("Departed private film"));
    assert!(private_titles(&owner).is_empty());
    owner.admit_continue_watching(&current, saved("Saved private film"));
    assert_eq!(private_titles(&owner), ["Saved private film"]);
    owner.command(
        Command::Navigate(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    owner.set_account_session(None);
    owner.command(Command::Restore(Page::Home), Page::Home, runtime.handle());
    assert!(
        private_titles(&owner).is_empty(),
        "logout scrubs retained mixed public/private history"
    );
    assert!(
        rows(&owner)
            .iter()
            .any(|(title, _)| title == "Public before")
    );
    owner.set_account_session(Some(2));
    let next = owner.begin_continue_watching(2).unwrap();
    owner.admit_continue_watching(&current, saved("Previous subscriber"));
    owner.admit_continue_watching(&next, saved("New subscriber"));
    assert_eq!(private_titles(&owner), ["New subscriber"]);
}
#[test]
fn background_retires_pending_work_and_ready_rows_survive_without_refetch() {
    let (mut owner, runtime, _, calls) = fixture();
    let departed = owner.begin_continue_watching(1).unwrap();
    owner.background();
    assert!(!owner.continue_watching_owns(&departed));
    assert!(owner.begin_continue_watching(1).is_none());
    owner.admit_continue_watching(&departed, saved("Background private film"));
    assert!(private_titles(&owner).is_empty());
    owner.foreground(runtime.handle());
    let current = owner.begin_continue_watching(1).unwrap();
    owner.admit_continue_watching(&current, saved("Saved private film"));
    owner.background();
    owner.foreground(runtime.handle());
    assert!(
        owner.begin_continue_watching(1).is_none(),
        "ready gallery has no unsolicited foreground read"
    );
    assert_eq!(private_titles(&owner), ["Saved private film"]);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "settled public Home survives backgrounding"
    );
    let supplied = rows(&owner)
        .into_iter()
        .find(|(title, _)| title == "Supplied saved films")
        .unwrap();
    assert_eq!(
        supplied.1[0].1,
        Some(0.98),
        "near-complete saved films remain visible"
    );
}

#[test]
fn positions_snapshot_is_returned_only_with_successful_current_gallery_publication() {
    let (mut owner, runtime, _, _) = fixture();
    let old = owner.begin_continue_watching(1).unwrap();
    owner.command(
        Command::Navigate(Page::Search),
        Page::Search,
        runtime.handle(),
    );
    assert!(
        owner
            .admit_continue_watching(&old, saved("Departed private title"))
            .is_none()
    );
    assert!(private_titles(&owner).is_empty());
    owner.command(Command::Restore(Page::Home), Page::Home, runtime.handle());
    let current = owner.begin_continue_watching(1).unwrap();
    let mut data = saved("Current private title");
    data.positions.push(Position {
        media_id: MediaId::new("Other001").unwrap(),
        pos: i64::MAX,
        dur: 100,
        commentary_track: Some("Synthetic context".into()),
        series_id: None,
        series_title: None,
    });
    let snapshot = owner.admit_continue_watching(&current, data).unwrap();
    assert_eq!(private_titles(&owner), ["Current private title"]);
    assert_eq!(
        snapshot
            .position(&MediaId::new("Other001").unwrap())
            .unwrap()
            .pos,
        i64::MAX
    );
    assert!(
        snapshot
            .position(&MediaId::new("Other002").unwrap())
            .is_none(),
        "exact lookup cannot choose a neighboring indexed row"
    );
    assert!(
        owner
            .admit_continue_watching(&current, saved("Replay private title"))
            .is_none()
    );
    assert_eq!(private_titles(&owner), ["Current private title"]);
}
