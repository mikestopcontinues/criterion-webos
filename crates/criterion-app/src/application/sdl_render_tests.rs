// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared test-only native display lifetime and stock desktop SDL key encoding.
use criterion_platform::Window;
use criterion_ui::GlowRenderer;
use std::{
    ffi::{CString, c_void},
    sync::Arc,
};

unsafe extern "C" {
    fn SDL_PushEvent(event: *mut c_void) -> i32;
}

pub(super) struct Display {
    pub(super) window: Window,
    pub(super) gl: Arc<glow::Context>,
    pub(super) painter: GlowRenderer,
}
impl Display {
    pub(super) fn open(title: &str) -> Self {
        let window = Window::open(title).unwrap();
        // SAFETY: the window owns the current context on this test thread.
        let gl = Arc::new(unsafe {
            glow::Context::from_loader_function(|name| {
                window.gl_proc_address(&CString::new(name).unwrap())
            })
        });
        // SAFETY: renderer creation and destruction remain on the window thread.
        let painter = unsafe { GlowRenderer::new(gl.clone()) }.unwrap();
        Self {
            window,
            gl,
            painter,
        }
    }
}
impl Drop for Display {
    fn drop(&mut self) {
        // The current context and its window remain live throughout destruction.
        self.painter.destroy();
    }
}

pub(super) fn push_key(scancode: u32, keycode: i32, pressed: bool) {
    let mut raw = [0_u8; 56];
    raw[..4].copy_from_slice(&(if pressed { 0x300_u32 } else { 0x301 }).to_le_bytes());
    raw[12] = u8::from(pressed);
    raw[16..20].copy_from_slice(&scancode.to_le_bytes());
    raw[20..24].copy_from_slice(&keycode.to_le_bytes());
    let mut aligned = [0_u64; 7];
    for (word, bytes) in aligned.iter_mut().zip(raw.as_chunks::<8>().0) {
        *word = u64::from_le_bytes(*bytes);
    }
    // SAFETY: initialized56-byte stock desktopSDL event, eight-byte alignment;
    // SDL synchronously copies it on the window's owning thread.
    assert_eq!(
        unsafe { SDL_PushEvent(aligned.as_mut_ptr().cast::<c_void>()) },
        1
    );
}
