// SPDX-License-Identifier: GPL-3.0-or-later
// Inert host metadata-only source typecheck. Do not link, run or use as an app.
// ARM32 layout assertions are target-qualified; main_bus/abi.c owns SDK type checks.
use criterion_platform::write_fence::{Db8Transport, FenceError};
mod write_fence {
    pub use criterion_platform::write_fence::FenceError;
}
#[path = "../main_bus.rs"]
mod main_bus;
#[path = "native.rs"]
mod native;

pub fn typecheck_only() {
    let _factory: fn() -> Result<native::NativeTransport, FenceError> =
        native::NativeTransport::open;
    fn transport<T: Db8Transport>() {}
    transport::<native::NativeTransport>();
}
