// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure native My List state. The application owns Session epochs, execution and
//! deadlines. Every returned cursor was admitted from this group's response.
use aws_lc_rs::digest::{Context, SHA256};
use criterion_account::{MediaKind, MediaSummary, WatchList, WatchListFilter, WatchListRequest};
use criterion_provider::PageCursor;
use std::mem::size_of;

// Local retention and processing policy, independent of provider page/count claims.
const CARDS: usize = 180;
const CARD_BYTES: usize = 512 * 1024;
const CHECKPOINTS: usize = 256;
const KEYS: usize = 4096;
const META_BYTES: usize = 192 * 1024;
const RAW_ROWS: usize = 512;
const CURSOR_BYTES: usize = 512;
const NEAR_TAIL: usize = 12;
const FILTERS: [WatchListFilter; 6] = [
    WatchListFilter::All,
    WatchListFilter::FilmSeries,
    WatchListFilter::Collection,
    WatchListFilter::OriginalFranchise,
    WatchListFilter::Supplement,
    WatchListFilter::Category,
];

#[derive(Clone)]
pub(crate) struct Read {
    pub(crate) operation: u64,
    pub(crate) request: WatchListRequest,
}
impl std::fmt::Debug for Read {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MyListRead")
            .field("operation", &self.operation)
            .field("filter", &self.request.filter)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Read,
    ResourceLimit,
    Changed,
    CursorCycle,
    Stale,
    Retired,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tail {
    More,
    Loading,
    Error(Failure),
    End,
}
pub(crate) struct View<'a> {
    pub(crate) filter: WatchListFilter,
    pub(crate) rows: &'a [MediaSummary],
    pub(crate) first: usize,
    pub(crate) anchor: usize,
    pub(crate) target: usize,
    pub(crate) loaded: bool,
    pub(crate) tail: Tail,
}
pub(crate) struct Group {
    pub(crate) filter: WatchListFilter,
    pub(crate) count: i64,
    pub(crate) available: bool,
}
// Vector order is the first admitted ordinal. Only a compact first-value digest
// survives window eviction; replay must reproduce that native tuple exactly.
#[derive(Clone)]
struct Key {
    id: [u8; 8],
    fingerprint: [u8; 32],
}
#[derive(Clone)]
struct Checkpoint {
    input: Option<PageCursor>,
    next: Option<PageCursor>,
    first: usize,
    count: usize,
}
#[derive(Clone, Copy)]
struct Span {
    first: usize,
    count: usize,
}
#[derive(Clone, Copy)]
enum Mode {
    New,
    Known { checkpoint: usize, append: bool },
}
#[derive(Clone)]
struct Work {
    read: Read,
    mode: Mode,
}
#[derive(Clone, Default)]
struct GroupState {
    rows: Vec<MediaSummary>,
    spans: Vec<Span>,
    keys: Vec<Key>,
    checkpoints: Vec<Checkpoint>,
    next: Option<PageCursor>,
    loaded: bool,
    first: usize,
    anchor: usize,
    target: usize,
    resume: Option<Work>,
    failure: Option<Failure>,
}
fn index(filter: WatchListFilter) -> usize {
    FILTERS
        .iter()
        .position(|f| *f == filter)
        .expect("closed filter")
}
fn key(media: &MediaSummary) -> [u8; 8] {
    media
        .id
        .as_str()
        .as_bytes()
        .try_into()
        .expect("validated media ID")
}
fn fingerprint(media: &MediaSummary) -> [u8; 32] {
    let mut hash = Context::new(&SHA256);
    hash.update(&key(media));
    hash.update(&(media.title.len() as u64).to_le_bytes());
    hash.update(media.title.as_bytes());
    hash.update(&[match media.kind {
        MediaKind::Category => 0,
        MediaKind::Collection => 1,
        MediaKind::Series => 2,
        MediaKind::Original => 3,
        MediaKind::Episode => 4,
        MediaKind::Franchise => 5,
        MediaKind::Live => 6,
        MediaKind::Film => 7,
        MediaKind::Supplement => 8,
    }]);
    match media.duration {
        None => hash.update(&[0]),
        Some(value) => {
            hash.update(&[1]);
            hash.update(&value.to_bits().to_le_bytes());
        }
    }
    match media.release_date {
        None => hash.update(&[0]),
        Some(date) => {
            hash.update(&[1]);
            hash.update(&date.year().to_le_bytes());
            hash.update(&[date.month() as u8, date.day()]);
        }
    }
    hash.finish().as_ref().try_into().expect("SHA256 length")
}
fn row_bytes(rows: &[MediaSummary], capacity: usize) -> usize {
    capacity * size_of::<MediaSummary>()
        + rows.iter().map(|r| r.title.capacity() + 8).sum::<usize>()
}
fn cursor_bytes(cursor: &Option<PageCursor>) -> usize {
    usize::from(cursor.is_some()) * CURSOR_BYTES
}
fn metadata(group: &GroupState) -> usize {
    group.keys.capacity() * size_of::<Key>()
        + group.checkpoints.capacity() * size_of::<Checkpoint>()
        + group.spans.capacity() * size_of::<Span>()
        + cursor_bytes(&group.next)
        + group
            .checkpoints
            .iter()
            .map(|c| cursor_bytes(&c.input) + cursor_bytes(&c.next))
            .sum::<usize>()
        + group
            .resume
            .as_ref()
            .map_or(0, |w| cursor_bytes(&w.read.request.cursor))
}
#[derive(Clone)]
pub(crate) struct MyListState {
    operation: u64,
    filter: WatchListFilter,
    pending: Option<Work>,
    groups: [GroupState; 6],
    counts: [i64; 6],
    retired: bool,
}
impl MyListState {
    pub(crate) fn new() -> Self {
        Self {
            operation: 0,
            filter: WatchListFilter::All,
            pending: None,
            groups: std::array::from_fn(|_| GroupState::default()),
            counts: [0; 6],
            retired: false,
        }
    }
    fn active(&self) -> Result<(), Failure> {
        if self.retired {
            Err(Failure::Retired)
        } else {
            Ok(())
        }
    }
    fn issue(&mut self, input: Option<PageCursor>, mode: Mode) -> Result<Option<Read>, Failure> {
        self.operation = self
            .operation
            .checked_add(1)
            .ok_or(Failure::ResourceLimit)?;
        let read = Read {
            operation: self.operation,
            request: WatchListRequest {
                filter: self.filter,
                cursor: input,
            },
        };
        let group = &mut self.groups[index(self.filter)];
        group.resume = None;
        group.failure = None;
        self.pending = Some(Work {
            read: read.clone(),
            mode,
        });
        Ok(Some(read))
    }
    fn known(&mut self, ordinal: usize, append: bool) -> Result<Option<Read>, Failure> {
        let group = &self.groups[index(self.filter)];
        let checkpoint = group
            .checkpoints
            .iter()
            .position(|c| ordinal >= c.first && ordinal < c.first + c.count)
            .ok_or(Failure::Changed)?;
        self.issue(
            group.checkpoints[checkpoint].input.clone(),
            Mode::Known { checkpoint, append },
        )
    }
    fn continuation(&mut self) -> Result<Option<Read>, Failure> {
        let group = &self.groups[index(self.filter)];
        let Some(input) = group.next.clone() else {
            return Ok(None);
        };
        if let Some(checkpoint) = group
            .checkpoints
            .iter()
            .position(|c| c.input.as_ref() == Some(&input))
        {
            let append = group.checkpoints[checkpoint].first == group.first + group.rows.len();
            self.issue(Some(input), Mode::Known { checkpoint, append })
        } else {
            self.issue(Some(input), Mode::New)
        }
    }
    pub(crate) fn select(&mut self, filter: WatchListFilter) -> Result<Option<Read>, Failure> {
        self.active()?;
        if self.filter != filter {
            self.cancel();
            self.filter = filter;
        }
        if self.pending.is_some() {
            return Ok(None);
        }
        let group = &self.groups[index(filter)];
        if group.failure.is_some() {
            return Ok(None);
        }
        if !group.loaded {
            return self.issue(None, Mode::New);
        }
        if group.rows.is_empty() && !group.keys.is_empty() {
            return self.known(group.anchor, false);
        }
        if group.rows.is_empty() {
            return self.continuation();
        }
        Ok(None)
    }
    pub(crate) fn demand(&mut self, anchor: usize, target: usize) -> Result<Option<Read>, Failure> {
        self.active()?;
        let selected = index(self.filter);
        let group = &self.groups[selected];
        if target >= KEYS || (!group.keys.is_empty() && anchor >= group.keys.len()) {
            self.cancel();
            self.groups[selected].failure = Some(Failure::ResourceLimit);
            return Err(Failure::ResourceLimit);
        }
        let obsolete = self
            .pending
            .as_ref()
            .or(group.resume.as_ref())
            .is_some_and(|work| {
                !group.rows.is_empty()
                    && match work.mode {
                        Mode::New | Mode::Known { append: true, .. } => {
                            target >= group.first
                                && target.saturating_add(NEAR_TAIL) < group.first + group.rows.len()
                        }
                        Mode::Known { append: false, .. } => target != group.target,
                    }
            });
        if obsolete {
            self.cancel();
            let group = &mut self.groups[selected];
            group.resume = None;
            group.failure = None;
        }
        let group = &mut self.groups[selected];
        group.anchor = anchor;
        group.target = target;
        if self.pending.is_some() || group.failure.is_some() {
            return Ok(None);
        }
        group.resume = None;
        if !group.loaded {
            return self.issue(None, Mode::New);
        }
        if group.rows.is_empty() && !group.keys.is_empty() {
            return self.known(anchor, false);
        }
        let end = group.first + group.rows.len();
        if target < group.keys.len() && (target < group.first || target >= end) {
            return self.known(target, target == end);
        }
        if target >= end.saturating_sub(NEAR_TAIL) {
            self.continuation()
        } else {
            Ok(None)
        }
    }
    pub(crate) fn retry(&mut self) -> Result<Option<Read>, Failure> {
        self.active()?;
        if self.pending.is_some() {
            return Ok(None);
        }
        let group = &self.groups[index(self.filter)];
        if group.loaded && group.rows.is_empty() && !group.keys.is_empty() {
            return self.known(group.anchor, false);
        }
        if let Some(work) = self.groups[index(self.filter)].resume.take() {
            self.issue(work.read.request.cursor, work.mode)
        } else {
            let group = &self.groups[index(self.filter)];
            let (anchor, target) = (group.anchor, group.target);
            self.groups[index(self.filter)].failure = None;
            self.demand(anchor, target)
        }
    }
    fn matches(&self, read: &Read) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|w| w.read.operation == read.operation && w.read.request == read.request)
    }
    pub(crate) fn fail(&mut self, read: &Read, failure: Failure) -> bool {
        if !self.matches(read) {
            return false;
        }
        let group = &mut self.groups[index(self.filter)];
        group.resume = self.pending.take();
        group.failure = Some(failure);
        true
    }
    pub(crate) fn cancel(&mut self) {
        if let Some(work) = self.pending.take() {
            let group = &mut self.groups[index(work.read.request.filter)];
            group.resume = Some(work);
            group.failure = None;
        }
    }
    /// History may release card allocations without forgetting private traversal.
    pub(crate) fn evict_windows(&mut self) {
        self.cancel();
        for group in &mut self.groups {
            group.rows = Vec::new();
            group.spans = Vec::new();
        }
    }
    /// Terminal privacy disposal: this owner cannot be reused for another epoch.
    pub(crate) fn retire(&mut self) {
        self.pending = None;
        self.groups = std::array::from_fn(|_| GroupState::default());
        self.counts = [0; 6];
        self.retired = true;
    }
    fn refusal(&mut self, read: &Read, failure: Failure) -> Result<Option<Read>, Failure> {
        self.fail(read, failure);
        Err(failure)
    }
    pub(crate) fn admit(&mut self, read: &Read, page: WatchList) -> Result<Option<Read>, Failure> {
        self.active()?;
        if !self.matches(read) {
            return Err(Failure::Stale);
        }
        if page.playlist.len() > RAW_ROWS
            || page.type_counts.len() > 128
            || row_bytes(&page.playlist, page.playlist.capacity()) > CARD_BYTES
        {
            return self.refusal(read, Failure::ResourceLimit);
        }
        let selected = index(self.filter);
        let work = self.pending.as_ref().expect("matched read").clone();
        let group = &self.groups[selected];
        let mut keys = Vec::with_capacity(group.keys.len() + page.playlist.len());
        keys.extend(group.keys.iter().cloned());
        let mut checkpoints = Vec::with_capacity(group.checkpoints.len() + 1);
        checkpoints.extend(group.checkpoints.iter().cloned());
        let mut incoming = Vec::with_capacity(page.playlist.len());
        let (first, append) = match work.mode {
            Mode::New => (keys.len(), true),
            Mode::Known { checkpoint, append } => {
                let c = &checkpoints[checkpoint];
                if c.next != page.paging.next_pagination_key {
                    return self.refusal(read, Failure::Changed);
                }
                (c.first, append)
            }
        };
        for media in page.playlist {
            let id = key(&media);
            let ordinal = keys.iter().position(|k| k.id == id);
            match work.mode {
                Mode::New => {
                    if ordinal.is_none() {
                        keys.push(Key {
                            id,
                            fingerprint: fingerprint(&media),
                        });
                        incoming.push(media);
                    }
                }
                Mode::Known { checkpoint, .. } => {
                    let c = &checkpoints[checkpoint];
                    let Some(ordinal) = ordinal else {
                        return self.refusal(read, Failure::Changed);
                    };
                    if ordinal < c.first || incoming.iter().any(|r| key(r) == id) {
                        continue;
                    }
                    if ordinal != c.first + incoming.len()
                        || ordinal >= c.first + c.count
                        || fingerprint(&media) != keys[ordinal].fingerprint
                    {
                        return self.refusal(read, Failure::Changed);
                    }
                    incoming.push(media);
                }
            }
        }
        match work.mode {
            Mode::Known { checkpoint, .. } => {
                if incoming.len() != checkpoints[checkpoint].count {
                    return self.refusal(read, Failure::Changed);
                }
            }
            Mode::New => {
                if let Some(next) = &page.paging.next_pagination_key
                    && (read.request.cursor.as_ref() == Some(next)
                        || checkpoints.iter().any(|c| c.input.as_ref() == Some(next)))
                {
                    return self.refusal(read, Failure::CursorCycle);
                }
                checkpoints.push(Checkpoint {
                    input: read.request.cursor.clone(),
                    next: page.paging.next_pagination_key.clone(),
                    first,
                    count: incoming.len(),
                });
            }
        }
        if incoming.len() > CARDS || row_bytes(&incoming, incoming.len()) > CARD_BYTES {
            return self.refusal(read, Failure::ResourceLimit);
        }
        let mut spans = Vec::with_capacity(group.spans.len() + 1);
        if append {
            spans.extend(group.spans.iter().copied());
        }
        if !incoming.is_empty() {
            spans.push(Span {
                first,
                count: incoming.len(),
            });
        }
        let mut drop_rows = 0;
        let old_len = if append { group.rows.len() } else { 0 };
        while old_len - drop_rows + incoming.len() > CARDS
            || row_bytes(
                &group.rows[if append { drop_rows } else { group.rows.len() }..],
                old_len - drop_rows + incoming.len(),
            ) + incoming
                .iter()
                .map(|r| r.title.capacity() + 8)
                .sum::<usize>()
                > CARD_BYTES
        {
            let Some(span) = spans.first().copied() else {
                return self.refusal(read, Failure::ResourceLimit);
            };
            if span.first >= first {
                return self.refusal(read, Failure::ResourceLimit);
            }
            drop_rows += span.count;
            spans.remove(0);
        }
        let rows_len = old_len - drop_rows + incoming.len();
        let new_first = spans.first().map_or(first, |s| s.first);
        let new_end = new_first + rows_len;
        if append
            && group.loaded
            && !group.rows.is_empty()
            && group.anchor < new_first
            && !(group.target >= new_first && group.target < new_end)
        {
            return self.refusal(read, Failure::ResourceLimit);
        }
        // Charge exact prospective vector capacities before moving committed rows.
        keys.shrink_to_fit();
        checkpoints.shrink_to_fit();
        spans.shrink_to_fit();
        let candidate = GroupState {
            keys,
            checkpoints,
            spans,
            next: page.paging.next_pagination_key,
            loaded: true,
            first: new_first,
            anchor: group.anchor,
            target: group.target,
            ..GroupState::default()
        };
        let key_count = candidate.keys.len()
            + self
                .groups
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != selected)
                .map(|(_, g)| g.keys.len())
                .sum::<usize>();
        let checkpoint_count = candidate.checkpoints.len()
            + self
                .groups
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != selected)
                .map(|(_, g)| g.checkpoints.len())
                .sum::<usize>();
        let meta = size_of::<Self>()
            + CURSOR_BYTES
            + metadata(&candidate)
            + self
                .groups
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != selected)
                .map(|(_, g)| metadata(g))
                .sum::<usize>();
        if key_count > KEYS || checkpoint_count > CHECKPOINTS || meta > META_BYTES {
            return self.refusal(read, Failure::ResourceLimit);
        };
        let candidate_bytes = row_bytes(
            &group.rows[if append { drop_rows } else { group.rows.len() }..],
            rows_len,
        ) + incoming
            .iter()
            .map(|r| r.title.capacity() + 8)
            .sum::<usize>();
        let mut evict = [false; 6];
        let mut total_rows = rows_len
            + self
                .groups
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != selected)
                .map(|(_, g)| g.rows.len())
                .sum::<usize>();
        let mut total_bytes = candidate_bytes
            + self
                .groups
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != selected)
                .map(|(_, g)| row_bytes(&g.rows, g.rows.capacity()))
                .sum::<usize>();
        for (i, g) in self.groups.iter().enumerate() {
            if i != selected && (total_rows > CARDS || total_bytes > CARD_BYTES) {
                evict[i] = true;
                total_rows -= g.rows.len();
                total_bytes -= row_bytes(&g.rows, g.rows.capacity());
            }
        }
        let counts = if self.filter == WatchListFilter::All && !group.loaded {
            grouped_counts(&page.type_counts)
        } else {
            self.counts
        };
        let empty = incoming.is_empty();
        let mut rows = Vec::with_capacity(rows_len);
        if append {
            rows.extend(
                std::mem::take(&mut self.groups[selected].rows)
                    .into_iter()
                    .skip(drop_rows),
            );
        }
        rows.extend(incoming);
        let mut candidate = candidate;
        candidate.rows = rows;
        if candidate.target >= new_first
            && candidate.target < new_end
            && (candidate.anchor < new_first || candidate.anchor >= new_end)
        {
            candidate.anchor = candidate.target;
        }
        self.groups[selected] = candidate;
        for (i, drop) in evict.into_iter().enumerate() {
            if drop {
                self.groups[i].rows = Vec::new();
                self.groups[i].spans = Vec::new();
            }
        }
        self.counts = counts;
        self.pending = None;
        let group = &self.groups[selected];
        if !group.rows.is_empty() && group.target < group.first {
            self.known(group.target, false)
        } else if group.next.is_some()
            && (empty || group.rows.is_empty() || group.target >= group.first + group.rows.len())
        {
            self.continuation()
        } else {
            Ok(None)
        }
    }
    pub(crate) fn view(&self) -> View<'_> {
        let group = &self.groups[index(self.filter)];
        View {
            filter: self.filter,
            rows: &group.rows,
            first: group.first,
            anchor: group.anchor,
            target: group.target,
            loaded: group.loaded,
            tail: if self.pending.is_some() {
                Tail::Loading
            } else if let Some(failure) = group.failure {
                Tail::Error(failure)
            } else if !self.retired && (!group.loaded || group.next.is_some()) {
                Tail::More
            } else {
                Tail::End
            },
        }
    }
    pub(crate) fn groups(&self) -> [Group; 6] {
        std::array::from_fn(|i| Group {
            filter: FILTERS[i],
            count: self.counts[i],
            available: i == 0 || self.counts[i] > 0,
        })
    }
    /// Charges retained capacities and conservative validated cursor allocations.
    pub(crate) fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + self
                .groups
                .iter()
                .map(|g| metadata(g) + row_bytes(&g.rows, g.rows.capacity()))
                .sum::<usize>()
            + self
                .pending
                .as_ref()
                .map_or(0, |w| cursor_bytes(&w.read.request.cursor))
    }
}
fn grouped_counts(counts: &[criterion_account::TypeCount]) -> [i64; 6] {
    let mut result = [0_i64; 6];
    for count in counts {
        let group = match count.content_type.as_str() {
            "film" | "series" => Some(1),
            "collection" => Some(2),
            "original" | "franchise" => Some(3),
            "supplement" => Some(4),
            "category" => Some(5),
            "episode" => None,
            _ => continue,
        };
        result[0] = result[0]
            .checked_add(i64::from(count.count))
            .expect("bounded native counts");
        if let Some(i) = group {
            result[i] = result[i]
                .checked_add(i64::from(count.count))
                .expect("bounded native counts");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_an_unvisited_group_requests_its_exact_first_page() {
        let mut state = MyListState::new();
        let read = state
            .select(WatchListFilter::FilmSeries)
            .unwrap()
            .expect("first page");
        assert_eq!(
            read.request,
            WatchListRequest {
                filter: WatchListFilter::FilmSeries,
                cursor: None
            }
        );
        assert_eq!(read.operation, 1);
    }
    fn media(id: &str, title: &str, kind: criterion_account::MediaKind) -> MediaSummary {
        MediaSummary {
            id: criterion_provider::MediaId::new(id).unwrap(),
            title: title.into(),
            kind,
            duration: Some(90.5),
            release_date: None,
        }
    }
    fn page(rows: Vec<MediaSummary>, next: Option<&str>) -> WatchList {
        WatchList {
            playlist: rows,
            paging: criterion_account::PagingInfo {
                page_limit: -1,
                next_pagination_key: next.map(|c| criterion_provider::PageCursor::new(c).unwrap()),
            },
            type_counts: Vec::new(),
        }
    }
    #[test]
    fn first_native_row_key_wins_with_original_values_and_order() {
        let mut state = MyListState::new();
        let read = state.select(WatchListFilter::All).unwrap().unwrap();
        let response = page(
            vec![
                media("FixtureA", "First film", criterion_account::MediaKind::Film),
                media(
                    "FixtureA",
                    "Duplicate supplement",
                    criterion_account::MediaKind::Supplement,
                ),
                media("FixtureB", "Second", criterion_account::MediaKind::Original),
            ],
            None,
        );
        assert!(state.admit(&read, response).unwrap().is_none());
        let view = state.view();
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[0].title, "First film");
        assert_eq!(view.rows[0].kind, criterion_account::MediaKind::Film);
        assert_eq!(view.rows[0].duration, Some(90.5));
        assert_eq!(view.rows[1].id.as_str(), "FixtureB");
    }

    #[test]
    fn continuation_uses_only_the_observed_cursor_and_appends_first_keys() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(
                &first,
                page(
                    vec![
                        media("FixtureA", "First", criterion_account::MediaKind::Film),
                        media("FixtureB", "Second", criterion_account::MediaKind::Series),
                    ],
                    Some("opaque +/=% one"),
                ),
            )
            .unwrap();
        let next = state.demand(1, 2).unwrap().expect("observed continuation");
        assert_eq!(
            next.request.cursor.as_ref().unwrap().as_str(),
            "opaque +/=% one"
        );
        assert_eq!(next.request.filter, WatchListFilter::All);
        state
            .admit(
                &next,
                page(
                    vec![
                        media(
                            "FixtureA",
                            "Later duplicate",
                            criterion_account::MediaKind::Supplement,
                        ),
                        media("FixtureC", "Third", criterion_account::MediaKind::Category),
                    ],
                    None,
                ),
            )
            .unwrap();
        let view = state.view();
        assert_eq!(
            view.rows
                .iter()
                .map(|m| m.title.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second", "Third"]
        );
        assert_eq!(view.tail, Tail::End);
        assert!(state.demand(2, 3).unwrap().is_none());
    }

    #[test]
    fn group_windows_and_cursors_are_independent_and_warm_return_keeps_values() {
        let mut state = MyListState::new();
        let all = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(
                &all,
                page(
                    vec![media(
                        "FixtureA",
                        "All first",
                        criterion_account::MediaKind::Film,
                    )],
                    Some("all cursor"),
                ),
            )
            .unwrap();
        let film = state.select(WatchListFilter::FilmSeries).unwrap().unwrap();
        state
            .admit(
                &film,
                page(
                    vec![media(
                        "FixtureA",
                        "Filtered same key",
                        criterion_account::MediaKind::Film,
                    )],
                    Some("film cursor"),
                ),
            )
            .unwrap();
        assert!(state.select(WatchListFilter::All).unwrap().is_none());
        assert_eq!(state.view().rows[0].title, "All first");
        let next = state.demand(0, 1).unwrap().unwrap();
        assert_eq!(next.request.cursor.as_ref().unwrap().as_str(), "all cursor");
        state.admit(&next, page(Vec::new(), None)).unwrap();
        assert!(state.select(WatchListFilter::FilmSeries).unwrap().is_none());
        assert_eq!(state.view().rows[0].title, "Filtered same key");
        assert_eq!(
            state
                .demand(0, 1)
                .unwrap()
                .unwrap()
                .request
                .cursor
                .unwrap()
                .as_str(),
            "film cursor"
        );
    }

    fn films(first: usize, count: usize) -> Vec<MediaSummary> {
        (first..first + count)
            .map(|i| {
                media(
                    &format!("F{i:07X}"),
                    &format!("Film {i}"),
                    criterion_account::MediaKind::Film,
                )
            })
            .collect()
    }
    #[test]
    fn committed_card_window_is_bounded_and_keeps_global_positions() {
        let mut state = MyListState::new();
        let mut read = state.select(WatchListFilter::All).unwrap().unwrap();
        for number in 0..4 {
            let next = format!("opaque {number} page");
            state
                .admit(&read, page(films(number * 50, 50), Some(&next)))
                .unwrap();
            if number < 3 {
                read = state
                    .demand(number * 50 + 49, number * 50 + 50)
                    .unwrap()
                    .unwrap();
            }
        }
        let view = state.view();
        assert_eq!(view.rows.len(), 150);
        assert_eq!(view.first, 50);
        assert_eq!(view.rows[0].title, "Film 50");
        assert_eq!(view.rows[149].title, "Film 199");
    }

    #[test]
    fn backward_replay_restores_original_ordinals_and_refuses_changed_first_values() {
        let mut state = MyListState::new();
        let mut read = state.select(WatchListFilter::All).unwrap().unwrap();
        for number in 0..4 {
            let next = format!("opaque {number} page");
            state
                .admit(&read, page(films(number * 50, 50), Some(&next)))
                .unwrap();
            if number < 3 {
                read = state
                    .demand(number * 50 + 49, number * 50 + 50)
                    .unwrap()
                    .unwrap();
            }
        }
        let back = state.demand(151, 0).unwrap().unwrap();
        assert!(
            back.request.cursor.is_none(),
            "observed first-page input cursor"
        );
        state
            .admit(&back, page(films(0, 50), Some("opaque 0 page")))
            .unwrap();
        assert_eq!(state.view().first, 0);
        assert_eq!(state.view().rows.len(), 50);
        assert_eq!(state.view().rows[0].title, "Film 0");
        let later = state.demand(49, 199).unwrap().unwrap();
        assert_eq!(
            later.request.cursor.as_ref().unwrap().as_str(),
            "opaque 2 page"
        );
        let mut changed = films(150, 50);
        changed[49].title = "Changed first value".into();
        assert!(matches!(
            state.admit(&later, page(changed, Some("opaque 3 page"))),
            Err(Failure::Changed)
        ));
        assert_eq!(state.view().first, 0);
        assert_eq!(state.view().rows.len(), 50);
        assert_eq!(state.view().rows[0].title, "Film 0");
    }

    #[test]
    fn history_eviction_rehydrates_saved_anchor_before_resuming_continuation() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 50), Some("second")))
            .unwrap();
        let second = state.demand(49, 50).unwrap().unwrap();
        state
            .admit(&second, page(films(50, 50), Some("third")))
            .unwrap();
        let third = state.demand(99, 100).unwrap().unwrap();
        let before = state.retained_bytes();
        state.evict_windows();
        assert!(state.view().rows.is_empty());
        assert!(state.retained_bytes() < before);
        assert_eq!((state.view().anchor, state.view().target), (99, 100));
        assert_eq!(state.view().tail, Tail::More);
        assert!(matches!(
            state.admit(&third, page(films(100, 50), None)),
            Err(Failure::Stale)
        ));
        let restore = state.retry().unwrap().unwrap();
        assert_eq!(restore.request.cursor.as_ref().unwrap().as_str(), "second");
        assert!(restore.operation > third.operation);
        let follow = state
            .admit(&restore, page(films(50, 50), Some("third")))
            .unwrap()
            .unwrap();
        assert_eq!(state.view().first, 50);
        assert_eq!(state.view().rows[49].title, "Film 99");
        assert_eq!(state.view().anchor, 99);
        assert_eq!(follow.request.cursor.as_ref().unwrap().as_str(), "third");
        state.admit(&follow, page(films(100, 50), None)).unwrap();
        assert_eq!(state.view().first, 50);
        assert_eq!(state.view().rows.len(), 100);
    }

    #[test]
    fn cancellation_retry_and_failure_keep_the_exact_intent_and_committed_tail() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 50), Some("private cursor")))
            .unwrap();
        assert!(state.demand(0, 10).unwrap().is_none());
        let read = state.demand(49, 50).unwrap().unwrap();
        state.cancel();
        assert_eq!(state.view().tail, Tail::More);
        assert_eq!((state.view().anchor, state.view().target), (49, 50));
        assert!(matches!(
            state.admit(&read, page(films(50, 50), None)),
            Err(Failure::Stale)
        ));
        let retry = state.retry().unwrap().unwrap();
        assert_eq!(read.request, retry.request);
        assert!(retry.operation > read.operation);
        assert!(state.fail(&retry, Failure::Read));
        assert_eq!(state.view().tail, Tail::Error(Failure::Read));
        assert_eq!(state.view().rows.len(), 50);
        assert!(!state.fail(&read, Failure::Changed));
        let retry2 = state.retry().unwrap().unwrap();
        assert_eq!(retry.request, retry2.request);
        state.admit(&retry2, page(films(50, 50), None)).unwrap();
        assert_eq!(state.view().tail, Tail::End);
        let safe = format!("{retry2:?}");
        assert!(!safe.contains("private cursor"));
        assert!(!safe.contains("Film"));
    }

    #[test]
    fn switching_groups_rejects_old_completion_and_terminal_retirement_erases_data() {
        let mut state = MyListState::new();
        let all = state.select(WatchListFilter::All).unwrap().unwrap();
        let films_read = state.select(WatchListFilter::FilmSeries).unwrap().unwrap();
        assert!(matches!(
            state.admit(&all, page(films(0, 5), None)),
            Err(Failure::Stale)
        ));
        assert_eq!(state.view().filter, WatchListFilter::FilmSeries);
        assert!(!state.view().loaded);
        state.admit(&films_read, page(films(50, 50), None)).unwrap();
        assert!(state.view().loaded);
        let before = state.retained_bytes();
        state.retire();
        assert!(state.view().rows.is_empty());
        assert_eq!(state.view().tail, Tail::End);
        assert!(state.retained_bytes() < before);
        assert!(matches!(
            state.select(WatchListFilter::All),
            Err(Failure::Retired)
        ));
        assert!(matches!(state.demand(0, 0), Err(Failure::Retired)));
        assert!(matches!(state.retry(), Err(Failure::Retired)));
        assert!(matches!(
            state.admit(&films_read, page(Vec::new(), None)),
            Err(Failure::Retired)
        ));
        assert!(!state.fail(&films_read, Failure::Read));
    }

    fn counts(values: &[(&str, i32)]) -> Vec<criterion_account::TypeCount> {
        values
            .iter()
            .map(|(kind, count)| criterion_account::TypeCount {
                content_type: (*kind).into(),
                count: *count,
            })
            .collect()
    }
    #[test]
    fn native_count_groups_preserve_signed_values_and_never_select_eof() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        let mut response = page(films(0, 1), Some("despite counts"));
        response.type_counts = counts(&[
            ("film", -3),
            ("series", 2),
            ("collection", 4),
            ("original", 5),
            ("franchise", 6),
            ("supplement", 7),
            ("category", 8),
            ("episode", 9),
            ("live", 100),
            ("unknown", 100),
        ]);
        state.admit(&first, response).unwrap();
        let groups = state.groups();
        assert_eq!(groups.map(|g| g.count), [38, -1, 4, 11, 7, 8]);
        let groups = state.groups();
        assert_eq!(groups.map(|g| g.filter), FILTERS);
        assert_eq!(
            state.groups().map(|g| g.available),
            [true, false, true, true, true, true]
        );
        let next = state.demand(0, 1).unwrap().unwrap();
        let mut response = page(Vec::new(), None);
        response.type_counts = counts(&[("film", 999)]);
        state.admit(&next, response).unwrap();
        assert_eq!(state.groups().map(|g| g.count), [38, -1, 4, 11, 7, 8]);
        let filtered = state.select(WatchListFilter::Collection).unwrap().unwrap();
        let mut response = page(Vec::new(), None);
        response.type_counts = counts(&[("collection", 123)]);
        state.admit(&filtered, response).unwrap();
        assert_eq!(state.groups().map(|g| g.count), [38, -1, 4, 11, 7, 8]);
        let mut negative = MyListState::new();
        let read = negative.select(WatchListFilter::All).unwrap().unwrap();
        let mut response = page(Vec::new(), None);
        response.type_counts = counts(&[("film", -1)]);
        negative.admit(&read, response).unwrap();
        assert_eq!(negative.groups()[0].count, -1);
        assert!(negative.groups()[0].available);
    }

    #[test]
    fn empty_and_duplicate_only_pages_advance_observed_cursors_until_terminal() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        let second = state
            .admit(&first, page(Vec::new(), Some("empty first")))
            .unwrap()
            .unwrap();
        assert_eq!(
            second.request.cursor.as_ref().unwrap().as_str(),
            "empty first"
        );
        state
            .admit(&second, page(films(0, 1), Some("duplicate")))
            .unwrap();
        let duplicate = state.demand(0, 1).unwrap().unwrap();
        let next = state
            .admit(
                &duplicate,
                page(
                    vec![media(
                        "F0000000",
                        "Changed duplicate",
                        criterion_account::MediaKind::Supplement,
                    )],
                    Some("empty tail"),
                ),
            )
            .unwrap()
            .unwrap();
        assert_eq!(state.view().rows[0].title, "Film 0");
        assert_eq!(next.request.cursor.as_ref().unwrap().as_str(), "empty tail");
        state.admit(&next, page(Vec::new(), None)).unwrap();
        assert_eq!(state.view().rows.len(), 1);
        assert_eq!(state.view().tail, Tail::End);
    }

    #[test]
    fn cycle_refusal_is_atomic_and_retry_keeps_observed_input() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 1), Some("repeat")))
            .unwrap();
        let repeat = state.demand(0, 1).unwrap().unwrap();
        let mut response = page(films(1, 1), Some("repeat"));
        response.type_counts = counts(&[("film", 999)]);
        assert!(matches!(
            state.admit(&repeat, response),
            Err(Failure::CursorCycle)
        ));
        assert_eq!(state.view().rows.len(), 1);
        assert_eq!(state.view().tail, Tail::Error(Failure::CursorCycle));
        assert_eq!(state.groups()[0].count, 0);
        let retry = state.retry().unwrap().unwrap();
        assert_eq!(retry.request, repeat.request);
        state.admit(&retry, page(films(1, 1), None)).unwrap();
        assert_eq!(state.view().rows.len(), 2);
    }

    #[test]
    fn raw_rows_card_count_and_card_bytes_have_atomic_visible_refusal() {
        for bad in [
            films(50, 513),
            films(50, 181),
            vec![media(
                "FixtureZ",
                &"x".repeat(512 * 1024),
                criterion_account::MediaKind::Film,
            )],
        ] {
            let mut state = MyListState::new();
            let first = state.select(WatchListFilter::All).unwrap().unwrap();
            state
                .admit(&first, page(films(0, 1), Some("limit input")))
                .unwrap();
            let read = state.demand(0, 1).unwrap().unwrap();
            assert!(matches!(
                state.admit(&read, page(bad, None)),
                Err(Failure::ResourceLimit)
            ));
            assert_eq!(state.view().rows.len(), 1);
            assert_eq!(state.view().rows[0].title, "Film 0");
            assert_eq!(state.view().tail, Tail::Error(Failure::ResourceLimit));
            assert_eq!(state.retry().unwrap().unwrap().request, read.request);
        }
    }

    #[test]
    fn aggregate_windows_evict_cards_but_preserve_other_group_identity_and_anchor() {
        let mut state = MyListState::new();
        let all = state.select(WatchListFilter::All).unwrap().unwrap();
        state.admit(&all, page(films(0, 100), None)).unwrap();
        state.demand(70, 71).unwrap();
        let filtered = state.select(WatchListFilter::FilmSeries).unwrap().unwrap();
        state.admit(&filtered, page(films(200, 100), None)).unwrap();
        let restore = state.select(WatchListFilter::All).unwrap().unwrap();
        assert!(restore.request.cursor.is_none());
        assert_eq!(state.view().anchor, 70);
        assert_eq!(state.view().target, 71);
        assert!(state.view().rows.is_empty());
        state.admit(&restore, page(films(0, 100), None)).unwrap();
        assert_eq!(state.view().rows[70].title, "Film 70");
        assert_eq!(state.view().anchor, 70);
        assert!(state.retained_bytes() <= 512 * 1024 + 192 * 1024);
    }

    #[test]
    fn replaying_an_adjacent_known_page_keeps_previous_rows_and_order() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 50), Some("page two")))
            .unwrap();
        let second = state.demand(49, 50).unwrap().unwrap();
        state.admit(&second, page(films(50, 50), None)).unwrap();
        state.demand(30, 31).unwrap();
        state.evict_windows();
        let restore = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&restore, page(films(0, 50), Some("page two")))
            .unwrap();
        let next = state.demand(49, 50).unwrap().unwrap();
        state.admit(&next, page(films(50, 50), None)).unwrap();
        assert_eq!(state.view().first, 0);
        assert_eq!(state.view().rows.len(), 100);
        assert_eq!(state.view().rows[0].title, "Film 0");
        assert_eq!(state.view().rows[99].title, "Film 99");
    }

    #[test]
    fn bounded_empty_page_ledger_refuses_without_losing_committed_cards() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 1), Some("input 0")))
            .unwrap();
        let mut read = state.demand(0, 1).unwrap().unwrap();
        let mut admitted = 1;
        loop {
            let next = format!("input {admitted}");
            match state.admit(&read, page(Vec::new(), Some(&next))) {
                Ok(Some(successor)) => {
                    read = successor;
                    admitted += 1;
                    assert!(admitted <= 256);
                }
                Err(Failure::ResourceLimit) => break,
                _ => panic!("empty changed cursor must advance or refuse its bound"),
            }
        }
        assert!(admitted > 1);
        assert_eq!(state.view().rows[0].title, "Film 0");
        assert_eq!(state.view().rows.len(), 1);
        assert_eq!(state.view().tail, Tail::Error(Failure::ResourceLimit));
        assert!(state.retained_bytes() < 512 * 1024 + 192 * 1024);
    }

    #[test]
    fn replay_fingerprint_distinguishes_kind_date_duration_presence_and_float_bits() {
        for variation in 0..5 {
            let mut state = MyListState::new();
            let read = state.select(WatchListFilter::All).unwrap().unwrap();
            let mut original = media(
                "FixtureA",
                "Original",
                criterion_account::MediaKind::Original,
            );
            original.duration = Some(0.0);
            state.admit(&read, page(vec![original], None)).unwrap();
            state.evict_windows();
            let restore = state.select(WatchListFilter::All).unwrap().unwrap();
            let mut changed = media(
                "FixtureA",
                "Original",
                criterion_account::MediaKind::Original,
            );
            changed.duration = Some(0.0);
            match variation {
                0 => changed.kind = criterion_account::MediaKind::Film,
                1 => {
                    changed.release_date =
                        Some(time::Date::from_calendar_date(2026, time::Month::October, 9).unwrap())
                }
                2 => changed.duration = None,
                3 => changed.duration = Some(-0.0),
                _ => changed.title = "Other".into(),
            }
            assert!(matches!(
                state.admit(&restore, page(vec![changed], None)),
                Err(Failure::Changed)
            ));
            assert!(state.view().rows.is_empty());
            let retry = state.retry().unwrap().unwrap();
            let mut original = media(
                "FixtureA",
                "Original",
                criterion_account::MediaKind::Original,
            );
            original.duration = Some(0.0);
            state.admit(&retry, page(vec![original], None)).unwrap();
            assert_eq!(state.view().first, 0);
            assert_eq!(state.view().rows[0].title, "Original");
        }
    }

    #[test]
    fn returning_to_cancelled_empty_group_resumes_its_observed_continuation() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        let next = state
            .admit(&first, page(Vec::new(), Some("observed after empty")))
            .unwrap()
            .unwrap();
        let other = state.select(WatchListFilter::Category).unwrap().unwrap();
        state.admit(&other, page(Vec::new(), None)).unwrap();
        assert!(matches!(
            state.admit(&next, page(films(0, 1), None)),
            Err(Failure::Stale)
        ));
        let resume = state
            .select(WatchListFilter::All)
            .unwrap()
            .expect("return must resume empty observed group");
        assert_eq!(
            resume.request.cursor.as_ref().unwrap().as_str(),
            "observed after empty"
        );
        state.admit(&resume, page(films(0, 1), None)).unwrap();
        assert_eq!(state.view().rows[0].title, "Film 0");
    }

    #[test]
    fn key_fingerprints_and_cursor_metadata_refuse_the_byte_bound_before_unbounded_growth() {
        let mut state = MyListState::new();
        let mut read = state.select(WatchListFilter::All).unwrap().unwrap();
        let mut admitted = 0;
        loop {
            let before_first = state.view().first;
            let before_title = state.view().rows.first().map(|r| r.title.clone());
            let next = format!("page {admitted}");
            match state.admit(&read, page(films(admitted * 50, 50), Some(&next))) {
                Ok(None) => {
                    admitted += 1;
                    assert!(admitted * 50 <= 4096);
                    read = state
                        .demand(admitted * 50 - 1, admitted * 50)
                        .unwrap()
                        .unwrap();
                }
                Err(Failure::ResourceLimit) => {
                    assert_eq!(state.view().first, before_first);
                    assert_eq!(
                        state.view().rows.first().map(|r| r.title.clone()),
                        before_title
                    );
                    break;
                }
                _ => panic!("unique native pages must append or refuse a bound"),
            }
        }
        assert!(admitted > 3);
        assert!(admitted * 50 < 4096, "metadata bytes win before key count");
        assert_eq!(state.view().tail, Tail::Error(Failure::ResourceLimit));
        state.evict_windows();
        assert!(state.retained_bytes() <= 192 * 1024);
        let restore = state.retry().unwrap().unwrap();
        let expected = format!("page {}", admitted - 2);
        assert_eq!(restore.request.cursor.as_ref().unwrap().as_str(), expected);
    }

    #[test]
    fn exactly_512_raw_duplicates_are_bounded_before_first_key_projection() {
        let mut state = MyListState::new();
        let read = state.select(WatchListFilter::All).unwrap().unwrap();
        let rows = (0..512)
            .map(|i| {
                media(
                    "FixtureA",
                    &format!("Value {i}"),
                    criterion_account::MediaKind::Episode,
                )
            })
            .collect();
        state.admit(&read, page(rows, None)).unwrap();
        assert_eq!(state.view().rows.len(), 1);
        assert_eq!(state.view().rows[0].title, "Value 0");
        assert_eq!(
            state.view().rows[0].kind,
            criterion_account::MediaKind::Episode
        );
    }

    #[test]
    fn moved_away_demand_discards_cancelled_continuation_intent() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 50), Some("cancelled tail")))
            .unwrap();
        let read = state.demand(49, 50).unwrap().unwrap();
        state.cancel();
        assert!(state.demand(0, 1).unwrap().is_none());
        assert!(state.retry().unwrap().is_none());
        assert_eq!((state.view().anchor, state.view().target), (0, 1));
        assert!(matches!(
            state.admit(&read, page(films(50, 50), None)),
            Err(Failure::Stale)
        ));
    }

    #[test]
    fn replay_changed_order_missing_rows_or_next_cursor_preserves_the_previous_window() {
        for variation in 0..3 {
            let mut state = MyListState::new();
            let first = state.select(WatchListFilter::All).unwrap().unwrap();
            state
                .admit(&first, page(films(0, 50), Some("second")))
                .unwrap();
            let second = state.demand(49, 50).unwrap().unwrap();
            state.admit(&second, page(films(50, 50), None)).unwrap();
            state.demand(60, 61).unwrap();
            state.evict_windows();
            let restore = state.select(WatchListFilter::All).unwrap().unwrap();
            state.admit(&restore, page(films(50, 50), None)).unwrap();
            let back = state.demand(60, 0).unwrap().unwrap();
            let mut changed = films(0, 50);
            let mut next = Some("second");
            match variation {
                0 => changed.swap(0, 1),
                1 => {
                    changed.pop();
                }
                _ => next = Some("different"),
            }
            assert!(matches!(
                state.admit(&back, page(changed, next)),
                Err(Failure::Changed)
            ));
            assert_eq!(state.view().first, 50);
            assert_eq!(state.view().rows.len(), 50);
            assert_eq!(state.view().rows[10].title, "Film 60");
        }
    }

    #[test]
    fn empty_prefetch_pages_continue_even_when_the_focus_is_still_inside_the_window() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 50), Some("prefetch")))
            .unwrap();
        let read = state.demand(45, 46).unwrap().unwrap();
        let successor = state
            .admit(&read, page(Vec::new(), Some("after empty")))
            .unwrap()
            .expect("empty prefetch must traverse its observed continuation");
        assert_eq!(
            successor.request.cursor.as_ref().unwrap().as_str(),
            "after empty"
        );
        assert_eq!(state.view().anchor, 45);
        assert_eq!(state.view().rows.len(), 50);
        state.admit(&successor, page(films(50, 50), None)).unwrap();
        assert_eq!(state.view().rows.len(), 100);
    }

    #[test]
    fn evicted_interrupted_backward_intent_restores_anchor_then_continues_to_target() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 100), Some("second")))
            .unwrap();
        let second = state.demand(99, 100).unwrap().unwrap();
        state.admit(&second, page(films(100, 50), None)).unwrap();
        state.demand(120, 121).unwrap();
        state.evict_windows();
        let restore = state.select(WatchListFilter::All).unwrap().unwrap();
        state.admit(&restore, page(films(100, 50), None)).unwrap();
        let back = state.demand(120, 0).unwrap().unwrap();
        state.evict_windows();
        assert!(matches!(
            state.admit(&back, page(films(0, 100), Some("second"))),
            Err(Failure::Stale)
        ));
        let anchor = state.retry().unwrap().unwrap();
        assert_eq!(anchor.request.cursor.as_ref().unwrap().as_str(), "second");
        let target = state
            .admit(&anchor, page(films(100, 50), None))
            .unwrap()
            .expect("interrupted backward target must remain demanded");
        assert!(target.request.cursor.is_none());
        state
            .admit(&target, page(films(0, 100), Some("second")))
            .unwrap();
        assert_eq!(state.view().first, 0);
        assert_eq!(state.view().anchor, 0);
        assert_eq!(state.view().rows[0].title, "Film 0");
    }

    #[test]
    fn out_of_budget_demand_refuses_visibly_and_invalidates_an_inflight_completion() {
        let mut state = MyListState::new();
        let first = state.select(WatchListFilter::All).unwrap().unwrap();
        state
            .admit(&first, page(films(0, 50), Some("pending")))
            .unwrap();
        let pending = state.demand(49, 50).unwrap().unwrap();
        assert!(matches!(
            state.demand(49, usize::MAX),
            Err(Failure::ResourceLimit)
        ));
        assert_eq!(state.view().tail, Tail::Error(Failure::ResourceLimit));
        assert!(matches!(
            state.admit(&pending, page(films(50, 50), None)),
            Err(Failure::Stale)
        ));
        assert_eq!(state.view().rows.len(), 50);
    }
    #[test]
    fn moving_away_from_pending_or_failed_prefetch_retires_the_obsolete_intent() {
        for failed in [false, true] {
            let mut state = MyListState::new();
            let first = state.select(WatchListFilter::All).unwrap().unwrap();
            state
                .admit(&first, page(films(0, 50), Some("obsolete prefetch")))
                .unwrap();
            let read = state.demand(49, 50).unwrap().unwrap();
            if failed {
                assert!(state.fail(&read, Failure::Read));
            }
            assert!(state.demand(0, 1).unwrap().is_none());
            assert_eq!(state.view().tail, Tail::More);
            assert!(state.retry().unwrap().is_none());
            assert!(matches!(
                state.admit(&read, page(films(50, 50), None)),
                Err(Failure::Stale)
            ));
            assert_eq!(state.view().rows.len(), 50);
        }
    }
}
