// SPDX-License-Identifier: GPL-3.0-or-later
//! Application input without native calls. The runtime owns asynchronous work and graphics.
mod input;
pub use input::{InputAdapter, InputFrame, MAX_EVENTS_PER_FRAME, MAX_TEXT_BYTES_PER_FRAME};
