//! Opt-in Root-only configuration read; source candidates are never fetched.
use crate::*;
use criterion_provider::MediaId;
use criterion_session::{MonotonicClock, Session};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

trait Wait {
    fn until(&self, at: Duration) -> impl Future<Output = ()>;
}
trait ActivationOutput {
    fn write(
        &mut self,
        instructions: &criterion_session::LinkInstructions,
    ) -> Result<(), &'static str>;
}
#[derive(Default, serde::Serialize)]
struct Report {
    experiment: &'static str,
    drm_policy: &'static str,
    census: Census,
    outcome: &'static str,
    selection: Option<&'static str>,
    dash_nonempty: Option<bool>,
    license_present: Option<bool>,
    license_nonempty: Option<bool>,
    thumbnail_present: Option<bool>,
    logout: &'static str,
}
async fn measure<
    S: criterion_session::Transport,
    T: Transport,
    C: MonotonicClock + Clone,
    W: Wait,
    O: ActivationOutput,
>(
    session_transport: S,
    account_transport: T,
    clock: C,
    wait: W,
    output: &mut O,
    media_id: MediaId,
) -> Report {
    let census = Arc::new(Mutex::new(Census::default()));
    let session_transport = SessionOnly {
        inner: session_transport,
        census: census.clone(),
    };
    let account_transport = PlaybackOnly {
        inner: account_transport,
        census: census.clone(),
        media_id: media_id.clone(),
    };
    let started = clock.now();
    let session = Session::with_transport(
        criterion_session::Configuration::production(),
        session_transport,
        clock.clone(),
    );
    let account = AccountClient::with_transport(account_transport);
    let mut report = Report {
        experiment: "native_playback_configuration_only",
        drm_policy: "medium_measurement_not_platform_default",
        outcome: "not_started",
        logout: "not_issued_no_local_grant",
        ..Report::default()
    };
    let mut authorized = false;
    let result = async {
        let instructions = session
            .start_link()
            .await
            .map_err(|_| "activation_unavailable")?;
        output.write(&instructions)?;
        let activation_deadline = started
            .saturating_add(Duration::from_secs(180))
            .min(instructions.expires_at);
        loop {
            if clock.now() >= activation_deadline || clock.now() < started {
                return Err("activation_deadline");
            }
            match session
                .poll_once()
                .await
                .map_err(|_| "activation_unconfirmed")?
            {
                criterion_session::PollOutcome::Authorized => {
                    authorized = true;
                    if clock.now() >= activation_deadline || clock.now() < started {
                        return Err("activation_deadline");
                    }
                    break;
                }
                criterion_session::PollOutcome::WaitUntil(at)
                | criterion_session::PollOutcome::Pending(at) => {
                    if at >= activation_deadline {
                        return Err("activation_deadline");
                    }
                    wait.until(at).await;
                    if clock.now() < at {
                        return Err("activation_clock_refused");
                    }
                }
                criterion_session::PollOutcome::RetryAt(_) => return Err("activation_unconfirmed"),
            }
        }
        if clock.now() >= started.saturating_add(Duration::from_secs(210)) || clock.now() < started
        {
            return Err("read_deadline");
        }
        account.bootstrap().await.map_err(account_failure)?;
        if clock.now() >= started.saturating_add(Duration::from_secs(210)) || clock.now() < started
        {
            return Err("read_deadline");
        }
        let selection = account
            .playback(
                &session,
                NativePlaybackRequest {
                    media_id,
                    drm_policy: DrmPolicy::Medium,
                },
            )
            .await
            .map_err(account_failure)?;
        if clock.now() >= started.saturating_add(Duration::from_secs(220)) || clock.now() < started
        {
            return Err("read_deadline");
        }
        match selection {
            NativePlaybackSelection::Selected(playback) => {
                report.selection = Some("selected");
                report.dash_nonempty = Some(!playback.dash_file().is_empty());
                report.license_present = Some(playback.widevine_license().is_some());
                report.license_nonempty =
                    playback.widevine_license().map(|value| !value.is_empty());
                report.thumbnail_present = Some(playback.thumbnail().is_some());
            }
            NativePlaybackSelection::EmptyPlaylist => report.selection = Some("empty_playlist"),
            NativePlaybackSelection::NoDash => report.selection = Some("no_dash"),
        }
        Ok(())
    }
    .await;
    report.outcome = result.err().unwrap_or("measured_configuration_only");
    // No outer timeout drops an issued request. Production transports own their
    // ten-second bounds; every read has settled before this explicit logout.
    account.dispose();
    if authorized {
        report.logout = if session.logout().await.is_ok() {
            "acknowledged"
        } else {
            "unconfirmed"
        };
    } else {
        session.cancel();
    }
    session.dispose();
    report.census = census
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    report
}
fn account_failure(error: Error) -> &'static str {
    match error {
        Error::HttpStatus(_) => "http_status_refused",
        Error::Deadline => "read_deadline",
        Error::InvalidResponse => "invalid_response",
        Error::ResponseTooLarge => "response_too_large",
        _ => "read_unavailable",
    }
}

// A closed census permits only this attempt's fixed production operations.
// It retains status/counts, never a form, header, body or candidate string.
#[derive(Clone, Default, serde::Serialize)]
struct Census {
    device: usize,
    token_polls: usize,
    bootstrap: usize,
    playback: usize,
    revoke: usize,
    bootstrap_status: Option<u16>,
    playback_status: Option<u16>,
    revoke_status: Option<u16>,
}
struct SessionOnly<T> {
    inner: T,
    census: Arc<Mutex<Census>>,
}
impl<T: criterion_session::Transport> criterion_session::Transport for SessionOnly<T> {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        let endpoint = request.endpoint;
        {
            let mut census = self
                .census
                .lock()
                .map_err(|_| criterion_session::Error::InvalidRequest)?;
            let (count, max) = match endpoint {
                criterion_session::Endpoint::DeviceCode => (&mut census.device, 1),
                criterion_session::Endpoint::Token => {
                    let mut grants = url::form_urlencoded::parse(request.body.expose())
                        .filter(|(key, _)| key == "grant_type");
                    if !grants.next().is_some_and(|(_, value)| {
                        value == "urn:ietf:params:oauth:grant-type:device_code"
                    }) || grants.next().is_some()
                    {
                        return Err(criterion_session::Error::InvalidRequest);
                    }
                    (&mut census.token_polls, 60)
                }
                criterion_session::Endpoint::Revoke => (&mut census.revoke, 1),
            };
            if *count >= max {
                return Err(criterion_session::Error::InvalidRequest);
            }
            *count += 1;
        }
        let response = self.inner.post(request).await;
        if endpoint == criterion_session::Endpoint::Revoke {
            self.census
                .lock()
                .map_err(|_| criterion_session::Error::InvalidResponse)?
                .revoke_status = response.as_ref().ok().map(|value| value.status);
        }
        response
    }
}
struct PlaybackOnly<T> {
    inner: T,
    census: Arc<Mutex<Census>>,
    media_id: MediaId,
}
impl<T: Transport> Transport for PlaybackOnly<T> {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        let bootstrap = match &request {
            Request::Bootstrap => true,
            Request::Subscriber {
                target: SubscriberTarget::Playback { request, .. },
                ..
            } if request.media_id == self.media_id && request.drm_policy == DrmPolicy::Medium => {
                false
            }
            _ => return Err(Error::InvalidRequest),
        };
        {
            let mut census = self.census.lock().map_err(|_| Error::InvalidRequest)?;
            let count = if bootstrap {
                &mut census.bootstrap
            } else {
                &mut census.playback
            };
            if *count != 0 {
                return Err(Error::InvalidRequest);
            }
            *count += 1;
        }
        let response = self.inner.send(request).await;
        let mut census = self.census.lock().map_err(|_| Error::InvalidResponse)?;
        let status = if bootstrap {
            &mut census.bootstrap_status
        } else {
            &mut census.playback_status
        };
        *status = match &response {
            Ok(value) => Some(value.status),
            Err(Error::HttpStatus(value)) => Some(*value),
            _ => None,
        };
        response
    }
}

#[derive(Default)]
struct Calls(Mutex<Vec<&'static str>>);
struct SessionFixture {
    calls: Arc<Calls>,
    revoke: bool,
}
impl criterion_session::Transport for SessionFixture {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        let (call, body) = match request.endpoint {
            criterion_session::Endpoint::DeviceCode => ("device", br#"{"device_code":"private-device","user_code":"PRIVATE-CODE","verification_uri_complete":"https://login.criterion.com/activate?user_code=PRIVATE-CODE","expires_in":900,"interval":5}"#.as_slice()),
            criterion_session::Endpoint::Token => ("token", br#"{"access_token":"private-subscriber","refresh_token":"private-refresh","expires_in":3600}"#.as_slice()),
            criterion_session::Endpoint::Revoke => ("revoke", b"{}".as_slice()),
        };
        self.calls.0.lock().unwrap().push(call);
        Ok(criterion_session::Response {
            status: if call == "revoke" && !self.revoke {
                503
            } else {
                200
            },
            body: SecretBody::new(body.to_vec()),
        })
    }
}
struct AccountFixture {
    calls: Arc<Calls>,
    response: Result<(u16, Vec<u8>), Error>,
}
impl Transport for AccountFixture {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        let (status, body) = match request {
            Request::Bootstrap => {
                self.calls.0.lock().unwrap().push("bootstrap");
                (200, br#"{"country":"CA","token":"private-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec())
            }
            Request::Subscriber {
                target:
                    SubscriberTarget::Playback {
                        region: Region::Ca,
                        request,
                    },
                credentials,
            } => {
                self.calls.0.lock().unwrap().push("playback");
                assert_eq!(request.media_id.as_str(), "Chosen01");
                assert_eq!(request.drm_policy, DrmPolicy::Medium);
                assert_eq!(credentials.subscriber().as_bytes(), b"private-subscriber");
                self.response.clone()?
            }
            _ => panic!("unexpected admission target"),
        };
        Ok(Response {
            status,
            body: SecretBody::new(body),
        })
    }
}
#[derive(Clone, Default)]
struct Clock(Arc<Mutex<Duration>>);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap()
    }
}
impl Wait for Clock {
    async fn until(&self, at: Duration) {
        *self.0.lock().unwrap() = at;
    }
}
#[derive(Default)]
struct Output(Vec<u8>);
impl ActivationOutput for Output {
    fn write(
        &mut self,
        instructions: &criterion_session::LinkInstructions,
    ) -> Result<(), &'static str> {
        self.0
            .extend_from_slice(instructions.user_code.expose().as_bytes());
        Ok(())
    }
}
const SELECTED: &[u8] = br#"{"playlist":[{"contentType":"film","mediaid":"Returned","title":"PRIVATE-TITLE","sources":[{"type":"application/dash+xml","file":"PRIVATE-DASH","drm":{"widevine":{"url":"PRIVATE-LICENSE"}}}]}]}"#;

#[tokio::test]
async fn linked_configuration_read_uses_one_explicit_medium_request_and_acknowledged_logout() {
    let calls = Arc::new(Calls::default());
    let clock = Clock::default();
    let mut output = Output::default();
    let report = measure(
        SessionFixture {
            calls: calls.clone(),
            revoke: true,
        },
        AccountFixture {
            calls: calls.clone(),
            response: Ok((200, SELECTED.to_vec())),
        },
        clock.clone(),
        clock,
        &mut output,
        MediaId::new("Chosen01").unwrap(),
    )
    .await;
    assert_eq!(
        *calls.0.lock().unwrap(),
        ["device", "token", "bootstrap", "playback", "revoke"]
    );
    assert_eq!(report.selection, Some("selected"));
    assert_eq!(report.dash_nonempty, Some(true));
    assert_eq!(report.license_present, Some(true));
    assert_eq!(report.logout, "acknowledged");
    assert_eq!(output.0, b"PRIVATE-CODE");
    let encoded = serde_json::to_string(&report).unwrap();
    for private in [
        "Chosen01",
        "Returned",
        "PRIVATE-TITLE",
        "PRIVATE-CODE",
        "PRIVATE-DASH",
        "PRIVATE-LICENSE",
        "private-subscriber",
        "private-refresh",
        "private-bootstrap",
    ] {
        assert!(!encoded.contains(private));
    }
}

#[tokio::test]
async fn unselected_and_failed_playback_settle_then_logout_without_any_additional_read() {
    let cases = [
        (Ok((200, br#"{"playlist":[]}"#.to_vec())), "measured_configuration_only", Some("empty_playlist")),
        (Ok((200, br#"{"playlist":[{"contentType":"film","mediaid":"Returned","title":"PRIVATE-TITLE","sources":[]}] }"#.to_vec())), "measured_configuration_only", Some("no_dash")),
        (Ok((403, SELECTED.to_vec())), "http_status_refused", None),
        (Ok((200, br#"{"playlist":null,"private":"PRIVATE-RAW"}"#.to_vec())), "invalid_response", None),
        (Err(Error::Deadline), "read_deadline", None),
    ];
    for (response, outcome, selection) in cases {
        let calls = Arc::new(Calls::default());
        let clock = Clock::default();
        let report = measure(
            SessionFixture {
                calls: calls.clone(),
                revoke: true,
            },
            AccountFixture {
                calls: calls.clone(),
                response,
            },
            clock.clone(),
            clock,
            &mut Output::default(),
            MediaId::new("Chosen01").unwrap(),
        )
        .await;
        assert_eq!(
            *calls.0.lock().unwrap(),
            ["device", "token", "bootstrap", "playback", "revoke"]
        );
        assert_eq!(report.outcome, outcome);
        assert_eq!(report.selection, selection);
        assert_eq!(report.logout, "acknowledged");
        assert_eq!(report.census.playback, 1);
        assert!(!serde_json::to_string(&report).unwrap().contains("PRIVATE"));
    }
}

struct HeldRevoke {
    calls: Arc<Calls>,
    issued: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl criterion_session::Transport for HeldRevoke {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        if request.endpoint == criterion_session::Endpoint::Revoke {
            self.issued.notify_one();
            self.release.notified().await;
        }
        SessionFixture {
            calls: self.calls.clone(),
            revoke: false,
        }
        .post(request)
        .await
    }
}
#[tokio::test]
async fn issued_logout_is_joined_and_an_unconfirmed_result_is_retained_without_revoke_retry() {
    let calls = Arc::new(Calls::default());
    let issued = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let clock = Clock::default();
    let mut output = Output::default();
    let attempt = measure(
        HeldRevoke {
            calls: calls.clone(),
            issued: issued.clone(),
            release: release.clone(),
        },
        AccountFixture {
            calls: calls.clone(),
            response: Ok((200, SELECTED.to_vec())),
        },
        clock.clone(),
        clock,
        &mut output,
        MediaId::new("Chosen01").unwrap(),
    );
    tokio::pin!(attempt);
    tokio::select! { biased; _ = issued.notified() => {}, _ = &mut attempt => panic!("returned before issued logout settled") }
    assert_eq!(
        *calls.0.lock().unwrap(),
        ["device", "token", "bootstrap", "playback"]
    );
    release.notify_one();
    let report = attempt.await;
    assert_eq!(report.logout, "unconfirmed");
    assert_eq!(report.census.revoke, 1);
    assert_eq!(report.census.revoke_status, Some(503));
    assert_eq!(
        *calls.0.lock().unwrap(),
        ["device", "token", "bootstrap", "playback", "revoke"]
    );
}

const PRIVATE_DIRECTORY: &str = "/criterion-playback-admission";
const MAX_MOUNTINFO: u64 = 65_536;
const MAX_REPORT: u64 = 4_096;

/// Root creates a fresh host /tmp (or /private/tmp) directory of this exact
/// prefix, chmod0700, and binds it here separately from read-only /workspace.
fn external_mount(mounts: &str) -> Result<(), &'static str> {
    let root = |point| {
        mounts.lines().find_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            let root = fields.nth(3)?;
            (fields.next()? == point).then_some(root)
        })
    };
    let private = std::path::Path::new(root(PRIVATE_DIRECTORY).ok_or("private_mount")?);
    let workspace = std::path::Path::new(root("/workspace").ok_or("workspace_mount")?);
    if !private.is_absolute()
        || !workspace.is_absolute()
        || private.starts_with(workspace)
        || workspace.starts_with(private)
        || private.as_os_str().as_encoded_bytes().contains(&b'\\')
        || workspace.as_os_str().as_encoded_bytes().contains(&b'\\')
        || !matches!(
            private.parent().and_then(std::path::Path::to_str),
            Some("/tmp" | "/private/tmp")
        )
        || !private
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| {
                name.strip_prefix("criterion-playback-admission.")
                    .is_some_and(|suffix| {
                        (6..=64).contains(&suffix.len())
                            && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    })
            })
    {
        return Err("external_private_mount");
    }
    Ok(())
}
fn check_live_mount() -> Result<(), &'static str> {
    use std::io::Read;
    let mut mounts = String::new();
    std::fs::File::open("/proc/self/mountinfo")
        .map_err(|_| "private_mount")?
        .take(MAX_MOUNTINFO + 1)
        .read_to_string(&mut mounts)
        .map_err(|_| "private_mount")?;
    if mounts.len() as u64 > MAX_MOUNTINFO {
        return Err("private_mount");
    }
    external_mount(&mounts)
}
struct PrivateOutput {
    activation: std::fs::File,
    report: std::fs::File,
    activation_path: std::path::PathBuf,
}
impl PrivateOutput {
    fn reserve(path: &std::path::Path) -> Result<Self, &'static str> {
        use std::{
            io::Write,
            os::unix::fs::{MetadataExt, OpenOptionsExt},
        };
        let directory = std::fs::symlink_metadata(path).map_err(|_| "private_directory")?;
        let process = std::fs::metadata("/proc/self").map_err(|_| "private_owner")?;
        if !directory.is_dir()
            || directory.mode() & 0o7777 != 0o700
            || (process.uid() != 0 && process.uid() != directory.uid())
            || std::fs::read_dir(path)
                .map_err(|_| "private_directory")?
                .next()
                .is_some()
        {
            return Err("fresh_private_directory");
        }
        let create = |name| -> Result<std::fs::File, &'static str> {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path.join(name))
                .map_err(|_| "private_file_reservation")?;
            let metadata = file.metadata().map_err(|_| "private_file")?;
            if !metadata.is_file() || metadata.mode() & 0o7777 != 0o600 || metadata.nlink() != 1 {
                return Err("private_file");
            }
            Ok(file)
        };
        let mut attempt = create("attempt.json")?;
        attempt.write_all(br#"{"experiment":"native_playback_configuration_only","drmPolicy":"medium_measurement_not_platform_default","maxDeviceCode":1,"maxTokenPolls":60,"maxBootstrap":1,"maxPlayback":1,"maxRevoke":1,"state":"reserved_before_contact"}"#).map_err(|_| "private_attempt")?;
        attempt.sync_all().map_err(|_| "private_attempt")?;
        Ok(Self {
            activation: create("activation.json")?,
            report: create("report.json")?,
            activation_path: path.join("activation.json"),
        })
    }
    fn finish(&mut self, report: &Report) -> Result<(), &'static str> {
        use std::io::Write;
        let encoded = serde_json::to_vec(report).map_err(|_| "report_encoding")?;
        if encoded.len() as u64 > MAX_REPORT {
            return Err("report_bound");
        }
        self.report
            .write_all(&encoded)
            .map_err(|_| "private_report")?;
        self.report.sync_all().map_err(|_| "private_report")?;
        // An unconfirmed grant/revoke retains the directory and private artifact
        // for Root's held outcome. The attempt ledger always prevents reuse.
        if report.logout == "acknowledged" {
            std::fs::remove_file(&self.activation_path).map_err(|_| "activation_cleanup")?;
        }
        Ok(())
    }
}
impl ActivationOutput for PrivateOutput {
    fn write(
        &mut self,
        instructions: &criterion_session::LinkInstructions,
    ) -> Result<(), &'static str> {
        use std::io::Write;
        #[derive(serde::Serialize)]
        struct Instructions<'a> {
            user_code: &'a str,
            verification_uri_complete: &'a str,
        }
        let value = Instructions {
            user_code: instructions.user_code.expose(),
            verification_uri_complete: instructions.verification_uri_complete.expose(),
        };
        if value.user_code.len() > 128 || value.verification_uri_complete.len() > 2048 {
            return Err("activation_bound");
        }
        serde_json::to_writer(&mut self.activation, &value).map_err(|_| "private_activation")?;
        self.activation.flush().map_err(|_| "private_activation")?;
        self.activation.sync_all().map_err(|_| "private_activation")
    }
}

struct PublicFilm;
impl criterion_provider::RequestTransport for PublicFilm {
    async fn get(
        &self,
        request: criterion_provider::Request,
    ) -> Result<criterion_provider::Response, criterion_provider::Error> {
        // Only the admitted public fixture is read. No production catalog fetch.
        if request.url.as_str() != "https://www.criterionchannel.com/api/media/qvwT6mJ4" {
            return Err(criterion_provider::Error::InvalidRequest);
        }
        Ok(criterion_provider::Response {
            status: 200,
            content_type: "application/json".into(),
            body: include_bytes!("../../../tests/fixtures/provider/media-film.json").to_vec(),
        })
    }
}
async fn supplied_public_film() -> Result<MediaId, &'static str> {
    let selected = MediaId::new("qvwT6mJ4").map_err(|_| "public_fixture")?;
    let detail = criterion_provider::Catalog::with_transport(PublicFilm)
        .detail(&selected)
        .await
        .map_err(|_| "public_fixture")?;
    if detail.media.kind != criterion_provider::MediaKind::Film {
        return Err("ordinary_film_fixture");
    }
    Ok(detail.media.id)
}
struct LiveWait(criterion_session::SystemClock);
impl Wait for LiveWait {
    async fn until(&self, at: Duration) {
        tokio::time::sleep(at.saturating_sub(self.0.now())).await;
    }
}

/// Compile/review offline first. Root alone supplies one fresh private mount and
/// network bridge, completes first-party activation from activation.json, and
/// runs with a separate240s process watchdog. Killing a process is an unresolved
/// remote outcome: preserve the ledger, never rerun/retry that attempt.
///
/// Medium measures the signed reference's non-L1 request. This grants no webOS
/// policy, URL, CDM, license or playback capability; all candidates die in memory.
#[tokio::test(flavor = "current_thread")]
#[ignore = "Root-only linked configuration GET; requires fresh private external mount and explicit execution"]
async fn live_authenticated_playback_configuration_once() -> Result<(), &'static str> {
    check_live_mount()?;
    let mut output = PrivateOutput::reserve(std::path::Path::new(PRIVATE_DIRECTORY))?;
    let media_id = supplied_public_film().await?;
    let session = criterion_session::HttpTransport::new().map_err(|_| "session_transport")?;
    let account = HttpTransport::new().map_err(|_| "account_transport")?;
    let clock = criterion_session::SystemClock::default();
    let report = measure(
        session,
        account,
        clock.clone(),
        LiveWait(clock),
        &mut output,
        media_id,
    )
    .await;
    output.finish(&report)?;
    if report.logout != "acknowledged" {
        return Err("logout_or_activation_unconfirmed_held_no_retry");
    }
    if report.outcome != "measured_configuration_only" {
        return Err("configuration_measurement_refused");
    }
    Ok(())
}

struct PollScript {
    calls: Arc<Calls>,
    fast: bool,
    replies: Mutex<std::collections::VecDeque<u16>>,
}
impl criterion_session::Transport for PollScript {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        if request.endpoint == criterion_session::Endpoint::DeviceCode && self.fast {
            self.calls.0.lock().unwrap().push("device");
            return Ok(criterion_session::Response { status: 200, body: SecretBody::new(br#"{"device_code":"private-device","user_code":"PRIVATE-CODE","verification_uri_complete":"https://login.criterion.com/activate?user_code=PRIVATE-CODE","expires_in":900,"interval":1}"#.to_vec()) });
        }
        if request.endpoint == criterion_session::Endpoint::Token {
            let status = self.replies.lock().unwrap().pop_front().unwrap_or(400);
            if status != 200 {
                self.calls.0.lock().unwrap().push("token");
                return Ok(criterion_session::Response {
                    status,
                    body: SecretBody::new(br#"{"error":"authorization_pending"}"#.to_vec()),
                });
            }
        }
        SessionFixture {
            calls: self.calls.clone(),
            revoke: true,
        }
        .post(request)
        .await
    }
}
#[tokio::test]
async fn native_pending_poll_is_scheduled_but_transient_failure_is_never_retried() {
    for (replies, expected, outcome, logout) in [
        (
            vec![400, 200],
            vec![
                "device",
                "token",
                "token",
                "bootstrap",
                "playback",
                "revoke",
            ],
            "measured_configuration_only",
            "acknowledged",
        ),
        (
            vec![503, 200],
            vec!["device", "token"],
            "activation_unconfirmed",
            "not_issued_no_local_grant",
        ),
        (
            vec![400; 60],
            vec!["device"]
                .into_iter()
                .chain(std::iter::repeat_n("token", 35))
                .collect(),
            "activation_deadline",
            "not_issued_no_local_grant",
        ),
    ] {
        let calls = Arc::new(Calls::default());
        let clock = Clock::default();
        let report = measure(
            PollScript {
                calls: calls.clone(),
                fast: false,
                replies: Mutex::new(replies.into()),
            },
            AccountFixture {
                calls: calls.clone(),
                response: Ok((200, SELECTED.to_vec())),
            },
            clock.clone(),
            clock,
            &mut Output::default(),
            MediaId::new("Chosen01").unwrap(),
        )
        .await;
        assert_eq!(*calls.0.lock().unwrap(), expected);
        assert_eq!(report.outcome, outcome);
        assert_eq!(report.logout, logout);
    }
}
struct FailedOutput;
impl ActivationOutput for FailedOutput {
    fn write(&mut self, _: &criterion_session::LinkInstructions) -> Result<(), &'static str> {
        Err("private_activation")
    }
}
#[tokio::test]
async fn activation_output_failure_prevents_poll_bootstrap_and_playback_contact() {
    let calls = Arc::new(Calls::default());
    let clock = Clock::default();
    let report = measure(
        SessionFixture {
            calls: calls.clone(),
            revoke: true,
        },
        AccountFixture {
            calls: calls.clone(),
            response: Ok((200, SELECTED.to_vec())),
        },
        clock.clone(),
        clock,
        &mut FailedOutput,
        MediaId::new("Chosen01").unwrap(),
    )
    .await;
    assert_eq!(*calls.0.lock().unwrap(), ["device"]);
    assert_eq!(report.outcome, "private_activation");
    assert_eq!(report.census.playback, 0);
    assert_eq!(report.census.revoke, 0);
}
#[tokio::test]
async fn selected_film_is_the_existing_admitted_public_fixture() {
    assert_eq!(supplied_public_film().await.unwrap().as_str(), "qvwT6mJ4");
}
#[test]
fn private_mount_is_fresh_external_temp_storage_and_not_any_checkout() {
    for root in [
        "/tmp/criterion-playback-admission.a1B2c3",
        "/private/tmp/criterion-playback-admission.a1B2c3",
    ] {
        assert!(external_mount(&format!("1 0 0:1 {root} /criterion-playback-admission rw - x x rw\n2 0 0:2 /Users/mike/Code/criterion-webos /workspace ro - x x ro\n")).is_ok());
    }
    for root in [
        "/Users/mike/Code/criterion-webos/.local",
        "/tmp/criterion-playback-admission.a",
        "/tmp/criterion-playback-admission.ab%def",
        "/tmp/a/../criterion-playback-admission.a1B2c3",
    ] {
        assert!(external_mount(&format!("1 0 0:1 {root} /criterion-playback-admission rw - x x rw\n2 0 0:2 /Users/mike/Code/criterion-webos /workspace ro - x x ro\n")).is_err());
    }
    assert!(external_mount("").is_err());
}

struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "criterion-playback-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[tokio::test]
async fn private_files_hold_unknown_logout_and_remove_activation_only_after_acknowledgment() {
    use std::os::unix::fs::MetadataExt;
    for confirmed in [false, true] {
        let directory = Temp::new();
        let mut output = PrivateOutput::reserve(&directory.0).unwrap();
        let calls = Arc::new(Calls::default());
        let clock = Clock::default();
        let report = measure(
            SessionFixture {
                calls: calls.clone(),
                revoke: confirmed,
            },
            AccountFixture {
                calls: calls.clone(),
                response: Ok((200, SELECTED.to_vec())),
            },
            clock.clone(),
            clock,
            &mut output,
            MediaId::new("Chosen01").unwrap(),
        )
        .await;
        output.finish(&report).unwrap();
        assert_eq!(directory.0.join("activation.json").exists(), !confirmed);
        if !confirmed {
            let activation = std::fs::read(directory.0.join("activation.json")).unwrap();
            assert!(
                std::str::from_utf8(&activation)
                    .unwrap()
                    .contains("PRIVATE-CODE")
            );
        }
        for name in ["report.json", "attempt.json"] {
            let path = directory.0.join(name);
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            assert_eq!(metadata.mode() & 0o7777, 0o600);
            assert_eq!(metadata.nlink(), 1);
            let public = std::fs::read_to_string(path).unwrap();
            for secret in [
                "PRIVATE",
                "private-subscriber",
                "private-refresh",
                "private-bootstrap",
                "Chosen01",
                "Returned",
            ] {
                assert!(!public.contains(secret));
            }
        }
        assert!(
            PrivateOutput::reserve(&directory.0).is_err(),
            "ledger prevents repeating even after acknowledged cleanup"
        );
    }
}
#[test]
fn private_output_refuses_existing_files_symlinks_and_permissive_directories() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = Temp::new();
    std::fs::set_permissions(&directory.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(PrivateOutput::reserve(&directory.0).is_err());
    std::fs::set_permissions(&directory.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    symlink("/dev/null", directory.0.join("activation.json")).unwrap();
    assert!(PrivateOutput::reserve(&directory.0).is_err());
    std::fs::remove_file(directory.0.join("activation.json")).unwrap();
    std::fs::write(directory.0.join("report.json"), "existing").unwrap();
    assert!(PrivateOutput::reserve(&directory.0).is_err());
    assert_eq!(
        std::fs::read(directory.0.join("report.json")).unwrap(),
        b"existing"
    );
}

struct StagnantOnce {
    clock: Clock,
    first: std::sync::atomic::AtomicBool,
}
impl Wait for StagnantOnce {
    async fn until(&self, at: Duration) {
        if !self.first.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        self.clock.until(at).await;
    }
}
#[tokio::test]
async fn a_wait_that_has_not_reached_the_native_poll_time_refuses_before_any_token_request() {
    let calls = Arc::new(Calls::default());
    let clock = Clock::default();
    let report = measure(
        SessionFixture {
            calls: calls.clone(),
            revoke: true,
        },
        AccountFixture {
            calls: calls.clone(),
            response: Ok((200, SELECTED.to_vec())),
        },
        clock.clone(),
        StagnantOnce {
            clock,
            first: std::sync::atomic::AtomicBool::new(false),
        },
        &mut Output::default(),
        MediaId::new("Chosen01").unwrap(),
    )
    .await;
    assert_eq!(*calls.0.lock().unwrap(), ["device"]);
    assert_eq!(report.outcome, "activation_clock_refused");
}

#[tokio::test]
async fn minimum_native_interval_still_cannot_issue_more_than_sixty_grant_polls() {
    let calls = Arc::new(Calls::default());
    let clock = Clock::default();
    let report = measure(
        PollScript {
            calls: calls.clone(),
            fast: true,
            replies: Mutex::new(vec![400; 61].into()),
        },
        AccountFixture {
            calls: calls.clone(),
            response: Ok((200, SELECTED.to_vec())),
        },
        clock.clone(),
        clock,
        &mut Output::default(),
        MediaId::new("Chosen01").unwrap(),
    )
    .await;
    let actual = calls.0.lock().unwrap();
    assert_eq!(actual.len(), 61);
    assert_eq!(actual[0], "device");
    assert!(actual[1..].iter().all(|call| *call == "token"));
    assert_eq!(report.census.token_polls, 60);
    assert_eq!(report.census.bootstrap, 0);
    assert_eq!(report.outcome, "activation_unconfirmed");
}

struct LateGrant {
    inner: SessionFixture,
    clock: Clock,
}
impl criterion_session::Transport for LateGrant {
    async fn post(
        &self,
        request: criterion_session::Request,
    ) -> Result<criterion_session::Response, criterion_session::Error> {
        let token = request.endpoint == criterion_session::Endpoint::Token;
        let response = self.inner.post(request).await;
        if token {
            *self.clock.0.lock().unwrap() = Duration::from_secs(181);
        }
        response
    }
}
#[tokio::test]
async fn authorization_settled_after_original_activation_deadline_is_logged_out_without_read() {
    let calls = Arc::new(Calls::default());
    let clock = Clock::default();
    let report = measure(
        LateGrant {
            inner: SessionFixture {
                calls: calls.clone(),
                revoke: true,
            },
            clock: clock.clone(),
        },
        AccountFixture {
            calls: calls.clone(),
            response: Ok((200, SELECTED.to_vec())),
        },
        clock.clone(),
        clock,
        &mut Output::default(),
        MediaId::new("Chosen01").unwrap(),
    )
    .await;
    assert_eq!(*calls.0.lock().unwrap(), ["device", "token", "revoke"]);
    assert_eq!(report.outcome, "activation_deadline");
    assert_eq!(report.logout, "acknowledged");
}

struct LateRead {
    inner: AccountFixture,
    clock: Clock,
    late_bootstrap: bool,
}
impl Transport for LateRead {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        let bootstrap = matches!(request, Request::Bootstrap);
        let response = self.inner.send(request).await;
        if bootstrap == self.late_bootstrap {
            *self.clock.0.lock().unwrap() = Duration::from_secs(if bootstrap { 210 } else { 220 });
        }
        response
    }
}
#[tokio::test]
async fn settled_read_at_its_original_deadline_cannot_admit_a_candidate_or_issue_another_read() {
    for late_bootstrap in [true, false] {
        let calls = Arc::new(Calls::default());
        let clock = Clock::default();
        let report = measure(
            SessionFixture {
                calls: calls.clone(),
                revoke: true,
            },
            LateRead {
                inner: AccountFixture {
                    calls: calls.clone(),
                    response: Ok((200, SELECTED.to_vec())),
                },
                clock: clock.clone(),
                late_bootstrap,
            },
            clock.clone(),
            clock,
            &mut Output::default(),
            MediaId::new("Chosen01").unwrap(),
        )
        .await;
        assert_eq!(
            *calls.0.lock().unwrap(),
            if late_bootstrap {
                vec!["device", "token", "bootstrap", "revoke"]
            } else {
                vec!["device", "token", "bootstrap", "playback", "revoke"]
            }
        );
        assert_eq!(report.outcome, "read_deadline");
        assert_eq!(report.selection, None);
        assert_eq!(report.dash_nonempty, None);
        assert_eq!(report.logout, "acknowledged");
    }
}
