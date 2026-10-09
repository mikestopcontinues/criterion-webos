// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact narrow SDL2 ABI. No SDL structs or vendor player symbols cross this seam.
use crate::{Activity, Event, EventLayout, Size, Surface, decode_event};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    fmt,
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

const VIDEO: u32 = 0x20;
const EVENTS: u32 = 0x4000;
const INPUT_FOCUS: u32 = 0x200;
static VIDEO_OWNED: AtomicBool = AtomicBool::new(false);

#[link(name = "SDL2")]
unsafe extern "C" {
    fn SDL_SetMainReady();
    fn SDL_InitSubSystem(flags: u32) -> c_int;
    fn SDL_QuitSubSystem(flags: u32);
    fn SDL_WasInit(flags: u32) -> u32;
    fn SDL_GetError() -> *const c_char;
    fn SDL_GL_SetAttribute(attribute: c_int, value: c_int) -> c_int;
    fn SDL_GL_GetAttribute(attribute: c_int, value: *mut c_int) -> c_int;
    fn SDL_CreateWindow(
        title: *const c_char,
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
        flags: u32,
    ) -> *mut c_void;
    fn SDL_DestroyWindow(window: *mut c_void);
    fn SDL_GL_CreateContext(window: *mut c_void) -> *mut c_void;
    fn SDL_GL_DeleteContext(context: *mut c_void);
    fn SDL_GL_GetProcAddress(name: *const c_char) -> *mut c_void;
    fn SDL_GL_SetSwapInterval(interval: c_int) -> c_int;
    fn SDL_GL_SwapWindow(window: *mut c_void);
    fn SDL_GetWindowSize(window: *mut c_void, width: *mut c_int, height: *mut c_int);
    fn SDL_GL_GetDrawableSize(window: *mut c_void, width: *mut c_int, height: *mut c_int);
    fn SDL_GetWindowFlags(window: *mut c_void) -> u32;
    fn SDL_PollEvent(event: *mut c_void) -> c_int;
    fn SDL_StartTextInput();
    fn SDL_StopTextInput();
    fn SDL_free(memory: *mut c_void);
}

#[derive(Debug)]
pub enum PlatformError {
    UnsupportedTarget,
    AlreadyOpen,
    AlreadyInitialized,
    InvalidTitle,
    BackPolicyMissing,
    NotForeground,
    KeyboardFocusUnavailable,
    InvalidSurface,
    ContextMismatch,
    Event(&'static str),
    Sdl {
        operation: &'static str,
        message: String,
    },
}
impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedTarget => {
                f.write_str("SDL backend requires little-endian Linux; webOS requires ARM32")
            }
            Self::AlreadyOpen => f.write_str("a platform window already owns SDL video"),
            Self::AlreadyInitialized => f.write_str("SDL video or events already has an owner"),
            Self::InvalidTitle => f.write_str("window title contains a nul byte"),
            Self::BackPolicyMissing => f.write_str(
                "configure SDL_WEBOS_ACCESS_POLICY_KEYS_BACK=true before SDL initialization",
            ),
            Self::NotForeground => f.write_str("window is not in the foreground"),
            Self::KeyboardFocusUnavailable => f.write_str("window does not own keyboard focus"),
            Self::InvalidSurface => f.write_str("SDL returned invalid surface dimensions"),
            Self::ContextMismatch => f.write_str("SDL did not provide an RGBA8 framebuffer"),
            Self::Event(message) => f.write_str(message),
            Self::Sdl { operation, message } => write!(f, "{operation}: {message}"),
        }
    }
}
impl std::error::Error for PlatformError {}

fn sdl_error(operation: &'static str) -> PlatformError {
    // SDL owns this nul-terminated diagnostic until the next SDL error operation.
    let pointer = unsafe { SDL_GetError() };
    let message = if pointer.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .chars()
            .take(512)
            .collect()
    };
    PlatformError::Sdl { operation, message }
}
fn attribute(attribute: c_int) -> Result<c_int, PlatformError> {
    let mut value = 0;
    if unsafe { SDL_GL_GetAttribute(attribute, &mut value) } != 0 {
        return Err(sdl_error("read GL context attribute"));
    }
    Ok(value)
}

struct VideoReference;
impl VideoReference {
    fn acquire() -> Result<Self, PlatformError> {
        if !cfg!(all(target_os = "linux", target_endian = "little"))
            || (cfg!(feature = "webos")
                && !cfg!(all(target_arch = "arm", target_pointer_width = "32")))
        {
            return Err(PlatformError::UnsupportedTarget);
        }
        if cfg!(feature = "webos")
            && std::env::var_os("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK").as_deref()
                != Some(std::ffi::OsStr::new("true"))
        {
            return Err(PlatformError::BackPolicyMissing);
        }
        VIDEO_OWNED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PlatformError::AlreadyOpen)?;
        // The application opens on its main thread before any other SDL user. Reject
        // existing references so failed initialization cannot decrement their EVENTS.
        if unsafe { SDL_WasInit(VIDEO | EVENTS) } != 0 {
            VIDEO_OWNED.store(false, Ordering::Release);
            return Err(PlatformError::AlreadyInitialized);
        }
        // This program uses a Rust entry point rather than SDL_main. VideoReference owns
        // one SDL subsystem reference; failure releases our process-local ownership claim.
        unsafe {
            SDL_SetMainReady();
        }
        if unsafe { SDL_InitSubSystem(VIDEO) } != 0 {
            let error = sdl_error("initialize SDL video");
            // Older SDL forks increment EVENTS before VIDEO and do not roll it back
            // when VIDEO fails. Exclusive ownership makes this dependency cleanup safe.
            unsafe {
                SDL_QuitSubSystem(VIDEO);
            }
            VIDEO_OWNED.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(Self)
    }
}
impl Drop for VideoReference {
    fn drop(&mut self) {
        unsafe {
            SDL_QuitSubSystem(VIDEO);
        }
        VIDEO_OWNED.store(false, Ordering::Release);
    }
}

/// SDL-reported context configuration and queried framebuffer properties.
/// SDL can return cached major/minor/profile requests; the renderer must inspect the
/// driver's GL_VERSION and API support before admitting its actual graphics profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextInfo {
    pub major: i32,
    pub minor: i32,
    pub profile: i32,
    pub red_bits: i32,
    pub green_bits: i32,
    pub blue_bits: i32,
    pub alpha_bits: i32,
    pub depth_bits: i32,
    pub stencil_bits: i32,
}

/// One SDL window and current GL context, owned exclusively by its initialization thread.
/// Open it on the application's main thread, before other SDL users. The webOS launcher
/// must configure the Back policy environment before any threads start.
///
/// ```compile_fail
/// # use criterion_platform::Window;
/// fn move_to_worker(window: Window) {
///     std::thread::spawn(move || drop(window));
/// }
/// ```
///
/// ```compile_fail
/// # use criterion_platform::Window;
/// fn share_with_worker(window: &Window) {
///     std::thread::scope(|scope| { scope.spawn(move || window.context_info()); });
/// }
/// ```
pub struct Window {
    window: NonNull<c_void>,
    context: NonNull<c_void>,
    info: ContextInfo,
    activity: Activity,
    text_input: bool,
    // Keep this field alive through Drop, after context/window cleanup.
    _video: VideoReference,
    _thread: PhantomData<Rc<()>>,
}
impl Window {
    /// Request a1920×1080 GLES2 window. Only webOS requests fullscreen; the Linux
    /// host window is resizable/high-DPI for rendering and scaling verification.
    pub fn open(title: &str) -> Result<Self, PlatformError> {
        let title = CString::new(title).map_err(|_| PlatformError::InvalidTitle)?;
        let video = VideoReference::acquire()?;
        // SDL_GLattr is a stable scalar enum. No depth/stencil is needed by this2D app.
        for (key, value) in [
            (21, 4),
            (17, 2),
            (18, 0),
            (0, 8),
            (1, 8),
            (2, 8),
            (3, 8),
            (4, 32),
            (6, 0),
            (7, 0),
        ] {
            if unsafe { SDL_GL_SetAttribute(key, value) } != 0 {
                return Err(sdl_error("request GL context attribute"));
            }
        }
        let flags = if cfg!(feature = "webos") {
            0x2 | 0x1
        } else {
            0x2 | 0x20 | 0x2000
        };
        let window =
            NonNull::new(unsafe { SDL_CreateWindow(title.as_ptr(), 0, 0, 1920, 1080, flags) })
                .ok_or_else(|| sdl_error("create SDL window"))?;
        let Some(context) = NonNull::new(unsafe { SDL_GL_CreateContext(window.as_ptr()) }) else {
            let error = sdl_error("create GLES context");
            unsafe {
                SDL_DestroyWindow(window.as_ptr());
            }
            return Err(error);
        };
        // Ownership exists before subsequent fallible queries so their errors clean up.
        let mut owned = Self {
            window,
            context,
            info: ContextInfo {
                major: 0,
                minor: 0,
                profile: 0,
                red_bits: 0,
                green_bits: 0,
                blue_bits: 0,
                alpha_bits: 0,
                depth_bits: 0,
                stencil_bits: 0,
            },
            activity: Activity::Foreground,
            text_input: false,
            _video: video,
            _thread: PhantomData,
        };
        owned.info = ContextInfo {
            major: attribute(17)?,
            minor: attribute(18)?,
            profile: attribute(21)?,
            red_bits: attribute(0)?,
            green_bits: attribute(1)?,
            blue_bits: attribute(2)?,
            alpha_bits: attribute(3)?,
            depth_bits: attribute(6)?,
            stencil_bits: attribute(7)?,
        };
        if [
            owned.info.red_bits,
            owned.info.green_bits,
            owned.info.blue_bits,
            owned.info.alpha_bits,
        ]
        .iter()
        .any(|bits| *bits < 8)
        {
            return Err(PlatformError::ContextMismatch);
        }
        owned.surface()?;
        if unsafe { SDL_GL_SetSwapInterval(1) } != 0 {
            return Err(sdl_error("request synchronized presentation"));
        }
        // Desktop SDL starts text input implicitly. This owner admits it only when requested.
        unsafe {
            SDL_StopTextInput();
        }
        Ok(owned)
    }
    pub fn context_info(&self) -> ContextInfo {
        self.info
    }
    pub fn activity(&self) -> Activity {
        self.activity
    }
    pub fn surface(&self) -> Result<Surface, PlatformError> {
        let (mut width, mut height, mut drawable_width, mut drawable_height) = (0, 0, 0, 0);
        unsafe {
            SDL_GetWindowSize(self.window.as_ptr(), &mut width, &mut height);
            SDL_GL_GetDrawableSize(
                self.window.as_ptr(),
                &mut drawable_width,
                &mut drawable_height,
            );
        }
        if width <= 0 || height <= 0 || drawable_width <= 0 || drawable_height <= 0 {
            return Err(PlatformError::InvalidSurface);
        }
        Ok(Surface {
            window: Size {
                width: width as u32,
                height: height as u32,
            },
            drawable: Size {
                width: drawable_width as u32,
                height: drawable_height as u32,
            },
        })
    }
    /// Return a foreign GL function pointer, possibly null. The renderer owns validation
    /// and invocation; the pointer must not outlive this context or cross threads.
    pub fn gl_proc_address(&self, name: &CStr) -> *const c_void {
        unsafe { SDL_GL_GetProcAddress(name.as_ptr()) }.cast_const()
    }
    /// Poll exactly one event. Every returned event has already updated lifecycle admission.
    pub fn poll_event(&mut self) -> Result<Option<Event>, PlatformError> {
        #[repr(C, align(8))]
        struct Buffer([u8; 128]);
        let mut buffer = Buffer([0; 128]);
        if unsafe { SDL_PollEvent(buffer.0.as_mut_ptr().cast()) } == 0 {
            return Ok(None);
        }
        let layout = if cfg!(feature = "webos") {
            EventLayout::WebOs
        } else {
            EventLayout::Desktop
        };
        let decoded = decode_event(&buffer.0, layout);
        free_owned_event_payload(&buffer.0);
        let event = decoded.map_err(PlatformError::Event)?;
        self.activity.observe(&event);
        if self.activity != Activity::Foreground || event == Event::KeyboardFocus(false) {
            self.stop_text_input();
        }
        Ok(Some(event))
    }
    /// A swap is admitted only while foregrounded. SDL's void swap provides no presentation
    /// completion acknowledgment; actual compositor/device evidence remains separate.
    pub fn present(&self) -> Result<(), PlatformError> {
        if self.activity != Activity::Foreground {
            return Err(PlatformError::NotForeground);
        }
        unsafe {
            SDL_GL_SwapWindow(self.window.as_ptr());
        }
        Ok(())
    }
    /// Request committed text from SDL/IME. This does not prove an on-screen keyboard appeared.
    pub fn text_input(&mut self, enabled: bool) -> Result<(), PlatformError> {
        if !enabled {
            self.stop_text_input();
            return Ok(());
        }
        if self.activity != Activity::Foreground {
            return Err(PlatformError::NotForeground);
        }
        if unsafe { SDL_GetWindowFlags(self.window.as_ptr()) } & INPUT_FOCUS == 0 {
            return Err(PlatformError::KeyboardFocusUnavailable);
        }
        if !self.text_input {
            unsafe {
                SDL_StartTextInput();
            }
            self.text_input = true;
        }
        Ok(())
    }
    fn stop_text_input(&mut self) {
        if self.text_input {
            unsafe {
                SDL_StopTextInput();
            }
            self.text_input = false;
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        self.stop_text_input();
        // SDL owns the opaque handles. Each successful construction has exactly one Drop.
        unsafe {
            SDL_GL_DeleteContext(self.context.as_ptr());
            SDL_DestroyWindow(self.window.as_ptr());
        }
    }
}

fn free_owned_event_payload(raw: &[u8; 128]) {
    let kind = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    // DROPFILE/DROPTEXT and desktop TEXTEDITING_EXT allocate strings that the receiver
    // must release with SDL_free, even when we don't expose that event. User pointers are
    // caller-owned and are never freed. These pointer offsets come from SDL2's public ABI.
    let offset = match kind {
        0x1000 | 0x1001 => 8,
        0x305 if !cfg!(feature = "webos") => {
            if cfg!(target_pointer_width = "64") {
                16
            } else {
                12
            }
        }
        _ => return,
    };
    let mut bytes = [0; std::mem::size_of::<usize>()];
    bytes.copy_from_slice(&raw[offset..offset + std::mem::size_of::<usize>()]);
    let pointer = usize::from_le_bytes(bytes) as *mut c_void;
    if !pointer.is_null() {
        unsafe {
            SDL_free(pointer);
        }
    }
}
