// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded All Films traversal. Only observed input cursors can be requested.
use crate::presentation::Presentation;
use criterion_provider::{BrowseRequest, CatalogPage, Filter, MediaId, PageCursor};
use criterion_ui::{CatalogTail, CatalogWindow, LoadState};
use std::{collections::VecDeque, sync::Arc, time::Duration};

pub(super) const WINDOW_CARDS: usize = 180;
pub(super) const WINDOW_BYTES: usize = 512 * 1024;
const BOOKMARKS: usize = 256;
const BOOKMARK_BYTES: usize = 192 * 1024;
const TRAVERSAL_DEADLINE: Duration = Duration::from_secs(60);
const MAX_POSITION: usize = 1_000_000;
#[derive(Clone)]
struct Bookmark {
    first: usize,
    count: usize,
    cursor: Option<PageCursor>,
}
impl Bookmark {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.cursor.as_ref().map_or(0, |_| 512)
    }
}
struct Pending {
    mark: Bookmark,
    target: usize,
    append: bool,
    expected: Option<MediaId>,
    due: Duration,
}
pub(super) struct Pager {
    base: Arc<BrowseRequest>,
    bookmarks: VecDeque<Bookmark>,
    window: VecDeque<Bookmark>,
    next: Option<PageCursor>,
    pending: Option<Pending>,
    failed: Option<Pending>,
    anchor: usize,
    anchor_id: Option<MediaId>,
    total: Option<u32>,
}
impl Pager {
    pub(super) fn new(base: Arc<BrowseRequest>, now: Duration) -> Self {
        let mut owner = Self {
            base,
            bookmarks: VecDeque::new(),
            window: VecDeque::new(),
            next: None,
            pending: None,
            failed: None,
            anchor: 0,
            anchor_id: None,
            total: None,
        };
        owner.pending = Some(Pending {
            mark: Bookmark {
                first: 0,
                count: 0,
                cursor: None,
            },
            target: 0,
            append: false,
            expected: None,
            due: now.saturating_add(TRAVERSAL_DEADLINE),
        });
        owner
    }
    fn request(&self, mark: &Bookmark) -> BrowseRequest {
        BrowseRequest {
            page_limit: self.base.page_limit,
            cursor: mark.cursor.clone(),
            sort: self.base.sort,
            direction: self.base.direction,
            filters: self
                .base
                .filters
                .iter()
                .map(|f| Filter {
                    group: f.group,
                    value: f.value.clone(),
                })
                .collect(),
        }
    }
    fn first(&self) -> usize {
        self.window.front().map_or(0, |m| m.first)
    }
    fn end(&self) -> usize {
        self.window.back().map_or(0, |m| m.first + m.count)
    }
    fn window_bytes(&self) -> usize {
        self.window.capacity() * std::mem::size_of::<Bookmark>()
            + self
                .window
                .iter()
                .map(|m| m.cursor.as_ref().map_or(0, |_| 512))
                .sum::<usize>()
    }
    pub(super) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + super::request_bytes(&self.base)
            + self.window_bytes()
            + self.bookmark_bytes()
            + self.next.as_ref().map_or(0, |_| 512)
            + self.pending.as_ref().map_or(0, |p| {
                p.mark.bytes() + p.expected.as_ref().map_or(0, |id| id.as_str().len())
            })
            + self.failed.as_ref().map_or(0, |p| {
                p.mark.bytes() + p.expected.as_ref().map_or(0, |id| id.as_str().len())
            })
            + self.anchor_id.as_ref().map_or(0, |id| id.as_str().len())
    }
    fn bookmark_bytes(&self) -> usize {
        self.bookmarks.capacity() * std::mem::size_of::<Bookmark>()
            + self
                .bookmarks
                .iter()
                .map(|m| m.cursor.as_ref().map_or(0, |_| 512))
                .sum::<usize>()
    }
    // A checkpoint stores only its observed input cursor and global span; base
    // filters/sort are process-owned and never copied into each bookmark.
    fn remember(&mut self, mark: Bookmark) {
        if self.bookmarks.iter().any(|m| m.first == mark.first) {
            return;
        }
        if self.bookmarks.len() == BOOKMARKS {
            self.bookmarks.pop_front();
        }
        self.bookmarks.push_back(mark);
        self.bookmarks.shrink_to_fit();
        while self.bookmark_bytes() > BOOKMARK_BYTES {
            self.bookmarks.pop_front();
            self.bookmarks.shrink_to_fit();
        }
    }
    pub(super) fn loaded(
        &mut self,
        page: CatalogPage,
        view: &mut Presentation,
    ) -> Option<BrowseRequest> {
        let mut work = self.pending.take()?;
        let count = page.items.len();
        if (count == 0 && (page.next_cursor.is_some() || (!work.append && self.has_window())))
            || work.mark.first.saturating_add(count) > MAX_POSITION
            || page.next_cursor.as_ref().is_some_and(|c| {
                Some(c) == work.mark.cursor.as_ref()
                    || self
                        .bookmarks
                        .iter()
                        .any(|m| Some(c) == m.cursor.as_ref() && m.first != work.mark.first + count)
            })
        {
            self.failed = Some(work);
            self.publish(view);
            return None;
        }
        work.mark.count = count;
        // Earlier bookmarks may have been evicted. The provider admits only
        // forward cursors, so replay one response at a time without accumulating
        // intermediate cards. Retry retains this one last-known checkpoint.
        if !work.append && work.target >= work.mark.first + count && count > 0 {
            if let Some(cursor) = page.next_cursor {
                let next = Bookmark {
                    first: work.mark.first + count,
                    count: 0,
                    cursor: Some(cursor),
                };
                self.remember(work.mark);
                work.mark = next;
                let request = self.request(&work.mark);
                self.pending = Some(work);
                self.publish(view);
                return Some(request);
            }
            self.failed = Some(work);
            self.publish(view);
            return None;
        }
        let next = page.next_cursor.clone();
        self.total.get_or_insert(page.total);
        let projection = Presentation::catalog("All Films", page);
        if work.expected.as_ref().is_some_and(|id| {
            projection
                .catalog_id(work.target.saturating_sub(work.mark.first))
                .as_ref()
                != Some(id)
        }) || projection.catalog_bytes() + work.mark.bytes() > WINDOW_BYTES
        {
            self.failed = Some(work);
            self.publish(view);
            return None;
        }
        if work.append {
            view.merge_catalog(projection);
            self.window.push_back(work.mark.clone());
        } else {
            view.replace_catalog(projection);
            self.window.clear();
            self.window.push_back(work.mark.clone());
        }
        while view.catalog_len() > WINDOW_CARDS
            || view.catalog_bytes() + self.window_bytes() > WINDOW_BYTES
        {
            if let Some(old) = self.window.pop_front() {
                view.drop_catalog_prefix(old.count);
            } else {
                break;
            }
        }
        self.window.shrink_to_fit();
        let follow = if work.append && work.target >= work.mark.first + count && next.is_some() {
            Some(Pending {
                mark: Bookmark {
                    first: work.mark.first + count,
                    count: 0,
                    cursor: next.clone(),
                },
                target: work.target,
                append: true,
                expected: None,
                due: work.due,
            })
        } else {
            None
        };
        self.remember(work.mark);
        self.next = next;
        self.failed = None;
        self.anchor = if work.append {
            self.anchor
        } else {
            work.target
        };
        self.anchor_id = view.catalog_id(self.anchor.saturating_sub(self.first()));
        self.pending = follow;
        let request = self.pending.as_ref().map(|p| self.request(&p.mark));
        self.publish(view);
        request
    }
    pub(super) fn retire_obsolete(&mut self, target: usize, view: &mut Presentation) -> bool {
        let obsolete = self
            .pending
            .as_ref()
            .or(self.failed.as_ref())
            .is_some_and(|p| {
                if p.expected.is_some() {
                    false
                } else if p.append {
                    target >= self.first() && target.saturating_add(12) < self.end()
                } else {
                    target != p.target && self.has_window()
                }
            });
        if obsolete {
            self.cancel(view);
        }
        obsolete
    }
    pub(super) fn demand(
        &mut self,
        anchor: usize,
        target: usize,
        view: &mut Presentation,
        now: Duration,
    ) -> Option<BrowseRequest> {
        if anchor >= MAX_POSITION || target >= MAX_POSITION {
            return None;
        }
        if anchor != self.anchor && anchor >= self.first() && anchor < self.end() {
            self.anchor = anchor;
            self.anchor_id = view.catalog_id(anchor - self.first());
        }
        if self.pending.is_some() || self.failed.is_some() {
            return None;
        }
        let outside = target < self.first() || target >= self.end();
        let (mark, append) = if outside {
            let mark = self
                .bookmarks
                .iter()
                .find(|m| target >= m.first && target < m.first + m.count)
                .cloned();
            if let Some(mark) = mark {
                (mark, false)
            } else if target >= self.end() {
                (
                    Bookmark {
                        first: self.end(),
                        count: 0,
                        cursor: Some(self.next.clone()?),
                    },
                    true,
                )
            } else {
                (
                    Bookmark {
                        first: 0,
                        count: 0,
                        cursor: None,
                    },
                    false,
                )
            }
        } else if target.saturating_add(12) >= self.end() {
            (
                Bookmark {
                    first: self.end(),
                    count: 0,
                    cursor: Some(self.next.clone()?),
                },
                true,
            )
        } else {
            return None;
        };
        let target = if append && target < mark.first {
            mark.first
        } else {
            target
        };
        let request = self.request(&mark);
        self.pending = Some(Pending {
            mark,
            target,
            append,
            expected: None,
            due: now.saturating_add(TRAVERSAL_DEADLINE),
        });
        self.publish(view);
        Some(request)
    }
    pub(super) fn rehydrate(
        &mut self,
        view: &mut Presentation,
        now: Duration,
    ) -> Option<BrowseRequest> {
        let mark = self
            .window
            .iter()
            .find(|m| self.anchor >= m.first && self.anchor < m.first + m.count)
            .cloned()?;
        self.pending = Some(Pending {
            mark: mark.clone(),
            target: self.anchor,
            append: false,
            expected: self.anchor_id.clone(),
            due: now.saturating_add(TRAVERSAL_DEADLINE),
        });
        self.window.clear();
        self.window.shrink_to_fit();
        self.next = None;
        self.failed = None;
        self.publish(view);
        Some(self.request(&mark))
    }
    pub(super) fn fail(&mut self, view: &mut Presentation) {
        if let Some(work) = self.pending.take() {
            self.failed = Some(work);
        }
        self.publish(view);
    }
    pub(super) fn retry(
        &mut self,
        view: &mut Presentation,
        now: Duration,
    ) -> Option<BrowseRequest> {
        let mut work = self.failed.take()?;
        work.due = now.saturating_add(TRAVERSAL_DEADLINE);
        let request = self.request(&work.mark);
        self.pending = Some(work);
        self.publish(view);
        Some(request)
    }
    pub(super) fn expired(&self, now: Duration) -> bool {
        self.pending.as_ref().is_some_and(|p| now >= p.due)
    }
    pub(super) fn cancel(&mut self, view: &mut Presentation) {
        self.pending = None;
        self.failed = None;
        self.publish(view);
    }
    pub(super) fn has_window(&self) -> bool {
        !self.window.is_empty()
    }
    fn publish(&self, view: &mut Presentation) {
        if !self.has_window() {
            view.set_status(if self.pending.is_some() {
                LoadState::Loading
            } else if self.failed.is_some() {
                LoadState::Error
            } else {
                LoadState::Empty
            });
        }
        if let Some(total) = self.total {
            view.set_catalog_total(total);
        }
        view.set_catalog_window(CatalogWindow {
            first: self.first(),
            tail: if self.pending.is_some() {
                CatalogTail::Loading
            } else if self.failed.is_some() {
                CatalogTail::Error
            } else if self.next.is_some() {
                CatalogTail::More
            } else {
                CatalogTail::End
            },
        });
    }
}
