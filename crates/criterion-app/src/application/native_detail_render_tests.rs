// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in hybrid: synthetic native origin, production anonymous Film/artwork.
//! No issuer, website catalog or subscriber request may leave this harness.
use super::*;
use crate::presentation::{ImageSource, Presentation};
use criterion_account::{AccountClient, MediaKind, MediaSummary, NativeDetail};
use criterion_platform::Size;
use criterion_provider::MediaId;
use criterion_ui::{Focus, GlowRenderer, LoginView, Page, Target};
use glow::HasContext;
use std::{
    ffi::{CString, c_void},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

// Independently recorded in tests/fixtures/provider/search.json; do not derive
// the expected title or requested identity from the production response.
const FILM_ID: &str = "qvwT6mJ4";
const FILM_TITLE: &str = "The Hitcher";
const SYNTHETIC_ORIGIN: &str = "Synthetic native provenance origin";

unsafe extern "C" {
    fn SDL_PushEvent(event: *mut c_void) -> i32;
}

#[derive(Default)]
struct Observed {
    bootstrap: AtomicUsize,
    detail: AtomicUsize,
    refused: AtomicUsize,
    website: AtomicUsize,
    issuer: AtomicUsize,
    active: AtomicUsize,
    maximum: AtomicUsize,
}
struct Flight(Arc<Observed>);
impl Drop for Flight {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Anonymous<T> {
    inner: T,
    observed: Arc<Observed>,
}
impl<T: criterion_account::Transport> criterion_account::Transport for Anonymous<T> {
    async fn send(
        &self,
        request: criterion_account::Request,
    ) -> Result<criterion_account::Response, criterion_account::Error> {
        // Match the closed request capability before invoking the real transport.
        // A synthetic subscriber credential can never enter its passthrough.
        let counter = match &request {
            criterion_account::Request::Bootstrap => &self.observed.bootstrap,
            criterion_account::Request::Detail { media_id, .. } if media_id.as_str() == FILM_ID => {
                &self.observed.detail
            }
            _ => {
                self.observed.refused.fetch_add(1, Ordering::SeqCst);
                return Err(criterion_account::Error::InvalidRequest);
            }
        };
        if counter.fetch_add(1, Ordering::SeqCst) != 0 {
            self.observed.refused.fetch_add(1, Ordering::SeqCst);
            return Err(criterion_account::Error::InvalidRequest);
        }
        let active = self.observed.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.observed.maximum.fetch_max(active, Ordering::SeqCst);
        let _flight = Flight(self.observed.clone());
        if active != 1 {
            self.observed.refused.fetch_add(1, Ordering::SeqCst);
            return Err(criterion_account::Error::InvalidRequest);
        }
        self.inner.send(request).await
    }
}

#[derive(Clone)]
struct Refused(Arc<Observed>);
impl RequestTransport for Refused {
    async fn get(
        &self,
        _: criterion_provider::Request,
    ) -> Result<criterion_provider::Response, criterion_provider::Error> {
        self.0.website.fetch_add(1, Ordering::SeqCst);
        Err(criterion_provider::Error::Unavailable)
    }
}
impl Transport for Refused {
    async fn post(
        &self,
        _: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        self.0.issuer.fetch_add(1, Ordering::SeqCst);
        Err(criterion_session::Error::Unavailable)
    }
}
type App = Application<Refused, Refused, SystemClock, Anonymous<criterion_account::HttpTransport>>;

fn require(condition: bool, failure: &'static str) -> Result<(), &'static str> {
    condition.then_some(()).ok_or(failure)
}
fn surface() -> Surface {
    let size = Size {
        width: 1920,
        height: 1080,
    };
    Surface {
        window: size,
        drawable: size,
    }
}
fn media(id: &str, title: &str, kind: MediaKind) -> MediaSummary {
    MediaSummary {
        id: MediaId::new(id).unwrap(),
        title: title.into(),
        kind,
        series_id: None,
        series_title: None,
        duration: None,
        release_date: None,
    }
}
fn synthetic_origin() -> NativeDetail {
    NativeDetail {
        media: media("Synth001", SYNTHETIC_ORIGIN, MediaKind::Collection),
        metadata: Default::default(),
        playlists: vec![criterion_account::NativePlaylist::Generic(
            criterion_account::NativeGenericPlaylist {
                title: SYNTHETIC_ORIGIN.into(),
                playlist_id: "Synthetic origin only".into(),
                key: Default::default(),
                children: vec![media(FILM_ID, "Synthetic selected Film", MediaKind::Film)],
                raw_child_count: 1,
            },
        )],
        featured: None,
        is_first_tab_sortable: None,
    }
}

// Create only a fresh owned directory. A failed run cannot erase prior evidence.
struct Captures {
    directory: PathBuf,
    keep: bool,
}
impl Captures {
    fn new() -> Result<Self, &'static str> {
        let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/e2e");
        std::fs::create_dir_all(&parent).map_err(|_| "capture parent directory")?;
        let directory = parent.join(format!("native-hybrid-film-{}", std::process::id()));
        std::fs::create_dir(&directory).map_err(|_| "fresh owned capture directory")?;
        Ok(Self {
            directory,
            keep: false,
        })
    }
    fn remove(&self) -> Result<(), &'static str> {
        for name in [
            "anonymous-film.png",
            "anonymous-information.png",
            "synthetic-origin-back.png",
        ] {
            let path = self.directory.join(name);
            if path.exists() {
                std::fs::remove_file(path).map_err(|_| "owned failed capture removal")?;
            }
        }
        std::fs::remove_dir(&self.directory).map_err(|_| "owned failed capture directory removal")
    }
}
impl Drop for Captures {
    fn drop(&mut self) {
        if !self.keep && self.directory.exists() {
            let _ = self.remove();
        }
    }
}

#[derive(Clone, Copy)]
enum Capture {
    Film,
    Information,
    Origin,
}
struct Live<'a> {
    app: &'a mut App,
    runtime: &'a Runtime,
    window: &'a mut criterion_platform::Window,
    painter: &'a mut GlowRenderer,
    gl: &'a glow::Context,
    observed: &'a Observed,
    captures: &'a Captures,
    start: Instant,
}
impl Live<'_> {
    fn safe(&self) -> Result<(), &'static str> {
        require(
            self.start.elapsed() < Duration::from_secs(60),
            "whole hybrid journey deadline",
        )?;
        require(
            matches!(self.app.authentication.view(), LoginView::SignedOut),
            "anonymous session changed",
        )?;
        require(
            self.observed.issuer.load(Ordering::SeqCst) == 0
                && self.observed.refused.load(Ordering::SeqCst) == 0
                && self.observed.website.load(Ordering::SeqCst) <= 1,
            "unexpected issuer, subscriber, extra native or website request",
        )
    }
    fn native_ready(&self) -> bool {
        self.app
            .controller
            .view
            .with_view(LoginView::SignedOut, |data| {
                data.status == LoadState::Ready
                    && data.detail.as_ref().is_some_and(|detail| {
                        detail.kind == criterion_ui::DetailKind::Film
                            && detail.card.key == &Target::Native(MediaId::new(FILM_ID).unwrap())
                            && detail.card.title == FILM_TITLE
                            && detail
                                .primary_playback_target
                                .is_some_and(|id| id.as_str() == FILM_ID)
                            && !detail.description.is_empty()
                            && detail
                                .card
                                .artwork_key
                                .is_some_and(|key| self.app.ui.image_bytes(key).is_some()
                                    && self.app.controller.view.artwork_bindings().iter().any(|binding| {
                                        binding.key == key && matches!(&binding.source,
                                            ImageSource::Media { id, label: criterion_provider::ImageLabel::Landscape,
                                                role: criterion_artwork::ImageRole::Backdrop }
                                            if id.as_str() == FILM_ID)
                                    }))
                    })
            })
    }
    fn paint(&mut self, capture: Option<Capture>) -> Result<(), &'static str> {
        self.safe()?;
        let Some(mut output) = self.app.take_output() else {
            return require(capture.is_none(), "capture output absent");
        };
        let size = self.window.surface().map_err(|_| "SDL surface")?.drawable;
        require(
            size.width == 1920 && size.height == 1080,
            "original full frame size",
        )?;
        if let Some(stage) = capture {
            let expected = match stage {
                Capture::Origin => SYNTHETIC_ORIGIN,
                _ => FILM_TITLE,
            };
            require(output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == expected)
            }), "independent exact title absent from actual painted shapes")?;
            let rect = match stage {
                Capture::Origin => egui::Rect::from_min_size(
                    egui::pos2(150.0, 324.0),
                    egui::vec2(378.0, 378.0 * 9.0 / 16.0),
                ),
                _ => egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0)),
            };
            require(
                output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Mesh(mesh)
                    if mesh.texture_id != egui::TextureId::default() && mesh.calc_bounds() == rect)
                }),
                "owning artwork texture absent from actual painted shapes",
            )?;
            if !matches!(stage, Capture::Origin) {
                require(
                    self.native_ready() && self.app.ui.page() == Page::Detail,
                    "capture is not the admitted exact native Film",
                )?;
                require(output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "WATCH NOW")
                }), "native Film primary action absent")?;
                if matches!(stage, Capture::Information) {
                    for label in ["Starring", "Countries", "Languages"] {
                        require(
                            output.shapes.iter().any(|shape| {
                                matches!(&shape.shape, egui::Shape::Text(text)
                                if text.galley.job.text == label)
                            }),
                            "native Information metadata headings absent",
                        )?;
                    }
                }
            }
        }
        self.painter
            .paint([size.width, size.height], self.app.context(), &mut output)
            .map_err(|_| "GLES paint")?;
        if let Some(stage) = capture {
            let mut pixels = vec![0_u8; 1920 * 1080 * 4];
            // SAFETY: main-thread current SDL context; exact RGBA8 allocation,
            // read the newly painted back buffer before its presentation.
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
            let (name, focus, region) = match stage {
                Capture::Film => (
                    "anonymous-film.png",
                    Focus::DetailAction(0),
                    (150..610, 620..700),
                ),
                Capture::Information => (
                    "anonymous-information.png",
                    Focus::InformationPrimary,
                    (348..1572, 903..983),
                ),
                Capture::Origin => (
                    "synthetic-origin-back.png",
                    Focus::Card { row: 0, column: 0 },
                    (142..536, 316..545),
                ),
            };
            require(
                self.app.ui.focus() == focus && gold_pixels(&pixels, region.0, region.1) > 200,
                "canonical focus absent from actual framebuffer",
            )?;
            if !matches!(stage, Capture::Information) {
                let region = if matches!(stage, Capture::Origin) {
                    (156..520, 330..528)
                } else {
                    (1360..1840, 64..240)
                };
                require(
                    artwork_contrast(&pixels, region.0, region.1),
                    "owning artwork interior is spatially uniform in framebuffer",
                )?;
            }
            let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(1920, 1080, pixels)
                .ok_or("full frame allocation")?;
            image::imageops::flip_vertical(&image)
                .save(self.captures.directory.join(name))
                .map_err(|_| "full frame capture save")?;
        }
        // SAFETY: same current context; all GL operations occur on this thread.
        require(
            unsafe { self.gl.get_error() } == glow::NO_ERROR,
            "GLES error",
        )?;
        self.window.present().map_err(|_| "SDL present")
    }
    fn tick(&mut self) -> Result<(), &'static str> {
        self.app.poll(self.runtime, true);
        for _ in 0..128 {
            let Some(event) = self.window.poll_event().map_err(|_| "SDL poll")? else {
                self.app.consume(self.runtime, self.start.elapsed());
                return self.paint(None);
            };
            self.app.event(
                event,
                self.window.surface().map_err(|_| "SDL surface")?,
                self.runtime,
                self.start.elapsed(),
            );
            self.paint(None)?;
        }
        Err("SDL event drain exceeded bound")
    }
    fn wait(&mut self, predicate: impl Fn(&Self) -> bool) -> Result<(), &'static str> {
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            self.tick()?;
            require(Instant::now() < deadline, "hybrid stage deadline")?;
            if predicate(self) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn key(&mut self, scan: u32, key: i32) -> Result<(), &'static str> {
        for pressed in [true, false] {
            let mut raw = [0_u8; 56];
            raw[..4].copy_from_slice(&(if pressed { 0x300_u32 } else { 0x301 }).to_le_bytes());
            raw[12] = u8::from(pressed);
            raw[16..20].copy_from_slice(&scan.to_le_bytes());
            raw[20..24].copy_from_slice(&key.to_le_bytes());
            let mut aligned = [0_u64; 7];
            for (word, bytes) in aligned.iter_mut().zip(raw.as_chunks::<8>().0) {
                *word = u64::from_le_bytes(*bytes);
            }
            // SAFETY: initialized stock desktop SDL_Event is 56 bytes/aligned8;
            // synchronous SDL copying keeps every native pointer on this thread.
            require(
                unsafe { SDL_PushEvent(aligned.as_mut_ptr().cast::<c_void>()) } == 1,
                "SDL key injection",
            )?;
            self.tick()?;
        }
        Ok(())
    }
    fn capture(&mut self, stage: Capture) -> Result<(), &'static str> {
        self.app.consume(self.runtime, self.start.elapsed());
        self.paint(Some(stage))
    }
    fn journey(&mut self) -> Result<(), &'static str> {
        self.wait(|live| {
            live.app
                .controller
                .view
                .with_view(LoginView::SignedOut, |data| {
                    data.status == LoadState::Offline
                })
        })?;
        // Replace only the settled offline origin. This seed is never a live
        // My List/account/discovery response and makes no provider acceptance.
        self.app.controller.view = Presentation::native_detail(synthetic_origin(), None)
            .map_err(|_| "synthetic origin projection")?;
        self.tick()?;
        self.key(81, 1_073_741_905)?;
        require(
            self.app.ui.focus() == Focus::Card { row: 0, column: 0 },
            "synthetic origin native card focus",
        )?;
        let scroll = self.app.ui.scroll_y();
        self.key(40, 13)?;
        self.wait(Self::native_ready)?;
        self.capture(Capture::Film)?;
        self.key(79, 1_073_741_903)?;
        self.key(40, 13)?;
        self.capture(Capture::Information)?;
        self.key(41, 27)?;
        require(
            self.app.ui.page() == Page::Detail
                && self.app.ui.focus() == Focus::DetailAction(1)
                && self.native_ready(),
            "Information Back must restore native Detail focus",
        )?;
        self.key(41, 27)?;
        self.wait(|live| {
            live.app
                .controller
                .view
                .with_view(LoginView::SignedOut, |data| {
                    data.status == LoadState::Ready
                        && data
                            .detail
                            .as_ref()
                            .is_some_and(|detail| detail.card.title == SYNTHETIC_ORIGIN)
                        && data.rails.len() == 1
                        && data.rails[0].cards.len() == 1
                        && data.rails[0].cards[0].key
                            == &Target::Native(MediaId::new(FILM_ID).unwrap())
                        && data.rails[0].cards[0].title == "Synthetic selected Film"
                        && data.rails[0].cards[0]
                            .artwork_key
                            .is_some_and(|key| live.app.ui.image_bytes(key).is_some())
                })
        })?;
        require(
            self.app.ui.page() == Page::Home && self.app.ui.scroll_y() == scroll,
            "Back must restore exact warm synthetic origin and scroll",
        )?;
        self.capture(Capture::Origin)?;
        require(
            self.observed.bootstrap.load(Ordering::SeqCst) == 1
                && self.observed.detail.load(Ordering::SeqCst) == 1
                && self.observed.website.load(Ordering::SeqCst) == 1
                && self.observed.maximum.load(Ordering::SeqCst) == 1,
            "exact single anonymous native read without website fallback or warm reread",
        )
    }
}
fn gold_pixels(pixels: &[u8], x: std::ops::Range<usize>, y: std::ops::Range<usize>) -> usize {
    y.flat_map(|y| x.clone().map(move |x| ((1079 - y) * 1920 + x) * 4))
        .filter(|&at| pixels[at] > 120 && pixels[at + 1] > 80 && pixels[at + 2] < 70)
        .count()
}
fn artwork_contrast(pixels: &[u8], x: std::ops::Range<usize>, y: std::ops::Range<usize>) -> bool {
    let (mut low, mut high) = (u16::MAX, 0);
    for y in y {
        for x in x.clone() {
            let at = ((1079 - y) * 1920 + x) * 4;
            let value =
                u16::from(pixels[at]) + u16::from(pixels[at + 1]) + u16::from(pixels[at + 2]);
            low = low.min(value);
            high = high.max(value);
        }
    }
    high.saturating_sub(low) >= 48
}

#[test]
fn hybrid_frame_oracles_refuse_uniform_artwork_and_missing_focus() {
    let blank = vec![17; 1920 * 1080 * 4];
    assert!(!artwork_contrast(&blank, 1360..1840, 64..240));
    assert_eq!(gold_pixels(&blank, 150..610, 620..700), 0);
}

#[derive(Clone, Default)]
struct Probe(Arc<AtomicUsize>);
impl criterion_account::Transport for Probe {
    async fn send(
        &self,
        request: criterion_account::Request,
    ) -> Result<criterion_account::Response, criterion_account::Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        match request {
            criterion_account::Request::Bootstrap => Ok(criterion_account::Response {
                status: 200,
                body: criterion_account::SecretBody::new(br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()),
            }),
            _ => Err(criterion_account::Error::Unavailable),
        }
    }
}

#[tokio::test]
async fn hybrid_gate_refuses_wrong_native_identity_before_inner_contact() {
    let probe = Probe::default();
    let observed = Arc::new(Observed::default());
    let client = AccountClient::with_transport(Anonymous {
        inner: probe.clone(),
        observed: observed.clone(),
    });
    client.bootstrap().await.unwrap();
    assert_eq!(
        client.detail(&MediaId::new("Other001").unwrap()).await,
        Err(criterion_account::Error::InvalidRequest)
    );
    assert_eq!(probe.0.load(Ordering::SeqCst), 1);
    assert_eq!(observed.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(observed.detail.load(Ordering::SeqCst), 0);
    assert_eq!(observed.refused.load(Ordering::SeqCst), 1);
    assert_eq!(observed.active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn hybrid_gate_permits_only_one_bootstrap_and_one_exact_native_detail() {
    let probe = Probe::default();
    let observed = Arc::new(Observed::default());
    let gate = Anonymous {
        inner: probe.clone(),
        observed: observed.clone(),
    };
    let client = AccountClient::with_transport(gate);
    client.bootstrap().await.unwrap();
    assert_eq!(
        client.detail(&MediaId::new(FILM_ID).unwrap()).await,
        Err(criterion_account::Error::Unavailable)
    );
    assert_eq!(
        client.detail(&MediaId::new(FILM_ID).unwrap()).await,
        Err(criterion_account::Error::InvalidRequest)
    );
    assert_eq!(probe.0.load(Ordering::SeqCst), 2);
    assert_eq!(observed.bootstrap.load(Ordering::SeqCst), 1);
    assert_eq!(observed.detail.load(Ordering::SeqCst), 2);
    assert_eq!(observed.refused.load(Ordering::SeqCst), 1);
    assert_eq!(observed.active.load(Ordering::SeqCst), 0);
    assert_eq!(observed.maximum.load(Ordering::SeqCst), 1);
}

#[test]
#[ignore = "hybrid synthetic native origin + live anonymous Film/artwork; serialized Root SDL/GLES executor"]
fn native_hybrid_anonymous_film_information_and_back_end_to_end() {
    super::super::prepare_process();
    let captures = Captures::new().unwrap();
    let mut window =
        criterion_platform::Window::open("Criterion Unofficial Hybrid Native Film E2E").unwrap();
    // SAFETY: this thread owns the current SDL context until explicit destruction.
    let gl = Arc::new(unsafe {
        glow::Context::from_loader_function(|name| {
            window.gl_proc_address(&CString::new(name).unwrap())
        })
    });
    let mut painter = unsafe { GlowRenderer::new(gl.clone()) }.unwrap();
    let observed = Arc::new(Observed::default());
    let refused = Refused(observed.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let clock = SystemClock::default();
    let session = Arc::new(criterion_session::Session::with_transport(
        criterion_session::Configuration::production(),
        refused.clone(),
        clock.clone(),
    ));
    let mut app = Application::with_parts(
        surface(),
        Controller::new(Catalog::with_transport(refused.clone()), runtime.handle()),
        Authentication::with_session(session.clone(), clock),
        Accounts::from_parts(
            Arc::new(AccountClient::with_transport(Anonymous {
                inner: criterion_account::HttpTransport::new().unwrap(),
                observed: observed.clone(),
            })),
            session,
        ),
        Artwork::new().unwrap(),
    );
    let mut captures = captures;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Live {
            app: &mut app,
            runtime: &runtime,
            window: &mut window,
            painter: &mut painter,
            gl: &gl,
            observed: &observed,
            captures: &captures,
            start: Instant::now(),
        }
        .journey()
    }));
    // Cleanup runs with the current context on both error and panic paths.
    let disposal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.background();
        app.finish(&runtime)
    }));
    drop(app);
    runtime.shutdown_timeout(Duration::from_secs(3));
    painter.destroy();
    let clean = matches!(disposal, Ok(true)) && observed.active.load(Ordering::SeqCst) == 0;
    match outcome {
        Ok(Ok(())) if clean => {
            captures.keep = true;
            println!(
                "hybrid synthetic native origin; actual anonymous native Film, current artwork, Information/Back; full frames: {}",
                captures.directory.display()
            );
        }
        Ok(result) => {
            captures.remove().expect("failed hybrid capture cleanup");
            assert!(clean, "hybrid application/transport cleanup did not settle");
            panic!("hybrid native Film journey: {}", result.unwrap_err());
        }
        Err(panic) => {
            captures.remove().expect("panicking hybrid capture cleanup");
            assert!(clean, "panicking hybrid cleanup did not settle");
            std::panic::resume_unwind(panic);
        }
    }
}
