// SPDX-License-Identifier: GPL-3.0-or-later
//! Main-thread public artwork ownership; runtime shutdown settles aborted work.
//! Callers use Presentation's immutable source keys and update current bindings
//! before poll. Aborted handles retain capacity until joined; final Drop aborts
//! without blocking and the runtime owns settling detached blocking decodes.
use crate::presentation::{ImageBinding, ImageSource};
use criterion_artwork::{ArtworkError, ArtworkLoader, ArtworkSource, DecodedArtwork, ImageRole};
use criterion_ui::AppUi;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

type ImageFuture = Pin<Box<dyn Future<Output = Result<DecodedArtwork, ArtworkError>> + Send>>;
type LoadImage = Arc<dyn Fn(ArtworkSource) -> ImageFuture + Send + Sync>;
const MAX_VISIBLE: usize = 32;
const MAX_TASKS: usize = 2;
const MAX_CARD_BYTES: usize = 4 * 1024 * 1024;
const MAX_BACKDROP_BYTES: usize = 8 * 1024 * 1024;
const WORKING_BYTES: usize = 24 * 1024 * 1024;

pub(crate) struct Artwork {
    load: LoadImage,
    requests: Vec<Request>,
    jobs: Vec<Job>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Attempt {
    Idle,
    Running,
    Deferred,
    Failed,
}
struct Request {
    key: String,
    source: Option<ArtworkSource>,
    attempt: Attempt,
    bytes: Option<usize>,
    selected: bool,
    invalidate_cache: bool,
    busy_retry_used: bool,
}
struct Job {
    key: String,
    source: ArtworkSource,
    retiring: bool,
    retry: bool,
    task: JoinHandle<Result<DecodedArtwork, ArtworkError>>,
}

impl Artwork {
    pub(crate) fn new() -> Result<Self, ArtworkError> {
        let loader = Arc::new(ArtworkLoader::new()?);
        Ok(Self::with_loader(Arc::new(move |source| {
            let loader = loader.clone();
            Box::pin(async move { loader.load(&source).await })
        })))
    }
    pub(crate) fn with_loader(load: LoadImage) -> Self {
        Self {
            load,
            requests: Vec::new(),
            jobs: Vec::new(),
        }
    }
    #[cfg(test)]
    pub(crate) fn offline() -> Self {
        Self::with_loader(Arc::new(|_| {
            Box::pin(async { Err(ArtworkError::Unavailable) })
        }))
    }
    pub(crate) fn update(&mut self, visible: &[String], bindings: &[ImageBinding]) {
        let mut previous = std::mem::take(&mut self.requests);
        for key in visible {
            if self.requests.len() >= MAX_VISIBLE {
                break;
            }
            if key.is_empty()
                || key.len() > 128
                || key.chars().any(char::is_control)
                || self.requests.iter().any(|request| request.key == *key)
            {
                continue;
            }
            let source = bindings
                .iter()
                .find(|binding| binding.key == *key)
                .filter(|first| {
                    !bindings
                        .iter()
                        .any(|binding| binding.key == *key && binding.source != first.source)
                })
                .map(|binding| match &binding.source {
                    ImageSource::Media { id, label, role } => ArtworkSource::Media {
                        id: id.clone(),
                        label: *label,
                        role: *role,
                    },
                    ImageSource::Editorial(image) => ArtworkSource::Editorial(image.clone()),
                });
            let old = previous
                .iter()
                .position(|request| request.key == *key)
                .map(|index| previous.remove(index));
            let changed = old.as_ref().is_some_and(|request| request.source != source);
            if let Some(old) = old.filter(|request| request.source == source) {
                self.requests.push(old);
            } else {
                self.requests.push(Request {
                    key: key.clone(),
                    source,
                    attempt: Attempt::Idle,
                    bytes: None,
                    selected: false,
                    invalidate_cache: changed,
                    busy_retry_used: false,
                });
            }
        }
        self.select();
        self.retire();
    }
    fn select_with_cache(&mut self, ui: &AppUi) {
        for request in &mut self.requests {
            request.bytes = ui.image_bytes(&request.key);
        }
        self.select();
    }
    fn select(&mut self) {
        let mut charged = 0;
        for request in &mut self.requests {
            let bytes = request.bytes.unwrap_or_else(|| {
                match request.source.as_ref().map(ArtworkSource::role) {
                    Some(ImageRole::Backdrop) => MAX_BACKDROP_BYTES,
                    _ => MAX_CARD_BYTES,
                }
            });
            request.selected = request.source.is_some()
                && request.attempt != Attempt::Failed
                && bytes <= WORKING_BYTES - charged;
            if request.selected {
                charged += bytes;
            }
        }
    }
    fn retire(&mut self) {
        for job in &mut self.jobs {
            if !job.retiring
                && !self.requests.iter().any(|request| {
                    request.selected
                        && request.key == job.key
                        && request.source.as_ref() == Some(&job.source)
                })
            {
                job.retiring = true;
                job.task.abort();
                if let Some(request) = self.requests.iter_mut().find(|request| {
                    request.key == job.key && request.source.as_ref() == Some(&job.source)
                }) {
                    request.attempt = Attempt::Idle;
                }
            }
        }
    }
    pub(crate) fn poll(&mut self, runtime: &Runtime, ui: &mut AppUi) {
        for request in &mut self.requests {
            if request.invalidate_cache {
                ui.discard_image(&request.key);
                request.invalidate_cache = false;
            }
        }
        self.select_with_cache(ui);
        self.retire();
        let mut index = 0;
        while index < self.jobs.len() {
            if self.jobs[index].task.is_finished() {
                let job = self.jobs.remove(index);
                let result = runtime.block_on(job.task);
                if !job.retiring
                    && let Some(request) = self.requests.iter_mut().find(|request| {
                        request.selected
                            && request.key == job.key
                            && request.source.as_ref() == Some(&job.source)
                    })
                {
                    request.attempt = Attempt::Failed;
                    if let Ok(Ok(image)) = result {
                        let bytes = image.rgba().len();
                        let color = egui::ColorImage::from_rgba_unmultiplied(
                            image.dimensions(),
                            image.rgba(),
                        );
                        if ui.admit_image(&job.key, color).is_ok() {
                            request.bytes = Some(bytes);
                            request.attempt = Attempt::Idle;
                        }
                    } else if matches!(result, Ok(Err(ArtworkError::Busy)))
                        && !request.busy_retry_used
                    {
                        request.busy_retry_used = true;
                        request.attempt = Attempt::Deferred;
                    }
                }
            } else {
                index += 1;
            }
        }
        self.select_with_cache(ui);
        self.retire();
        for request in &self.requests {
            if !request.selected {
                ui.discard_image(&request.key);
            }
        }
        if let Some(request) = self
            .requests
            .iter_mut()
            .find(|request| request.selected && request.attempt == Attempt::Deferred)
        {
            if self.jobs.is_empty()
                && let Some(source) = &request.source
            {
                request.attempt = Attempt::Running;
                self.jobs.push(Job {
                    key: request.key.clone(),
                    source: source.clone(),
                    retiring: false,
                    retry: true,
                    task: runtime.spawn((self.load)(source.clone())),
                });
            }
            return;
        }
        if self.jobs.iter().any(|job| job.retry) {
            return;
        }
        for request in &mut self.requests {
            if self.jobs.len() >= MAX_TASKS {
                break;
            }
            if request.selected && request.attempt == Attempt::Idle && !ui.has_image(&request.key) {
                let Some(source) = &request.source else {
                    continue;
                };
                request.attempt = Attempt::Running;
                self.jobs.push(Job {
                    key: request.key.clone(),
                    source: source.clone(),
                    retiring: false,
                    retry: false,
                    task: runtime.spawn((self.load)(source.clone())),
                });
            }
        }
    }
    pub(crate) fn clear(&mut self) {
        self.requests.clear();
        for job in &mut self.jobs {
            job.retiring = true;
            job.task.abort();
        }
    }
}
impl Drop for Artwork {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use criterion_artwork::decode_artwork;
    use criterion_provider::{ImageLabel, MediaId};
    use std::time::{Duration, Instant};

    fn binding(key: &str) -> ImageBinding {
        ImageBinding {
            key: key.to_owned(),
            source: ImageSource::Media {
                id: MediaId::new("qvwT6mJ4").unwrap(),
                label: ImageLabel::Landscape,
                role: criterion_artwork::ImageRole::Card,
            },
        }
    }
    fn pixels() -> Result<DecodedArtwork, ArtworkError> {
        decode_artwork(
            ImageRole::Card,
            "image/png",
            include_bytes!("../../criterion-artwork/tests/fixtures/two-pixels.png"),
        )
    }
    fn wait_until(mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready() {
            assert!(Instant::now() < deadline, "artwork fixture did not settle");
            std::thread::yield_now();
        }
    }
    fn wait_for_job(artwork: &Artwork, key: &str) {
        assert!(artwork.jobs.iter().any(|job| job.key == key));
        wait_until(|| {
            artwork
                .jobs
                .iter()
                .any(|job| job.key == key && job.task.is_finished())
        });
    }
    fn settle_jobs(artwork: &mut Artwork, runtime: &Runtime, ui: &mut AppUi) {
        wait_until(|| {
            artwork.poll(runtime, ui);
            artwork.jobs.is_empty()
        });
    }
    fn settle(artwork: &mut Artwork, runtime: &Runtime, ui: &mut AppUi, count: usize) {
        wait_until(|| {
            artwork.poll(runtime, ui);
            ui.image_cache_len() >= count
        });
        assert_eq!(ui.image_cache_len(), count);
    }
    #[test]
    fn selected_provider_pixels_are_admitted_on_the_main_thread() {
        let runtime = Runtime::new().unwrap();
        let mut ui = AppUi::new();
        let mut artwork = Artwork::with_loader(Arc::new(|_| Box::pin(async { pixels() })));
        artwork.update(&["one".to_owned()], &[binding("one")]);
        settle(&mut artwork, &runtime, &mut ui, 1);
        assert_eq!(ui.image_cache_bytes(), 8);
    }
    #[test]
    fn at_most_two_loads_start_even_when_many_visible_images_wait() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        }));
        let visible: Vec<_> = (0..35).map(|index| format!("key{index}")).collect();
        let bindings: Vec<_> = visible.iter().map(|key| binding(key)).collect();
        artwork.update(&visible, &bindings);
        artwork.poll(&runtime, &mut AppUi::new());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn continuous_visible_failure_is_attempted_once_while_later_keys_can_progress() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                // Rejection may finish after many caller yields, even without I/O.
                tokio::time::sleep(Duration::from_millis(20)).await;
                Err(ArtworkError::Unavailable)
            })
        }));
        let visible = vec!["one".to_owned(), "two".to_owned(), "three".to_owned()];
        let bindings: Vec<_> = visible.iter().map(|key| binding(key)).collect();
        let mut ui = AppUi::new();
        wait_until(|| {
            artwork.update(&visible, &bindings);
            artwork.poll(&runtime, &mut ui);
            artwork.jobs.is_empty()
        });
        artwork.update(&visible, &bindings);
        settle_jobs(&mut artwork, &runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(ui.image_cache_len(), 0);
    }
    #[test]
    fn a_finished_removed_result_never_enters_the_image_cache() {
        let runtime = Runtime::new().unwrap();
        let mut artwork = Artwork::with_loader(Arc::new(|_| Box::pin(async { pixels() })));
        let mut ui = AppUi::new();
        artwork.update(&["old".to_owned()], &[binding("old")]);
        artwork.poll(&runtime, &mut ui);
        wait_for_job(&artwork, "old");
        artwork.update(&[], &[]);
        settle_jobs(&mut artwork, &runtime, &mut ui);
        assert_eq!(ui.image_cache_len(), 0);
    }
    #[test]
    fn clear_keeps_aborting_tasks_in_capacity_until_their_destructors_settle() {
        use std::sync::{
            Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        struct HoldDrop {
            gate: Arc<(Mutex<bool>, Condvar)>,
            dropping: Arc<AtomicUsize>,
        }
        impl Drop for HoldDrop {
            fn drop(&mut self) {
                self.dropping.fetch_add(1, Ordering::SeqCst);
                let (lock, wake) = &*self.gate;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = wake.wait(released).unwrap();
                }
            }
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(AtomicUsize::new(0));
        let dropping = Arc::new(AtomicUsize::new(0));
        let (load_calls, load_entered, load_dropping, load_gate) = (
            calls.clone(),
            entered.clone(),
            dropping.clone(),
            gate.clone(),
        );
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            let index = load_calls.fetch_add(1, Ordering::SeqCst);
            let (entered, dropping, gate) = (
                load_entered.clone(),
                load_dropping.clone(),
                load_gate.clone(),
            );
            Box::pin(async move {
                if index < 2 {
                    let _guard = HoldDrop { gate, dropping };
                    entered.fetch_add(1, Ordering::SeqCst);
                    std::future::pending().await
                } else {
                    pixels()
                }
            })
        }));
        let mut ui = AppUi::new();
        artwork.update(
            &["old1".to_owned(), "old2".to_owned()],
            &[binding("old1"), binding("old2")],
        );
        artwork.poll(&runtime, &mut ui);
        let deadline = Instant::now() + Duration::from_secs(2);
        while entered.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        artwork.clear();
        let deadline = Instant::now() + Duration::from_secs(2);
        while dropping.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        artwork.update(
            &["new1".to_owned(), "new2".to_owned()],
            &[binding("new1"), binding("new2")],
        );
        artwork.poll(&runtime, &mut ui);
        let observed = calls.load(Ordering::SeqCst);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert_eq!(entered.load(Ordering::SeqCst), 2);
        assert_eq!(dropping.load(Ordering::SeqCst), 2);
        assert_eq!(observed, 2);
        settle(&mut artwork, &runtime, &mut ui, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }
    #[test]
    fn only_the_first_thirty_two_distinct_visible_keys_are_tracked() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { pixels() })
        }));
        let visible: Vec<_> = (0..35).map(|index| format!("key{index}")).collect();
        let bindings: Vec<_> = visible.iter().map(|key| binding(key)).collect();
        let mut ui = AppUi::new();
        artwork.update(&visible, &bindings);
        settle(&mut artwork, &runtime, &mut ui, 32);
        settle_jobs(&mut artwork, &runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 32);
    }
    #[test]
    fn invalid_duplicate_and_ambiguous_keys_never_start_a_load() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        }));
        let invalid = "x".repeat(129);
        let mut other = binding("conflict");
        other.source = ImageSource::Media {
            id: MediaId::new("AbcD1234").unwrap(),
            label: ImageLabel::Landscape,
            role: criterion_artwork::ImageRole::Card,
        };
        let bindings = vec![
            binding("one"),
            binding("conflict"),
            other,
            binding(&invalid),
            binding("bad\nkey"),
            binding(""),
        ];
        artwork.update(
            &[
                "one".to_owned(),
                "one".to_owned(),
                "conflict".to_owned(),
                invalid,
                "bad\nkey".to_owned(),
                String::new(),
                "missing".to_owned(),
            ],
            &bindings,
        );
        artwork.poll(&runtime, &mut AppUi::new());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn maximum_images_choose_a_stable_priority_working_set_without_cache_thrash() {
        use image::ImageEncoder;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(
                &vec![255; 1024 * 1024 * 4],
                1024,
                1024,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let encoded = Arc::new(encoded);
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            let encoded = encoded.clone();
            Box::pin(async move { decode_artwork(ImageRole::Card, "image/png", &encoded) })
        }));
        let runtime = Runtime::new().unwrap();
        let visible: Vec<_> = (0..32).map(|index| format!("key{index}")).collect();
        let bindings: Vec<_> = visible.iter().map(|key| binding(key)).collect();
        let mut ui = AppUi::new();
        artwork.update(&visible, &bindings);
        settle(&mut artwork, &runtime, &mut ui, 6);
        artwork.update(&visible, &bindings);
        settle_jobs(&mut artwork, &runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 6);
        assert_eq!(ui.image_cache_bytes(), 24 * 1024 * 1024);
        assert!((0..6).all(|index| ui.has_image(&format!("key{index}"))));
        let reversed: Vec<_> = visible.iter().rev().cloned().collect();
        artwork.update(&reversed, &bindings);
        artwork.poll(&runtime, &mut ui);
        settle(&mut artwork, &runtime, &mut ui, 6);
        assert_eq!(calls.load(Ordering::SeqCst), 12);
        assert!((26..32).all(|index| ui.has_image(&format!("key{index}"))));
        assert!(!ui.has_image("key0"));
    }
    #[test]
    fn returning_cached_image_avoids_a_second_load() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        }));
        let mut ui = AppUi::new();
        ui.admit_image(
            "one",
            egui::ColorImage::filled([2, 1], egui::Color32::WHITE),
        )
        .unwrap();
        artwork.update(&["one".to_owned()], &[binding("one")]);
        artwork.poll(&runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(ui.image_cache_len(), 1);
    }
    #[test]
    fn cached_thumbnail_costs_allow_more_than_six_visible_cache_hits() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        }));
        let mut ui = AppUi::new();
        let visible: Vec<_> = (0..12).map(|index| format!("key{index}")).collect();
        for key in &visible {
            ui.admit_image(key, egui::ColorImage::filled([2, 1], egui::Color32::WHITE))
                .unwrap();
        }
        let bindings: Vec<_> = visible.iter().map(|key| binding(key)).collect();
        artwork.update(&visible, &bindings);
        artwork.poll(&runtime, &mut ui);
        assert_eq!(ui.image_cache_len(), 12);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn busy_decode_retries_once_after_other_jobs_settle() {
        use std::sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        let runtime = Runtime::new().unwrap();
        let (release, receive) = tokio::sync::oneshot::channel();
        let receiver = Arc::new(Mutex::new(Some(receive)));
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            let index = called.fetch_add(1, Ordering::SeqCst);
            let receiver = if index == 0 {
                receiver.lock().unwrap().take()
            } else {
                None
            };
            Box::pin(async move {
                if let Some(receiver) = receiver {
                    receiver.await.unwrap();
                }
                if index == 1 {
                    Err(ArtworkError::Busy)
                } else {
                    pixels()
                }
            })
        }));
        let mut ui = AppUi::new();
        let visible = vec!["one".to_owned(), "two".to_owned()];
        let bindings = vec![binding("one"), binding("two")];
        artwork.update(&visible, &bindings);
        artwork.poll(&runtime, &mut ui);
        wait_for_job(&artwork, "two");
        artwork.poll(&runtime, &mut ui);
        artwork.poll(&runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        release.send(()).unwrap();
        settle(&mut artwork, &runtime, &mut ui, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn repeated_busy_rejection_is_bounded_to_one_retry_per_visit() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(ArtworkError::Busy) })
        }));
        let mut ui = AppUi::new();
        wait_until(|| {
            artwork.update(&["one".to_owned()], &[binding("one")]);
            artwork.poll(&runtime, &mut ui);
            artwork.jobs.is_empty()
        });
        artwork.update(&["one".to_owned()], &[binding("one")]);
        settle_jobs(&mut artwork, &runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(ui.image_cache_len(), 0);
    }
    #[test]
    fn changed_binding_invalidates_cached_pixels_before_replacement() {
        let runtime = Runtime::new().unwrap();
        let mut ui = AppUi::new();
        let mut artwork = Artwork::with_loader(Arc::new(|_| Box::pin(std::future::pending())));
        artwork.update(&["one".to_owned()], &[binding("one")]);
        ui.admit_image(
            "one",
            egui::ColorImage::filled([2, 1], egui::Color32::WHITE),
        )
        .unwrap();
        artwork.poll(&runtime, &mut ui);
        assert!(ui.has_image("one"));
        let mut changed = binding("one");
        changed.source = ImageSource::Media {
            id: MediaId::new("AbcD1234").unwrap(),
            label: ImageLabel::Portrait,
            role: ImageRole::Card,
        };
        artwork.update(&["one".to_owned()], &[changed]);
        artwork.poll(&runtime, &mut ui);
        assert!(!ui.has_image("one"));
    }
    #[test]
    fn retired_same_key_result_cannot_revive_when_the_key_returns() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            let index = called.fetch_add(1, Ordering::SeqCst);
            if index == 0 {
                Box::pin(async { pixels() })
            } else {
                Box::pin(std::future::pending())
            }
        }));
        let mut ui = AppUi::new();
        artwork.update(&["one".to_owned()], &[binding("one")]);
        artwork.poll(&runtime, &mut ui);
        wait_for_job(&artwork, "one");
        artwork.update(&[], &[]);
        artwork.update(&["one".to_owned()], &[binding("one")]);
        artwork.poll(&runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(ui.image_cache_len(), 0);
    }
    #[test]
    fn evicted_backdrop_restores_unknown_reserve_before_reloading() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        }));
        let mut ui = AppUi::new();
        let backdrop = ImageBinding {
            key: "backdrop".to_owned(),
            source: ImageSource::Media {
                id: MediaId::new("qvwT6mJ4").unwrap(),
                label: ImageLabel::Landscape,
                role: ImageRole::Backdrop,
            },
        };
        ui.admit_image(
            "backdrop",
            egui::ColorImage::filled([2, 1], egui::Color32::WHITE),
        )
        .unwrap();
        artwork.update(&["backdrop".to_owned()], std::slice::from_ref(&backdrop));
        artwork.poll(&runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(ui.discard_image("backdrop"));

        let cards: Vec<_> = (0..5).map(|index| format!("card{index}")).collect();
        for key in &cards {
            ui.admit_image(
                key,
                egui::ColorImage::filled([1024, 1024], egui::Color32::WHITE),
            )
            .unwrap();
        }
        let mut visible = cards.clone();
        visible.push("backdrop".to_owned());
        let mut bindings: Vec<_> = cards.iter().map(|key| binding(key)).collect();
        bindings.push(backdrop);
        artwork.update(&visible, &bindings);
        artwork.poll(&runtime, &mut ui);

        assert_eq!(ui.image_cache_bytes(), 20 * 1024 * 1024);
        assert!(cards.iter().all(|key| ui.has_image(key)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!ui.has_image("backdrop"));
    }
    #[test]
    fn in_poll_eviction_restores_backdrop_reserve_before_starting_another_load() {
        use image::ImageEncoder;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(
                &vec![255; 1024 * 1024 * 4],
                1024,
                1024,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let encoded = Arc::new(encoded);
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |source| {
            let index = called.fetch_add(1, Ordering::SeqCst);
            let encoded = encoded.clone();
            Box::pin(async move {
                if index == 0 {
                    decode_artwork(source.role(), "image/png", &encoded)
                } else {
                    std::future::pending().await
                }
            })
        }));
        let runtime = Runtime::new().unwrap();
        let mut ui = AppUi::new();
        let backdrop = ImageBinding {
            key: "backdrop".to_owned(),
            source: ImageSource::Media {
                id: MediaId::new("qvwT6mJ4").unwrap(),
                label: ImageLabel::Landscape,
                role: ImageRole::Backdrop,
            },
        };
        ui.admit_image(
            "backdrop",
            egui::ColorImage::filled([2, 1], egui::Color32::WHITE),
        )
        .unwrap();
        artwork.update(&["backdrop".to_owned()], std::slice::from_ref(&backdrop));
        artwork.poll(&runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let cards: Vec<_> = (0..4).map(|index| format!("card{index}")).collect();
        for key in std::iter::once(&"filler".to_owned()).chain(cards.iter()) {
            ui.admit_image(
                key,
                egui::ColorImage::filled([1024, 1024], egui::Color32::WHITE),
            )
            .unwrap();
        }
        let mut visible = cards.clone();
        visible.extend(["new-card".to_owned(), "backdrop".to_owned()]);
        let mut bindings: Vec<_> = cards.iter().map(|key| binding(key)).collect();
        bindings.extend([binding("new-card"), backdrop]);
        artwork.update(&visible, &bindings);
        artwork.poll(&runtime, &mut ui);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        wait_for_job(&artwork, "new-card");
        artwork.poll(&runtime, &mut ui);

        assert!(!ui.has_image("backdrop"));
        assert!(cards.iter().all(|key| ui.has_image(key)));
        assert!(ui.has_image("new-card"));
        assert_eq!(ui.image_cache_bytes(), 24 * 1024 * 1024);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn a_successful_evicted_key_can_reload_within_the_current_budget() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = Runtime::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { pixels() })
        }));
        let mut ui = AppUi::new();
        artwork.update(&["one".to_owned()], &[binding("one")]);
        settle(&mut artwork, &runtime, &mut ui, 1);
        ui.discard_image("one");
        artwork.update(&["one".to_owned()], &[binding("one")]);
        settle(&mut artwork, &runtime, &mut ui, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn dropping_the_owner_aborts_its_pending_load() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct MarkDrop(Arc<AtomicBool>);
        impl Drop for MarkDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let runtime = Runtime::new().unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let disposed = Arc::new(AtomicBool::new(false));
        let (load_entered, load_disposed) = (entered.clone(), disposed.clone());
        let mut artwork = Artwork::with_loader(Arc::new(move |_| {
            let (entered, disposed) = (load_entered.clone(), load_disposed.clone());
            Box::pin(async move {
                let _drop = MarkDrop(disposed);
                entered.store(true, Ordering::SeqCst);
                std::future::pending().await
            })
        }));
        artwork.update(&["one".to_owned()], &[binding("one")]);
        artwork.poll(&runtime, &mut AppUi::new());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !entered.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        drop(artwork);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !disposed.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(entered.load(Ordering::SeqCst));
        assert!(disposed.load(Ordering::SeqCst));
    }
}
