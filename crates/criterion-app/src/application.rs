// SPDX-License-Identifier: GPL-3.0-or-later
//! Rendered application ownership, independent of the SDL/GLES main-thread lifetime.
use crate::{
    artwork::Artwork,
    authentication::Authentication,
    controller::{Controller, Effect},
};
use criterion_app::InputAdapter;
use criterion_platform::{Event, Surface};
use criterion_provider::{Catalog, HttpTransport, RequestTransport};
use criterion_session::{MonotonicClock, SystemClock, Transport};
use criterion_ui::{AppUi, Command, LoadState};
use std::time::Duration;
use tokio::runtime::{Handle, Runtime};

pub(crate) struct Application<
    P = HttpTransport,
    T: Transport = criterion_session::HttpTransport,
    C: MonotonicClock = SystemClock,
> {
    ui: AppUi,
    controller: Controller<P>,
    authentication: Authentication<T, C>,
    artwork: Artwork,
    input: InputAdapter,
    output: Option<egui::FullOutput>,
    exiting: bool,
    queued_input: bool,
    active: bool,
}
impl<P, T: Transport, C: MonotonicClock> Drop for Application<P, T, C> {
    fn drop(&mut self) {
        // Exiting/background disposal intentionally drops unpainted texture work.
        // The main-thread renderer is destroyed after this application owner.
        if let Some(output) = &mut self.output {
            output.textures_delta.clear();
        }
    }
}

impl Application {
    pub(crate) fn new(surface: Surface, runtime: &Handle) -> Result<Self, &'static str> {
        Ok(Self::with_parts(
            surface,
            Controller::new(Catalog::new().map_err(|_| "the catalog")?, runtime),
            Authentication::new().map_err(|_| "the subscriber session")?,
            Artwork::new().map_err(|_| "the artwork client")?,
        ))
    }
}
impl<
    P: RequestTransport + Send + Sync + 'static,
    T: Transport + 'static,
    C: MonotonicClock + Clone + 'static,
> Application<P, T, C>
{
    fn with_parts(
        surface: Surface,
        controller: Controller<P>,
        authentication: Authentication<T, C>,
        artwork: Artwork,
    ) -> Self {
        Self {
            ui: AppUi::new(),
            controller,
            authentication,
            artwork,
            input: InputAdapter::new(surface),
            output: None,
            exiting: false,
            queued_input: false,
            active: true,
        }
    }
    pub(crate) fn event(
        &mut self,
        event: Event,
        surface: Surface,
        runtime: &Runtime,
        now: Duration,
    ) {
        let key_boundary = matches!(event, Event::Key(_));
        let editing_boundary = matches!(
            event,
            Event::Key(_) | Event::Text(_) | Event::Composition { .. }
        );
        // Pointer activation and committed IME text queued earlier in the SDL
        // drain must settle before this key's editing ownership is decided.
        if editing_boundary && self.queued_input {
            self.consume(runtime, now);
        }
        self.input.set_text_input(self.ui.wants_text_input());
        self.input.push(event, surface, now);
        self.queued_input = true;
        if key_boundary {
            self.consume(runtime, now);
        }
    }
    pub(crate) fn consume(&mut self, runtime: &Runtime, now: Duration) {
        self.queued_input = false;
        let batch = self.input.take_frame(now);
        let frame = self
            .controller
            .view
            .with_view(self.authentication.view(), |data| {
                self.ui.render(batch.raw, data)
            });
        for command in frame.commands {
            if self.exiting {
                break;
            }
            self.command(command, runtime.handle());
        }
        for action in batch.actions {
            if self.exiting {
                break;
            }
            let commands = self
                .controller
                .view
                .with_view(self.authentication.view(), |data| {
                    self.ui.handle(action, data)
                });
            for command in commands {
                self.command(command, runtime.handle());
            }
        }
        // Admission uses only the current projection's immutable bindings. A
        // command that changed the view cannot fetch a previous frame's source.
        if self.active && !self.exiting {
            self.artwork.update(
                &frame.visible_artwork,
                self.controller.view.artwork_bindings(),
            );
            self.artwork.poll(runtime, &mut self.ui);
        }
        match &mut self.output {
            Some(output) => output.append(frame.output),
            None => self.output = Some(frame.output),
        }
        self.input.set_text_input(self.ui.wants_text_input());
    }
    fn command(&mut self, command: Command, runtime: &Handle) {
        let retained_search = (matches!(command, Command::Navigate(criterion_ui::Page::Search))
            && !self.ui.query().trim().is_empty())
        .then(|| Command::Search {
            query: self.ui.query().to_owned(),
            group: self.ui.search_group(),
        });
        match self.controller.command(command, self.ui.page(), runtime) {
            Effect::None => (),
            Effect::Exit => self.exiting = true,
            Effect::Authenticate | Effect::RetryAuthentication => {
                self.authentication.begin(runtime)
            }
            Effect::CancelAuthentication => self.authentication.cancel(),
            Effect::Logout => self.authentication.logout(runtime),
            Effect::Play(_) | Effect::ToggleList(_) if !self.authentication.signed_in() => {
                for command in self.ui.begin_authentication() {
                    self.command(command, runtime);
                }
            }
            // Subscriber shelf and licensed playback admission are separate reviewed
            // layers. This development executable cannot advertise those as available.
            Effect::Play(id) | Effect::ToggleList(id) => {
                drop(id);
                self.controller.view.set_status(LoadState::Error);
            }
            Effect::VoiceSearch => self.controller.view.set_status(LoadState::Error),
        }
        if let Some(search) = retained_search {
            // A fresh rail visit keeps the visible query/group. Only Navigate
            // pushes history; replay its typed request without another snapshot.
            self.command(search, runtime);
        }
    }
    pub(crate) fn poll(&mut self, runtime: &Runtime, active: bool) {
        if active {
            self.controller.poll(runtime);
        }
        self.authentication.poll(runtime, active);
    }
    pub(crate) fn foreground(&mut self, runtime: &Handle) {
        self.active = true;
        self.controller.foreground(runtime);
    }
    pub(crate) fn background(&mut self) {
        self.active = false;
        self.artwork.clear();
        self.controller.background();
    }
    pub(crate) fn finish(&mut self, runtime: &Runtime) -> bool {
        self.authentication.finish(runtime)
    }
    pub(crate) fn exiting(&self) -> bool {
        self.exiting
    }
    pub(crate) fn exit(&mut self) {
        self.exiting = true;
    }
    pub(crate) fn wants_text_input(&self) -> bool {
        self.ui.wants_text_input()
    }
    pub(crate) fn context(&self) -> &egui::Context {
        self.ui.context()
    }
    pub(crate) fn take_output(&mut self) -> Option<egui::FullOutput> {
        self.output.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use criterion_platform::{KeyEvent, Size};
    use criterion_provider::{Error, Request, Response};
    use criterion_ui::{Action, Focus, Page};
    use std::sync::Arc;

    struct Offline;
    impl RequestTransport for Offline {
        async fn get(&self, _request: Request) -> Result<Response, Error> {
            Err(Error::Unavailable)
        }
    }
    impl Transport for Offline {
        async fn post(
            &self,
            _request: criterion_session::Request,
        ) -> Result<criterion_session::Response, criterion_session::Error> {
            Err(criterion_session::Error::Unavailable)
        }
    }
    fn surface() -> Surface {
        Surface {
            window: Size {
                width: 1920,
                height: 1080,
            },
            drawable: Size {
                width: 1920,
                height: 1080,
            },
        }
    }
    fn runtime() -> Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }
    fn artwork_region_has_content(pixels: &[u8], width: usize, height: usize) -> bool {
        if width < 4 || height < 4 || pixels.len() != width * height * 4 {
            return false;
        }
        // The top right quarter has background artwork without native labels,
        // controls or the rail. GL readback rows start at the bottom.
        let mut low = u16::MAX;
        let mut high = 0;
        for y in height * 3 / 4..height {
            for x in width / 2..width {
                let at = (y * width + x) * 4;
                let brightness =
                    u16::from(pixels[at]) + u16::from(pixels[at + 1]) + u16::from(pixels[at + 2]);
                low = low.min(brightness);
                high = high.max(brightness);
            }
        }
        high.saturating_sub(low) >= 48
    }
    #[test]
    fn artwork_readback_oracle_rejects_blank_and_untextured_frames() {
        assert!(!artwork_region_has_content(&vec![0; 32 * 24 * 4], 32, 24));
        assert!(!artwork_region_has_content(&vec![17; 32 * 24 * 4], 32, 24));
        assert!(!artwork_region_has_content(&[], 1920, 1080));
    }
    fn app(runtime: &Runtime) -> Application<Offline, Offline> {
        let clock = SystemClock::default();
        Application::with_parts(
            surface(),
            Controller::new(Catalog::with_transport(Offline), runtime.handle()),
            Authentication::with_session(
                Arc::new(criterion_session::Session::with_transport(
                    criterion_session::Configuration::production(),
                    Offline,
                    clock.clone(),
                )),
                clock,
            ),
            Artwork::new().unwrap(),
        )
    }
    fn action(app: &mut Application<Offline, Offline>, runtime: &Runtime, action: Action) {
        let commands = app
            .controller
            .view
            .with_view(app.authentication.view(), |data| {
                app.ui.handle(action, data)
            });
        for command in commands {
            app.command(command, runtime.handle());
        }
    }
    #[test]
    fn pointer_focus_precedes_editing_key_in_same_native_drain() {
        let runtime = runtime();
        let mut app = app(&runtime);
        for key in [Action::Left, Action::Up, Action::Select] {
            action(&mut app, &runtime, key);
        }
        assert_eq!(app.ui.page(), Page::Search);
        app.event(
            Event::Text("abc".into()),
            surface(),
            &runtime,
            Duration::ZERO,
        );
        app.consume(&runtime, Duration::ZERO);
        assert_eq!(app.ui.query(), "abc");
        for _ in 0..6 {
            action(&mut app, &runtime, Action::Right);
        }
        action(&mut app, &runtime, Action::Down);
        assert!(!app.ui.wants_text_input());
        app.consume(&runtime, Duration::ZERO);
        for pressed in [true, false] {
            app.event(
                Event::PointerButton {
                    button: 1,
                    pressed,
                    x: 850,
                    y: 160,
                },
                surface(),
                &runtime,
                Duration::ZERO,
            );
        }
        app.event(
            Event::Key(KeyEvent {
                scancode: 42,
                keycode: 8,
                pressed: true,
                repeat: false,
            }),
            surface(),
            &runtime,
            Duration::ZERO,
        );
        assert_eq!(app.ui.focus(), Focus::SearchField);
        assert_eq!(
            app.ui.query(),
            "ab",
            "earlier pointer activation must own editing before this key is admitted"
        );
    }
    #[test]
    fn pointer_focus_precedes_direct_text_and_composition_without_a_key_event() {
        let runtime = runtime();
        let mut app = app(&runtime);
        for key in [Action::Left, Action::Up, Action::Select] {
            action(&mut app, &runtime, key);
        }
        for _ in 0..6 {
            action(&mut app, &runtime, Action::Right);
        }
        action(&mut app, &runtime, Action::Down);
        app.consume(&runtime, Duration::ZERO);
        assert!(!app.ui.wants_text_input());
        for pressed in [true, false] {
            app.event(
                Event::PointerButton {
                    button: 1,
                    pressed,
                    x: 850,
                    y: 160,
                },
                surface(),
                &runtime,
                Duration::ZERO,
            );
        }
        app.event(
            Event::Composition {
                text: "preedit".into(),
                start: 0,
                length: 0,
            },
            surface(),
            &runtime,
            Duration::ZERO,
        );
        app.event(
            Event::Text("xyz".into()),
            surface(),
            &runtime,
            Duration::ZERO,
        );
        app.consume(&runtime, Duration::ZERO);
        assert_eq!(
            app.ui.query(),
            "xyz",
            "IME commits must follow the earlier pointer field ownership"
        );
    }
    #[test]
    fn fresh_search_rail_entry_reissues_the_retained_query_and_group() {
        let runtime = runtime();
        let mut app = app(&runtime);
        for key in [Action::Left, Action::Up, Action::Select, Action::Select] {
            action(&mut app, &runtime, key);
        }
        for _ in 0..6 {
            action(&mut app, &runtime, Action::Right);
        }
        for key in [Action::Down, Action::Right, Action::Select] {
            action(&mut app, &runtime, key);
        }
        assert_eq!(app.ui.query(), "a");
        assert_eq!(app.ui.search_group(), criterion_ui::SearchGroup::Films);
        for _ in 0..3 {
            action(&mut app, &runtime, Action::Left);
        }
        assert_eq!(app.ui.focus(), Focus::Rail(criterion_ui::RailItem::Search));
        for key in [
            Action::Down,
            Action::Select,
            Action::Left,
            Action::Up,
            Action::Select,
        ] {
            action(&mut app, &runtime, key);
        }
        assert_eq!(app.ui.page(), Page::Search);
        assert_eq!(app.ui.query(), "a");
        assert_eq!(app.ui.search_group(), criterion_ui::SearchGroup::Films);
        assert_eq!(
            app.controller
                .view
                .with_view(app.authentication.view(), |data| data.status),
            LoadState::Loading,
            "the visible retained query must have an active matching request after a fresh rail visit"
        );
    }
    #[test]
    #[ignore = "requires a serialized SDL/GLES display; run under canonical xvfb-run"]
    fn native_window_input_and_search_frame_present_end_to_end() {
        use criterion_ui::GlowRenderer;
        use glow::HasContext;
        use std::{
            ffi::{CString, c_void},
            io::Write,
        };
        unsafe extern "C" {
            fn SDL_PushEvent(event: *mut c_void) -> i32;
        }
        super::super::prepare_process();
        let mut window = criterion_platform::Window::open("Criterion Unofficial E2E").unwrap();
        // SAFETY: the SDL window owns this current context on this test thread.
        let gl = Arc::new(unsafe {
            glow::Context::from_loader_function(|name| {
                window.gl_proc_address(&CString::new(name).unwrap())
            })
        });
        let mut painter = unsafe { GlowRenderer::new(gl.clone()) }.unwrap();
        let runtime = runtime();
        let mut app = app(&runtime);
        for _ in 0..128 {
            let Some(event) = window.poll_event().unwrap() else {
                break;
            };
            app.event(event, window.surface().unwrap(), &runtime, Duration::ZERO);
        }
        for (scancode, keycode) in [(80u32, 1_073_741_904i32), (82, 1_073_741_906), (40, 13)] {
            let mut raw = [0u8; 56];
            raw[..4].copy_from_slice(&0x300u32.to_le_bytes());
            raw[12] = 1;
            raw[16..20].copy_from_slice(&scancode.to_le_bytes());
            raw[20..24].copy_from_slice(&keycode.to_le_bytes());
            // SAFETY: the pinned desktop SDL_Event ABI is56 bytes, aligned to8;
            // SDL copies this initialized event during the synchronous call.
            let mut aligned = [0u64; 7];
            for (word, bytes) in aligned.iter_mut().zip(raw.as_chunks::<8>().0) {
                *word = u64::from_le_bytes(*bytes);
            }
            assert_eq!(
                unsafe { SDL_PushEvent(aligned.as_mut_ptr().cast::<c_void>()) },
                1
            );
        }
        let mut text = [0u64; 7];
        let mut raw = [0u8; 56];
        raw[..4].copy_from_slice(&0x303u32.to_le_bytes());
        raw[12..15].copy_from_slice(b"abc");
        for (word, bytes) in text.iter_mut().zip(raw.as_chunks::<8>().0) {
            *word = u64::from_le_bytes(*bytes);
        }
        // SAFETY: same initialized/aligned desktop SDL_Event ABI as above.
        assert_eq!(
            unsafe { SDL_PushEvent(text.as_mut_ptr().cast::<c_void>()) },
            1
        );
        for _ in 0..128 {
            let Some(event) = window.poll_event().unwrap() else {
                break;
            };
            app.event(
                event,
                window.surface().unwrap(),
                &runtime,
                Duration::from_millis(16),
            );
        }
        app.consume(&runtime, Duration::from_millis(16));
        assert_eq!(app.ui.page(), Page::Search);
        assert_eq!(app.ui.query(), "abc");
        // Input handling follows layout in the same CPU frame; present the next
        // settled frame so the capture also contains the committed query.
        app.consume(&runtime, Duration::from_millis(32));
        let drawable = window.surface().unwrap().drawable;
        let mut frame = app.take_output().unwrap();
        painter
            .paint([drawable.width, drawable.height], app.context(), &mut frame)
            .unwrap();
        let mut pixels = vec![0; drawable.width as usize * drawable.height as usize * 4];
        // SAFETY: the context is current, RGBA/UNSIGNED_BYTE writes exactly the
        // checked drawable allocation; no GL/native resource crosses a thread.
        unsafe {
            gl.read_pixels(
                0,
                0,
                drawable.width as i32,
                drawable.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] > 100 && p[1] > 100 && p[2] > 100)
                .count()
                > 500,
            "the actual GLES framebuffer must contain rendered text and controls"
        );
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/e2e");
        std::fs::create_dir_all(&directory).unwrap();
        let mut capture = std::fs::File::create(directory.join("native-search.ppm")).unwrap();
        write!(capture, "P6\n{} {}\n255\n", drawable.width, drawable.height).unwrap();
        for row in pixels.chunks_exact(drawable.width as usize * 4).rev() {
            for pixel in row.as_chunks::<4>().0 {
                capture.write_all(&pixel[..3]).unwrap();
            }
        }
        window.present().unwrap();
        assert_eq!(unsafe { gl.get_error() }, glow::NO_ERROR);
        app.background();
        drop(app);
        painter.destroy();
        println!("native SDL input + GLES rendered search E2E passed");
    }
    #[test]
    #[ignore = "live anonymous provider/artwork and serialized SDL/GLES; root executor only"]
    fn native_public_catalog_artwork_and_detail_roundtrip_end_to_end() {
        use criterion_ui::GlowRenderer;
        use glow::HasContext;
        use std::{
            ffi::{CString, c_void},
            time::Instant,
        };
        unsafe extern "C" {
            fn SDL_PushEvent(event: *mut c_void) -> i32;
        }
        fn key(
            window: &mut criterion_platform::Window,
            app: &mut Application,
            runtime: &Runtime,
            scancode: u32,
            keycode: i32,
            now: Duration,
        ) {
            for pressed in [true, false] {
                let mut raw = [0u8; 56];
                raw[..4].copy_from_slice(&(if pressed { 0x300u32 } else { 0x301 }).to_le_bytes());
                raw[12] = u8::from(pressed);
                raw[16..20].copy_from_slice(&scancode.to_le_bytes());
                raw[20..24].copy_from_slice(&keycode.to_le_bytes());
                let mut aligned = [0u64; 7];
                for (word, bytes) in aligned.iter_mut().zip(raw.as_chunks::<8>().0) {
                    *word = u64::from_le_bytes(*bytes);
                }
                // SAFETY: initialized56-byte stock desktopSDL event, eight-byte
                // alignment; SDL synchronously copies it on the window thread.
                assert_eq!(
                    unsafe { SDL_PushEvent(aligned.as_mut_ptr().cast::<c_void>()) },
                    1
                );
                for _ in 0..128 {
                    let Some(event) = window.poll_event().unwrap() else {
                        break;
                    };
                    app.event(event, window.surface().unwrap(), runtime, now);
                }
            }
        }
        fn ready(app: &mut Application, runtime: &Runtime, start: Instant, want_detail: bool) {
            let deadline = Instant::now() + Duration::from_secs(45);
            loop {
                app.poll(runtime, true);
                app.consume(runtime, start.elapsed());
                let (status, matches, artwork_ready) =
                    app.controller
                        .view
                        .with_view(app.authentication.view(), |data| {
                            (
                                data.status,
                                if want_detail {
                                    data.detail.is_some()
                                } else {
                                    !data.cards.is_empty()
                                },
                                if want_detail {
                                    data.detail
                                        .as_ref()
                                        .and_then(|detail| detail.card.artwork_key)
                                } else {
                                    data.cards.first().and_then(|card| card.artwork_key)
                                }
                                .is_some_and(|key| app.ui.has_image(key)),
                            )
                        });
                assert!(
                    !matches!(status, LoadState::Error | LoadState::Offline),
                    "live public response must be admitted"
                );
                if status == LoadState::Ready && matches && artwork_ready {
                    // Artwork may have been admitted after this frame's layout.
                    // Paint one settled frame with the owning image uploaded.
                    app.consume(runtime, start.elapsed());
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "live public catalog/artwork readiness deadline"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        super::super::prepare_process();
        let mut window =
            criterion_platform::Window::open("Criterion Unofficial Public E2E").unwrap();
        // SAFETY: this thread owns the live SDL context until painter destruction.
        let gl = Arc::new(unsafe {
            glow::Context::from_loader_function(|name| {
                window.gl_proc_address(&CString::new(name).unwrap())
            })
        });
        let mut painter = unsafe { GlowRenderer::new(gl.clone()) }.unwrap();
        let runtime = runtime();
        let mut app = Application::new(window.surface().unwrap(), runtime.handle()).unwrap();
        let start = Instant::now();
        // Rail: Home→New→AllFilms. No subscriber/linking/player action is issued.
        for (scancode, keycode) in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
        ] {
            key(
                &mut window,
                &mut app,
                &runtime,
                scancode,
                keycode,
                start.elapsed(),
            );
        }
        assert_eq!(app.ui.page(), Page::AllFilms);
        ready(&mut app, &runtime, start, false);
        let origin = app.ui.focus();
        key(&mut window, &mut app, &runtime, 40, 13, start.elapsed());
        assert_eq!(app.ui.page(), Page::Detail);
        ready(&mut app, &runtime, start, true);
        let drawable = window.surface().unwrap().drawable;
        let mut frame = app.take_output().unwrap();
        assert!(
            frame.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Mesh(mesh)
                if mesh.texture_id != egui::TextureId::default()
                && mesh.calc_bounds().width() >= 1900.0
                && mesh.calc_bounds().height() >= 1000.0)
            }),
            "the settled detail must contain the owning background texture"
        );
        painter
            .paint([drawable.width, drawable.height], app.context(), &mut frame)
            .unwrap();
        let mut pixels = vec![0u8; drawable.width as usize * drawable.height as usize * 4];
        // SAFETY: current context and exact drawableRGBA8 buffer on this thread.
        unsafe {
            gl.read_pixels(
                0,
                0,
                drawable.width as i32,
                drawable.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        assert_eq!(unsafe { gl.get_error() }, glow::NO_ERROR);
        assert!(
            artwork_region_has_content(&pixels, drawable.width as usize, drawable.height as usize),
            "the actual artwork region must contain image contrast, not a blank frame"
        );
        let buffer = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
            drawable.width,
            drawable.height,
            pixels,
        )
        .unwrap();
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/e2e");
        std::fs::create_dir_all(&directory).unwrap();
        image::imageops::flip_vertical(&buffer)
            .save(directory.join("native-public-detail.png"))
            .unwrap();
        window.present().unwrap();
        key(&mut window, &mut app, &runtime, 41, 27, start.elapsed());
        assert_eq!(app.ui.page(), Page::AllFilms);
        assert_eq!(app.ui.focus(), origin);
        assert!(!app.authentication.signed_in());
        app.background();
        assert!(app.finish(&runtime));
        drop(app);
        painter.destroy();
        println!("live anonymous catalog + artwork + native SDL detail/Back + GLES passed");
    }
}
