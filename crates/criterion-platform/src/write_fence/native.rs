// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed DB8 adapter; the MAIN owner holds registration, context and callbacks.
use super::{Db8Transport, FenceError};
use crate::main_bus::MainBus;
use std::time::Instant;

pub(super) struct NativeTransport {
    bus: MainBus,
}
// Only the private fence worker factory constructs this adapter. It creates,
// uses and drops the non-Send bus on that same thread; no handle is exposed.
unsafe impl Send for NativeTransport {}

impl NativeTransport {
    pub(super) fn open() -> Result<Self, FenceError> {
        Ok(Self {
            bus: MainBus::open()?,
        })
    }
}
impl Db8Transport for NativeTransport {
    fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError> {
        self.bus.db8_get(deadline)
    }
    fn put(
        &mut self,
        expected_rev: u64,
        possibly_issued: bool,
        deadline: Instant,
    ) -> Result<Vec<u8>, FenceError> {
        self.bus.db8_put(expected_rev, possibly_issued, deadline)
    }
}
