// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in synthetic account lifecycle through actual SDL input/GLES frames.
use super::my_list_tests::{Fixture, NEXT, Snapshot, rendered_fixture};
use super::*;
use criterion_account::{WatchListFilter, WatchListRequest};
use criterion_provider::{MediaId, PageCursor};
use criterion_ui::{CatalogTail, Focus, GlowRenderer, MyListGroup, Page, Target};
use glow::HasContext;
use std::{
    ffi::{CString, c_void},
    sync::Arc,
    time::Instant,
};

unsafe extern "C" {
    fn SDL_PushEvent(event: *mut c_void) -> i32;
}

const LABELS: [&str; 6] = [
    "All",
    "Films & Series",
    "Collections",
    "Originals & Franchises",
    "Supplements",
    "Categories",
];
// Current project layout, not a measured official-app geometry contract.
const SLOTS: [(f32, f32); 6] = [
    (150.0, 260.0),
    (278.0, 558.0),
    (576.0, 816.0),
    (834.0, 1224.0),
    (1242.0, 1522.0),
    (1540.0, 1800.0),
];

enum Capture {
    TailError,
    GroupPage,
    SignedOut,
}
impl Capture {
    fn name(&self) -> &'static str {
        match self {
            Self::TailError => "native-my-list-tail-error.png",
            Self::GroupPage => "native-my-list-group-paging.png",
            Self::SignedOut => "native-my-list-logout.png",
        }
    }
}

fn require(condition: bool, message: &'static str) -> Result<(), &'static str> {
    condition.then_some(()).ok_or(message)
}

struct Rendered<'a> {
    fixture: &'a mut Fixture,
    window: &'a mut criterion_platform::Window,
    painter: &'a mut GlowRenderer,
    gl: &'a glow::Context,
    start: Instant,
}

impl Rendered<'_> {
    fn deadline(&self) -> Result<(), &'static str> {
        require(
            self.start.elapsed() < Duration::from_secs(45),
            "synthetic rendered journey exceeded its bound",
        )
    }

    fn paint(&mut self, capture: Option<Capture>) -> Result<(), &'static str> {
        self.deadline()?;
        let Some(mut frame) = self.fixture.render_output() else {
            return require(capture.is_none(), "synthetic capture frame is absent");
        };
        let drawable = self.window.surface().map_err(|_| "SDL surface")?.drawable;
        require(
            drawable.width == 1920 && drawable.height == 1080,
            "synthetic capture requires original 1920 by 1080 drawable",
        )?;
        if let Some(capture) = &capture {
            let text = frame
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>();
            match capture {
                Capture::TailError | Capture::GroupPage => {
                    for (label, (left, right)) in LABELS.into_iter().zip(SLOTS) {
                        require(
                            text.iter().any(|text| {
                                text.galley.job.text.starts_with(label)
                                    && (text.pos.x - (left + 12.0)).abs() < 0.5
                                    && text.pos.x + text.galley.size().x <= right - 12.0
                                    && (text.pos.y - 156.0).abs() < 0.5
                            }),
                            "six group labels must fit their distinct visible slots",
                        )?;
                    }
                    require(
                        text.iter()
                            .any(|text| text.galley.job.text.starts_with("Synthetic grouped ")),
                        "the settled frame must paint synthetic group rows",
                    )?;
                    if matches!(capture, Capture::TailError) {
                        require(
                            text.iter()
                                .any(|text| text.galley.job.text == "Unable to load — retry"),
                            "tail failure must paint its explicit retry control",
                        )?;
                    }
                }
                Capture::SignedOut => {
                    require(
                        text.iter().any(|text| text.galley.job.text == "LOG IN"),
                        "signed-out frame must paint login control",
                    )?;
                    require(
                        !text.iter().any(|text| {
                            text.galley.job.text.starts_with("Synthetic grouped ")
                                || LABELS.iter().any(|label| {
                                    text.galley.job.text.starts_with(label)
                                        && (134.0..206.0).contains(&text.pos.y)
                                })
                        }),
                        "signed-out frame must omit synthetic private rows and group header",
                    )?;
                }
            }
        }
        self.painter
            .paint(
                [drawable.width, drawable.height],
                self.fixture.render_context(),
                &mut frame,
            )
            .map_err(|_| "synthetic GLES paint")?;
        if let Some(capture) = capture {
            let mut pixels = vec![0u8; 1920 * 1080 * 4];
            // SAFETY: current main-thread SDL context and exact drawable RGBA8
            // allocation; read the freshly painted back buffer before swapping.
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
            if !matches!(capture, Capture::SignedOut) {
                require(
                    gold_pixels(&pixels, 290..546, 199..206) > 100,
                    "selected Films & Series underline must appear in its framebuffer slot",
                )?;
                require(
                    gold_pixels(&pixels, 162..248, 199..206) < 10,
                    "unselected All slot must not paint a selected underline",
                )?;
                if matches!(capture, Capture::GroupPage) {
                    let Snapshot { focus, scroll, .. } = self.fixture.snapshot();
                    let Focus::Card { row, column } = focus else {
                        return Err("settled group card focus is absent");
                    };
                    let left = 150 + column * 414;
                    let top = 248.0 + row as f32 * 321.0 - scroll;
                    require(
                        (8.0..859.0).contains(&top) && left >= 8 && left + 386 <= 1920,
                        "focused group card must fit the framebuffer",
                    )?;
                    require(
                        gold_pixels(
                            &pixels,
                            left - 8..left + 386,
                            top as usize - 8..top as usize + 221,
                        ) > 500,
                        "settled card focus must appear in framebuffer",
                    )?;
                }
            }
            let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(1920, 1080, pixels)
                .ok_or("synthetic capture allocation")?;
            let directory =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/e2e");
            std::fs::create_dir_all(&directory).map_err(|_| "synthetic capture directory")?;
            image::imageops::flip_vertical(&image)
                .save(directory.join(capture.name()))
                .map_err(|_| "synthetic capture save")?;
        }
        // SAFETY: same current main-thread context owns all preceding calls.
        require(
            unsafe { self.gl.get_error() } == glow::NO_ERROR,
            "GLES error",
        )?;
        self.window.present().map_err(|_| "SDL presentation")?;
        self.deadline()
    }

    fn drain(&mut self) -> Result<(), &'static str> {
        for _ in 0..128 {
            let Some(event) = self.window.poll_event().map_err(|_| "SDL polling")? else {
                return Ok(());
            };
            let surface = self.window.surface().map_err(|_| "SDL surface")?;
            self.fixture
                .render_event(event, surface, self.start.elapsed());
            self.paint(None)?;
        }
        Err("SDL event drain exceeded its bound")
    }

    fn tick(&mut self) -> Result<(), &'static str> {
        self.fixture.render_poll();
        self.drain()?;
        self.fixture.render_consume(self.start.elapsed());
        self.paint(None)
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
            // SAFETY: stock desktop SDL's initialized 56-byte event is eight
            // byte aligned; SDL copies it on this window's owning thread.
            require(
                unsafe { SDL_PushEvent(aligned.as_mut_ptr().cast::<c_void>()) } == 1,
                "synthetic SDL key injection",
            )?;
            self.drain()?;
        }
        self.tick()
    }

    fn wait(&mut self, predicate: impl Fn(&Fixture) -> bool) -> Result<(), &'static str> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.tick()?;
            require(
                Instant::now() < deadline,
                "synthetic rendered stage deadline",
            )?;
            if predicate(self.fixture) {
                self.tick()?;
                require(
                    Instant::now() < deadline,
                    "synthetic rendered stage deadline",
                )?;
                if predicate(self.fixture) {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn capture(&mut self, capture: Capture) -> Result<(), &'static str> {
        self.fixture.render_consume(self.start.elapsed());
        self.paint(Some(capture))
    }

    fn journey(&mut self) -> Result<(), &'static str> {
        self.wait(|fixture| fixture.render_status() == LoadState::Offline)?;
        require(self.fixture.render_signed_in(), "synthetic initial session")?;
        for (scan, key) in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
        ] {
            self.key(scan, key)?;
        }
        self.wait(|fixture| fixture.render_status() == LoadState::Ready)?;
        let all = self.fixture.snapshot();
        require(
            all.selected == MyListGroup::All
                && all.cards.len() == 50
                && all.tail == CatalogTail::End,
            "initial native All fifty rows",
        )?;
        if matches!(self.fixture.render_focus(), Focus::Card { .. }) {
            self.key(82, 1_073_741_906)?;
        }
        require(
            self.fixture.render_focus() == Focus::MyListGroup(MyListGroup::All),
            "All header focus",
        )?;
        self.key(79, 1_073_741_903)?;
        self.key(40, 13)?;
        self.wait(|fixture| {
            fixture.render_status() == LoadState::Ready
                && fixture.snapshot().selected == MyListGroup::FilmsAndSeries
        })?;
        let group_initial = self.fixture.snapshot();
        require(
            group_initial.cards.len() == 50,
            "group initial native fifty rows",
        )?;
        self.key(81, 1_073_741_905)?;
        for _ in 0..12 {
            self.key(81, 1_073_741_905)?;
        }
        self.wait(|fixture| fixture.snapshot().tail == CatalogTail::Error)?;
        let failed = self.fixture.snapshot();
        require(
            failed.cards == group_initial.cards
                && failed.status == LoadState::Ready
                && failed.focus == Focus::Card { row: 12, column: 0 },
            "tail failure preserves committed fifty rows and focus",
        )?;
        self.capture(Capture::TailError)?;
        self.key(81, 1_073_741_905)?;
        require(
            self.fixture.render_focus() == Focus::CatalogRetry,
            "native explicit tail retry focus",
        )?;
        self.key(40, 13)?;
        self.wait(|fixture| {
            fixture.snapshot().tail == CatalogTail::End && fixture.snapshot().cards.len() == 60
        })?;
        let saved = self.fixture.snapshot();
        require(
            saved.selected == MyListGroup::FilmsAndSeries
                && saved.first == 0
                && saved.focus == Focus::Card { row: 13, column: 0 }
                && saved.cards[..50] == failed.cards,
            "retry preserves prefix and native grouped window",
        )?;
        let requests = self.fixture.render_calls();
        require(
            requests.len() == 4
                && requests[0]
                    == WatchListRequest {
                        filter: WatchListFilter::All,
                        cursor: None,
                    }
                && requests[1]
                    == WatchListRequest {
                        filter: WatchListFilter::FilmSeries,
                        cursor: None,
                    }
                && requests[2]
                    == WatchListRequest {
                        filter: WatchListFilter::FilmSeries,
                        cursor: Some(PageCursor::new(NEXT).unwrap()),
                    }
                && requests[3] == requests[2],
            "native grouping and exact observed-cursor retry requests",
        )?;
        self.capture(Capture::GroupPage)?;
        let target = saved.cards[52].0.clone();
        require(
            target == Target::Media(MediaId::new("M0000152").unwrap()),
            "synthetic exact native selected target",
        )?;
        self.key(40, 13)?;
        self.wait(|fixture| {
            fixture.render_page() == Page::Detail
                && fixture.render_status() == LoadState::Ready
                && fixture.render_detail() == Some(target.clone())
        })?;
        self.key(41, 27)?;
        require(
            self.fixture.render_page() == Page::MyList
                && self.fixture.snapshot() == saved
                && self.fixture.render_calls() == requests,
            "warm Detail/Back must preserve exact grouped window without native read",
        )?;
        for (scan, key) in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
            (40, 13),
        ] {
            self.key(scan, key)?;
        }
        self.wait(|fixture| {
            fixture.render_signed_out()
                && !fixture.render_has_private_projection()
                && fixture.render_revokes() == 1
        })?;
        require(
            self.fixture.render_page() == Page::Login && self.fixture.render_calls() == requests,
            "logout retains no synthetic shelf projection or extra read",
        )?;
        self.capture(Capture::SignedOut)
    }
}

fn gold_pixels(pixels: &[u8], x: std::ops::Range<usize>, y: std::ops::Range<usize>) -> usize {
    y.flat_map(|y| x.clone().map(move |x| ((1079 - y) * 1920 + x) * 4))
        .filter(|&offset| {
            pixels[offset] > 120 && pixels[offset + 1] > 80 && pixels[offset + 2] < 70
        })
        .count()
}

#[test]
#[ignore = "synthetic Account/Session only; serialized actual SDL/GLES root executor"]
fn native_synthetic_my_list_groups_paging_retry_detail_and_logout_end_to_end() {
    super::super::prepare_process();
    let mut window =
        criterion_platform::Window::open("Criterion Unofficial Synthetic My List E2E").unwrap();
    // SAFETY: this thread keeps the window/current context alive until after
    // fixture lifetimes settle and the painter is explicitly destroyed.
    let gl = Arc::new(unsafe {
        glow::Context::from_loader_function(|name| {
            window.gl_proc_address(&CString::new(name).unwrap())
        })
    });
    let mut painter = unsafe { GlowRenderer::new(gl.clone()) }.unwrap();
    let mut fixture = match std::panic::catch_unwind(rendered_fixture) {
        Ok(fixture) => fixture,
        Err(panic) => {
            painter.destroy();
            std::panic::resume_unwind(panic);
        }
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Rendered {
            fixture: &mut fixture,
            window: &mut window,
            painter: &mut painter,
            gl: &gl,
            start: Instant::now(),
        }
        .journey()
    }));
    // Preserve disposal verification without allowing a cleanup assertion to
    // skip GL destruction. Both panic paths retain the current SDL context.
    let disposed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(fixture)));
    painter.destroy();
    match outcome {
        Ok(Ok(())) => {
            if let Err(panic) = disposed {
                std::panic::resume_unwind(panic);
            }
        }
        Ok(Err(error)) => panic!("synthetic rendered My List journey: {error}"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    println!(
        "synthetic native My List groups + fixed fifty/cursor retry + exact Detail/Back + logout + SDL/GLES passed"
    );
}
