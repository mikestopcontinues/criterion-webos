// SPDX-License-Identifier: GPL-3.0-or-later
//! Root-only subscriber admission. Private activation pixels stay outside the checkout.
use super::*;
use criterion_ui::{LoginView, Page};
use std::time::Instant;

#[path = "subscriber_membership_admission_tests.rs"]
mod subscriber_membership_admission_tests;
use subscriber_membership_admission_tests::{MembershipTrace, PaintObserver};

#[path = "subscriber_continue_watching_admission_tests.rs"]
mod subscriber_continue_watching_admission_tests;
use subscriber_continue_watching_admission_tests::{
    ContinueWatchingJourney, ContinueWatchingTrace,
};

fn finish_admission<P, T, C, A>(
    app: &mut Application<P, T, C, A>,
    runtime: &Runtime,
    grant_seen: bool,
    logout_issued: bool,
) -> Result<(), &'static str>
where
    P: RequestTransport + Send + Sync + 'static,
    T: Transport + 'static,
    C: MonotonicClock + Clone + 'static,
    A: criterion_account::Transport + 'static,
{
    // Retain an already-issued authorization for the production transport's
    // ten-second total deadline plus a scheduling margin. Inactive polling
    // joins completion but cannot issue another grant poll or refresh. Awaiting
    // also represents an idle challenge, so cancellation remains conservative
    // if this quiet interval produces no locally admitted grant.
    let settle_until = Instant::now() + Duration::from_secs(11);
    while !logout_issued
        && matches!(
            app.authentication.view(),
            LoginView::Requesting | LoginView::Awaiting { .. }
        )
        && Instant::now() < settle_until
    {
        app.authentication.poll(runtime, false);
        if matches!(
            app.authentication.view(),
            LoginView::Requesting | LoginView::Awaiting { .. }
        ) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    app.authentication.poll(runtime, false);
    let logout_issued = logout_issued || matches!(app.authentication.view(), LoginView::SigningOut);
    if !grant_seen && !app.authentication.signed_in() && !logout_issued {
        app.command(Command::CancelAuthentication, runtime.handle());
        app.authentication.poll(runtime, false);
    }
    let admitted = grant_seen || app.authentication.signed_in() || logout_issued;
    if admitted && !logout_issued && !matches!(app.authentication.view(), LoginView::SigningOut) {
        // Earlier native/UI failure: one explicit logout, never a revoke retry.
        app.command(Command::Logout, runtime.handle());
    }
    app.background();
    let confirmed = app.finish(runtime);
    app.sync_account_session();
    if !confirmed
        || (admitted
            && (!matches!(app.authentication.view(), LoginView::SignedOut)
                || app.authentication.session().status() != criterion_session::Status::SignedOut))
    {
        return Err("logout confirmation (remote state unconfirmed)");
    }
    if !admitted {
        // Dropped/absent grant completion cannot attest the remote activation.
        return Err("activation cancelled (remote authorization unconfirmed)");
    }
    if app.account_signed_in
        || app.shelf_pending.is_some()
        || app.shelf_generation.is_some()
        || (matches!(app.ui.page(), Page::MyList | Page::Login)
            && (!app.controller.view.artwork_bindings().is_empty()
                || !app.controller.view.with_view(LoginView::SignedOut, |view| {
                    view.cards.is_empty()
                        && view.rails.is_empty()
                        && view.hero.is_none()
                        && view.detail.is_none()
                })))
    {
        return Err("private view disposal");
    }
    Ok(())
}

const PRIVATE_DIRECTORY: &str = "/criterion-admission";
const ACTIVATION_CAPTURE: &str = "/criterion-admission/activation.png";

fn private_activation_mount() -> Result<(), &'static str> {
    use std::{io::Read, os::unix::fs::MetadataExt};
    let directory =
        std::fs::symlink_metadata(PRIVATE_DIRECTORY).map_err(|_| "private activation mount")?;
    let process = std::fs::metadata("/proc/self").map_err(|_| "private activation owner")?;
    if !directory.is_dir()
        || directory.mode() & 0o7777 != 0o700
        || (process.uid() != 0 && directory.uid() != process.uid())
    {
        return Err("private activation permissions");
    }
    // Linux mountinfo's fifth field is the mount point. The fixed ASCII path
    // contains no escaped characters. A checkout directory is never admitted.
    let mut mounts = String::new();
    std::fs::File::open("/proc/self/mountinfo")
        .map_err(|_| "private activation mount")?
        .take(65_537)
        .read_to_string(&mut mounts)
        .map_err(|_| "private activation mount")?;
    if mounts.len() > 65_536 {
        return Err("private activation mount");
    }
    let mount_root = |point| {
        mounts.lines().find_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            let root = fields.nth(3)?;
            (fields.next()? == point).then_some(root)
        })
    };
    let private_root = mount_root(PRIVATE_DIRECTORY).ok_or("private activation mount")?;
    let workspace_root = mount_root("/workspace").ok_or("canonical workspace mount")?;
    // Require the executor's fresh setup directly under the host temporary root.
    // Comparing only this worktree would admit the primary or a sibling repo.
    // Linux and macOS canonical temp roots are both supported; root's command
    // creates criterion-admission.XXXXXX there before mounting it.
    let private_root = std::path::Path::new(private_root);
    let workspace_root = std::path::Path::new(workspace_root);
    if !private_root.is_absolute()
        || !workspace_root.is_absolute()
        || private_root.as_os_str().as_encoded_bytes().contains(&b'\\')
        || workspace_root
            .as_os_str()
            .as_encoded_bytes()
            .contains(&b'\\')
        || private_root.starts_with(workspace_root)
        || workspace_root.starts_with(private_root)
        || !matches!(
            private_root.parent().and_then(std::path::Path::to_str),
            Some("/tmp" | "/private/tmp")
        )
        || !private_root
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| {
                name.strip_prefix("criterion-admission.")
                    .is_some_and(|suffix| {
                        suffix.len() >= 6
                            && suffix.len() <= 64
                            && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    })
            })
    {
        return Err("external activation mount");
    }
    match std::fs::symlink_metadata(ACTIVATION_CAPTURE) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("fresh private activation capture"),
    }
}

fn frame(
    app: &mut SubscriberApp,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    journey: &mut Journey,
) -> Result<(), &'static str> {
    journey.check()?;
    journey.observe(app)?;
    app.poll(runtime, true);
    journey.observe(app)?;
    for _ in 0..128 {
        let Some(event) = window.poll_event().map_err(|_| "native input")? else {
            break;
        };
        let surface = window.surface().map_err(|_| "render surface")?;
        app.event(event, surface, runtime, journey.start.elapsed());
        // The ordinary UI Select command synchronously enters SigningOut. Keep
        // its ownership even if later presentation or the next event fails.
        journey.logout_issued |= matches!(app.authentication.view(), LoginView::SigningOut);
        journey.observe(app)?;
    }
    if window.activity() != criterion_platform::Activity::Foreground || app.exiting() {
        return Err("native foreground");
    }
    app.consume(runtime, journey.start.elapsed());
    journey.observe(app)?;
    let drawable = window.surface().map_err(|_| "render surface")?.drawable;
    let mut output = app.take_output().ok_or("render frame")?;
    let admission = match journey.prepare_membership_paint(app, &output) {
        Ok(admission) => admission,
        Err(error) => {
            output.textures_delta.clear();
            return Err(error);
        }
    };
    let watching_admission = match journey
        .continue_watching
        .as_mut()
        .map_or(Ok(false), |watching| watching.prepare(app, &output))
    {
        Ok(admission) => admission,
        Err(error) => {
            output.textures_delta.clear();
            return Err(error);
        }
    };
    if painter
        .paint(
            [drawable.width, drawable.height],
            app.context(),
            &mut output,
        )
        .is_err()
    {
        output.textures_delta.clear();
        return Err("render frame");
    }
    window.present().map_err(|_| "native presentation")?;
    if admission && let Some(observer) = &mut journey.membership {
        observer.painted = true;
    }
    if watching_admission && let Some(watching) = &mut journey.continue_watching {
        watching.presented(app)?;
    }
    journey.check()
}

fn key(
    app: &mut SubscriberApp,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    journey: &mut Journey,
    key: (u32, i32),
) -> Result<(), &'static str> {
    use std::ffi::c_void;
    unsafe extern "C" {
        fn SDL_PushEvent(event: *mut c_void) -> i32;
    }
    for pressed in [true, false] {
        let mut raw = [0u8; 56];
        raw[..4].copy_from_slice(&(if pressed { 0x300u32 } else { 0x301 }).to_le_bytes());
        raw[12] = u8::from(pressed);
        raw[16..20].copy_from_slice(&key.0.to_le_bytes());
        raw[20..24].copy_from_slice(&key.1.to_le_bytes());
        let mut aligned = [0u64; 7];
        for (word, bytes) in aligned.iter_mut().zip(raw.as_chunks::<8>().0) {
            *word = u64::from_le_bytes(*bytes);
        }
        // SAFETY: initialized56-byte stock desktopSDL event, eight-byte
        // alignment; SDL copies it synchronously on the owning window thread.
        if unsafe { SDL_PushEvent(aligned.as_mut_ptr().cast::<c_void>()) } != 1 {
            return Err("native key injection");
        }
        frame(app, window, painter, runtime, journey)?;
    }
    Ok(())
}

fn capture_activation(
    app: &mut SubscriberApp,
    window: &criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    gl: &glow::Context,
    runtime: &Runtime,
    journey: &Journey,
) -> Result<(), &'static str> {
    use glow::HasContext;
    use std::{
        io::Write,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };
    journey.check()?;
    // Flush a settled activation-only frame. This is the sole framebuffer read
    // in the subscriber journey; My List is never read back or captured.
    app.consume(runtime, journey.start.elapsed());
    if app.ui.page() != Page::Login
        || !matches!(app.authentication.view(), LoginView::Awaiting { .. })
    {
        return Err("activation capture phase");
    }
    let drawable = window.surface().map_err(|_| "activation surface")?.drawable;
    if drawable.width != 1920 || drawable.height != 1080 {
        return Err("activation surface dimensions");
    }
    let mut output = app.take_output().ok_or("activation frame")?;
    painter
        .paint([1920, 1080], app.context(), &mut output)
        .map_err(|_| "activation frame")?;
    let mut pixels = vec![0u8; 1920 * 1080 * 4];
    // SAFETY: live current context on this thread, fixed1920×1080 RGBA8 allocation.
    unsafe {
        gl.read_pixels(
            0,
            0,
            1920,
            1080,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );
    }
    if unsafe { gl.get_error() } != glow::NO_ERROR {
        return Err("activation framebuffer");
    }
    if !pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200)
    {
        return Err("activation framebuffer content");
    }
    let buffer = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(1920, 1080, pixels)
        .ok_or("activation pixels")?;
    // Recheck the private mount before creating the code-bearing artifact.
    private_activation_mount()?;
    let mut capture = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(ACTIVATION_CAPTURE)
        .map_err(|_| "private activation capture")?;
    let metadata = capture
        .metadata()
        .map_err(|_| "private activation capture")?;
    if metadata.mode() & 0o7777 != 0o600 || metadata.nlink() != 1 {
        return Err("private activation capture permissions");
    }
    image::DynamicImage::ImageRgba8(image::imageops::flip_vertical(&buffer))
        .write_to(&mut capture, image::ImageFormat::Png)
        .map_err(|_| "private activation encoding")?;
    capture.flush().map_err(|_| "private activation capture")?;
    capture
        .sync_all()
        .map_err(|_| "private activation capture")?;
    window.present().map_err(|_| "activation presentation")?;
    journey.check()
}

// Exact private requests live only in this bounded trace, with no Debug or persistence.
const MAX_READS: usize = 16;
struct ReadAttempt {
    request: criterion_account::WatchListRequest,
    returned: Option<bool>,
}
#[derive(Default)]
struct ReadTrace {
    reads: Vec<ReadAttempt>,
    refused: bool,
    membership: Option<MembershipTrace>,
    continue_watching: Option<ContinueWatchingTrace>,
}
enum ReadReceipt {
    Shelf(usize),
    Detail,
    Ids,
    ContinueWatchingBootstrap,
    ContinueWatching,
}
struct TracedAccount<A> {
    inner: A,
    trace: std::sync::Arc<std::sync::Mutex<ReadTrace>>,
}
impl<A: criterion_account::Transport> criterion_account::Transport for TracedAccount<A> {
    async fn send(
        &self,
        request: criterion_account::Request,
    ) -> Result<criterion_account::Response, criterion_account::Error> {
        use criterion_account::{Error, Request, SubscriberTarget};
        let index = {
            let mut trace = self.trace.lock().map_err(|_| Error::Unavailable)?;
            if let Some(watching) = &mut trace.continue_watching {
                match &request {
                    Request::Bootstrap if watching.admit_bootstrap() => {
                        Some(ReadReceipt::ContinueWatchingBootstrap)
                    }
                    Request::Subscriber {
                        target: SubscriberTarget::ContinueWatching(region),
                        ..
                    } if watching.admit_read(*region) => Some(ReadReceipt::ContinueWatching),
                    _ => {
                        trace.refused = true;
                        return Err(Error::InvalidRequest);
                    }
                }
            } else {
                match &request {
                    Request::Bootstrap => None,
                    Request::Subscriber {
                        target: SubscriberTarget::WatchList { request, .. },
                        ..
                    } if trace.reads.len() < MAX_READS
                        && trace.membership.as_ref().is_none_or(|membership| {
                            membership.admit_shelf(trace.reads.len(), request)
                        }) =>
                    {
                        let index = trace.reads.len();
                        trace.reads.push(ReadAttempt {
                            request: request.clone(),
                            returned: None,
                        });
                        Some(ReadReceipt::Shelf(index))
                    }
                    Request::Detail {
                        region, media_id, ..
                    } if trace
                        .membership
                        .as_mut()
                        .is_some_and(|membership| membership.admit_detail(*region, media_id)) =>
                    {
                        Some(ReadReceipt::Detail)
                    }
                    Request::Subscriber {
                        target: SubscriberTarget::MyListIds(region),
                        ..
                    } if trace
                        .membership
                        .as_mut()
                        .is_some_and(|membership| membership.admit_ids(*region)) =>
                    {
                        Some(ReadReceipt::Ids)
                    }
                    Request::Detail { .. } | Request::Subscriber { .. } => {
                        trace.refused = true;
                        return Err(Error::InvalidRequest);
                    }
                }
            }
        };
        // Preserve the real transport's credential/TLS/body/URL owner intact.
        // Only the new Continue Watching oracle borrows the returned body in
        // memory. Existing modes do not inspect it; no mode inspects headers or retries.
        let result = self.inner.send(request).await;
        if let Some(receipt) = index {
            let mut trace = self.trace.lock().map_err(|_| Error::Unavailable)?;
            let returned = Some(result.as_ref().is_ok_and(|response| response.status == 200));
            match receipt {
                ReadReceipt::Shelf(index) => trace.reads[index].returned = returned,
                ReadReceipt::Detail => {
                    trace
                        .membership
                        .as_mut()
                        .ok_or(Error::Unavailable)?
                        .detail_returned = returned;
                }
                ReadReceipt::Ids => {
                    trace
                        .membership
                        .as_mut()
                        .ok_or(Error::Unavailable)?
                        .ids_returned = returned;
                }
                ReadReceipt::ContinueWatchingBootstrap => {
                    trace
                        .continue_watching
                        .as_mut()
                        .ok_or(Error::Unavailable)?
                        .complete_bootstrap(&result);
                }
                ReadReceipt::ContinueWatching => {
                    trace
                        .continue_watching
                        .as_mut()
                        .ok_or(Error::Unavailable)?
                        .complete_read(&result);
                }
            }
        }
        result
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AdmissionMode {
    Initial,
    Grouped,
    Membership,
    ContinueWatching,
}
type SubscriberApp = Application<
    HttpTransport,
    criterion_session::HttpTransport,
    SystemClock,
    TracedAccount<criterion_account::HttpTransport>,
>;
type SharedTrace = std::sync::Arc<std::sync::Mutex<ReadTrace>>;
struct ObservedRead {
    read: crate::my_list::Read,
    epoch: u64,
    generation: Option<u64>,
    pending_seen: bool,
}
struct Journey {
    start: Instant,
    deadline: Instant,
    logout_issued: bool,
    reads: Vec<ObservedRead>,
    membership: Option<PaintObserver>,
    membership_retired: bool,
    continue_watching: Option<ContinueWatchingJourney>,
}
impl Journey {
    fn new() -> Self {
        let start = Instant::now();
        Self {
            start,
            deadline: start + Duration::from_secs(540),
            logout_issued: false,
            reads: Vec::new(),
            membership: None,
            membership_retired: false,
            continue_watching: None,
        }
    }
    fn check(&self) -> Result<(), &'static str> {
        if Instant::now() >= self.deadline {
            Err("subscriber journey deadline")
        } else {
            Ok(())
        }
    }
    fn stage(&self, duration: Duration) -> Instant {
        self.deadline.min(Instant::now() + duration)
    }
    fn observe(&mut self, app: &SubscriberApp) -> Result<(), &'static str> {
        if let Some(watching) = &mut self.continue_watching {
            watching.observe(app)?;
        }
        let Some(read) = app
            .shelf_pending
            .as_ref()
            .or_else(|| app.shelf_generation.as_ref().map(|issued| &issued.read))
        else {
            return Ok(());
        };
        let epoch = app.account_epoch.ok_or("private read epoch")?;
        if !app.controller.shelf_owns(epoch, read) {
            return Err("private read authority");
        }
        let generation = app
            .shelf_generation
            .as_ref()
            .map(|issued| issued.generation);
        if app
            .shelf_generation
            .as_ref()
            .is_some_and(|issued| issued.epoch != epoch || issued.generation == 0)
        {
            return Err("private read generation");
        }
        if let Some(observed) = self
            .reads
            .iter_mut()
            .find(|observed| observed.epoch == epoch && observed.read.operation == read.operation)
        {
            if observed.read.request != read.request
                || (observed.generation.is_some()
                    && generation.is_some()
                    && observed.generation != generation)
            {
                return Err("private read identity");
            }
            if generation.is_some() {
                observed.generation = generation;
            }
            observed.pending_seen |= app.shelf_pending.is_some();
        } else {
            if self.reads.len() >= MAX_READS {
                return Err("private read observation bound");
            }
            self.reads.push(ObservedRead {
                read: read.clone(),
                epoch,
                generation,
                pending_seen: app.shelf_pending.is_some(),
            });
        }
        Ok(())
    }
    fn prepare_membership_paint(
        &mut self,
        app: &SubscriberApp,
        output: &egui::FullOutput,
    ) -> Result<bool, &'static str> {
        if self.membership_retired {
            subscriber_membership_admission_tests::retired_frame(app, output)?;
        }
        self.membership
            .as_mut()
            .map_or(Ok(false), |observer| observer.prepare(app, output))
    }
}
fn verify_trace(trace: &SharedTrace, journey: &Journey) -> Result<(), &'static str> {
    if journey.continue_watching.is_some() {
        return subscriber_continue_watching_admission_tests::verify_trace(trace);
    }
    let trace = trace.lock().map_err(|_| "private request trace")?;
    if trace.refused || trace.reads.is_empty() || trace.reads.len() != journey.reads.len() {
        return Err("read-only exact request trace");
    }
    // Application::poll may issue and drain a fast read in one call. Generation
    // checks run whenever that live witness is visible; completed admission
    // remains the production Application/Accounts owner's authority.
    for (attempt, observed) in trace.reads.iter().zip(&journey.reads) {
        if attempt.returned != Some(true)
            || attempt.request != observed.read.request
            || !observed.pending_seen
        {
            return Err("authenticated request admission");
        }
    }
    Ok(())
}
// No Debug: native row keys/titles are compared only in memory, never in failures.
struct PrivateWindow {
    selected: criterion_ui::MyListGroup,
    first: usize,
    tail: criterion_ui::CatalogTail,
    cards: Vec<(criterion_ui::Target, String, String, Option<String>)>,
}
fn private_window(app: &SubscriberApp) -> Result<PrivateWindow, &'static str> {
    app.controller
        .view
        .with_view(app.authentication.view(), |view| {
            if view.cards.len() > 180 {
                return Err("private window bound");
            }
            let selected = view.my_list.ok_or("published native groups")?.selected;
            let window = view.catalog.ok_or("published native window")?;
            Ok(PrivateWindow {
                selected,
                first: window.first,
                tail: window.tail,
                cards: view
                    .cards
                    .iter()
                    .map(|card| {
                        (
                            card.key.clone(),
                            card.title.to_owned(),
                            card.year.to_owned(),
                            card.duration_label.map(str::to_owned),
                        )
                    })
                    .collect(),
            })
        })
}
fn first_rows_preserved(before: &PrivateWindow, after: &PrivateWindow) -> bool {
    before.selected == after.selected
        && before.first == 0
        && after.first == 0
        && !before.cards.is_empty()
        && after.cards.starts_with(&before.cards)
        && after.cards.iter().enumerate().all(|(index, card)| {
            !after.cards[..index]
                .iter()
                .any(|earlier| earlier.0 == card.0)
        })
}
fn wait_shelf(
    app: &mut SubscriberApp,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    journey: &mut Journey,
    deadline: Instant,
) -> Result<(), &'static str> {
    loop {
        frame(app, window, painter, runtime, journey)?;
        if Instant::now() >= deadline {
            return Err("subscriber grouped stage deadline");
        }
        if !app.authentication.signed_in()
            || app.ui.page() != Page::MyList
            || !app.controller.is_shelf()
        {
            return Err("subscriber grouped authority departed");
        }
        let (status, tail) = app
            .controller
            .view
            .with_view(app.authentication.view(), |view| {
                (view.status, view.catalog.map(|window| window.tail))
            });
        if matches!(status, LoadState::Error | LoadState::Offline)
            || tail == Some(criterion_ui::CatalogTail::Error)
        {
            return Err("subscriber grouped admission");
        }
        if matches!(status, LoadState::Ready | LoadState::Empty)
            && app.shelf_pending.is_none()
            && app.shelf_generation.is_none()
            && tail != Some(criterion_ui::CatalogTail::Loading)
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}
fn admit_grouped(
    app: &mut SubscriberApp,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    journey: &mut Journey,
    trace: &SharedTrace,
) -> Result<(), &'static str> {
    use criterion_account::{WatchListFilter, WatchListRequest};
    use criterion_ui::{CatalogTail, Focus, MyListGroup};
    let deadline = journey.stage(Duration::from_secs(45));
    let (choices, selected) = app
        .controller
        .view
        .with_view(app.authentication.view(), |view| {
            view.my_list
                .map(|groups| (groups.choices.to_vec(), available_group(groups.choices)))
        })
        .ok_or("published native groups")?;
    let selected =
        selected.ok_or("grouped scope unadmitted: no published positive filtered group")?;
    let expected_filter = match selected {
        MyListGroup::FilmsAndSeries => WatchListFilter::FilmSeries,
        MyListGroup::Collections => WatchListFilter::Collection,
        MyListGroup::OriginalsAndFranchises => WatchListFilter::OriginalFranchise,
        MyListGroup::Supplements => WatchListFilter::Supplement,
        MyListGroup::Categories => WatchListFilter::Category,
        MyListGroup::All => return Err("filtered group choice"),
    };
    let initial_reads = trace
        .lock()
        .map_err(|_| "private request trace")?
        .reads
        .len();
    // The observed native choice controls selection; no fixture group is imposed.
    if matches!(app.ui.focus(), Focus::Card { row: 0, .. }) {
        key(app, window, painter, runtime, journey, (82, 1_073_741_906))?;
    }
    if app.ui.focus() != Focus::MyListGroup(MyListGroup::All) {
        return Err("native All header focus");
    }
    let position = choices
        .iter()
        .position(|choice| choice.group == selected)
        .ok_or("published filtered group choice")?;
    for _ in 0..position {
        key(app, window, painter, runtime, journey, (79, 1_073_741_903))?;
    }
    if app.ui.focus() != Focus::MyListGroup(selected) {
        return Err("native filtered header focus");
    }
    key(app, window, painter, runtime, journey, (40, 13))?;
    wait_shelf(app, window, painter, runtime, journey, deadline)?;
    verify_trace(trace, journey)?;
    let filtered = private_window(app)?;
    {
        let trace = trace.lock().map_err(|_| "private request trace")?;
        if trace.reads.len() != initial_reads + 1
            || trace.reads[initial_reads].request
                != (WatchListRequest {
                    filter: expected_filter,
                    cursor: None,
                })
        {
            return Err("grouped first-page scope unadmitted: exact filtered request unavailable");
        }
    }
    if filtered.selected != selected || filtered.first != 0 || filtered.cards.is_empty() {
        return Err("grouped first-page scope unadmitted: retained rows unavailable");
    }
    println!("subscriber admission: native filtered group admitted");
    if filtered.tail != CatalogTail::More {
        return Err("continuation scope unadmitted: no observed next cursor");
    }
    let demand_deadline = journey.stage(Duration::from_secs(60));
    let continuation_start = journey.reads.len();
    // Only actual native Down creates continuation demand. No manual request,
    // invented cursor, item activation, membership or progress mutation occurs.
    for _ in 0..(filtered.cards.len().div_ceil(4) + 2) {
        key(app, window, painter, runtime, journey, (81, 1_073_741_905))?;
        if Instant::now() >= demand_deadline {
            return Err("native continuation demand deadline");
        }
        if journey.reads.len() > continuation_start {
            break;
        }
    }
    if journey.reads.len() == continuation_start {
        return Err("continuation scope unadmitted: native Down produced no read");
    }
    // frame observed the pure owner's pending read after input and before the
    // next poll issued it. Its opaque cursor is the admitted production value.
    let continuation = &journey.reads[continuation_start];
    if !continuation.pending_seen
        || continuation.read.request.filter != expected_filter
        || continuation.read.request.cursor.is_none()
    {
        return Err("native observed continuation identity");
    }
    let epoch = journey.reads[initial_reads].epoch;
    if continuation.epoch != epoch || journey.reads.first().is_none_or(|read| read.epoch != epoch) {
        return Err("native grouped session epoch");
    }
    wait_shelf(app, window, painter, runtime, journey, demand_deadline)?;
    verify_trace(trace, journey)?;
    if journey.reads[continuation_start..].iter().any(|read| {
        read.epoch != epoch
            || read.read.request.filter != expected_filter
            || read.read.request.cursor.is_none()
    }) {
        return Err("native grouped continuation ownership");
    }
    let continued_reads = &journey.reads[continuation_start..];
    if continued_reads.iter().enumerate().any(|(index, read)| {
        continued_reads[..index]
            .iter()
            .any(|earlier| earlier.read.request.cursor == read.read.request.cursor)
    }) {
        return Err("native observed cursor progress");
    }
    let continued = private_window(app)?;
    if !first_rows_preserved(&filtered, &continued) {
        return Err("native continuation first-key rows and current group");
    }
    println!(
        "subscriber admission: exact observed continuation admitted; first-key rows preserved"
    );
    Ok(())
}

fn available_group(choices: &[criterion_ui::MyListChoice]) -> Option<criterion_ui::MyListGroup> {
    use criterion_ui::MyListGroup;
    let positive = |choice: &&criterion_ui::MyListChoice| {
        choice.group != MyListGroup::All && choice.count.is_some_and(|count| count > 0)
    };
    choices
        .iter()
        .take(6)
        .find(|choice| choice.group == MyListGroup::FilmsAndSeries && positive(choice))
        .or_else(|| choices.iter().take(6).find(positive))
        .map(|choice| choice.group)
}

fn run_subscriber_admission(mode: AdmissionMode) -> Result<(), &'static str> {
    use criterion_ui::{Focus, GlowRenderer};
    use std::{
        ffi::CString,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Arc,
    };
    // Refuse before creating any client/window or issuing an activation request.
    private_activation_mount()?;
    super::super::prepare_process();
    let mut window = criterion_platform::Window::open("Criterion Unofficial Subscriber Admission")
        .map_err(|_| "native window")?;
    // SAFETY: this test thread owns the live SDL context through painter disposal.
    let gl = Arc::new(unsafe {
        glow::Context::from_loader_function(|name| {
            CString::new(name).map_or(std::ptr::null(), |name| window.gl_proc_address(&name))
        })
    });
    let mut painter = unsafe { GlowRenderer::new(gl.clone()) }.map_err(|_| "GLES renderer")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| "request runtime")?;
    let trace = Arc::new(std::sync::Mutex::new(ReadTrace {
        membership: (mode == AdmissionMode::Membership).then(MembershipTrace::default),
        continue_watching: (mode == AdmissionMode::ContinueWatching)
            .then(ContinueWatchingTrace::default),
        ..ReadTrace::default()
    }));
    let authentication = Authentication::new().map_err(|_| "the subscriber session")?;
    let account = criterion_account::AccountClient::with_transport(TracedAccount {
        inner: criterion_account::HttpTransport::new().map_err(|_| "the account client")?,
        trace: trace.clone(),
    });
    let accounts = Accounts::from_parts(Arc::new(account), authentication.session());
    let mut app = Application::with_parts(
        window.surface().map_err(|_| "render surface")?,
        Controller::new(Catalog::new().map_err(|_| "the catalog")?, runtime.handle()),
        authentication,
        accounts,
        Artwork::new().map_err(|_| "the artwork client")?,
    );
    let mut journey_state = Journey::new();
    if mode == AdmissionMode::ContinueWatching {
        journey_state.continue_watching = Some(ContinueWatchingJourney::new(trace.clone()));
    }
    let mut grant_seen = false;
    let mut admitted_reads = None;
    let journey = catch_unwind(AssertUnwindSafe(|| -> Result<(), &'static str> {
        frame(
            &mut app,
            &mut window,
            &mut painter,
            &runtime,
            &mut journey_state,
        )?;
        if mode == AdmissionMode::ContinueWatching {
            subscriber_continue_watching_admission_tests::settle_original_home(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                &mut journey_state,
            )?;
        }
        // Actual SDL rail input: Home → Account activates the production issuer.
        for remote in [
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (81, 1_073_741_905),
            (40, 13),
        ] {
            key(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                &mut journey_state,
                remote,
            )?;
        }
        if app.ui.page() != Page::Login {
            return Err("activation navigation");
        }
        println!("subscriber admission: activation requested");
        let deadline = journey_state.stage(Duration::from_secs(45));
        loop {
            frame(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                &mut journey_state,
            )?;
            if Instant::now() >= deadline {
                return Err("subscriber stage deadline");
            }
            match app.authentication.view() {
                LoginView::Awaiting { .. } => break,
                LoginView::SignedOut
                | LoginView::Expired
                | LoginView::Denied
                | LoginView::Error => return Err("activation admission"),
                _ => (),
            }
            if Instant::now() >= deadline {
                return Err("activation request deadline");
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        capture_activation(
            &mut app,
            &window,
            &mut painter,
            &gl,
            &runtime,
            &journey_state,
        )?;
        println!("subscriber admission: activation capture ready");
        // Root completes only the first-party browser activation from the private
        // capture. The test neither reads codes/URIs nor accepts a bearer token.
        let deadline = journey_state.stage(Duration::from_secs(300));
        loop {
            frame(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                &mut journey_state,
            )?;
            if Instant::now() >= deadline {
                return Err("subscriber stage deadline");
            }
            match app.authentication.view() {
                LoginView::SignedIn if app.authentication.access_ready() => {
                    grant_seen = true;
                    break;
                }
                LoginView::SignedOut
                | LoginView::Expired
                | LoginView::Denied
                | LoginView::Error => return Err("subscriber authorization"),
                _ => (),
            }
            if Instant::now() >= deadline {
                return Err("subscriber activation deadline (remote authorization unconfirmed)");
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        println!("subscriber admission: signed in");
        if mode == AdmissionMode::ContinueWatching {
            subscriber_continue_watching_admission_tests::admit_home(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                &mut journey_state,
            )?;
        } else {
            // The observed subscriber rail owns My List independently of optional
            // anonymous Home content: Account → All Films → My List.
            for remote in [
                (80, 1_073_741_904),
                (82, 1_073_741_906),
                (82, 1_073_741_906),
                (40, 13),
            ] {
                key(
                    &mut app,
                    &mut window,
                    &mut painter,
                    &runtime,
                    &mut journey_state,
                    remote,
                )?;
            }
            if app.ui.page() != Page::MyList || !app.controller.is_shelf() {
                return Err("My List navigation");
            }
            let deadline = journey_state.stage(Duration::from_secs(45));
            loop {
                frame(
                    &mut app,
                    &mut window,
                    &mut painter,
                    &runtime,
                    &mut journey_state,
                )?;
                if Instant::now() >= deadline {
                    return Err("subscriber My List deadline");
                }
                if !app.authentication.signed_in() {
                    return Err("subscriber session departed");
                }
                let status = app
                    .controller
                    .view
                    .with_view(app.authentication.view(), |view| view.status);
                if matches!(status, LoadState::Error | LoadState::Offline) {
                    return Err("subscriber My List admission");
                }
                if matches!(status, LoadState::Ready | LoadState::Empty)
                    && app.shelf_pending.is_none()
                    && app.shelf_generation.is_none()
                {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("subscriber My List deadline");
                }
                std::thread::sleep(Duration::from_millis(16));
            }
            verify_trace(&trace, &journey_state)?;
            {
                let trace = trace.lock().map_err(|_| "private request trace")?;
                if trace.reads.first().is_none_or(|read| {
                    read.request != criterion_account::WatchListRequest::default()
                }) || trace
                    .reads
                    .iter()
                    .any(|read| read.request.filter != criterion_account::WatchListFilter::All)
                {
                    return Err("initial All request admission");
                }
            }
            println!("subscriber admission: My List admitted");
            if mode == AdmissionMode::Grouped {
                admit_grouped(
                    &mut app,
                    &mut window,
                    &mut painter,
                    &runtime,
                    &mut journey_state,
                    &trace,
                )?;
            }
            if mode == AdmissionMode::Membership {
                subscriber_membership_admission_tests::admit_membership(
                    &mut app,
                    &mut window,
                    &mut painter,
                    &runtime,
                    &mut journey_state,
                    &trace,
                )?;
            }
        }
        admitted_reads = Some(
            trace
                .lock()
                .map_err(|_| "private request trace")?
                .reads
                .len(),
        );
        // Return to Account, then actual SDL Select
        // executes the UI's explicit Logout command. Cleanup never replays it.
        if mode == AdmissionMode::ContinueWatching {
            subscriber_continue_watching_admission_tests::return_to_account(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                &mut journey_state,
            )?;
        } else {
            for remote in [
                (80, 1_073_741_904),
                (81, 1_073_741_905),
                (81, 1_073_741_905),
                (40, 13),
            ] {
                key(
                    &mut app,
                    &mut window,
                    &mut painter,
                    &runtime,
                    &mut journey_state,
                    remote,
                )?;
            }
        }
        if app.ui.page() != Page::Login || app.ui.focus() != Focus::LoginPrimary {
            return Err("logout native focus");
        }
        key(
            &mut app,
            &mut window,
            &mut painter,
            &runtime,
            &mut journey_state,
            (40, 13),
        )?;
        if !journey_state.logout_issued {
            return Err("explicit native logout");
        }
        Ok(())
    }))
    .unwrap_or(Err("interrupted journey"));
    let cleanup = catch_unwind(AssertUnwindSafe(|| {
        finish_admission(&mut app, &runtime, grant_seen, journey_state.logout_issued).and_then(
            |()| {
                if mode == AdmissionMode::ContinueWatching {
                    subscriber_continue_watching_admission_tests::verify_cleanup(&app)
                } else {
                    Ok(())
                }
            },
        )
    }))
    .unwrap_or(Err("interrupted cleanup (remote state unconfirmed)"));
    let text_cleanup = window.text_input(false).map_err(|_| "native text disposal");
    journey_state.membership = None;
    let watching_cleanup = journey_state
        .continue_watching
        .as_mut()
        .map_or(Ok(()), ContinueWatchingJourney::retire);
    drop(app); // Retained private projections and unpainted frames lose their owner.
    runtime.shutdown_timeout(Duration::from_secs(2));
    painter.destroy();
    // Inspect only after read workers and logout settled. A forbidden attempt or
    // extra native read during logout cannot be hidden by a successful journey.
    let final_trace = if journey.is_ok() {
        verify_trace(&trace, &journey_state).and_then(|()| {
            let count = trace
                .lock()
                .map_err(|_| "private request trace")?
                .reads
                .len();
            if Some(count) == admitted_reads {
                if mode == AdmissionMode::Membership {
                    subscriber_membership_admission_tests::verify_membership_trace(&trace)
                } else {
                    Ok(())
                }
            } else {
                Err("extra subscriber read during logout")
            }
        })
    } else {
        Ok(())
    };
    journey_state.reads.clear();
    if let Ok(mut trace) = trace.lock() {
        trace.reads.clear();
        trace.membership = None;
        trace.continue_watching = None;
    }
    if let Err(phase) = journey {
        println!("subscriber admission: stopped during {phase}");
    }
    if cleanup.is_ok() {
        println!("subscriber admission: issuer logout acknowledged; application disposed");
    }
    cleanup?;
    text_cleanup?;
    watching_cleanup?;
    journey?;
    final_trace?;
    println!("subscriber admission: journey passed");
    Ok(())
}

#[test]
#[ignore = "actual subscriber/provider and serialized SDL/GLES; private external mount; root executor only"]
fn native_subscriber_activation_my_list_and_logout_end_to_end() {
    subscriber_test(AdmissionMode::Initial);
}

#[test]
#[ignore = "actual subscriber/provider and serialized SDL/GLES; private external mount; root executor only"]
fn native_subscriber_grouped_my_list_continuation_and_logout_end_to_end() {
    subscriber_test(AdmissionMode::Grouped);
}

fn subscriber_test(mode: AdmissionMode) {
    // Redact panic payloads from every request worker as well as this test. The
    // root runs this test alone; restoration happens after all private owners die.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| eprintln!("subscriber admission: interrupted")));
    let outcome = std::panic::catch_unwind(|| run_subscriber_admission(mode))
        .unwrap_or(Err("interrupted admission (remote state unconfirmed)"));
    std::panic::set_hook(previous_hook);
    if let Err(phase) = outcome {
        panic!("subscriber admission stopped during {phase}");
    }
}

#[cfg(test)]
mod admission_oracle_tests {
    use super::*;
    use criterion_ui::{MyListChoice, MyListGroup};

    #[test]
    fn traced_read_forwards_exact_request_and_refuses_other_methods() {
        use criterion_account::{Region, SubscriberTarget, WatchListFilter, WatchListRequest};
        use criterion_provider::PageCursor;
        use criterion_session::SecretBody;
        use std::sync::{Arc, Mutex};
        struct Receiver(Arc<Mutex<Vec<WatchListRequest>>>);
        impl criterion_account::Transport for Receiver {
            async fn send(
                &self,
                request: criterion_account::Request,
            ) -> Result<criterion_account::Response, criterion_account::Error> {
                let criterion_account::Request::Subscriber {
                    target: SubscriberTarget::WatchList { request, .. },
                    credentials,
                } = request
                else {
                    panic!("unexpected synthetic method")
                };
                assert_eq!(
                    credentials.bootstrap().as_bytes(),
                    b"Bearer synthetic-bootstrap"
                );
                assert_eq!(credentials.subscriber().as_bytes(), b"synthetic-access");
                assert!(
                    credentials.bootstrap().is_sensitive()
                        && credentials.subscriber().is_sensitive()
                );
                self.0.lock().unwrap().push(request);
                Err(criterion_account::Error::Unavailable)
            }
        }
        let received = Arc::new(Mutex::new(Vec::new()));
        let trace = Arc::new(Mutex::new(ReadTrace::default()));
        let transport = TracedAccount {
            inner: Receiver(received.clone()),
            trace: trace.clone(),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        struct Bootstrap;
        impl criterion_account::Transport for Bootstrap {
            async fn send(
                &self,
                request: criterion_account::Request,
            ) -> Result<criterion_account::Response, criterion_account::Error> {
                match request {
                    criterion_account::Request::Bootstrap => Ok(criterion_account::Response {
                        status: 200,
                        body: SecretBody::new(br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()),
                    }),
                    criterion_account::Request::Detail { .. } | criterion_account::Request::Subscriber { .. } => panic!("synthetic capabilities fixture only bootstraps"),
                }
            }
        }
        #[derive(Clone)]
        struct Clock(Arc<std::sync::atomic::AtomicU64>);
        impl MonotonicClock for Clock {
            fn now(&self) -> Duration {
                Duration::from_secs(self.0.load(std::sync::atomic::Ordering::SeqCst))
            }
        }
        struct Issuer;
        impl criterion_session::Transport for Issuer {
            async fn post(
                &self,
                request: criterion_session::Request,
            ) -> Result<criterion_session::Response, criterion_session::Error> {
                let body = match request.endpoint {
                    criterion_session::Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
                    criterion_session::Endpoint::Token => br#"{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","expires_in":3600}"#.to_vec(),
                    criterion_session::Endpoint::Revoke => panic!("synthetic forwarding fixture never revokes"),
                };
                Ok(criterion_session::Response {
                    status: 200,
                    body: SecretBody::new(body),
                })
            }
        }
        let time = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let session = criterion_session::Session::with_transport(
            criterion_session::Configuration::production(),
            Issuer,
            Clock(time.clone()),
        );
        runtime.block_on(session.start_link()).unwrap();
        time.store(5, std::sync::atomic::Ordering::SeqCst);
        runtime.block_on(session.poll_once()).unwrap();
        let capabilities = criterion_account::AccountClient::with_transport(Bootstrap);
        runtime.block_on(capabilities.bootstrap()).unwrap();
        let expected = WatchListRequest {
            filter: WatchListFilter::FilmSeries,
            cursor: Some(PageCursor::new("synthetic opaque +/%").unwrap()),
        };
        let result = runtime.block_on(criterion_account::Transport::send(
            &transport,
            criterion_account::Request::Subscriber {
                target: SubscriberTarget::WatchList {
                    region: Region::Us,
                    request: expected.clone(),
                },
                credentials: capabilities.credentials(&session).unwrap(),
            },
        ));
        assert!(matches!(result, Err(criterion_account::Error::Unavailable)));
        assert!(*received.lock().unwrap() == [expected.clone()]);
        let recorded = trace.lock().unwrap();
        assert!(
            recorded.reads.len() == 1
                && recorded.reads[0].request == expected
                && recorded.reads[0].returned == Some(false)
        );
        drop(recorded);
        let result = runtime.block_on(criterion_account::Transport::send(
            &transport,
            criterion_account::Request::Subscriber {
                target: SubscriberTarget::ContinueWatching(Region::Us),
                credentials: capabilities.credentials(&session).unwrap(),
            },
        ));
        assert!(matches!(
            result,
            Err(criterion_account::Error::InvalidRequest)
        ));
        assert!(trace.lock().unwrap().refused);
        assert!(received.lock().unwrap().len() == 1);
        for _ in 1..MAX_READS {
            let result = runtime.block_on(criterion_account::Transport::send(
                &transport,
                criterion_account::Request::Subscriber {
                    target: SubscriberTarget::WatchList {
                        region: Region::Us,
                        request: expected.clone(),
                    },
                    credentials: capabilities.credentials(&session).unwrap(),
                },
            ));
            assert!(matches!(result, Err(criterion_account::Error::Unavailable)));
        }
        let result = runtime.block_on(criterion_account::Transport::send(
            &transport,
            criterion_account::Request::Subscriber {
                target: SubscriberTarget::WatchList {
                    region: Region::Us,
                    request: expected,
                },
                credentials: capabilities.credentials(&session).unwrap(),
            },
        ));
        assert!(matches!(
            result,
            Err(criterion_account::Error::InvalidRequest)
        ));
        assert!(received.lock().unwrap().len() == MAX_READS);
        assert!(trace.lock().unwrap().reads.len() == MAX_READS);
        // Enabling the new mode grants no general subscriber capability. The
        // same synthetic receiver would panic if any forbidden method escaped.
        trace.lock().unwrap().membership = Some(MembershipTrace::default());
        let root = criterion_provider::MediaId::new("Synth001").unwrap();
        for target in [
            SubscriberTarget::MyListIds(Region::Us),
            SubscriberTarget::ContinueWatching(Region::Us),
            SubscriberTarget::Entitlement {
                region: Region::Us,
                captured_unix_time_ms: 0,
            },
            SubscriberTarget::Playback {
                region: Region::Us,
                request: criterion_account::NativePlaybackRequest {
                    media_id: root.clone(),
                    drm_policy: criterion_account::DrmPolicy::Low,
                },
            },
            SubscriberTarget::AddWatchList {
                region: Region::Us,
                media_id: root.clone(),
                content_type: criterion_account::WatchListContentType::Film,
            },
            SubscriberTarget::RemoveWatchList {
                region: Region::Us,
                media_id: root,
            },
        ] {
            let result = runtime.block_on(criterion_account::Transport::send(
                &transport,
                criterion_account::Request::Subscriber {
                    target,
                    credentials: capabilities.credentials(&session).unwrap(),
                },
            ));
            assert!(matches!(
                result,
                Err(criterion_account::Error::InvalidRequest)
            ));
        }
        assert!(received.lock().unwrap().len() == MAX_READS);
        let membership_trace = Arc::new(Mutex::new(ReadTrace {
            membership: Some(MembershipTrace::default()),
            ..ReadTrace::default()
        }));
        let membership_transport = TracedAccount {
            inner: Receiver(received.clone()),
            trace: membership_trace.clone(),
        };
        for (request, allowed) in [
            (
                WatchListRequest {
                    filter: WatchListFilter::FilmSeries,
                    cursor: None,
                },
                false,
            ),
            (
                WatchListRequest {
                    filter: WatchListFilter::All,
                    cursor: Some(PageCursor::new("synthetic-next").unwrap()),
                },
                false,
            ),
            (WatchListRequest::default(), true),
            (WatchListRequest::default(), false),
        ] {
            let result = runtime.block_on(criterion_account::Transport::send(
                &membership_transport,
                criterion_account::Request::Subscriber {
                    target: SubscriberTarget::WatchList {
                        region: Region::Us,
                        request,
                    },
                    credentials: capabilities.credentials(&session).unwrap(),
                },
            ));
            assert!(if allowed {
                matches!(result, Err(criterion_account::Error::Unavailable))
            } else {
                matches!(result, Err(criterion_account::Error::InvalidRequest))
            });
        }
        assert!(received.lock().unwrap().len() == MAX_READS + 1);
        let trace = membership_trace.lock().unwrap();
        assert!(
            trace.refused
                && trace.reads.len() == 1
                && trace.reads[0].request == WatchListRequest::default()
        );
    }

    #[test]
    fn continuation_oracle_refuses_changed_prefix_duplicates_group_and_eviction() {
        use criterion_provider::MediaId;
        use criterion_ui::{CatalogTail, MyListGroup, Target};
        let card = |id: &str, title: &str| {
            (
                Target::Media(MediaId::new(id).unwrap()),
                title.to_owned(),
                String::new(),
                None,
            )
        };
        let window = |selected, first, cards| PrivateWindow {
            selected,
            first,
            tail: CatalogTail::End,
            cards,
        };
        let before = window(
            MyListGroup::FilmsAndSeries,
            0,
            vec![
                card("AbCd1234", "Synthetic first"),
                card("EfGh5678", "Synthetic second"),
            ],
        );
        let mut after = window(
            MyListGroup::FilmsAndSeries,
            0,
            vec![
                card("AbCd1234", "Synthetic first"),
                card("EfGh5678", "Synthetic second"),
                card("IjKl9012", "Synthetic appended"),
            ],
        );
        assert!(first_rows_preserved(&before, &after));
        after.cards[0].1 = "Synthetic changed".into();
        assert!(!first_rows_preserved(&before, &after));
        after.cards[0].1 = "Synthetic first".into();
        after.cards.swap(0, 1);
        assert!(!first_rows_preserved(&before, &after));
        after.cards.swap(0, 1);
        after.cards.push(card("AbCd1234", "Synthetic duplicate"));
        assert!(!first_rows_preserved(&before, &after));
        after.cards.pop();
        after.selected = MyListGroup::Collections;
        assert!(!first_rows_preserved(&before, &after));
        after.selected = MyListGroup::FilmsAndSeries;
        after.first = 1;
        assert!(!first_rows_preserved(&before, &after));
    }

    #[test]
    fn exact_completed_trace_does_not_require_a_transient_generation_witness() {
        use criterion_account::{WatchListFilter, WatchListRequest};
        use std::sync::{Arc, Mutex};
        let request = WatchListRequest::default();
        let trace = Arc::new(Mutex::new(ReadTrace {
            reads: vec![ReadAttempt {
                request: request.clone(),
                returned: Some(true),
            }],
            refused: false,
            membership: None,
            continue_watching: None,
        }));
        let mut journey = Journey::new();
        journey.reads.push(ObservedRead {
            read: crate::my_list::Read {
                operation: 1,
                request,
            },
            epoch: 7,
            generation: None,
            pending_seen: true,
        });
        // A completed valid worker can be joined by the same Application poll
        // that issued it; the exact typed read still has its durable witness.
        assert!(verify_trace(&trace, &journey).is_ok());
        journey.reads[0].pending_seen = false;
        assert!(verify_trace(&trace, &journey).is_err());
        journey.reads[0].pending_seen = true;
        trace.lock().unwrap().reads[0].returned = Some(false);
        assert!(verify_trace(&trace, &journey).is_err());
        trace.lock().unwrap().reads[0].returned = Some(true);
        journey.reads[0].read.request.filter = WatchListFilter::Collection;
        assert!(verify_trace(&trace, &journey).is_err());
    }

    #[test]
    fn positive_published_group_prefers_films_and_series() {
        let choices = [
            MyListChoice {
                group: MyListGroup::All,
                count: Some(12),
            },
            MyListChoice {
                group: MyListGroup::Collections,
                count: Some(3),
            },
            MyListChoice {
                group: MyListGroup::FilmsAndSeries,
                count: Some(9),
            },
        ];
        assert_eq!(available_group(&choices), Some(MyListGroup::FilmsAndSeries));
        assert_eq!(
            available_group(&choices[..2]),
            Some(MyListGroup::Collections)
        );
        assert_eq!(
            available_group(&[
                MyListChoice {
                    group: MyListGroup::All,
                    count: Some(12)
                },
                MyListChoice {
                    group: MyListGroup::FilmsAndSeries,
                    count: Some(0)
                },
                MyListChoice {
                    group: MyListGroup::Collections,
                    count: None
                },
            ]),
            None
        );
    }
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    use criterion_account::{AccountClient, SecretBody};
    use criterion_platform::Size;
    use criterion_provider::{
        ContentTarget, DiscoveryArtwork, DiscoveryBlock, DiscoveryNavItem, DiscoveryPage,
        GalleryLayout, GalleryPresentation,
    };
    use criterion_session::{Configuration, Endpoint, Session};
    use criterion_ui::Action;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    };

    struct Offline;
    impl RequestTransport for Offline {
        async fn get(
            &self,
            _: criterion_provider::Request,
        ) -> Result<criterion_provider::Response, criterion_provider::Error> {
            Err(criterion_provider::Error::Unavailable)
        }
    }
    struct Middleware;
    impl criterion_account::Transport for Middleware {
        async fn send(
            &self,
            request: criterion_account::Request,
        ) -> Result<criterion_account::Response, criterion_account::Error> {
            let body = match request {
                criterion_account::Request::Bootstrap => br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec(),
                criterion_account::Request::Subscriber {
                    target: criterion_account::SubscriberTarget::WatchList { region: criterion_account::Region::Us, .. },
                    credentials,
                } => {
                    assert_eq!(credentials.bootstrap().as_bytes(), b"Bearer synthetic-bootstrap");
                    assert_eq!(credentials.subscriber().as_bytes(), b"synthetic-access");
                    assert!(credentials.bootstrap().is_sensitive() && credentials.subscriber().is_sensitive());
                    br#"{"paging":{"page_limit":60},"type_counts":{"film":1},"playlist":[{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic private selection"}]}"#.to_vec()
                }
                criterion_account::Request::Detail { .. } | criterion_account::Request::Subscriber { .. } => panic!("subscriber admission must remain read-only"),
            };
            Ok(criterion_account::Response {
                status: 200,
                body: SecretBody::new(body),
            })
        }
    }
    #[derive(Clone)]
    struct Clock(Arc<AtomicU64>);
    impl MonotonicClock for Clock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }
    #[derive(Clone, Default)]
    struct Issuer {
        revokes: Arc<AtomicUsize>,
        fail_revoke: Arc<AtomicBool>,
        tokens: Arc<AtomicUsize>,
        hold_token: Arc<AtomicBool>,
        release: Arc<tokio::sync::Notify>,
    }
    impl Transport for Issuer {
        async fn post(
            &self,
            request: criterion_session::Request,
        ) -> Result<criterion_session::Response, criterion_session::Error> {
            let body = match request.endpoint {
                Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
                Endpoint::Token => {
                    self.tokens.fetch_add(1, Ordering::SeqCst);
                    if self.hold_token.swap(false, Ordering::SeqCst) {
                        self.release.notified().await;
                    }
                    br#"{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","expires_in":3600}"#.to_vec()
                },
                Endpoint::Revoke => {
                    self.revokes.fetch_add(1, Ordering::SeqCst);
                    if self.fail_revoke.load(Ordering::SeqCst) {
                        return Err(criterion_session::Error::Unavailable);
                    }
                    b"{}".to_vec()
                }
            };
            Ok(criterion_session::Response {
                status: 200,
                body: SecretBody::new(body),
            })
        }
    }
    type App = Application<Offline, Issuer, Clock, Middleware>;
    fn fixture(signed_in: bool) -> (App, Runtime, Issuer, Clock) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let issuer = Issuer::default();
        issuer.hold_token.store(!signed_in, Ordering::SeqCst);
        let clock = Clock(Arc::new(AtomicU64::new(0)));
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            issuer.clone(),
            clock.clone(),
        ));
        if signed_in {
            runtime.block_on(session.start_link()).unwrap();
            clock.0.store(5, Ordering::SeqCst);
            runtime.block_on(session.poll_once()).unwrap();
        }
        let mut app = Application::with_parts(
            Surface {
                window: Size {
                    width: 1920,
                    height: 1080,
                },
                drawable: Size {
                    width: 1920,
                    height: 1080,
                },
            },
            Controller::new(Catalog::with_transport(Offline), runtime.handle()),
            Authentication::with_session(session.clone(), clock.clone()),
            Accounts::from_parts(Arc::new(AccountClient::with_transport(Middleware)), session),
            Artwork::offline(),
        );
        app.controller.background();
        app.controller.view = crate::presentation::Presentation::discovery(DiscoveryPage {
            blocks: vec![DiscoveryBlock::Navigation {
                id: 1,
                header: None,
                presentation: GalleryPresentation {
                    aspect_ratio_percent: 56.25,
                    cards_per_view: 4,
                    layout: GalleryLayout::Rail,
                    variant: 0,
                },
                items: vec![DiscoveryNavItem {
                    id: 1,
                    label: "My List".into(),
                    target: ContentTarget::MyList,
                    opens_new_window: false,
                    artwork: DiscoveryArtwork {
                        desktop: vec![],
                        mobile: vec![],
                        logo: None,
                    },
                }],
            }],
        });
        for key in [Action::Down, Action::Select] {
            action(&mut app, &runtime, key);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !(if signed_in {
            app.controller
                .view
                .with_view(LoginView::SignedIn, |view| view.status == LoadState::Ready)
        } else {
            matches!(app.authentication.view(), LoginView::Awaiting { .. })
        }) {
            app.poll(&runtime, true);
            runtime.block_on(tokio::task::yield_now());
            assert!(
                Instant::now() < deadline,
                "synthetic subscriber admission deadline"
            );
        }
        (app, runtime, issuer, clock)
    }
    fn action(app: &mut App, runtime: &Runtime, action: Action) {
        let commands = app
            .controller
            .view
            .with_view(app.authentication.view(), |data| {
                app.ui.handle(action, &data)
            });
        for command in commands {
            app.command(command, runtime.handle());
        }
    }
    #[test]
    fn subscriber_admission_failure_after_grant_issues_one_logout_and_settles() {
        let (mut app, runtime, issuer, _) = fixture(true);
        // A native/UI journey failed after actual Session adapter admission.
        let cleanup = finish_admission(&mut app, &runtime, true, false);
        assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
        assert!(cleanup.is_ok());
        assert!(matches!(app.authentication.view(), LoginView::SignedOut));
        assert!(!app.authentication.signed_in());
    }
    #[test]
    fn subscriber_admission_unknown_revoke_is_unconfirmed_without_replay() {
        let (mut app, runtime, issuer, _) = fixture(true);
        issuer.fail_revoke.store(true, Ordering::SeqCst);
        // Follow the ordinary Account/Log Out UI before the settlement boundary.
        for key in [
            Action::Left,
            Action::Down,
            Action::Down,
            Action::Down,
            Action::Select,
            Action::Select,
        ] {
            action(&mut app, &runtime, key);
        }
        let cleanup = finish_admission(&mut app, &runtime, true, true);
        assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
        assert_eq!(
            cleanup,
            Err("logout confirmation (remote state unconfirmed)")
        );
        assert!(!app.authentication.signed_in());
        assert!(
            app.controller
                .view
                .with_view(app.authentication.view(), |view| view.cards.is_empty())
        );
        assert!(finish_admission(&mut app, &runtime, true, true).is_err());
        assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn subscriber_admission_failure_with_issued_grant_settles_then_logs_out_once() {
        let (mut app, runtime, issuer, clock) = fixture(false);
        clock.0.store(5, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(2);
        while issuer.tokens.load(Ordering::SeqCst) == 0 {
            app.poll(&runtime, true);
            runtime.block_on(tokio::task::yield_now());
            assert!(Instant::now() < deadline, "synthetic issued-grant deadline");
        }
        let release = issuer.release.clone();
        runtime.spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            release.notify_one();
        });
        // Native failure while an already-issued authorization is still owned.
        let cleanup = finish_admission(&mut app, &runtime, false, false);
        assert_eq!(issuer.revokes.load(Ordering::SeqCst), 1);
        assert!(cleanup.is_ok());
        assert!(matches!(app.authentication.view(), LoginView::SignedOut));
    }
}
