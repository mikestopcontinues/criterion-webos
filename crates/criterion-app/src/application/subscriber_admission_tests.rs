// SPDX-License-Identifier: GPL-3.0-or-later
//! Root-only subscriber admission. Private activation pixels stay outside the checkout.
use super::*;
use criterion_ui::{LoginView, Page};
use std::time::Instant;

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
        || app.shelf_pending
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
    app: &mut Application,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    start: Instant,
    logout_issued: &mut bool,
) -> Result<(), &'static str> {
    app.poll(runtime, true);
    for _ in 0..128 {
        let Some(event) = window.poll_event().map_err(|_| "native input")? else {
            break;
        };
        let surface = window.surface().map_err(|_| "render surface")?;
        app.event(event, surface, runtime, start.elapsed());
        // The ordinary UI Select command synchronously enters SigningOut. Keep
        // its ownership even if later presentation or the next event fails.
        *logout_issued |= matches!(app.authentication.view(), LoginView::SigningOut);
    }
    if window.activity() != criterion_platform::Activity::Foreground || app.exiting() {
        return Err("native foreground");
    }
    app.consume(runtime, start.elapsed());
    let mut output = app.take_output().ok_or("render frame")?;
    let drawable = window.surface().map_err(|_| "render surface")?.drawable;
    painter
        .paint(
            [drawable.width, drawable.height],
            app.context(),
            &mut output,
        )
        .map_err(|_| "render frame")?;
    window.present().map_err(|_| "native presentation")
}

fn key(
    app: &mut Application,
    window: &mut criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    runtime: &Runtime,
    start: Instant,
    logout_issued: &mut bool,
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
        frame(app, window, painter, runtime, start, logout_issued)?;
    }
    Ok(())
}

fn capture_activation(
    app: &mut Application,
    window: &criterion_platform::Window,
    painter: &mut criterion_ui::GlowRenderer,
    gl: &glow::Context,
    runtime: &Runtime,
    start: Instant,
) -> Result<(), &'static str> {
    use glow::HasContext;
    use std::{
        io::Write,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };
    // Flush a settled activation-only frame. This is the sole framebuffer read
    // in the subscriber journey; My List is never read back or captured.
    app.consume(runtime, start.elapsed());
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
    window.present().map_err(|_| "activation presentation")
}

fn run_subscriber_admission() -> Result<(), &'static str> {
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
    let mut app = Application::new(
        window.surface().map_err(|_| "render surface")?,
        runtime.handle(),
    )?;
    let start = Instant::now();
    let mut grant_seen = false;
    let mut logout_issued = false;
    let journey = catch_unwind(AssertUnwindSafe(|| -> Result<(), &'static str> {
        frame(
            &mut app,
            &mut window,
            &mut painter,
            &runtime,
            start,
            &mut logout_issued,
        )?;
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
                start,
                &mut logout_issued,
                remote,
            )?;
        }
        if app.ui.page() != Page::Login {
            return Err("activation navigation");
        }
        println!("subscriber admission: activation requested");
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            frame(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                start,
                &mut logout_issued,
            )?;
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
        capture_activation(&mut app, &window, &mut painter, &gl, &runtime, start)?;
        println!("subscriber admission: activation capture ready");
        // Root completes only the first-party browser activation from the private
        // capture. The test neither reads codes/URIs nor accepts a bearer token.
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            frame(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                start,
                &mut logout_issued,
            )?;
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
                start,
                &mut logout_issued,
                remote,
            )?;
        }
        if app.ui.page() != Page::MyList || !app.controller.is_shelf() {
            return Err("My List navigation");
        }
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            frame(
                &mut app,
                &mut window,
                &mut painter,
                &runtime,
                start,
                &mut logout_issued,
            )?;
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
                && !app.shelf_pending
                && app.shelf_generation.is_none()
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("subscriber My List deadline");
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        println!("subscriber admission: My List admitted");
        // No private item/card action. Return to Account, then actual SDL Select
        // executes the UI's explicit Logout command. Cleanup never replays it.
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
                start,
                &mut logout_issued,
                remote,
            )?;
        }
        if app.ui.page() != Page::Login || app.ui.focus() != Focus::LoginPrimary {
            return Err("logout native focus");
        }
        key(
            &mut app,
            &mut window,
            &mut painter,
            &runtime,
            start,
            &mut logout_issued,
            (40, 13),
        )?;
        if !logout_issued {
            return Err("explicit native logout");
        }
        Ok(())
    }))
    .unwrap_or(Err("interrupted journey"));
    let cleanup = catch_unwind(AssertUnwindSafe(|| {
        finish_admission(&mut app, &runtime, grant_seen, logout_issued)
    }))
    .unwrap_or(Err("interrupted cleanup (remote state unconfirmed)"));
    let text_cleanup = window.text_input(false).map_err(|_| "native text disposal");
    drop(app); // Retained private projections and unpainted frames lose their owner.
    runtime.shutdown_timeout(Duration::from_secs(2));
    painter.destroy();
    if let Err(phase) = journey {
        println!("subscriber admission: stopped during {phase}");
    }
    if cleanup.is_ok() {
        println!("subscriber admission: issuer logout acknowledged; application disposed");
    }
    cleanup?;
    text_cleanup?;
    journey?;
    println!("subscriber admission: journey passed");
    Ok(())
}

#[test]
#[ignore = "actual subscriber/provider and serialized SDL/GLES; private external mount; root executor only"]
fn native_subscriber_activation_my_list_and_logout_end_to_end() {
    // Redact panic payloads from every request worker as well as this test. The
    // root runs this test alone; restoration happens after all private owners die.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| eprintln!("subscriber admission: interrupted")));
    let outcome = std::panic::catch_unwind(run_subscriber_admission)
        .unwrap_or(Err("interrupted admission (remote state unconfirmed)"));
    std::panic::set_hook(previous_hook);
    if let Err(phase) = outcome {
        panic!("subscriber admission stopped during {phase}");
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
            let body = match request.target {
                criterion_account::Target::Bootstrap => br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec(),
                criterion_account::Target::WatchList(criterion_account::Region::Us) => br#"{"paging":{"page_limit":60},"type_counts":{"film":1},"playlist":[{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic private selection"}]}"#.to_vec(),
                _ => panic!("subscriber admission must remain read-only"),
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
                app.ui.handle(action, data)
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
