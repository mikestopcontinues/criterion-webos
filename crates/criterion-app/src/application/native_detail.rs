// SPDX-License-Identifier: GPL-3.0-or-later
//! Native metadata is anonymous, while its worker and epoch retirement are shared.
use super::{Application, NativeDetailRead};
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
    pub(super) fn poll_native_detail(&mut self, runtime: &Runtime) {
        if self.controller.native_detail_expired() {
            if let Some(read) = self
                .native_detail_pending
                .as_ref()
                .or_else(|| {
                    self.native_detail_generation
                        .as_ref()
                        .map(|issued| &issued.read)
                })
                .cloned()
            {
                self.controller.fail_native_detail(&read);
            }
            self.cancel_native_detail_read();
        }
        if self.shelf_pending.is_none()
            && self.shelf_generation.is_none()
            && self.continue_watching_pending.is_none()
            && self.continue_watching_generation.is_none()
            && self.native_detail_pending.is_none()
            && self.native_detail_generation.is_none()
            && let Some(epoch) = self.account_epoch
        {
            self.native_detail_pending = self.controller.begin_native_detail(epoch);
        }
        if !self.accounts.write_active()
            && let Some(read) = self.native_detail_pending.take()
            && self.controller.native_detail_owns(&read)
        {
            match self.accounts.request(
                runtime.handle(),
                read.epoch,
                ReadRequest::NativeDetail {
                    media_id: read.id.clone(),
                },
            ) {
                Ok(generation) => {
                    self.native_detail_generation = Some(NativeDetailRead { read, generation })
                }
                Err(_) => self.controller.fail_native_detail(&read),
            }
        }
    }
    pub(super) fn complete_native_detail(
        &mut self,
        result: Result<LoadedAccount, Error>,
        runtime: &tokio::runtime::Handle,
    ) {
        let Some(issued) = self.native_detail_generation.take() else {
            return;
        };
        if !self.active
            || self.exiting
            || self.account_epoch != Some(issued.read.epoch)
            || !self.controller.native_detail_owns(&issued.read)
        {
            return;
        }
        match result {
            Ok(loaded)
                if issued.generation == loaded.generation()
                    && issued.read.epoch == loaded.session_generation()
                    && loaded.matches_request(&ReadRequest::NativeDetail {
                        media_id: issued.read.id.clone(),
                    }) =>
            {
                if let Loaded::NativeDetail(detail) = loaded.data {
                    let positions = self
                        .positions
                        .as_ref()
                        .filter(|positions| positions.epoch == issued.read.epoch)
                        .map(|positions| &positions.snapshot);
                    if let Some(selection) =
                        self.controller
                            .admit_native_detail(&issued.read, *detail, positions)
                    {
                        self.consume_native_play(selection, runtime);
                    }
                }
            }
            _ => self.controller.fail_native_detail(&issued.read),
        }
    }
    pub(super) fn retire_departed_native_detail(&mut self) {
        let owns = |read: &crate::controller::NativeDetailRead| {
            self.active
                && !self.exiting
                && self.account_epoch == Some(read.epoch)
                && self.controller.native_detail_owns(read)
        };
        if self
            .native_detail_pending
            .as_ref()
            .is_some_and(|read| !owns(read))
        {
            self.native_detail_pending = None;
        }
        if self
            .native_detail_generation
            .as_ref()
            .is_some_and(|issued| !owns(&issued.read))
        {
            self.native_detail_generation = None;
            self.accounts.background();
        }
    }
    pub(super) fn cancel_native_detail_read(&mut self) {
        self.native_detail_pending = None;
        if self.native_detail_generation.take().is_some() {
            self.accounts.background();
        }
        self.controller.cancel_native_detail();
    }
}
