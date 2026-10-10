// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in actual SDL/GLES journey. All HTTP, credentials and artwork are offline fixtures.
use super::*;

#[path = "native_featured_render_tests.rs"]
mod native_featured_render_tests;
use criterion_ui::{DetailKind, GlowRenderer, Target};
use glow::HasContext;
use std::{
    ffi::{CString, c_void},
    ops::Range,
    path::PathBuf,
    time::Instant,
};

unsafe extern "C" {
    fn SDL_PushEvent(event: *mut c_void) -> i32;
}

const RESUME: &str = "RESUME SEASON -2147483648, EPISODE 510";
const FIRST: &str = "WATCH FIRST EPISODE";
const DOWN: (u32, i32) = (81, 1_073_741_905);
const UP: (u32, i32) = (82, 1_073_741_906);
const RIGHT: (u32, i32) = (79, 1_073_741_903);
const SELECT: (u32, i32) = (40, 13);
const BACK: (u32, i32) = (41, 27);

fn series_fixture() -> Fixture {
    let fixture = boundary_series_fixture();
    fixture.script.steps.lock().unwrap().push_back(Step {
        kind: Kind::MyListIds,
        gate: None,
    });
    fixture
}

fn require(condition: bool, message: &'static str) -> Result<(), &'static str> {
    condition.then_some(()).ok_or(message)
}

#[derive(Clone, Copy)]
enum Journey {
    Series,
    Featured,
}

#[derive(Clone, Copy)]
enum Capture {
    Resume,
    Information,
    Episode,
    RetiredEpisode,
    RetiredPrimary,
    Featured(native_featured_render_tests::Capture),
}
const CAPTURES: [Capture; 5] = [
    Capture::Resume,
    Capture::Information,
    Capture::Episode,
    Capture::RetiredEpisode,
    Capture::RetiredPrimary,
];
impl Capture {
    fn name(self) -> &'static str {
        match self {
            Self::Resume => "synthetic-series-resume.png",
            Self::Information => "synthetic-series-information.png",
            Self::Episode => "synthetic-series-episode-511.png",
            Self::RetiredEpisode => "synthetic-series-retired-episode.png",
            Self::RetiredPrimary => "synthetic-series-retired-primary.png",
            Self::Featured(stage) => stage.name(),
        }
    }
    fn retired(self) -> bool {
        matches!(self, Self::RetiredEpisode | Self::RetiredPrimary)
    }
    fn focus(self) -> Focus {
        match self {
            Self::Resume | Self::RetiredPrimary => Focus::DetailAction(0),
            Self::Information => Focus::InformationPrimary,
            Self::Episode => Focus::Card {
                row: 0,
                column: 509,
            },
            Self::RetiredEpisode => Focus::Card { row: 0, column: 0 },
            Self::Featured(_) => Focus::FeaturedCard(0),
        }
    }
    fn focus_region(self) -> (Range<usize>, Range<usize>) {
        match self {
            Self::Resume | Self::RetiredPrimary => (150..610, 620..700),
            Self::Information => (348..1572, 903..983),
            Self::Episode => (1384..1778, 527..756),
            Self::RetiredEpisode => (142..536, 527..756),
            Self::Featured(_) => (142..536, 407..636),
        }
    }
}

// Admission requires a fresh directory; failure cleanup cannot erase prior evidence.
struct Captures {
    journey: Journey,
    directory: PathBuf,
    keep: bool,
}
impl Captures {
    fn new() -> Result<Self, &'static str> {
        Self::for_journey(Journey::Series)
    }
    fn for_journey(journey: Journey) -> Result<Self, &'static str> {
        let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/e2e");
        std::fs::create_dir_all(&parent).map_err(|_| "synthetic capture parent")?;
        let captured_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "synthetic capture clock")?
            .as_nanos();
        let prefix = match journey {
            Journey::Series => "native-synthetic-series",
            Journey::Featured => "native-synthetic-featured",
        };
        let directory = parent.join(format!("{prefix}-{}-{captured_at}", std::process::id()));
        std::fs::create_dir(&directory).map_err(|_| "fresh synthetic capture directory")?;
        Ok(Self {
            journey,
            directory,
            keep: false,
        })
    }
    fn remove(&self) -> Result<(), &'static str> {
        let names = match self.journey {
            Journey::Series => CAPTURES.map(Capture::name).to_vec(),
            Journey::Featured => native_featured_render_tests::CAPTURES
                .map(native_featured_render_tests::Capture::name)
                .to_vec(),
        };
        for name in names {
            let path = self.directory.join(name);
            if path.exists() {
                std::fs::remove_file(path).map_err(|_| "owned synthetic capture removal")?;
            }
        }
        std::fs::remove_dir(&self.directory).map_err(|_| "owned synthetic directory removal")
    }
}
impl Drop for Captures {
    fn drop(&mut self) {
        if !self.keep && self.directory.exists() {
            let _ = self.remove();
        }
    }
}

fn exact_series(fixture: &Fixture, retired: bool) -> bool {
    fixture.app.controller.view.with_view(fixture.app.authentication.view(), |view| {
        let Some(detail) = &view.detail else { return false };
        let Some(seasons) = &detail.seasons else { return false };
        let Some(rail) = view.rails.first() else { return false };
        let selected = if retired { 0 } else { 509 };
        let Some(card) = rail.cards.get(selected) else { return false };
        view.status == LoadState::Ready
            && detail.kind == DetailKind::Series
            && matches!(detail.card.key, Target::Native(id) if id.as_str() == "Listed01")
            && detail.card.title == "Synthetic boundary Series"
            && detail.primary_playback_target.is_some_and(|id| id.as_str() == if retired { "Ep000001" } else { "Ep000511" })
            && detail.primary_action == if retired { FIRST } else { RESUME }
            && seasons.selected == if retired { 0 } else { 1 }
            && seasons.choices.len() == 2
            && seasons.choices[1].number == i32::MIN
            && rail.cards.len() == if retired { 1 } else { 510 }
            && matches!(card.key, Target::Native(id) if id.as_str() == if retired { "Ep000001" } else { "Ep000511" })
            && card.title == if retired { "First Episode" } else { "Synthetic final Episode 511" }
            && card.saved_fraction == if retired { None } else { Some(0.2) }
            && (!retired || view.rails.iter().flat_map(|rail| rail.cards.iter()).all(|card| card.saved_fraction.is_none()))
    })
}

fn text_inside(output: &egui::FullOutput, value: &str, area: egui::Rect) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text == value && !text.galley.elided
                && area.contains_rect(shape.shape.visual_bounding_rect())
                && shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect()))
    })
}

fn frame_oracle(output: &egui::FullOutput, stage: Capture) -> Result<(), &'static str> {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let area = match stage {
        Capture::Resume | Capture::RetiredPrimary => {
            egui::Rect::from_min_size(egui::pos2(150.0, 620.0), egui::vec2(460.0, 80.0))
        }
        Capture::Information => {
            egui::Rect::from_min_size(egui::pos2(348.0, 903.0), egui::vec2(1224.0, 80.0))
        }
        Capture::Episode => {
            egui::Rect::from_min_size(egui::pos2(1392.0, 756.0), egui::vec2(346.0, 44.0))
        }
        Capture::RetiredEpisode => {
            egui::Rect::from_min_size(egui::pos2(150.0, 756.0), egui::vec2(346.0, 44.0))
        }
        Capture::Featured(_) => return Err("Feature stage cannot use Series shape oracle"),
    };
    let text = match stage {
        Capture::Resume | Capture::Information => RESUME,
        Capture::RetiredPrimary => FIRST,
        Capture::Episode => "Synthetic final Episode 511",
        Capture::RetiredEpisode => "First Episode",
        Capture::Featured(_) => return Err("Feature stage cannot use Series text oracle"),
    };
    require(
        text_inside(output, text, area),
        "exact complete current text must be unelided and visually inside its control and clip",
    )?;
    if matches!(stage, Capture::Episode | Capture::RetiredEpisode) {
        let left = if stage.retired() { 142.0 } else { 1384.0 };
        let focus = egui::Rect::from_min_size(egui::pos2(left, 527.0), egui::vec2(394.0, 229.0));
        require(
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect == focus && rect.stroke.width == 8.0
                && rect.stroke.color == egui::Color32::from_rgb(181, 138, 22)
                && shape.clip_rect.contains_rect(focus))
            }),
            "current selected child must paint its complete visible eight-pixel focus",
        )?;
        let image_left = left + 8.0;
        let progress =
            egui::Rect::from_min_size(egui::pos2(image_left, 745.0), egui::vec2(378.0, 3.0));
        if stage.retired() {
            require(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.rect.height() == 3.0 && progress.intersects(rect.rect))),
                "retired current child must omit every progress shape")?;
        } else {
            require(
                output.shapes.iter().any(|shape| {
                    matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.rect.min == progress.min
                    && (rect.rect.width() - 75.6).abs() < 0.001 && rect.rect.height() == 3.0
                    && rect.fill == egui::Color32::from_rgb(239, 239, 239))
                }),
                "current child must paint the literal twenty-percent fill geometry",
            )?;
        }
    } else {
        require(
            text_inside(output, "Synthetic boundary Series", screen),
            "exact current Series title absent from visible frame",
        )?;
    }
    if stage.retired() {
        require(!output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("RESUME SEASON") || text.galley.job.text == "Synthetic clicked Episode" || text.galley.job.text == "Synthetic final Episode 511")
        }), "retired current frame retained private resume or prior selected child text")?;
    }
    Ok(())
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    let offset = ((1079 - y) * 1920 + x) * 4;
    &pixels[offset..offset + 3]
}
fn gold_pixels(pixels: &[u8], x: Range<usize>, y: Range<usize>) -> usize {
    y.flat_map(|y| x.clone().map(move |x| pixel(pixels, x, y)))
        .filter(|rgb| rgb[0] > 120 && rgb[1] > 80 && rgb[2] < 70)
        .count()
}
fn progress_pixels(pixels: &[u8], retired: bool) -> bool {
    // Interior pixels avoid the separately asserted 8px gold focus stroke.
    let left = if retired { 150 } else { 1392 };
    (left + 12..left + 365).all(|x| {
        let rgb = pixel(pixels, x, 746);
        if retired {
            rgb.iter().all(|channel| (34..=44).contains(channel))
        } else if x < left + 72 {
            rgb.iter().all(|channel| *channel > 220)
        } else if x > left + 80 {
            rgb.iter().all(|channel| (50..180).contains(channel))
        } else {
            true
        }
    })
}

struct Rendered<'a> {
    fixture: &'a mut Fixture,
    window: &'a mut criterion_platform::Window,
    painter: &'a mut GlowRenderer,
    gl: &'a glow::Context,
    captures: &'a Captures,
    start: Instant,
}
impl Rendered<'_> {
    fn bounded(&self) -> Result<(), &'static str> {
        require(
            self.start.elapsed() < Duration::from_secs(90),
            "whole synthetic rendered journey deadline",
        )?;
        require(
            self.fixture.script.violation.lock().unwrap().is_none()
                && match self.captures.journey {
                    Journey::Series => self.fixture.script.calls.lock().unwrap().len() <= 3,
                    Journey::Featured => native_featured_render_tests::reads_bounded(self.fixture),
                }
                && self.fixture.script.bootstrap.load(Ordering::SeqCst) <= 1
                && self.fixture.script.maximum.load(Ordering::SeqCst) <= 1
                && self.fixture.issuer.tokens.load(Ordering::SeqCst) == 1
                && self.fixture.issuer.revokes.load(Ordering::SeqCst) <= 1
                && self
                    .fixture
                    .public_requests
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|path| path == "/"),
            "synthetic request census or sole-worker safety violated",
        )
    }
    fn paint(&mut self, capture: Option<Capture>) -> Result<(), &'static str> {
        self.bounded()?;
        let size = self.window.surface().map_err(|_| "SDL surface")?.drawable;
        require(
            size.width == 1920 && size.height == 1080,
            "original 1920 by 1080 drawable required",
        )?;
        let Some(mut output) = self.fixture.app.take_output() else {
            return require(capture.is_none(), "fresh capture output absent");
        };
        if let Some(stage) = capture {
            let admitted = match (self.captures.journey, stage) {
                (Journey::Featured, Capture::Featured(stage)) => stage.admit(self.fixture, &output),
                (Journey::Series, Capture::Featured(_)) | (Journey::Featured, _) => {
                    Err("capture journey mismatch")
                }
                (Journey::Series, _) => require(
                    self.fixture.app.ui.page() == Page::Detail
                        && exact_series(self.fixture, stage.retired()),
                    "exact current Series projection mismatch",
                )
                .and_then(|()| {
                    require(
                        self.fixture.app.ui.focus() == stage.focus(),
                        "canonical current focus mismatch",
                    )
                })
                .and_then(|()| frame_oracle(&output, stage)),
            };
            if let Err(error) = admitted {
                // Failed admission disposes this journey; retire its unpainted
                // texture delta before dropping the output and GL owner.
                output.textures_delta.clear();
                return Err(error);
            }
        }
        if self
            .painter
            .paint(
                [size.width, size.height],
                self.fixture.app.context(),
                &mut output,
            )
            .is_err()
        {
            output.textures_delta.clear();
            return Err("synthetic GLES paint");
        }
        if let Some(stage) = capture {
            let mut pixels = vec![0_u8; 1920 * 1080 * 4];
            // SAFETY: this thread owns the current SDL context and the exact
            // RGBA8 allocation. Read its freshly painted buffer before swapping.
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
            let (x, y) = stage.focus_region();
            require(
                gold_pixels(&pixels, x, y) > 500,
                "canonical focus absent from current framebuffer",
            )?;
            if matches!(stage, Capture::Episode | Capture::RetiredEpisode) {
                require(
                    progress_pixels(&pixels, stage.retired()),
                    "current Episode progress framebuffer mismatch",
                )?;
            }
            if let Capture::Featured(stage) = stage {
                require(
                    stage.pixels(&pixels),
                    "current Feature artwork/progress framebuffer mismatch",
                )?;
            }
            let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(1920, 1080, pixels)
                .ok_or("synthetic capture allocation")?;
            image::imageops::flip_vertical(&image)
                .save(self.captures.directory.join(stage.name()))
                .map_err(|_| "synthetic full frame save")?;
        }
        // SAFETY: the same current context owns every preceding graphics call.
        require(
            unsafe { self.gl.get_error() } == glow::NO_ERROR,
            "GLES error",
        )?;
        self.window.present().map_err(|_| "SDL present")?;
        self.bounded()
    }
    fn tick(&mut self) -> Result<(), &'static str> {
        self.bounded()?;
        self.fixture.app.poll(&self.fixture.runtime, true);
        self.fixture.runtime.block_on(tokio::task::yield_now());
        for _ in 0..128 {
            let Some(event) = self.window.poll_event().map_err(|_| "SDL poll")? else {
                self.fixture
                    .app
                    .consume(&self.fixture.runtime, self.fixture.clock.now());
                return self.paint(None);
            };
            self.fixture.app.event(
                event,
                self.window.surface().map_err(|_| "SDL surface")?,
                &self.fixture.runtime,
                self.fixture.clock.now(),
            );
            // Drain each key boundary, preserving texture deltas and bounding
            // accumulated output even through all 509 actual Right events.
            self.paint(None)?;
        }
        Err("SDL event drain exceeded bound")
    }
    fn inject(&mut self, raw: [u8; 56]) -> Result<(), &'static str> {
        let mut aligned = [0_u64; 7];
        for (word, bytes) in aligned.iter_mut().zip(raw.as_chunks::<8>().0) {
            *word = u64::from_le_bytes(*bytes);
        }
        // SAFETY: stock desktop SDL_Event is initialized, 56 bytes and aligned8;
        // SDL synchronously copies it on this window's owning thread.
        require(
            unsafe { SDL_PushEvent(aligned.as_mut_ptr().cast::<c_void>()) } == 1,
            "SDL input injection",
        )?;
        self.tick()
    }
    fn key(&mut self, (scan, key): (u32, i32)) -> Result<(), &'static str> {
        for pressed in [true, false] {
            let mut raw = [0_u8; 56];
            raw[..4].copy_from_slice(&(if pressed { 0x300_u32 } else { 0x301 }).to_le_bytes());
            raw[12] = u8::from(pressed);
            raw[16..20].copy_from_slice(&scan.to_le_bytes());
            raw[20..24].copy_from_slice(&key.to_le_bytes());
            self.inject(raw)?;
        }
        Ok(())
    }
    fn account(&mut self) -> Result<(), &'static str> {
        // Use the actual visible Account rail pointer target while retaining
        // the selected card history snapshot for logout/Back.
        for pressed in [true, false] {
            let mut raw = [0_u8; 56];
            raw[..4].copy_from_slice(&(if pressed { 0x401_u32 } else { 0x402 }).to_le_bytes());
            raw[16] = 1;
            raw[17] = u8::from(pressed);
            raw[20..24].copy_from_slice(&75_i32.to_le_bytes());
            raw[24..28].copy_from_slice(&726_i32.to_le_bytes());
            self.inject(raw)?;
        }
        require(
            self.fixture.app.ui.page() == Page::Login
                && self.fixture.app.ui.focus() == Focus::LoginPrimary,
            "actual Account pointer activation",
        )
    }
    fn wait(&mut self, ready: impl Fn(&Fixture) -> bool) -> Result<(), &'static str> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.tick()?;
            require(
                Instant::now() < deadline,
                "synthetic rendered stage deadline",
            )?;
            if ready(self.fixture) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn capture(&mut self, stage: Capture) -> Result<(), &'static str> {
        // The event frame precedes its command. Consume a new current frame
        // after every input settles, rather than capturing a previous focus.
        self.fixture
            .app
            .consume(&self.fixture.runtime, self.fixture.clock.now());
        self.paint(Some(stage))
    }
    fn journey(&mut self) -> Result<(), &'static str> {
        self.wait(|fixture| fixture.saved() == [("Synthetic clicked Episode".to_owned(), None)])?;
        require(
            self.fixture.app.ui.page() == Page::Home && self.fixture.app.authentication.signed_in(),
            "synthetic signed-in CW origin",
        )?;
        self.key(DOWN)?;
        self.key(DOWN)?;
        require(
            self.fixture.app.ui.focus() == Focus::Card { row: 1, column: 0 },
            "actual CW origin card focus",
        )?;
        self.key(SELECT)?;
        self.wait(|fixture| {
            fixture.native_ready("Listed01")
                && fixture.app.membership_view()
                    == criterion_ui::ListMembership::Known { present: true }
        })?;
        self.capture(Capture::Resume)?;
        self.key(RIGHT)?;
        self.key(SELECT)?;
        self.capture(Capture::Information)?;
        self.key(BACK)?;
        require(
            self.fixture.app.ui.focus() == Focus::DetailAction(1)
                && exact_series(self.fixture, false),
            "Information Back must retain exact Series resume",
        )?;
        for _ in 0..4 {
            self.key(DOWN)?;
        }
        require(
            self.fixture.app.ui.focus() == Focus::Card { row: 0, column: 0 },
            "actual first displayed Episode focus",
        )?;
        for column in 1..=509 {
            self.key(RIGHT)?;
            require(
                self.fixture.app.ui.focus() == Focus::Card { row: 0, column },
                "bounded real Right loop skipped or repeated a child",
            )?;
        }
        self.capture(Capture::Episode)?;
        self.account()?;
        self.key(SELECT)?;
        self.wait(|fixture| {
            matches!(fixture.app.authentication.view(), LoginView::SignedOut)
                && fixture.issuer.revokes.load(Ordering::SeqCst) == 1
        })?;
        require(
            self.fixture.app.positions.is_none(),
            "logout must retire private positions",
        )?;
        self.key(BACK)?;
        self.capture(Capture::RetiredEpisode)?;
        for _ in 0..4 {
            self.key(UP)?;
        }
        self.capture(Capture::RetiredPrimary)?;
        require(
            *self.fixture.script.calls.lock().unwrap()
                == [
                    Kind::ContinueWatching,
                    Kind::NativeDetail("Listed01"),
                    Kind::MyListIds,
                ]
                && self.fixture.script.bootstrap.load(Ordering::SeqCst) == 1
                && self.fixture.script.maximum.load(Ordering::SeqCst) == 1
                && self.fixture.script.active.load(Ordering::SeqCst) == 0
                && self.fixture.script.steps.lock().unwrap().is_empty()
                && *self.fixture.public_requests.lock().unwrap() == ["/"],
            "exact anonymous Detail, synthetic CW and membership census, signed-out Back without extra reads",
        )
    }
}

#[test]
fn series_capture_geometry_matches_current_actual_application_shapes() {
    let mut fixture = series_fixture();
    fixture.wait(|fixture| fixture.saved().len() == 1);
    fixture.key(DOWN.0, DOWN.1);
    fixture.key(DOWN.0, DOWN.1);
    fixture.key(SELECT.0, SELECT.1);
    fixture.wait(|fixture| {
        fixture.native_ready("Listed01")
            && fixture.app.membership_view()
                == criterion_ui::ListMembership::Known { present: true }
    });
    let retire = |fixture: &mut Fixture| {
        if let Some(mut output) = fixture.app.take_output() {
            output.textures_delta.clear();
        }
    };
    let fresh = |fixture: &mut Fixture| {
        // This CPU-only shape witness has no renderer. Its unused texture
        // deltas retire with the output; the SDL runner always paints them.
        retire(fixture);
        fixture.pump();
        let mut output = fixture.app.take_output().unwrap();
        output.textures_delta.clear();
        output
    };
    assert!(exact_series(&fixture, false));
    frame_oracle(&fresh(&mut fixture), Capture::Resume).unwrap();
    fixture.key(RIGHT.0, RIGHT.1);
    fixture.key(SELECT.0, SELECT.1);
    frame_oracle(&fresh(&mut fixture), Capture::Information).unwrap();
    fixture.key(BACK.0, BACK.1);
    for _ in 0..4 {
        fixture.key(DOWN.0, DOWN.1);
    }
    for _ in 0..509 {
        fixture.key(RIGHT.0, RIGHT.1);
        retire(&mut fixture);
    }
    assert_eq!(fixture.app.ui.focus(), Capture::Episode.focus());
    frame_oracle(&fresh(&mut fixture), Capture::Episode).unwrap();
}

#[test]
fn series_progress_pixel_oracle_refuses_absent_fraction_and_retained_fraction() {
    let mut pixels = vec![39_u8; 1920 * 1080 * 4];
    assert!(!progress_pixels(&pixels, false));
    assert!(progress_pixels(&pixels, true));
    for x in 1392..1770 {
        let offset = ((1079 - 746) * 1920 + x) * 4;
        pixels[offset..offset + 3].fill(if x < 1392 + 76 { 239 } else { 100 });
    }
    assert!(progress_pixels(&pixels, false));
    let offset = ((1079 - 746) * 1920 + 162) * 4;
    pixels[offset..offset + 3].fill(239);
    assert!(!progress_pixels(&pixels, true));
}

#[test]
#[ignore = "synthetic offline Series/CW/Session/artwork; serialized Root actual SDL/GLES executor"]
fn native_synthetic_series_resume_information_and_logout_back_end_to_end() {
    run(Journey::Series);
}

fn run(journey: Journey) {
    super::super::super::super::prepare_process();
    let mut captures = match journey {
        Journey::Series => Captures::new(),
        Journey::Featured => Captures::for_journey(journey),
    }
    .unwrap();
    let title = match journey {
        Journey::Series => "Criterion Unofficial Synthetic Series E2E",
        Journey::Featured => "Criterion Unofficial Synthetic Featured E2E",
    };
    let mut window = criterion_platform::Window::open(title).unwrap();
    // SAFETY: the current main-thread context outlives fixture disposal and painter destruction.
    let gl = Arc::new(unsafe {
        glow::Context::from_loader_function(|name| {
            window.gl_proc_address(&CString::new(name).unwrap())
        })
    });
    let mut painter = unsafe { GlowRenderer::new(gl.clone()) }.unwrap();
    let mut fixture = match std::panic::catch_unwind(|| match journey {
        Journey::Series => series_fixture(),
        Journey::Featured => native_featured_render_tests::fixture(),
    }) {
        Ok(fixture) => fixture,
        Err(panic) => {
            painter.destroy();
            std::panic::resume_unwind(panic);
        }
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut rendered = Rendered {
            fixture: &mut fixture,
            window: &mut window,
            painter: &mut painter,
            gl: &gl,
            captures: &captures,
            start: Instant::now(),
        };
        match journey {
            Journey::Series => rendered.journey(),
            Journey::Featured => native_featured_render_tests::journey(&mut rendered),
        }
    }));
    // Fixture disposal joins the real worker and issuer on every outcome. A
    // cleanup assertion cannot skip GL destruction or owned-file retirement.
    let failed = !matches!(outcome, Ok(Ok(())));
    let disposed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if matches!(journey, Journey::Featured) && failed {
            // Retire unissued synthetic steps on failure inside the protected
            // disposal path, so even a fixture assertion cannot skip GL cleanup.
            fixture.script.steps.lock().unwrap().clear();
        }
        drop(fixture);
    }));
    painter.destroy();
    match (outcome, disposed) {
        (Ok(Ok(())), Ok(())) => {
            captures.keep = true;
            println!(
                "{title}; actual SDL/GLES journey; all data, credentials and decoded artwork synthetic/offline; full frames: {}",
                captures.directory.display()
            );
        }
        (outcome, disposed) => {
            captures
                .remove()
                .expect("owned failed synthetic capture cleanup");
            if let Err(panic) = disposed {
                std::panic::resume_unwind(panic);
            }
            match outcome {
                Ok(result) => panic!("{title} journey: {}", result.unwrap_err()),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    }
}
