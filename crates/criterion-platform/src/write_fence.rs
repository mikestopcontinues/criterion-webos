// SPDX-License-Identifier: GPL-3.0-or-later
//! The nonsecret, restart-retained admission before an account write is polled.
use std::{future::Future, pin::Pin, sync::Arc, time::Instant};

#[cfg(any(
    test,
    all(
        feature = "webos-runtime",
        target_arch = "arm",
        target_pointer_width = "32"
    )
))]
mod native;
mod store;
mod worker;

pub const WRITE_FENCE_KIND: &str = "com.mikestopcontinues.criterion.unofficial.issuedwrite:1";
pub const WRITE_FENCE_ID: &str = "crit.write.v1";
pub const WRITE_FENCE_APP: &str = "com.mikestopcontinues.criterion.unofficial";

pub type FenceFuture<T> = Pin<Box<dyn Future<Output = Result<T, FenceError>> + Send + 'static>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceState {
    Clean,
    PossiblyIssued,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceError {
    Unavailable,
    Invalid,
    Unconfirmed,
    Held,
}

/// Produced only after the reservation's acknowledgment and exact readback.
/// This non-cloneable capability carries no account or content information.
pub struct WriteReservation {
    brand: Arc<()>,
    revision: u64,
}

impl std::fmt::Debug for WriteReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WriteReservation { <opaque> }")
    }
}

/// Completion is authorized only by definite NotIssued or an admitted write acknowledgment.
/// Matching provider reads, cancellation and logout do not authorize completion.
pub trait IssuedWriteFence: Send + Sync {
    /// Each operation has one eight-second deadline captured at invocation, including queueing.
    /// Creating its future issues no I/O; dropping an already polled future does not cancel it.
    fn state(&self) -> FenceFuture<FenceState>;
    fn reserve(&self) -> FenceFuture<WriteReservation>;
    fn complete(&self, reservation: WriteReservation) -> FenceFuture<()>;
}

/// Fixed DB8 external I/O. Calls must return before the original operation deadline.
/// Native transport admits sender/token/bounded payload before returning these bytes.
pub trait Db8Transport: Send + 'static {
    fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError>;
    fn put(
        &mut self,
        expected_rev: u64,
        possibly_issued: bool,
        deadline: Instant,
    ) -> Result<Vec<u8>, FenceError>;
}

pub struct Db8WriteFence {
    worker: worker::Worker,
}

impl Db8WriteFence {
    pub fn with_transport(transport: impl Db8Transport) -> Result<Self, FenceError> {
        Ok(Self {
            worker: worker::Worker::start(transport)?,
        })
    }
    pub fn new() -> Result<Self, FenceError> {
        #[cfg(all(
            feature = "webos-runtime",
            target_arch = "arm",
            target_pointer_width = "32"
        ))]
        return Ok(Self {
            worker: worker::Worker::start_factory(native::NativeTransport::open)?,
        });
        #[cfg(not(all(
            feature = "webos-runtime",
            target_arch = "arm",
            target_pointer_width = "32"
        )))]
        Err(FenceError::Unavailable)
    }
}

impl IssuedWriteFence for Db8WriteFence {
    fn state(&self) -> FenceFuture<FenceState> {
        self.worker.state()
    }
    fn reserve(&self) -> FenceFuture<WriteReservation> {
        self.worker.reserve()
    }
    fn complete(&self, reservation: WriteReservation) -> FenceFuture<()> {
        self.worker.complete(reservation)
    }
}

#[cfg(test)]
mod lifetimes;
#[cfg(test)]
mod scenarios;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod native_tests;
