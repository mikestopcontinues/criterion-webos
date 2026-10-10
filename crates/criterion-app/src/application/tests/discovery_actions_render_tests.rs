// SPDX-License-Identifier: GPL-3.0-or-later
//! Captured public rails through the actual Application; decoded image pixels are synthetic.
//! This opt-in host journey establishes neither provider availability nor TV/parity behavior.
use super::*;
use crate::application::sdl_render_tests::{Display, push_key};
use criterion_artwork::{ArtworkError, ArtworkSource, ImageRole};
use criterion_provider::{ContentTarget, ImageLabel};
use criterion_ui::Target;
use egui::{Color32, Rect, TextureId, pos2, vec2};
use glow::HasContext;
use image::ImageEncoder;
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

const LEFT: (u32, i32) = (80, 1_073_741_904);
const RIGHT: (u32, i32) = (79, 1_073_741_903);
const DOWN: (u32, i32) = (81, 1_073_741_905);
const UP: (u32, i32) = (82, 1_073_741_906);
const SELECT: (u32, i32) = (40, 13);
const BACK: (u32, i32) = (41, 27);
const REQUESTS: [&str; 3] = ["/", "/new", "/discover/newly-added"];
const TITLES: [[&str; 3]; 2] = [
    ["Barry Lyndon", "The Hitcher", "Salem’s Lot"],
    ["Barry Lyndon", "Jennifer’s Body", "The Gift"],
];
const IDS: [[&str; 3]; 2] = [
    ["aAUEybAm", "qvwT6mJ4", "KwgkF9Lq"],
    ["aAUEybAm", "sqbF7lmY", "oSjohtYk"],
];
const PATHS: [[&str; 3]; 2] = [
    [
        "/films/aAUEybAm/barry-lyndon",
        "/films/qvwT6mJ4/the-hitcher",
        "/films/KwgkF9Lq/salem-s-lot",
    ],
    [
        "/films/aAUEybAm/barry-lyndon",
        "/films/sqbF7lmY/jennifer-s-body",
        "/films/oSjohtYk/the-gift",
    ],
];
const WHITE: Color32 = Color32::from_rgb(239, 239, 239);
const GOLD: Color32 = Color32::from_rgb(181, 138, 22);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Pattern {
    film: usize,
    landscape: bool,
}
const PATTERNS: [Pattern; 6] = [
    Pattern {
        film: 0,
        landscape: false,
    },
    Pattern {
        film: 1,
        landscape: false,
    },
    Pattern {
        film: 2,
        landscape: false,
    },
    Pattern {
        film: 0,
        landscape: true,
    },
    Pattern {
        film: 1,
        landscape: true,
    },
    Pattern {
        film: 2,
        landscape: true,
    },
];
impl Pattern {
    fn label(self) -> ImageLabel {
        if self.landscape {
            ImageLabel::Landscape
        } else {
            ImageLabel::Regalia
        }
    }
    fn colors(self) -> [[u8; 4]; 2] {
        match (self.film, self.landscape) {
            (0, false) => [[20, 60, 160, 255], [40, 160, 240, 255]],
            (1, false) => [[160, 20, 20, 255], [240, 80, 30, 255]],
            (2, false) => [[20, 120, 20, 255], [80, 220, 80, 255]],
            (0, true) => [[90, 20, 150, 255], [180, 60, 240, 255]],
            (1, true) => [[20, 120, 140, 255], [50, 220, 230, 255]],
            (2, true) => [[170, 80, 20, 255], [240, 170, 50, 255]],
            _ => unreachable!("fixed six artwork patterns"),
        }
    }
    fn png(self) -> Vec<u8> {
        let pixels = self.colors().into_iter().flatten().collect::<Vec<_>>();
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&pixels, 2, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        encoded
    }
}

#[derive(Default)]
struct Images {
    attempts: AtomicUsize,
    active: AtomicUsize,
    completed: AtomicUsize,
}
struct Flight(Arc<Images>);
impl Drop for Flight {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
        self.0.completed.fetch_add(1, Ordering::SeqCst);
    }
}
fn loader(observed: Arc<Images>) -> Artwork {
    let encoded: Arc<BTreeMap<_, _>> = Arc::new(
        PATTERNS
            .into_iter()
            .map(|pattern| (pattern, pattern.png()))
            .collect(),
    );
    Artwork::with_loader(Arc::new(move |source| {
        let attempt = observed.attempts.fetch_add(1, Ordering::SeqCst);
        observed.active.fetch_add(1, Ordering::SeqCst);
        let flight = Flight(observed.clone());
        let encoded = encoded.clone();
        Box::pin(async move {
            let _flight = flight;
            if attempt >= 64 {
                return Err(ArtworkError::InvalidSource);
            }
            let ArtworkSource::Media { id, label, role } = &source else {
                // Other public thumbnails and heroes intentionally remain unavailable.
                return Err(ArtworkError::Unavailable);
            };
            let Some(pattern) = PATTERNS.into_iter().find(|pattern| {
                id.as_str() == IDS[usize::from(pattern.landscape)][pattern.film]
                    && *label == pattern.label()
                    && *role == ImageRole::Card
            }) else {
                return Err(ArtworkError::Unavailable);
            };
            criterion_artwork::decode_artwork(source.role(), "image/png", &encoded[&pattern])
        })
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Header,
    Discovery,
    Back,
}
const STAGES: [Stage; 3] = [Stage::Header, Stage::Discovery, Stage::Back];
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Header => "offline-captured-new-see-all.png",
            Self::Discovery => "offline-captured-newly-added.png",
            Self::Back => "offline-captured-new-see-all-back.png",
        }
    }
    fn row(self) -> usize {
        usize::from(self != Self::Discovery)
    }
    fn pattern(self, film: usize) -> Pattern {
        Pattern {
            film,
            landscape: self == Self::Discovery,
        }
    }
    fn focus(self) -> Focus {
        if self == Self::Discovery {
            Focus::Card { row: 0, column: 0 }
        } else {
            Focus::DiscoveryRailAction { row: 1, column: 2 }
        }
    }
    fn scroll(self) -> f32 {
        if self == Self::Discovery {
            632.0
        } else {
            1029.0
        }
    }
}
fn target(stage: Stage, film: usize) -> Target {
    Target::Content(
        ContentTarget::parse(PATHS[usize::from(stage == Stage::Discovery)][film]).unwrap(),
    )
}
fn require(ok: bool, reason: &'static str) -> Result<(), &'static str> {
    ok.then_some(()).ok_or(reason)
}
fn image_rect(stage: Stage, film: usize) -> Rect {
    let width = if stage == Stage::Discovery {
        378.0
    } else {
        516.0
    };
    Rect::from_min_size(
        pos2(150.0 + film as f32 * (width + 36.0), 324.0),
        vec2(width, width * 9.0 / 16.0),
    )
}
fn header_rect() -> Rect {
    Rect::from_min_max(pos2(1470.0, 256.0), pos2(1770.0, 308.0))
}
fn observe(textures: &mut BTreeMap<Pattern, TextureId>, output: &egui::FullOutput) {
    for (id, deltas) in &output.textures_delta.set {
        for delta in deltas {
            let egui::ImageData::Color(image) = &delta.image;
            for pattern in PATTERNS {
                if image.size == [2, 1]
                    && image
                        .pixels
                        .iter()
                        .map(|color| color.to_array())
                        .eq(pattern.colors())
                {
                    textures.insert(pattern, *id);
                }
            }
        }
    }
}

struct Fixture {
    app: Option<App>,
    runtime: Runtime,
    public: Public,
    images: Arc<Images>,
    textures: BTreeMap<Pattern, TextureId>,
}
impl Fixture {
    fn new() -> Self {
        let runtime = runtime();
        let public = Public::default();
        let images = Arc::new(Images::default());
        let clock = SystemClock::default();
        let session = Arc::new(criterion_session::Session::with_transport(
            criterion_session::Configuration::production(),
            Offline,
            clock.clone(),
        ));
        let app = Application::with_parts(
            surface(),
            Controller::new(Catalog::with_transport(public.clone()), runtime.handle()),
            Authentication::with_session(session.clone(), clock),
            Accounts::from_parts(
                Arc::new(criterion_account::AccountClient::with_transport(
                    AccountOffline(public.account_calls.clone()),
                )),
                session,
            ),
            loader(images.clone()),
        );
        Self {
            app: Some(app),
            runtime,
            public,
            images,
            textures: BTreeMap::new(),
        }
    }
    fn app(&self) -> &App {
        self.app.as_ref().unwrap()
    }
    fn scope(&self) -> Result<(), &'static str> {
        let calls = self.public.calls.lock().unwrap();
        require(
            calls.len() <= REQUESTS.len()
                && calls
                    .iter()
                    .zip(REQUESTS)
                    .all(|(actual, expected)| actual.as_str() == expected),
            "exact bounded public request prefix",
        )?;
        require(
            self.public.account_calls.load(Ordering::SeqCst) == 0
                && matches!(
                    self.app().authentication.view(),
                    criterion_ui::LoginView::SignedOut
                ),
            "signed-out journey makes no account call",
        )?;
        require(
            self.images.attempts.load(Ordering::SeqCst) <= 64,
            "finite offline public artwork demand",
        )
    }
    fn ready(&self) -> bool {
        self.app()
            .controller
            .view
            .with_view(self.app().authentication.view(), |data| {
                data.status == LoadState::Ready
            })
    }
    fn stage_ready(&self, stage: Stage) -> bool {
        self.ready()
            && self
                .app()
                .controller
                .view
                .with_view(self.app().authentication.view(), |data| {
                    data.rails.get(stage.row()).is_some_and(|rail| {
                        rail.cards.len() == 3
                            && rail.cards.iter().all(|card| {
                                card.artwork_key
                                    .is_some_and(|key| self.app().ui.has_image(key))
                            })
                    })
                })
    }
    fn pump(&mut self, elapsed: Duration) {
        let app = self.app.as_mut().unwrap();
        app.poll(&self.runtime, true);
        app.consume(&self.runtime, elapsed);
        if let Some(output) = &app.output {
            observe(&mut self.textures, output);
        }
    }
    fn cpu_key(&mut self, (scancode, keycode): (u32, i32)) {
        for pressed in [true, false] {
            self.app.as_mut().unwrap().event(
                Event::Key(KeyEvent {
                    scancode,
                    keycode,
                    pressed,
                    repeat: false,
                }),
                surface(),
                &self.runtime,
                Duration::ZERO,
            );
            self.pump(Duration::ZERO);
        }
    }
    fn cpu_wait(&mut self, predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.pump(Duration::ZERO);
            self.scope().unwrap();
            if predicate(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "offline rail CPU witness deadline"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn cpu_header(&mut self) {
        self.cpu_wait(Self::ready);
        for key in [LEFT, DOWN, SELECT] {
            self.cpu_key(key);
        }
        self.cpu_wait(Self::ready);
        for key in [DOWN, DOWN, RIGHT, RIGHT, UP] {
            self.cpu_key(key);
        }
        self.cpu_wait(|fixture| fixture.stage_ready(Stage::Header));
        self.pump(Duration::ZERO);
    }
    // Return taken-but-unpainted work to its Application owner on every early failure.
    // No decoded-image injection or explicit texture-delta clearing is used here.
    fn return_output(&mut self, output: egui::FullOutput) {
        let app = self.app.as_mut().unwrap();
        if let Some(current) = &mut app.output {
            current.append(output);
        } else {
            app.output = Some(output);
        }
    }
    fn dispose(&mut self) -> Result<(), &'static str> {
        let Some(mut app) = self.app.take() else {
            return Ok(());
        };
        let finished = app.finish(&self.runtime);
        drop(app);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.images.active.load(Ordering::SeqCst) != 0 {
            require(
                Instant::now() < deadline,
                "offline artwork disposal deadline",
            )?;
            self.runtime
                .block_on(async { tokio::task::yield_now().await });
        }
        require(
            finished
                && self.images.completed.load(Ordering::SeqCst)
                    == self.images.attempts.load(Ordering::SeqCst),
            "application and synthetic decode owners disposed",
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.dispose();
    }
}

#[derive(Debug, PartialEq)]
struct WindowSnapshot {
    cards: Vec<(Target, String, String)>,
    focus: Focus,
    scroll: f32,
}
fn snapshot(fixture: &Fixture) -> WindowSnapshot {
    let app = fixture.app();
    WindowSnapshot {
        cards: app
            .controller
            .view
            .with_view(app.authentication.view(), |data| {
                data.rails[1]
                    .cards
                    .iter()
                    .map(|card| {
                        (
                            card.key.clone(),
                            card.title.to_owned(),
                            card.artwork_key.unwrap().to_owned(),
                        )
                    })
                    .collect()
            }),
        focus: app.ui.focus(),
        scroll: app.ui.scroll_y(),
    }
}
fn state_oracle(fixture: &Fixture, stage: Stage) -> Result<(), &'static str> {
    fixture.scope()?;
    let app = fixture.app();
    let page = if stage == Stage::Discovery {
        Page::Discovery
    } else {
        Page::New
    };
    require(
        app.ui.page() == page
            && app.ui.focus() == stage.focus()
            && app.ui.scroll_y() == stage.scroll(),
        "current exact public page/focus/scroll",
    )?;
    let expected_reads = if stage == Stage::Header { 2 } else { 3 };
    require(
        fixture.public.calls.lock().unwrap().len() == expected_reads,
        "stage exact public read census",
    )?;
    require(
        app.controller.membership_visit().is_some(),
        "current ordinary discovery visit",
    )?;
    app.controller.view.with_view(app.authentication.view(), |data| {
        require(data.status == LoadState::Ready, "captured destination is actually Ready")?;
        let rail = data.rails.get(stage.row()).ok_or("current captured rail absent")?;
        require(rail.cards.len() == 3, "captured three-card window")?;
        if stage != Stage::Discovery {
            let action = rail.action.ok_or("current supplied action absent")?;
            let destination = Target::Content(ContentTarget::parse("/discover/newly-added").unwrap());
            require(rail.title == "Newly Added Films" && action.block == 825
                && action.label == "See all" && action.target == &destination,
                "independent supplied heading/block/action/target")?;
        }
        for (film, card) in rail.cards.iter().enumerate() {
            let key = card.artwork_key.ok_or("current card artwork key absent")?;
            let pattern = stage.pattern(film);
            require(card.key == &target(stage, film) && card.title == TITLES[usize::from(stage == Stage::Discovery)][film]
                && card.saved_fraction.is_none() && app.ui.has_image(key),
                "independent current public card identity and decoded image")?;
            require(app.controller.view.artwork_bindings().iter().any(|binding| {
                binding.key == key && matches!(&binding.source,
                    crate::presentation::ImageSource::Media { id, label, role }
                    if id.as_str() == IDS[usize::from(pattern.landscape)][film] && *label == pattern.label() && *role == ImageRole::Card)
            }), "exact current public image source")?;
        }
        Ok(())
    })
}
fn text_inside(
    shapes: &[egui::epaint::ClippedShape],
    literal: &str,
    area: Rect,
    white: bool,
) -> bool {
    shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text)
        if text.galley.job.text == literal && !text.galley.elided
        && area.contains_rect(shape.shape.visual_bounding_rect())
        && shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect())
        && (!white || text.galley.job.sections.iter().all(|section| section.format.color == WHITE)))
    })
}
fn shape_oracle(
    shapes: &[egui::epaint::ClippedShape],
    stage: Stage,
    textures: &BTreeMap<Pattern, TextureId>,
) -> Result<(), &'static str> {
    if stage != Stage::Discovery {
        require(
            text_inside(
                shapes,
                "Newly Added Films",
                Rect::from_min_max(pos2(150.0, 260.0), pos2(1400.0, 310.0)),
                true,
            ),
            "complete unclipped supplied heading",
        )?;
        require(
            text_inside(shapes, "See all", header_rect(), true),
            "complete contrasting current CTA",
        )?;
        require(
            shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.rect == header_rect() && rect.fill == GOLD
            && shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect()))
            }),
            "exact current focused header geometry",
        )?;
    } else {
        let area = image_rect(stage, 0).expand(8.0);
        require(
            shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.rect == area && rect.stroke.width == 8.0 && rect.stroke.color == GOLD
            && shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect()))
            }),
            "current focused destination card geometry",
        )?;
    }
    for (film, title) in TITLES[usize::from(stage == Stage::Discovery)]
        .iter()
        .enumerate()
    {
        let area = image_rect(stage, film);
        let id = textures
            .get(&stage.pattern(film))
            .ok_or("current decoded texture unobserved")?;
        require(
            *id != TextureId::default()
                && shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Mesh(mesh)
                if mesh.texture_id == *id && mesh.calc_bounds() == area
                && shape.clip_rect.contains_rect(area))
                }),
            "current owning image mesh and full bounds",
        )?;
        require(
            text_inside(
                shapes,
                title,
                Rect::from_min_max(
                    pos2(area.left(), area.bottom() + 5.0),
                    pos2(area.right(), area.bottom() + 45.0),
                ),
                stage == Stage::Discovery && film == 0,
            ),
            "complete current card caption inside its visual bounds",
        )?;
    }
    Ok(())
}

#[test]
fn actual_captured_rail_shape_witness_restores_nonzero_column_without_another_read() {
    let mut fixture = Fixture::new();
    fixture.cpu_header();
    let visit = fixture.app().controller.membership_visit().unwrap();
    let before = snapshot(&fixture);
    for stage in STAGES {
        if stage == Stage::Discovery {
            fixture.cpu_key(SELECT);
            fixture.cpu_wait(Fixture::ready);
            fixture.cpu_key(DOWN);
        } else if stage == Stage::Back {
            fixture.cpu_key(BACK);
        }
        fixture.cpu_wait(|fixture| fixture.stage_ready(stage));
        fixture.pump(Duration::ZERO);
        state_oracle(&fixture, stage).unwrap();
        shape_oracle(
            &fixture.app().output.as_ref().unwrap().shapes,
            stage,
            &fixture.textures,
        )
        .unwrap();
    }
    assert_eq!(snapshot(&fixture), before);
    assert_ne!(fixture.app().controller.membership_visit().unwrap(), visit);
    assert_eq!(&*fixture.public.calls.lock().unwrap(), &REQUESTS);
    fixture.dispose().unwrap();
}

#[test]
fn rail_prepaint_oracle_refuses_missing_cta_clipped_focus_and_stale_image_mesh() {
    let mut fixture = Fixture::new();
    fixture.cpu_header();
    let shapes = fixture.app().output.as_ref().unwrap().shapes.clone();
    shape_oracle(&shapes, Stage::Header, &fixture.textures).unwrap();
    let mut missing = shapes.clone();
    missing.retain(|shape| {
        !matches!(&shape.shape, egui::Shape::Text(text)
        if text.galley.job.text == "See all")
    });
    assert!(shape_oracle(&missing, Stage::Header, &fixture.textures).is_err());
    let mut clipped = shapes.clone();
    let focus = clipped
        .iter_mut()
        .find(|shape| {
            matches!(&shape.shape, egui::Shape::Rect(rect)
        if rect.rect == header_rect() && rect.fill == GOLD)
        })
        .unwrap();
    focus.clip_rect = header_rect().shrink(1.0);
    assert!(shape_oracle(&clipped, Stage::Header, &fixture.textures).is_err());
    let mut stale = shapes;
    let current = fixture.textures[&Stage::Header.pattern(0)];
    let mesh = stale
        .iter_mut()
        .find_map(|shape| match &mut shape.shape {
            egui::Shape::Mesh(mesh)
                if mesh.texture_id == current
                    && mesh.calc_bounds() == image_rect(Stage::Header, 0) =>
            {
                Some(mesh)
            }
            _ => None,
        })
        .unwrap();
    Arc::make_mut(mesh).texture_id = TextureId::Managed(u64::MAX);
    assert!(shape_oracle(&stale, Stage::Header, &fixture.textures).is_err());
    fixture.dispose().unwrap();
}

struct Captures {
    directory: PathBuf,
}
impl Captures {
    fn new() -> Self {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let directory = PathBuf::from(format!(
            ".local/e2e/native-offline-discovery-actions-{}-{}",
            std::process::id(),
            epoch.as_nanos(),
        ));
        std::fs::create_dir_all(directory.parent().unwrap()).unwrap();
        std::fs::create_dir(&directory).unwrap();
        Self { directory }
    }
    fn complete(&self) -> bool {
        STAGES
            .iter()
            .all(|stage| self.directory.join(stage.name()).is_file())
    }
}
fn count_pixels(pixels: &[u8], area: Rect, accepts: impl Fn([u8; 3]) -> bool) -> usize {
    (area.top() as usize..area.bottom() as usize)
        .flat_map(|y| (area.left() as usize..area.right() as usize).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let offset = ((1079 - y) * 1920 + x) * 4;
            accepts(pixels[offset..offset + 3].try_into().unwrap())
        })
        .count()
}
fn pixel_oracle(pixels: &[u8], stage: Stage) -> Result<(), &'static str> {
    require(
        pixels.len() == 1920 * 1080 * 4,
        "exact original rail framebuffer",
    )?;
    let focus = if stage == Stage::Discovery {
        image_rect(stage, 0).expand(8.0)
    } else {
        header_rect()
    };
    require(
        count_pixels(pixels, focus, |[r, g, b]| {
            r.abs_diff(181) < 5 && g.abs_diff(138) < 5 && b.abs_diff(22) < 5
        }) > 500,
        "current focus gold framebuffer",
    )?;
    let caption = if stage == Stage::Discovery {
        let card = image_rect(stage, 0);
        Rect::from_min_max(
            pos2(card.left(), card.bottom() + 5.0),
            pos2(card.right(), card.bottom() + 45.0),
        )
    } else {
        header_rect().shrink2(vec2(24.0, 8.0))
    };
    require(
        count_pixels(pixels, caption, |rgb| rgb.iter().all(|value| *value > 210)) > 100,
        "current contrasting caption framebuffer",
    )?;
    if stage != Stage::Discovery {
        require(
            count_pixels(
                pixels,
                Rect::from_min_max(pos2(150.0, 260.0), pos2(1000.0, 310.0)),
                |rgb| rgb.iter().all(|value| *value > 210),
            ) > 100,
            "current supplied heading framebuffer",
        )?;
    }
    for film in 0..3 {
        let image = image_rect(stage, film);
        let area = Rect::from_min_max(
            pos2(image.left() + 16.0, image.top() + 16.0),
            pos2(image.left() + image.width() / 5.0, image.bottom() - 16.0),
        );
        let expected = stage.pattern(film).colors()[0];
        require(
            count_pixels(pixels, area, |rgb| {
                rgb.into_iter()
                    .zip(expected)
                    .all(|(actual, expected)| actual.abs_diff(expected) < 5)
            }) > 1000,
            "current synthetic decoded image framebuffer",
        )?;
    }
    Ok(())
}

struct Rendered<'a> {
    fixture: &'a mut Fixture,
    display: &'a mut Display,
    captures: &'a Captures,
    start: Instant,
}
impl Rendered<'_> {
    fn bounded(&self) -> Result<(), &'static str> {
        require(
            self.start.elapsed() < Duration::from_secs(90),
            "whole rail journey deadline",
        )?;
        self.fixture.scope()
    }
    fn paint(&mut self, stage: Option<Stage>) -> Result<(), &'static str> {
        self.bounded()?;
        let drawable = self
            .display
            .window
            .surface()
            .map_err(|_| "rail SDL surface")?
            .drawable;
        require(
            drawable.width == 1920 && drawable.height == 1080,
            "original rail drawable dimensions",
        )?;
        let Some(mut output) = self.fixture.app.as_mut().unwrap().take_output() else {
            return require(stage.is_none(), "fresh current rail capture output");
        };
        // Keep taken output outside the unwind boundary, then return it to its
        // owner on either error or panic before any unapplied delta can drop.
        let painted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            observe(&mut self.fixture.textures, &output);
            if let Some(stage) = stage
                && let Err(error) = state_oracle(self.fixture, stage)
                    .and_then(|()| shape_oracle(&output.shapes, stage, &self.fixture.textures))
            {
                return Err(error);
            }
            if self
                .display
                .painter
                .paint([1920, 1080], self.fixture.app().context(), &mut output)
                .is_err()
            {
                return Err("rail GLES paint");
            }
            if let Some(stage) = stage {
                let mut pixels = vec![0_u8; 1920 * 1080 * 4];
                // SAFETY: exact allocation and current GLES context stay on the display thread.
                unsafe {
                    self.display.gl.read_pixels(
                        0,
                        0,
                        1920,
                        1080,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut pixels)),
                    );
                }
                let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(1920, 1080, pixels)
                    .ok_or("rail framebuffer image")?;
                let flipped = image::imageops::flip_vertical(&image);
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(self.captures.directory.join(stage.name()))
                    .map_err(|_| "fresh rail capture file")?;
                image::codecs::png::PngEncoder::new(file)
                    .write_image(
                        flipped.as_raw(),
                        1920,
                        1080,
                        image::ExtendedColorType::Rgba8,
                    )
                    .map_err(|_| "rail capture encoding")?;
                // Save this same original framebuffer before admitting its pixels; retain failures.
                pixel_oracle(image.as_raw(), stage)?;
            }
            self.display
                .window
                .present()
                .map_err(|_| "rail GLES present")?;
            // SAFETY: the display's context remains current on this thread.
            require(
                unsafe { self.display.gl.get_error() } == glow::NO_ERROR,
                "rail GLES error",
            )
        }));
        match painted {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                self.fixture.return_output(output);
                Err(error)
            }
            Err(panic) => {
                self.fixture.return_output(output);
                std::panic::resume_unwind(panic)
            }
        }
    }
    fn tick(&mut self) -> Result<(), &'static str> {
        self.bounded()?;
        self.fixture
            .app
            .as_mut()
            .unwrap()
            .poll(&self.fixture.runtime, true);
        for _ in 0..128 {
            let Some(event) = self
                .display
                .window
                .poll_event()
                .map_err(|_| "rail SDL event")?
            else {
                self.fixture
                    .app
                    .as_mut()
                    .unwrap()
                    .consume(&self.fixture.runtime, self.start.elapsed());
                return self.paint(None);
            };
            self.fixture.app.as_mut().unwrap().event(
                event,
                self.display
                    .window
                    .surface()
                    .map_err(|_| "rail SDL surface")?,
                &self.fixture.runtime,
                self.start.elapsed(),
            );
            self.paint(None)?;
        }
        Err("rail SDL drain exceeds128events")
    }
    fn key(&mut self, (scan, code): (u32, i32)) -> Result<(), &'static str> {
        for pressed in [true, false] {
            push_key(scan, code, pressed);
            self.tick()?;
        }
        Ok(())
    }
    fn wait(&mut self, predicate: impl Fn(&Fixture) -> bool) -> Result<(), &'static str> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.tick()?;
            if predicate(self.fixture) {
                return Ok(());
            }
            require(
                Instant::now() < deadline,
                "offline rail rendered stage deadline",
            )?;
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn capture(&mut self, stage: Stage) -> Result<(), &'static str> {
        self.fixture
            .app
            .as_mut()
            .unwrap()
            .consume(&self.fixture.runtime, self.start.elapsed());
        self.paint(Some(stage))
    }
    fn journey(&mut self) -> Result<(), &'static str> {
        self.wait(Fixture::ready)?;
        for key in [LEFT, DOWN, SELECT] {
            self.key(key)?;
        }
        self.wait(Fixture::ready)?;
        for key in [DOWN, DOWN, RIGHT, RIGHT, UP] {
            self.key(key)?;
        }
        self.wait(|fixture| fixture.stage_ready(Stage::Header))?;
        self.capture(Stage::Header)?;
        let visit = self.fixture.app().controller.membership_visit().unwrap();
        let before = snapshot(self.fixture);
        self.key(SELECT)?;
        require(
            self.fixture.app().ui.page() == Page::Discovery,
            "exact See all Discovery destination",
        )?;
        self.wait(Fixture::ready)?;
        self.key(DOWN)?;
        self.wait(|fixture| fixture.stage_ready(Stage::Discovery))?;
        self.capture(Stage::Discovery)?;
        self.key(BACK)?;
        self.wait(|fixture| fixture.stage_ready(Stage::Back))?;
        require(
            snapshot(self.fixture) == before,
            "exact warm Back focus/scroll/card-key window",
        )?;
        require(
            self.fixture.app().controller.membership_visit() != Some(visit),
            "fresh Back visit",
        )?;
        self.capture(Stage::Back)?;
        require(self.captures.complete(), "all three original rail frames")
    }
}

fn run_case(
    display: &mut Display,
    captures: &Captures,
    start: Instant,
) -> Result<(), &'static str> {
    let mut fixture = Fixture::new();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Rendered {
            fixture: &mut fixture,
            display,
            captures,
            start,
        }
        .journey()
    }));
    let disposed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fixture.dispose()));
    drop(fixture);
    match (outcome, disposed) {
        (Ok(result), Ok(Ok(()))) => result,
        (_, Err(panic)) | (Err(panic), _) => std::panic::resume_unwind(panic),
        (_, Ok(Err(error))) => Err(error),
    }
}

#[test]
#[ignore = "captured public source/synthetic decoded pixels; Root-only serialized SDL/GLES"]
fn native_offline_public_new_rail_see_all_and_back_end_to_end() {
    crate::prepare_process();
    let captures = Captures::new();
    let start = Instant::now();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut display = Display::open("Criterion Unofficial Offline Public Rail E2E");
        run_case(&mut display, &captures, start)
    }));
    match outcome {
        Ok(Ok(())) => println!(
            "actual public rail SDL/GLES; captured source offline, six decoded image patterns synthetic; other artwork unavailable; three original frames: {}",
            captures.directory.display(),
        ),
        outcome => {
            eprintln!(
                "failed/unaccepted public rail journey; owners disposed, partial original frames retained: {}",
                captures.directory.display()
            );
            match outcome {
                Err(panic) => std::panic::resume_unwind(panic),
                Ok(result) => panic!("public rail SDL/GLES: {}", result.unwrap_err()),
            }
        }
    }
}
