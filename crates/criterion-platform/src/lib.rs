// SPDX-License-Identifier: GPL-3.0-or-later
//! The narrow native window and input seam. Rendering and playback belong to their owners.

mod event;
pub use event::Activity;
pub use event::{Event, EventLayout, KeyEvent, Lifecycle, decode_event};
mod surface;
pub use surface::{Size, Surface, Viewport};
#[cfg(feature = "sdl")]
mod sdl;
#[cfg(feature = "sdl")]
pub use sdl::{ContextInfo, PlatformError, Window};
