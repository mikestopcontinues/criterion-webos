// SPDX-License-Identifier: GPL-3.0-or-later
//! Foreground gallery reads use the same account worker as My List. The
//! controller's logical demand retains its deadline through token refresh.
use super::{Application, ContinueWatchingRead};
use crate::account::{Loaded, LoadedAccount, ReadRequest};
use criterion_account::Error;
use criterion_provider::RequestTransport;
use criterion_session::{MonotonicClock, Transport};
use tokio::runtime::Runtime;

impl<
    P: RequestTransport + Send + Sync + 'static,
    T: Transport + 'static,
    C: MonotonicClock + Clone + 'static,
    A: criterion_account::Transport + 'static,
    D: MonotonicClock,
> Application<P, T, C, A, D>
{
    pub(super) fn poll_continue_watching(&mut self, runtime: &Runtime) {
        if self.controller.continue_watching_expired() {
            if let Some(read) = self.continue_watching_pending.or_else(|| {
                self.continue_watching_generation
                    .as_ref()
                    .map(|issued| issued.read)
            }) {
                self.controller.fail_continue_watching(&read);
            }
            self.cancel_continue_watching_read();
        }
        if self.authentication.signed_in()
            && self.shelf_pending.is_none()
            && self.shelf_generation.is_none()
            && self.continue_watching_pending.is_none()
            && self.continue_watching_generation.is_none()
            && let Some(epoch) = self.account_epoch
        {
            self.continue_watching_pending = self.controller.begin_continue_watching(epoch);
        }
        if self.authentication.access_ready()
            && let Some(read) = self.continue_watching_pending.take()
            && self.controller.continue_watching_owns(&read)
        {
            match self
                .accounts
                .request(runtime.handle(), read.epoch, ReadRequest::ContinueWatching)
            {
                Ok(generation) => {
                    self.continue_watching_generation =
                        Some(ContinueWatchingRead { read, generation });
                }
                Err(error) => self.retry_or_fail_continue_watching(read, error),
            }
        }
    }
    pub(super) fn complete_continue_watching(&mut self, result: Result<LoadedAccount, Error>) {
        let Some(issued) = self.continue_watching_generation.take() else {
            return;
        };
        if !self.active
            || self.exiting
            || self.account_epoch != Some(issued.read.epoch)
            || !self.controller.continue_watching_owns(&issued.read)
        {
            return;
        }
        match result {
            Ok(loaded)
                if self.authentication.access_ready()
                    && issued.generation == loaded.generation()
                    && issued.read.epoch == loaded.session_generation()
                    && loaded.matches_request(&ReadRequest::ContinueWatching) =>
            {
                if let Loaded::ContinueWatching(data) = loaded.data
                    && let Some(snapshot) =
                        self.controller.admit_continue_watching(&issued.read, data)
                {
                    self.positions = Some(super::AccountPositions {
                        epoch: issued.read.epoch,
                        snapshot,
                    });
                }
            }
            Err(error) => self.retry_or_fail_continue_watching(issued.read, error),
            _ => self.controller.fail_continue_watching(&issued.read),
        }
    }
    fn retry_or_fail_continue_watching(
        &mut self,
        read: crate::controller::ContinueWatchingRead,
        error: Error,
    ) {
        if matches!(error, Error::Stale | Error::Session(_)) && self.authentication.signed_in() {
            self.continue_watching_pending = self.controller.renew_continue_watching();
        } else {
            self.controller.fail_continue_watching(&read);
        }
    }
    pub(super) fn retire_departed_continue_watching(&mut self) {
        let owns = |read: &crate::controller::ContinueWatchingRead| {
            self.active
                && !self.exiting
                && self.account_epoch == Some(read.epoch)
                && self.controller.continue_watching_owns(read)
        };
        if self
            .continue_watching_pending
            .as_ref()
            .is_some_and(|read| !owns(read))
        {
            self.continue_watching_pending = None;
        }
        if self
            .continue_watching_generation
            .as_ref()
            .is_some_and(|issued| !owns(&issued.read))
        {
            self.continue_watching_generation = None;
            self.accounts.background();
        }
    }
    pub(super) fn cancel_continue_watching_read(&mut self) {
        self.continue_watching_pending = None;
        if self.continue_watching_generation.take().is_some() {
            self.accounts.background();
        }
        self.controller.cancel_continue_watching();
    }
}
