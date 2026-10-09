// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in anonymous production paging through real SDL input and GLES frames.
use super::*;
use criterion_ui::{CatalogTail, Focus, GlowRenderer, LoginView, Page, Target};
use glow::HasContext;
use std::{
    ffi::{CString, c_void},
    sync::Arc,
    time::Instant,
};

unsafe extern "C" {
    fn SDL_PushEvent(event: *mut c_void) -> i32;
}

#[derive(PartialEq)]
struct WindowSnapshot {
    first: usize,
    tail: CatalogTail,
    total: u32,
    targets: Vec<Target>,
    focus: Focus,
    scroll: f32,
}

struct Live<'a> {
    window: &'a mut criterion_platform::Window,
    app: &'a mut Application,
    runtime: &'a Runtime,
    painter: &'a mut GlowRenderer,
    gl: &'a glow::Context,
    start: Instant,
}

fn require(condition: bool, failure: &'static str) -> Result<(), &'static str> {
    condition.then_some(()).ok_or(failure)
}

fn spatial_contrast(pixels: impl Iterator<Item = [u8; 3]>) -> bool {
    let mut minimum = [u8::MAX; 3];
    let mut maximum = [u8::MIN; 3];
    for pixel in pixels {
        for channel in 0..3 {
            minimum[channel] = minimum[channel].min(pixel[channel]);
            maximum[channel] = maximum[channel].max(pixel[channel]);
        }
    }
    (0..3).any(|channel| maximum[channel].saturating_sub(minimum[channel]) > 32)
}

#[test]
fn focused_artwork_oracle_rejects_spatially_uniform_color() {
    assert!(!spatial_contrast([[200, 20, 20]; 8].into_iter()));
    assert!(spatial_contrast([[200, 20, 20], [20, 20, 20]].into_iter()));
    assert!(!spatial_contrast(
        [[200, 20, 20, 0], [200, 20, 20, 255]]
            .into_iter()
            .map(|rgba| [rgba[0], rgba[1], rgba[2]])
    ));
}

impl Live<'_> {
    fn snapshot(&self) -> Result<WindowSnapshot, &'static str> {
        self.app
            .controller
            .view
            .with_view(self.app.authentication.view(), |data| {
                require(data.status == LoadState::Ready, "catalog is not ready")?;
                let catalog = data.catalog.ok_or("catalog window is absent")?;
                require(
                    catalog.tail != CatalogTail::Error,
                    "live catalog tail failed",
                )?;
                Ok(WindowSnapshot {
                    first: catalog.first,
                    tail: catalog.tail,
                    total: data.total,
                    targets: data.cards.iter().map(|card| card.key.clone()).collect(),
                    focus: self.app.ui.focus(),
                    scroll: self.app.ui.scroll_y(),
                })
            })
    }

    fn check_frame(&self) -> Result<(), &'static str> {
        require(
            self.start.elapsed() < Duration::from_secs(180),
            "whole live paging journey deadline",
        )?;
        require(
            matches!(self.app.authentication.view(), LoginView::SignedOut),
            "the public journey must remain signed out",
        )?;
        self.app
            .controller
            .view
            .with_view(self.app.authentication.view(), |data| {
                require(
                    !matches!(data.status, LoadState::Error | LoadState::Offline),
                    "live public response failed",
                )?;
                if let Some(catalog) = data.catalog {
                    require(data.cards.len() <= 180, "catalog exceeded its card window")?;
                    require(
                        catalog.tail != CatalogTail::Error,
                        "live catalog tail failed",
                    )?;
                }
                Ok(())
            })
    }

    fn paint(&mut self, capture: Option<&str>) -> Result<(), &'static str> {
        self.check_frame()?;
        let Some(mut frame) = self.app.take_output() else {
            return require(capture.is_none(), "capture frame is absent");
        };
        let drawable = self
            .window
            .surface()
            .map_err(|_| "SDL surface failed")?
            .drawable;
        require(
            drawable.width == 1920 && drawable.height == 1080,
            "live capture requires the original 1920 by 1080 drawable",
        )?;
        if capture.is_some() {
            let rect = self.focused_image_rect()?;
            require(
                frame.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Mesh(mesh)
                        if mesh.texture_id != egui::TextureId::default()
                        && mesh.calc_bounds() == rect)
                }),
                "the settled focused card must contain an artwork texture",
            )?;
        }
        self.painter
            .paint(
                [drawable.width, drawable.height],
                self.app.context(),
                &mut frame,
            )
            .map_err(|_| "GLES painting failed")?;
        if let Some(name) = capture {
            let mut pixels = vec![0u8; 1920 * 1080 * 4];
            // SAFETY: this thread owns the current SDL GL context and the exact
            // drawable RGBA8 allocation. Read before swapping the painted frame.
            unsafe {
                self.gl.read_pixels(
                    0,
                    0,
                    1920,
                    1080,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
            }
            let rect = self.focused_image_rect()?.shrink(4.0);
            let crop = (rect.top() as usize..rect.bottom() as usize).flat_map(|y| {
                let pixels = &pixels;
                (rect.left() as usize..rect.right() as usize).map(move |x| {
                    // GL readback starts at the bottom; the UI starts at the top.
                    let offset = ((1079 - y) * 1920 + x) * 4;
                    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
                })
            });
            require(
                spatial_contrast(crop),
                "the focused artwork interior must contain framebuffer contrast",
            )?;
            let buffer = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(1920, 1080, pixels)
                .ok_or("public capture allocation failed")?;
            let directory =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/e2e");
            std::fs::create_dir_all(&directory).map_err(|_| "public capture directory failed")?;
            image::imageops::flip_vertical(&buffer)
                .save(directory.join(name))
                .map_err(|_| "public capture save failed")?;
        }
        // SAFETY: the same current main-thread context owns all preceding work.
        require(
            unsafe { self.gl.get_error() } == glow::NO_ERROR,
            "GLES frame error",
        )?;
        self.window.present().map_err(|_| "SDL presentation failed")
    }

    fn tick(&mut self) -> Result<(), &'static str> {
        self.app.poll(self.runtime, true);
        self.drain_events()?;
        self.app.consume(self.runtime, self.start.elapsed());
        self.paint(None)
    }

    fn drain_events(&mut self) -> Result<(), &'static str> {
        for _ in 0..128 {
            let Some(event) = self
                .window
                .poll_event()
                .map_err(|_| "SDL event polling failed")?
            else {
                return Ok(());
            };
            let surface = self.window.surface().map_err(|_| "SDL surface failed")?;
            self.app
                .event(event, surface, self.runtime, self.start.elapsed());
            // Key boundaries can render inside event(). Drain and present them
            // immediately rather than retaining shapes from preceding arrows.
            self.paint(None)?;
        }
        Err("SDL event drain exceeded its bound")
    }

    fn key(&mut self, scancode: u32, keycode: i32) -> Result<(), &'static str> {
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
            // SAFETY: initialized stock desktop SDL 56-byte event, eight-byte
            // alignment; SDL copies it synchronously on the window thread.
            require(
                unsafe { SDL_PushEvent(aligned.as_mut_ptr().cast::<c_void>()) } == 1,
                "SDL key injection failed",
            )?;
            self.drain_events()?;
        }
        self.tick()
    }

    fn arrows(&mut self, count: usize, down: bool) -> Result<(), &'static str> {
        for _ in 0..count {
            if down {
                self.key(81, 1_073_741_905)?;
            } else {
                self.key(82, 1_073_741_906)?;
            }
        }
        Ok(())
    }

    fn focused_image_rect(&self) -> Result<egui::Rect, &'static str> {
        let Focus::Card { row, column } = self.app.ui.focus() else {
            return Err("catalog card focus is absent");
        };
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                150.0 + column as f32 * 414.0,
                248.0 + row as f32 * 321.0 - self.app.ui.scroll_y(),
            ),
            egui::vec2(378.0, 213.0),
        );
        require(
            rect.min.x >= 0.0 && rect.min.y >= 0.0 && rect.max.x <= 1920.0 && rect.max.y <= 1080.0,
            "focused artwork is outside the drawable",
        )?;
        Ok(rect)
    }

    fn owning_artwork_ready(&self) -> bool {
        self.app
            .controller
            .view
            .with_view(self.app.authentication.view(), |data| {
                let key = if self.app.ui.page() == Page::Detail {
                    data.detail
                        .as_ref()
                        .and_then(|detail| detail.card.artwork_key)
                } else if let (Some(catalog), Focus::Card { row, column }) =
                    (data.catalog, self.app.ui.focus())
                {
                    (row * 4 + column)
                        .checked_sub(catalog.first)
                        .and_then(|index| data.cards.get(index))
                        .and_then(|card| card.artwork_key)
                } else {
                    None
                };
                data.status == LoadState::Ready && key.is_some_and(|key| self.app.ui.has_image(key))
            })
    }

    fn wait(&mut self, predicate: impl Fn(&Self) -> bool) -> Result<(), &'static str> {
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            self.tick()?;
            require(
                Instant::now() < deadline,
                "live catalog/artwork stage deadline",
            )?;
            if predicate(self) && self.owning_artwork_ready() {
                // Artwork admission follows layout. Render/present a fresh
                // owning frame before declaring the visible result settled.
                self.tick()?;
                require(
                    Instant::now() < deadline,
                    "live catalog/artwork stage deadline",
                )?;
                if predicate(self) && self.owning_artwork_ready() {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn capture(&mut self, name: &str) -> Result<(), &'static str> {
        self.app.consume(self.runtime, self.start.elapsed());
        self.paint(Some(name))
    }

    fn journey(&mut self) -> Result<(), &'static str> {
        // Actual anonymous rail navigation; no linking/account/player action.
        for (scancode, keycode) in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
        ] {
            self.key(scancode, keycode)?;
        }
        require(
            self.app.ui.page() == Page::AllFilms,
            "SDL did not open All Films",
        )?;
        self.wait(|live| {
            live.snapshot().is_ok_and(|view| {
                view.first == 0
                    && view.targets.len() == 60
                    && view.tail == CatalogTail::More
                    && view.focus == Focus::Card { row: 0, column: 0 }
            })
        })?;
        let mut observed = self.snapshot()?.targets;
        let mut second_page = Vec::new();
        for (page, steps, anchor, first, count) in [
            (2, 12, 48, 0, 120),
            (3, 15, 108, 0, 180),
            (4, 15, 168, 60, 180),
            (5, 15, 228, 120, 180),
        ] {
            self.arrows(steps, true)?;
            self.wait(|live| {
                live.snapshot().is_ok_and(|view| {
                    view.first == first
                        && view.targets.len() == count
                        && view.tail == CatalogTail::More
                        && view.focus
                            == Focus::Card {
                                row: anchor / 4,
                                column: 0,
                            }
                })
            })?;
            let view = self.snapshot()?;
            let retained = observed.len() - first;
            require(
                view.targets[..retained] == observed[first..],
                "continuation changed retained target order",
            )?;
            observed.extend_from_slice(&view.targets[retained..]);
            if page == 2 {
                second_page = observed[60..120].to_vec();
            }
            println!("live public paging page {page} admitted within the 180-card window");
        }
        let saved = self.snapshot()?;
        let selected = saved.targets[228 - saved.first].clone();
        require(
            selected.media_id().is_some(),
            "selected catalog target is not a film",
        )?;
        self.capture("native-public-paging.png")?;
        self.key(40, 13)?;
        require(
            self.app.ui.page() == Page::Detail,
            "SDL Select did not open detail",
        )?;
        self.wait(|live| {
            live.app
                .controller
                .view
                .with_view(live.app.authentication.view(), |data| {
                    data.status == LoadState::Ready
                        && data
                            .detail
                            .as_ref()
                            .is_some_and(|detail| detail.card.key == &selected)
                })
        })?;
        self.key(41, 27)?;
        require(
            self.app.ui.page() == Page::AllFilms,
            "SDL Back did not restore All Films",
        )?;
        require(
            self.snapshot()? == saved,
            "Detail/Back changed the exact warm catalog state",
        )?;
        // Five pages evict both page1 and page2. Returning to global116
        // exercises the recorded page2 bookmark, not the initial None cursor.
        self.arrows(28, false)?;
        self.wait(|live| {
            live.snapshot().is_ok_and(|view| {
                view.first == 60
                    && view.targets.len() >= 60
                    && view.targets[..60] == second_page
                    && view.focus == Focus::Card { row: 29, column: 0 }
            })
        })?;
        self.capture("native-public-paging-backward.png")?;
        println!("live public paging observed page2 identity/order restored at global116");
        Ok(())
    }
}

#[test]
#[ignore = "live anonymous provider/artwork and serialized SDL/GLES; root executor only"]
fn native_public_catalog_paging_and_detail_back_end_to_end() {
    super::super::prepare_process();
    let mut window =
        criterion_platform::Window::open("Criterion Unofficial Public Paging E2E").unwrap();
    // SAFETY: the window's current GL context stays on this thread through
    // application disposal and painter destruction below.
    let gl = Arc::new(unsafe {
        glow::Context::from_loader_function(|name| {
            window.gl_proc_address(&CString::new(name).unwrap())
        })
    });
    let mut painter = unsafe { GlowRenderer::new(gl.clone()) }.unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let mut app = Application::new(window.surface().unwrap(), runtime.handle()).unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Live {
            window: &mut window,
            app: &mut app,
            runtime: &runtime,
            painter: &mut painter,
            gl: &gl,
            start: Instant::now(),
        }
        .journey()
    }));
    // Settle owned application lifetimes on success, error and assertion panic.
    app.background();
    let disposed = app.finish(&runtime);
    drop(app);
    runtime.shutdown_timeout(Duration::from_secs(5));
    painter.destroy();
    assert!(disposed, "anonymous application disposal must complete");
    match outcome {
        Ok(result) => result.expect("live public paging journey"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    println!(
        "live anonymous five-page paging + exact Detail/Back + observed page2 restoration + SDL/GLES passed"
    );
}
