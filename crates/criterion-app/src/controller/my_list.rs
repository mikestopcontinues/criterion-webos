// SPDX-License-Identifier: GPL-3.0-or-later
//! Private shelf publication and demand deadlines. The account owner executes
//! typed reads; the pure reducer owns bounded groups, checkpoints and row keys.
use super::{Controller, Presentation};
use crate::my_list::{Failure, MyListState, Read, Tail};
use criterion_account::{Error, WatchList};
use criterion_provider::RequestTransport;
use criterion_session::MonotonicClock;
use criterion_ui::{LoadState, MyListGroup, Page};
use std::time::Duration;

const DEMAND_DEADLINE: Duration = Duration::from_secs(60);

impl<T: RequestTransport + Send + Sync + 'static, C: MonotonicClock> Controller<T, C> {
    pub(crate) fn begin_shelf(&mut self, epoch: u64) -> Option<Read> {
        if self.page != Page::MyList || self.account_session != Some(epoch) {
            return None;
        }
        self.jobs.cancel();
        self.search_due = None;
        self.search_loaded = false;
        self.query = None;
        self.pager = None;
        if self.private_epoch != Some(epoch) {
            self.my_list = None;
        }
        self.private_epoch = Some(epoch);
        if self.my_list.is_none() {
            // A fresh rail visit reuses the latest same-subscriber traversal;
            // keep the older snapshot intact for an exact Back destination.
            self.my_list = self.history.iter().rev().find_map(|snapshot| {
                (snapshot.page == Page::MyList && snapshot.private_epoch == Some(epoch))
                    .then(|| snapshot.my_list.clone())
                    .flatten()
            });
        }
        let state = self.my_list.get_or_insert_with(MyListState::new);
        let filter = state.view().filter;
        let result = state.select(filter);
        self.shelf_step(result, true)
    }

    pub(crate) fn shelf_owns(&self, epoch: u64, read: &Read) -> bool {
        self.page == Page::MyList
            && self.account_session == Some(epoch)
            && self.private_epoch == Some(epoch)
            && self.shelf_read.as_ref().is_some_and(|current| {
                current.operation == read.operation && current.request == read.request
            })
    }

    pub(crate) fn admit_shelf(&mut self, epoch: u64, read: &Read, page: WatchList) -> Option<Read> {
        if !self.shelf_owns(epoch, read) {
            return None;
        }
        if self.shelf_expired() {
            self.fail_shelf(epoch, read, Error::Deadline);
            return None;
        }
        let result = self.my_list.as_mut()?.admit(read, page);
        self.shelf_step(result, false)
    }

    pub(crate) fn fail_shelf(&mut self, epoch: u64, read: &Read, error: Error) {
        if !self.shelf_owns(epoch, read) {
            return;
        }
        if let Some(state) = &mut self.my_list {
            let _ = state.fail(read, Failure::Read);
            self.view = Presentation::my_list(state);
            if state.view().rows.is_empty() {
                self.view.set_status(match error {
                    Error::Unavailable | Error::Deadline => LoadState::Offline,
                    _ => LoadState::Error,
                });
            }
        }
        self.shelf_read = None;
        self.shelf_deadline = None;
    }

    pub(crate) fn shelf_expired(&self) -> bool {
        self.shelf_read.is_some()
            && self
                .shelf_deadline
                .is_some_and(|due| self.clock.now() >= due)
    }

    pub(crate) fn cancel_shelf(&mut self) {
        if let Some(state) = &mut self.my_list {
            state.cancel();
            if self.page == Page::MyList {
                self.view = Presentation::my_list(state);
            }
        }
        self.shelf_read = None;
        self.shelf_deadline = None;
    }

    pub(crate) fn resume_shelf(&mut self) -> Option<Read> {
        if self.page != Page::MyList
            || self.private_epoch.is_none()
            || self.private_epoch != self.account_session
        {
            return None;
        }
        let result = self.my_list.as_mut()?.retry();
        self.shelf_step(result, true)
    }

    /// Credential refresh keeps one logical demand deadline across read replacement.
    pub(crate) fn renew_shelf(&mut self) -> Option<Read> {
        if self.private_epoch.is_none() || self.private_epoch != self.account_session {
            return None;
        }
        let state = self.my_list.as_mut()?;
        state.cancel();
        let result = state.retry();
        self.shelf_step(result, false)
    }

    pub(super) fn clear_shelf(&mut self) {
        self.my_list = None;
        self.private_epoch = None;
        self.shelf_read = None;
        self.shelf_deadline = None;
    }

    pub(super) fn select_shelf_group(&mut self, group: MyListGroup) -> Option<Read> {
        if self.private_epoch.is_none() || self.private_epoch != self.account_session {
            return None;
        }
        let result = self
            .my_list
            .as_mut()?
            .select(crate::presentation::my_list_filter(group));
        self.shelf_step(result, true)
    }

    pub(super) fn demand_shelf(&mut self, anchor: usize, target: usize) -> Option<Read> {
        if self.private_epoch.is_none() || self.private_epoch != self.account_session {
            return None;
        }
        let state = self.my_list.as_mut()?;
        let current = state.view();
        // Repeated frames keep the same in-flight demand and its deadline.
        if current.anchor == anchor && current.target == target && current.tail == Tail::Loading {
            return None;
        }
        let tail = current.tail;
        let result = state.demand(anchor, target);
        if matches!(result, Ok(None)) && state.view().tail == tail {
            // Focus updates change reducer intent, not display data. Keep the
            // existing owned strings/artwork until a read or tail change publishes.
            return None;
        }
        self.shelf_step(result, true)
    }

    pub(super) fn retry_shelf(&mut self) -> Option<Read> {
        if self.private_epoch.is_none() || self.private_epoch != self.account_session {
            return None;
        }
        let result = self.my_list.as_mut()?.retry();
        self.shelf_step(result, true)
    }

    fn shelf_step(
        &mut self,
        result: Result<Option<Read>, Failure>,
        restart_deadline: bool,
    ) -> Option<Read> {
        let state = self.my_list.as_mut()?;
        let next = match result {
            Ok(next) => next,
            Err(Failure::Stale) => return None,
            Err(failure) => {
                if let Some(read) = &self.shelf_read {
                    let _ = state.fail(read, failure);
                }
                self.shelf_read = None;
                self.shelf_deadline = None;
                self.view = Presentation::my_list(state);
                if state.view().rows.is_empty() {
                    self.view.set_status(LoadState::Error);
                }
                return None;
            }
        };
        if let Some(read) = &next {
            if restart_deadline || self.shelf_deadline.is_none() {
                self.shelf_deadline = self.clock.now().checked_add(DEMAND_DEADLINE);
                if self.shelf_deadline.is_none() {
                    let _ = state.fail(read, Failure::ResourceLimit);
                    self.shelf_read = None;
                    self.view = Presentation::my_list(state);
                    return None;
                }
            }
            self.shelf_read = Some(read.clone());
        } else if state.view().tail != Tail::Loading {
            self.shelf_read = None;
            self.shelf_deadline = None;
        }
        self.view = Presentation::my_list(state);
        next
    }
}
