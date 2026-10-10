// SPDX-License-Identifier: GPL-3.0-or-later
// Inert host metadata-only source typecheck. Do not link, run or use as an app.
// ARM32 layout assertions are target-qualified; native/abi.c owns SDK type checks.
use criterion_platform::write_fence::{Db8Transport, FenceError};
mod store {
    pub(super) const MAX_REPLY: usize = 4096;
}
#[path = "native.rs"]
mod native;
#[path = "reply.rs"]
mod reply;

pub fn typecheck_only() {
    let _factory: fn() -> Result<native::NativeTransport, FenceError> =
        native::NativeTransport::open;
    fn transport<T: Db8Transport>() {}
    transport::<native::NativeTransport>();
}
