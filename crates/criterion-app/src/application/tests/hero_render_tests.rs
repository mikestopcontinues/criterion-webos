// SPDX-License-Identifier: GPL-3.0-or-later
//! Public captured discovery and synthetic decoded pixels; no live provider admission.
use super::*;
use crate::application::sdl_render_tests::{Display, push_key};
use criterion_artwork::{ArtworkError, ArtworkSource, ImageRole};
use egui::{Color32, Rect, TextureId, pos2, vec2};
use glow::HasContext;
use image::ImageEncoder;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

type App = Application<Public, Offline>;
const RIGHT: (u32, i32) = (79, 1_073_741_903);
const LEFT: (u32, i32) = (80, 1_073_741_904);
const SELECT: (u32, i32) = (40, 13);
const BACK: (u32, i32) = (41, 27);
const WHITE: Color32 = Color32::from_rgb(239, 239, 239);
const GOLD: Color32 = Color32::from_rgb(181, 138, 22);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Pattern {
    FirstBackdrop,
    FirstLogo,
    SecondBackdrop,
    SecondLogo,
}
impl Pattern {
    fn file(self) -> &'static str {
        match self {
            Self::FirstBackdrop => "POSSESSIONS_collection_hero_wide_1920x0.webp",
            Self::FirstLogo => "POSSESSIONS_collection_logo_default_760x0.webp",
            Self::SecondBackdrop => "MADE-FOR-TV_TERROR_collection_hero_wide_1920x0.webp",
            Self::SecondLogo => "MADE-FOR-TV_TERROR_collection_logo_slide_760x0.webp",
        }
    }
    fn pixels(self) -> [[u8; 4]; 2] {
        match self {
            Self::FirstBackdrop => [[20, 50, 170, 255], [30, 160, 240, 255]],
            Self::FirstLogo => [[20, 120, 50, 255], [40, 220, 80, 255]],
            Self::SecondBackdrop => [[160, 20, 20, 255], [240, 100, 30, 255]],
            Self::SecondLogo => [[160, 100, 20, 255], [240, 220, 30, 255]],
        }
    }
    fn png(self) -> Vec<u8> {
        let pixels = self.pixels().into_iter().flatten().collect::<Vec<_>>();
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&pixels, 2, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        encoded
    }
}
const PATTERNS: [Pattern; 4] = [
    Pattern::FirstBackdrop,
    Pattern::FirstLogo,
    Pattern::SecondBackdrop,
    Pattern::SecondLogo,
];

#[derive(Default)]
struct Images {
    calls: Mutex<Vec<Pattern>>,
    violation: Mutex<Option<&'static str>>,
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
    let encoded: Arc<BTreeMap<_, _>> =
        Arc::new(PATTERNS.into_iter().map(|p| (p, p.png())).collect());
    Artwork::with_loader(Arc::new(move |source| {
        let observed = observed.clone();
        let encoded = encoded.clone();
        Box::pin(async move {
            let ArtworkSource::Editorial(image) = &source else {
                *observed.violation.lock().unwrap() = Some("noneditorial hero image demand");
                return Err(ArtworkError::InvalidSource);
            };
            let Some(pattern) = PATTERNS
                .into_iter()
                .find(|p| image.url().path().rsplit('/').next() == Some(p.file()))
            else {
                *observed.violation.lock().unwrap() = Some("unknown hero image demand");
                return Err(ArtworkError::InvalidSource);
            };
            {
                let mut calls = observed.calls.lock().unwrap();
                if calls.len() >= 6 {
                    *observed.violation.lock().unwrap() = Some("hero image demand exceeds bound");
                    return Err(ArtworkError::InvalidSource);
                }
                calls.push(pattern);
            }
            observed.active.fetch_add(1, Ordering::SeqCst);
            let _flight = Flight(observed);
            criterion_artwork::decode_artwork(ImageRole::Backdrop, "image/png", &encoded[&pattern])
        })
    }))
}

#[derive(Clone, Copy, Debug)]
enum Stage {
    First,
    Second,
    WarmBack,
    Duplicate,
    Unavailable,
}
const STAGES: [Stage; 5] = [
    Stage::First,
    Stage::Second,
    Stage::WarmBack,
    Stage::Duplicate,
    Stage::Unavailable,
];
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::First => "offline-captured-hero-first.png",
            Self::Second => "offline-captured-hero-second.png",
            Self::WarmBack => "offline-captured-hero-warm-back.png",
            Self::Duplicate => "synthetic-hero-duplicate-last.png",
            Self::Unavailable => "synthetic-hero-unavailable.png",
        }
    }
    fn caption(self) -> &'static str {
        match self {
            Self::First => "Slide 1 of 7",
            Self::Second | Self::WarmBack => "Slide 2 of 7",
            Self::Duplicate => "Slide 32 of 32",
            Self::Unavailable => "Slide 2 of 2",
        }
    }
    fn focus(self) -> Focus {
        match self {
            Self::First | Self::WarmBack => Focus::Hero,
            Self::Second => Focus::HeroNext,
            Self::Duplicate | Self::Unavailable => Focus::HeroPrevious,
        }
    }
    fn patterns(self) -> Option<[Pattern; 2]> {
        match self {
            Self::Unavailable => None,
            Self::Second | Self::WarmBack => Some([Pattern::SecondBackdrop, Pattern::SecondLogo]),
            _ => Some([Pattern::FirstBackdrop, Pattern::FirstLogo]),
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
    fn new(stage: Stage) -> Self {
        let runtime = runtime();
        let public = Public::default();
        {
            let mut payload = public.payload.lock().unwrap();
            payload["blocks"] = serde_json::json!([payload["blocks"][0].clone()]);
            if matches!(stage, Stage::Duplicate) {
                let first = payload["blocks"][0]["slides"][0].clone();
                payload["blocks"][0]["slides"] = serde_json::json!(vec![first; 32]);
            }
        }
        if matches!(stage, Stage::Unavailable) {
            unavailable(&public, 2);
        }
        let images = Arc::new(Images::default());
        let mut app = fixture_app(&runtime, public.clone());
        app.artwork = loader(images.clone());
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
    fn pump(&mut self) {
        let app = self.app.as_mut().unwrap();
        app.poll(&self.runtime, true);
        app.consume(&self.runtime, Duration::ZERO);
    }
    fn ready(&self) -> bool {
        self.app()
            .controller
            .view
            .with_view(self.app().authentication.view(), |data| {
                data.status == LoadState::Ready
                    && data.hero.as_ref().is_none_or(|hero| {
                        [hero.background_key, hero.title_logo_key]
                            .into_iter()
                            .all(|key| key.is_some_and(|key| self.app().ui.has_image(key)))
                    })
            })
    }
    fn observe(&mut self, output: &egui::FullOutput) {
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                let egui::ImageData::Color(image) = &delta.image;
                for pattern in PATTERNS {
                    if image.size == [2, 1]
                        && image
                            .pixels
                            .iter()
                            .map(|c| c.to_array())
                            .eq(pattern.pixels())
                    {
                        self.textures.insert(pattern, *id);
                    }
                }
            }
        }
    }
    fn cpu_frame(&mut self) -> egui::FullOutput {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.pump();
            if self.ready() {
                break;
            }
            assert!(Instant::now() < deadline, "offline hero CPU readiness");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.pump();
        let mut frame = self.app.as_mut().unwrap().take_output().unwrap();
        self.observe(&frame);
        frame.textures_delta.clear();
        frame
    }
    fn cpu_key(&mut self, (scan, code): (u32, i32)) {
        for pressed in [true, false] {
            self.app.as_mut().unwrap().event(
                Event::Key(KeyEvent {
                    scancode: scan,
                    keycode: code,
                    pressed,
                    repeat: false,
                }),
                surface(),
                &self.runtime,
                Duration::ZERO,
            );
        }
    }
    fn dispose(&mut self) -> Result<(), &'static str> {
        let Some(mut app) = self.app.take() else {
            return Ok(());
        };
        let finished = app.finish(&self.runtime);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.images.active.load(Ordering::SeqCst) > 0 {
            if Instant::now() >= deadline {
                return Err("offline artwork disposal deadline");
            }
            self.runtime
                .block_on(async { tokio::task::yield_now().await });
        }
        drop(app);
        require(
            finished
                && self.images.completed.load(Ordering::SeqCst)
                    == self.images.calls.lock().unwrap().len(),
            "signed-out application and decoded-work disposal",
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.dispose();
    }
}
fn require(ok: bool, message: &'static str) -> Result<(), &'static str> {
    ok.then_some(()).ok_or(message)
}
fn control(focus: Focus) -> Rect {
    let (x, width) = match focus {
        Focus::Hero => (150., 214.),
        Focus::HeroPrevious => (402., 82.),
        Focus::HeroNext => (520., 82.),
        _ => panic!("hero capture focus"),
    };
    Rect::from_min_size(pos2(x, 740.), vec2(width, 80.))
}
fn text_inside(output: &egui::FullOutput, text: &str, area: Rect) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape,egui::Shape::Text(value)
        if value.galley.job.text==text && !value.galley.elided
        && area.contains_rect(shape.shape.visual_bounding_rect())
        && shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect()))
    })
}
fn shape_oracle(
    output: &egui::FullOutput,
    stage: Stage,
    textures: &BTreeMap<Pattern, TextureId>,
) -> Result<(), &'static str> {
    require(
        text_inside(
            output,
            stage.caption(),
            Rect::from_min_max(pos2(640., 750.), pos2(1440., 825.)),
        ),
        "complete current caption inside clip",
    )?;
    for x in [443., 561.] {
        let area = Rect::from_center_size(pos2(x, 780.), vec2(40., 40.));
        let icons = output
            .shapes
            .iter()
            .filter(|shape| {
                matches!(&shape.shape,egui::Shape::Path(path)
            if area.contains_rect(shape.shape.visual_bounding_rect())
            && path.stroke.color==egui::epaint::ColorMode::Solid(WHITE))
            })
            .collect::<Vec<_>>();
        require(
            icons.len() == 1
                && icons[0]
                    .clip_rect
                    .contains_rect(icons[0].shape.visual_bounding_rect()),
            "white full-stroke arrow inside clip",
        )?;
    }
    let focus = control(stage.focus());
    require(output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Rect(rect)
        if rect.rect==focus && rect.fill==GOLD && shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect()))),"current focused hero control")?;
    if let Some([backdrop, logo]) = stage.patterns() {
        require(
            text_inside(output, "See more", control(Focus::Hero)),
            "complete current CTA",
        )?;
        for (pattern, area) in [
            (
                backdrop,
                Rect::from_min_size(pos2(0., 0.), vec2(1920., 1080.)),
            ),
            (
                logo,
                Rect::from_min_size(pos2(150., 300.), vec2(480., 240.)),
            ),
        ] {
            let id = textures
                .get(&pattern)
                .ok_or("current decoded texture not observed")?;
            require(output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Mesh(mesh)
                if mesh.texture_id==*id && *id!=TextureId::default() && mesh.calc_bounds()==area && shape.clip_rect.contains_rect(area))),"owning current artwork mesh")?;
        }
    } else {
        require(
            text_inside(
                output,
                "Slide unavailable",
                Rect::from_min_max(pos2(150., 590.), pos2(1050., 650.)),
            ),
            "unavailable raw slot caption",
        )?;
        require(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="See more")),"unavailable slot must omit CTA")?;
        require(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.texture_id!=TextureId::default())),"unavailable slot must omit artwork")?;
    }
    Ok(())
}
fn public_only(fixture: &Fixture, maximum: usize) -> Result<(), &'static str> {
    let app = fixture.app();
    require(
        !app.authentication.signed_in()
            && !app.authentication.access_ready()
            && app.account_epoch == Some(0)
            && !app.account_signed_in
            && app.shelf_pending.is_none()
            && app.shelf_generation.is_none()
            && app.continue_watching_pending.is_none()
            && app.continue_watching_generation.is_none()
            && app.native_detail_pending.is_none()
            && app.native_detail_generation.is_none()
            && app.list_membership.is_none()
            && app.positions.is_none(),
        "public signed-out read ownership",
    )?;
    let calls = fixture.public.calls.lock().unwrap();
    require(
        calls.len() <= maximum
            && calls
                .iter()
                .enumerate()
                .all(|(i, p)| p == if i == 0 { "/" } else { "/api/media/p753ts71" }),
        "exact bounded public request paths",
    )?;
    require(
        fixture.images.violation.lock().unwrap().is_none()
            && fixture.images.calls.lock().unwrap().len() <= 6,
        "finite offline image requests",
    )
}
fn state_oracle(fixture: &Fixture, stage: Stage) -> Result<(), &'static str> {
    public_only(
        fixture,
        if matches!(stage, Stage::First | Stage::Duplicate | Stage::Unavailable) {
            1
        } else {
            2
        },
    )?;
    let counts = PATTERNS.map(|pattern| {
        fixture
            .images
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|p| **p == pattern)
            .count()
    });
    require(
        match stage {
            Stage::First | Stage::Duplicate => counts == [1, 1, 0, 0],
            Stage::Second => counts == [1, 1, 1, 1],
            Stage::WarmBack => {
                counts[0..2] == [1, 1] && (1..=2).contains(&counts[2]) && counts[2] == counts[3]
            }
            Stage::Unavailable => counts == [0; 4],
        },
        "only current hero image demand; no hidden prefetch or old restart",
    )?;
    let app = fixture.app();
    require(
        app.ui.page() == Page::Home && app.ui.focus() == stage.focus() && app.ui.scroll_y() == 0.,
        "current Home focus and scroll",
    )?;
    let current = cursor(app);
    let (index, total, slide) = match stage {
        Stage::First => (0, 7, 1595),
        Stage::Second | Stage::WarmBack => (1, 7, 1599),
        Stage::Duplicate => (31, 32, 1595),
        Stage::Unavailable => (1, 2, 1595),
    };
    require(
        current.block == 493 && current.index == index && current.slide == slide,
        "independent raw slot address",
    )?;
    app.controller.view.with_view(app.authentication.view(),|data| {
        let carousel=data.hero_carousel.ok_or("current raw slideshow missing")?;
        require(data.status==LoadState::Ready && carousel.total==total && carousel.caption==stage.caption(),"complete current slideshow metadata")?;
        if let Some(patterns)=stage.patterns() {
            let hero=data.hero.as_ref().ok_or("current admitted hero missing")?;
            let target=criterion_ui::Target::Content(criterion_provider::ContentTarget::parse(if matches!(stage,Stage::Second|Stage::WarmBack){"/collections/p753ts71/made-for-tv-terror"}else{"/collections/fhQRpxw4/possessions"}).unwrap());
            require(hero.card.key==&target && hero.action=="See more" && hero.card.saved_fraction.is_none(),"independent current CTA target")?;
            for (key,pattern) in [hero.background_key,hero.title_logo_key].into_iter().zip(patterns) {
                let key=key.ok_or("current image key missing")?;
                require(app.ui.has_image(key) && app.controller.view.artwork_bindings().iter().any(|binding|
                    binding.key==key && matches!(&binding.source,crate::presentation::ImageSource::Editorial(image) if image.url().path().rsplit('/').next()==Some(pattern.file()))),"exact current public source and admitted image")?;
            }
        } else {
            require(data.hero.is_none() && app.controller.view.artwork_bindings().is_empty() && app.ui.image_cache_len()==0,"unavailable slot has no invented image or CTA")?;
        }
        Ok(())
    })
}

#[test]
fn current_captured_first_frame_admits_complete_caption_vectors_and_decoded_images() {
    let mut fixture = Fixture::new(Stage::First);
    let frame = fixture.cpu_frame();
    state_oracle(&fixture, Stage::First).unwrap();
    let result = shape_oracle(&frame, Stage::First, &fixture.textures);
    fixture.dispose().unwrap();
    assert_eq!(result, Ok(()));
}

#[test]
fn capture_oracle_refuses_missing_caption_clipped_or_low_contrast_vector_and_stale_mesh() {
    let mut fixture = Fixture::new(Stage::First);
    let frame = fixture.cpu_frame();
    let mut absent = frame.clone();
    absent
        .shapes
        .retain(|s| !matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="Slide 1 of 7"));
    assert!(shape_oracle(&absent, Stage::First, &fixture.textures).is_err());
    let mut clipped = frame.clone();
    let area = Rect::from_center_size(pos2(443., 780.), vec2(40., 40.));
    let arrow = clipped
        .shapes
        .iter_mut()
        .find(|s| {
            matches!(s.shape, egui::Shape::Path(_))
                && area.contains_rect(s.shape.visual_bounding_rect())
        })
        .unwrap();
    arrow.clip_rect = arrow.shape.visual_bounding_rect().shrink(1.);
    assert!(shape_oracle(&clipped, Stage::First, &fixture.textures).is_err());
    let mut low_contrast = frame.clone();
    let arrow = low_contrast
        .shapes
        .iter_mut()
        .find(|s| {
            matches!(s.shape, egui::Shape::Path(_))
                && area.contains_rect(s.shape.visual_bounding_rect())
        })
        .unwrap();
    if let egui::Shape::Path(path) = &mut arrow.shape {
        path.stroke.color = egui::epaint::ColorMode::Solid(GOLD);
    }
    assert!(shape_oracle(&low_contrast, Stage::First, &fixture.textures).is_err());
    let mut stale = frame.clone();
    let id = fixture.textures[&Pattern::FirstBackdrop];
    let mesh = stale
        .shapes
        .iter_mut()
        .find_map(|s| {
            if let egui::Shape::Mesh(m) = &mut s.shape {
                (m.texture_id == id).then_some(m)
            } else {
                None
            }
        })
        .unwrap();
    Arc::make_mut(mesh).texture_id = TextureId::Managed(u64::MAX);
    assert!(shape_oracle(&stale, Stage::First, &fixture.textures).is_err());
    fixture.dispose().unwrap();
}

#[test]
fn actual_second_route_refusal_and_warm_back_keep_current_caption_source_and_two_reads() {
    let mut fixture = Fixture::new(Stage::First);
    let first = fixture.cpu_frame();
    for key in [RIGHT, RIGHT, SELECT] {
        fixture.cpu_key(key);
    }
    let second = fixture.cpu_frame();
    state_oracle(&fixture, Stage::Second).unwrap();
    shape_oracle(&second, Stage::Second, &fixture.textures).unwrap();
    assert!(
        shape_oracle(&first, Stage::Second, &fixture.textures).is_err(),
        "stale first frame must refuse"
    );
    let visit = cursor(fixture.app()).visit;
    for key in [LEFT, LEFT, SELECT] {
        fixture.cpu_key(key);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        fixture.pump();
        let error = fixture
            .app()
            .controller
            .view
            .with_view(fixture.app().authentication.view(), |data| {
                data.status == LoadState::Offline
            });
        if error {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "intentional Detail refusal must settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.app().ui.page(), Page::Detail);
    fixture.cpu_key(BACK);
    let back = fixture.cpu_frame();
    state_oracle(&fixture, Stage::WarmBack).unwrap();
    shape_oracle(&back, Stage::WarmBack, &fixture.textures).unwrap();
    assert_ne!(cursor(fixture.app()).visit, visit);
    assert_eq!(
        &*fixture.public.calls.lock().unwrap(),
        &["/", "/api/media/p753ts71"]
    );
    fixture.dispose().unwrap();
}

#[test]
fn duplicate_last_and_unavailable_slots_have_complete_actual_shape_bounds_without_reads() {
    for stage in [Stage::Duplicate, Stage::Unavailable] {
        let mut fixture = Fixture::new(stage);
        fixture.cpu_frame();
        if matches!(stage, Stage::Duplicate) {
            for key in [RIGHT, SELECT] {
                fixture.cpu_key(key);
            }
        } else {
            for key in [RIGHT, SELECT, SELECT, LEFT, SELECT] {
                fixture.cpu_key(key);
            }
        }
        let frame = fixture.cpu_frame();
        state_oracle(&fixture, stage).unwrap();
        shape_oracle(&frame, stage, &fixture.textures).unwrap();
        if matches!(stage, Stage::Duplicate) {
            for key in [RIGHT, SELECT] {
                fixture.cpu_key(key);
            }
            let frame = fixture.cpu_frame();
            assert_eq!(cursor(fixture.app()).index, 0);
            assert!(text_inside(
                &frame,
                "Slide 1 of 32",
                Rect::from_min_max(pos2(640., 750.), pos2(1440., 825.))
            ));
        }
        assert_eq!(&*fixture.public.calls.lock().unwrap(), &["/"]);
        fixture.dispose().unwrap();
    }
}

struct Captures {
    directory: PathBuf,
}
impl Captures {
    fn new() -> Self {
        let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/e2e");
        std::fs::create_dir_all(&parent).unwrap();
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = parent.join(format!("native-offline-hero-{}-{time}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        Self { directory }
    }
    fn complete(&self) -> bool {
        STAGES
            .iter()
            .all(|stage| self.directory.join(stage.name()).is_file())
    }
}
fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 3] {
    let offset = ((1079 - y) * 1920 + x) * 4;
    pixels[offset..offset + 3].try_into().unwrap()
}
fn count_pixels(pixels: &[u8], area: Rect, accept: impl Fn([u8; 3]) -> bool) -> usize {
    (area.top() as usize..area.bottom() as usize)
        .flat_map(|y| (area.left() as usize..area.right() as usize).map(move |x| (x, y)))
        .filter(|(x, y)| accept(pixel(pixels, *x, *y)))
        .count()
}
fn pixel_oracle(pixels: &[u8], stage: Stage) -> Result<(), &'static str> {
    require(
        pixels.len() == 1920 * 1080 * 4,
        "exact hero framebuffer allocation",
    )?;
    require(
        count_pixels(pixels, control(stage.focus()), |[r, g, b]| {
            r.abs_diff(181) < 5 && g.abs_diff(138) < 5 && b.abs_diff(22) < 5
        }) > 500,
        "current focus framebuffer",
    )?;
    for x in [443., 561.] {
        require(
            count_pixels(
                pixels,
                Rect::from_center_size(pos2(x, 780.), vec2(40., 40.)),
                |rgb| rgb.iter().all(|c| *c > 210),
            ) > 20,
            "white vector framebuffer",
        )?;
    }
    require(
        count_pixels(
            pixels,
            Rect::from_min_max(pos2(640., 750.), pos2(1050., 815.)),
            |rgb| rgb.iter().all(|c| *c > 210),
        ) > 100,
        "current caption framebuffer",
    )?;
    if stage.patterns().is_some() {
        let second = matches!(stage, Stage::Second | Stage::WarmBack);
        require(
            count_pixels(
                pixels,
                Rect::from_min_max(pos2(1500., 100.), pos2(1800., 400.)),
                |[r, _, b]| {
                    if second {
                        r > 150 && r.saturating_sub(b) > 100
                    } else {
                        b > 150 && b.saturating_sub(r) > 100
                    }
                },
            ) > 60_000,
            "current decoded backdrop framebuffer",
        )?;
        require(
            count_pixels(
                pixels,
                Rect::from_min_max(pos2(200., 340.), pos2(600., 500.)),
                |[r, g, b]| {
                    if second {
                        r > 120 && g > 70 && b < 60
                    } else {
                        g.saturating_sub(r) > 60 && g.saturating_sub(b) > 20
                    }
                },
            ) > 40_000,
            "current decoded logo framebuffer",
        )?;
    } else {
        require(
            count_pixels(
                pixels,
                Rect::from_min_max(pos2(150., 590.), pos2(600., 650.)),
                |rgb| rgb.iter().all(|c| (125..=180).contains(c)),
            ) > 100,
            "unavailable caption framebuffer",
        )?;
    }
    Ok(())
}
struct Rendered<'a> {
    fixture: &'a mut Fixture,
    display: &'a mut Display,
    captures: &'a Captures,
    start: Instant,
    maximum_reads: usize,
}
impl Rendered<'_> {
    fn bounded(&self) -> Result<(), &'static str> {
        require(
            self.start.elapsed() < Duration::from_secs(90),
            "whole public hero journey deadline",
        )?;
        public_only(self.fixture, self.maximum_reads)
    }
    fn paint(&mut self, stage: Option<Stage>) -> Result<(), &'static str> {
        self.bounded()?;
        let drawable = self
            .display
            .window
            .surface()
            .map_err(|_| "hero SDL surface")?
            .drawable;
        require(
            drawable.width == 1920 && drawable.height == 1080,
            "original hero drawable dimensions",
        )?;
        let Some(mut output) = self.fixture.app.as_mut().unwrap().take_output() else {
            return require(stage.is_none(), "fresh hero capture output");
        };
        self.fixture.observe(&output);
        if let Some(stage) = stage
            && let Err(error) = state_oracle(self.fixture, stage)
                .and_then(|()| shape_oracle(&output, stage, &self.fixture.textures))
        {
            output.textures_delta.clear();
            return Err(error);
        }
        if self
            .display
            .painter
            .paint([1920, 1080], self.fixture.app().context(), &mut output)
            .is_err()
        {
            output.textures_delta.clear();
            return Err("hero GLES paint");
        }
        if let Some(stage) = stage {
            let mut pixels = vec![0_u8; 1920 * 1080 * 4];
            // SAFETY: the helper's current main-thread context and exactRGBA8 allocation remain live.
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
            require(
                pixels.len() == 1920 * 1080 * 4,
                "exact hero framebuffer allocation",
            )?;
            let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(1920, 1080, pixels)
                .ok_or("hero framebuffer image")?;
            let flipped = image::imageops::flip_vertical(&image);
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.captures.directory.join(stage.name()))
                .map_err(|_| "fresh hero capture file")?;
            image::codecs::png::PngEncoder::new(file)
                .write_image(
                    flipped.as_raw(),
                    1920,
                    1080,
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|_| "hero capture encoding")?;
            // Preserve this exact original frame even when pixel admission fails.
            pixel_oracle(image.as_raw(), stage)?;
        }
        self.display
            .window
            .present()
            .map_err(|_| "hero GLES present")?;
        // SAFETY: the same current display context remains owned on this thread.
        require(
            unsafe { self.display.gl.get_error() } == glow::NO_ERROR,
            "hero GLES error",
        )
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
                .map_err(|_| "hero SDL event")?
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
                    .map_err(|_| "hero SDL surface")?,
                &self.fixture.runtime,
                self.start.elapsed(),
            );
            self.paint(None)?;
        }
        Err("hero SDL drain exceeds128events")
    }
    fn key(&mut self, (scan, code): (u32, i32)) -> Result<(), &'static str> {
        for pressed in [true, false] {
            push_key(scan, code, pressed);
            self.tick()?;
        }
        Ok(())
    }
    fn wait(&mut self, ready: impl Fn(&Fixture) -> bool) -> Result<(), &'static str> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.tick()?;
            if ready(self.fixture) {
                return Ok(());
            }
            require(
                Instant::now() < deadline,
                "offline hero rendered stage deadline",
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
    fn journey(&mut self, case: Stage) -> Result<(), &'static str> {
        self.wait(Fixture::ready)?;
        match case {
            Stage::First => {
                self.capture(Stage::First)?;
                for key in [RIGHT, RIGHT, SELECT] {
                    self.key(key)?;
                }
                self.wait(Fixture::ready)?;
                self.capture(Stage::Second)?;
                let visit = cursor(self.fixture.app()).visit;
                for key in [LEFT, LEFT, SELECT] {
                    self.key(key)?;
                }
                require(
                    self.fixture.app().ui.page() == Page::Detail,
                    "exact public CTA destination",
                )?;
                self.wait(|fixture| {
                    fixture
                        .app()
                        .controller
                        .view
                        .with_view(fixture.app().authentication.view(), |data| {
                            data.status == LoadState::Offline
                        })
                })?;
                self.key(BACK)?;
                self.wait(Fixture::ready)?;
                require(
                    cursor(self.fixture.app()).visit != visit,
                    "fresh warm Back visit",
                )?;
                self.capture(Stage::WarmBack)?;
            }
            Stage::Duplicate => {
                for key in [RIGHT, SELECT] {
                    self.key(key)?;
                }
                self.capture(Stage::Duplicate)?;
                for key in [RIGHT, SELECT] {
                    self.key(key)?;
                }
                require(
                    cursor(self.fixture.app()).index == 0
                        && self.fixture.app().ui.focus() == Focus::HeroNext,
                    "raw32 next wraps to0",
                )?;
                require(
                    self.fixture.app().controller.view.with_view(
                        self.fixture.app().authentication.view(),
                        |data| {
                            data.hero_carousel
                                .is_some_and(|c| c.caption == "Slide 1 of 32")
                        },
                    ),
                    "complete wrapped first caption",
                )?;
            }
            Stage::Unavailable => {
                for key in [RIGHT, SELECT, SELECT, LEFT, SELECT] {
                    self.key(key)?;
                }
                self.capture(Stage::Unavailable)?;
            }
            _ => return Err("hero journey must start at fixture origin"),
        }
        let expected = if matches!(case, Stage::First) {
            vec!["/", "/api/media/p753ts71"]
        } else {
            vec!["/"]
        };
        require(
            self.fixture
                .public
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(String::as_str)
                .eq(expected),
            "final exact public request census",
        )
    }
}
fn run_case(
    display: &mut Display,
    captures: &Captures,
    start: Instant,
    case: Stage,
) -> Result<(), &'static str> {
    let mut fixture = Fixture::new(case);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Rendered {
            fixture: &mut fixture,
            display,
            captures,
            start,
            maximum_reads: if matches!(case, Stage::First) { 2 } else { 1 },
        }
        .journey(case)
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
fn native_offline_public_hero_slots_current_cta_and_back_end_to_end() {
    crate::prepare_process();
    let captures = Captures::new();
    let mut display = Display::open("Criterion Unofficial Offline Public Hero E2E");
    let start = Instant::now();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for case in [Stage::First, Stage::Duplicate, Stage::Unavailable] {
            run_case(&mut display, &captures, start, case)?;
        }
        require(captures.complete(), "all five original hero frames")
    }));
    drop(display);
    match outcome {
        Ok(Ok(())) => {
            println!(
                "actual public hero SDL/GLES; captured discovery offline, all decoded pixels synthetic; selected Detail intentionally Offline; five original frames: {}",
                captures.directory.display()
            );
        }
        outcome => {
            // Runtime/loader/application and display are already disposed. At
            // most these five synthetic/public PNGs remain for Root inspection.
            eprintln!(
                "failed/unaccepted public hero journey; partial original frames retained: {}",
                captures.directory.display()
            );
            match outcome {
                Err(panic) => std::panic::resume_unwind(panic),
                Ok(result) => panic!("public hero SDL/GLES: {}", result.unwrap_err()),
            }
        }
    }
}
