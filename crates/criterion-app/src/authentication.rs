// SPDX-License-Identifier: GPL-3.0-or-later
//! Main-thread linking UI owner. Account credentials remain in the reviewed session.
use crate::jobs::Jobs;
use criterion_session::{
    Configuration, Error, HttpTransport, LinkInstructions, MonotonicClock, PollOutcome, Session,
    Status, SystemClock, Transport,
};
use criterion_ui::LoginView;
use std::{sync::Arc, time::Duration};
use tokio::runtime::{Handle, Runtime};

enum Completed {
    Link(LinkInstructions),
    Poll(PollOutcome),
    Refresh,
    Logout,
}
#[derive(Clone, Copy)]
enum Phase {
    SignedOut,
    Requesting,
    Awaiting,
    SignedIn,
    SigningOut,
    Expired,
    Denied,
    Error,
}
pub(crate) struct Authentication<T: Transport = HttpTransport, C: MonotonicClock = SystemClock> {
    session: Arc<Session<T, C>>,
    clock: C,
    jobs: Jobs<Result<Completed, Error>>,
    phase: Phase,
    link: Option<LinkInstructions>,
    next_poll: Duration,
    generation: u64,
    logout_requested: bool,
    revocation_confirmed: Option<bool>,
}
impl Authentication {
    pub(crate) fn new() -> Result<Self, Error> {
        let clock = SystemClock::default();
        Ok(Self::with_session(
            Arc::new(Session::with_transport(
                Configuration::production(),
                HttpTransport::new()?,
                clock.clone(),
            )),
            clock,
        ))
    }
}
impl<T: Transport + 'static, C: MonotonicClock + Clone + 'static> Authentication<T, C> {
    pub(crate) fn with_session(session: Arc<Session<T, C>>, clock: C) -> Self {
        Self {
            session,
            clock,
            jobs: Jobs::new(),
            phase: Phase::SignedOut,
            link: None,
            next_poll: Duration::ZERO,
            generation: 0,
            logout_requested: false,
            revocation_confirmed: None,
        }
    }
    pub(crate) fn begin(&mut self, runtime: &Handle) {
        if self.signed_in()
            || matches!(
                self.phase,
                Phase::Requesting | Phase::Awaiting | Phase::SigningOut
            )
        {
            return;
        }
        let Some(generation) = self.generation.checked_add(1) else {
            self.phase = Phase::Error;
            return;
        };
        self.cancel();
        self.revocation_confirmed = None;
        self.generation = generation;
        self.phase = Phase::Requesting;
        let session = self.session.clone();
        self.jobs.replace(runtime, async move {
            session.start_link().await.map(Completed::Link)
        });
    }
    pub(crate) fn cancel(&mut self) {
        if matches!(self.phase, Phase::SigningOut | Phase::SignedIn) {
            return;
        }
        self.jobs.cancel();
        self.session.cancel();
        self.link = None;
        self.phase = if self.signed_in() {
            Phase::SignedIn
        } else {
            Phase::SignedOut
        };
    }
    pub(crate) fn logout(&mut self, runtime: &Handle) {
        if matches!(self.phase, Phase::SigningOut) {
            return;
        }
        if !self.signed_in() {
            if matches!(self.phase, Phase::SignedIn) {
                // A finished, unpublished refresh failure can already have erased
                // the credential. Explicit logout cannot confirm that lost revoke.
                self.phase = Phase::Error;
                self.revocation_confirmed = Some(false);
            }
            return;
        }
        self.link = None;
        self.phase = Phase::SigningOut;
        self.logout_requested = true;
        self.revocation_confirmed = None;
        // An issued refresh may rotate the sole revocable credential. Let that
        // bounded request settle before asking the session to revoke its result.
        self.issue_logout(runtime);
    }
    pub(crate) fn signed_in(&self) -> bool {
        matches!(
            self.session.status(),
            Status::SignedIn { .. } | Status::RefreshRequired
        )
    }
    pub(crate) fn finish(&mut self, runtime: &Runtime) -> bool {
        while self.logout_requested && self.jobs.is_active() {
            if let Some(result) = runtime.block_on(self.jobs.finish()) {
                self.completed(result);
                self.issue_logout(runtime.handle());
            } else {
                self.logout_requested = false;
                self.revocation_confirmed = Some(false);
                self.phase = Phase::Error;
            }
        }
        self.revocation_confirmed.unwrap_or(true)
    }
    pub(crate) fn poll(&mut self, runtime: &Runtime, active: bool) {
        if let Some(result) = runtime.block_on(self.jobs.take_ready()) {
            self.completed(result);
        }
        // Explicit logout remains owned while backgrounded. Ordinary polling and
        // refresh wait for foreground, but an issued action must finish.
        self.issue_logout(runtime.handle());
        let status = self.session.status();
        if matches!(self.phase, Phase::Awaiting) && status == Status::Expired {
            self.link = None;
            self.phase = Phase::Expired;
        } else if matches!(self.phase, Phase::SignedIn)
            && !matches!(status, Status::SignedIn { .. } | Status::RefreshRequired)
        {
            self.phase = Phase::Error;
        }
        if self.jobs.is_active() || !active || self.logout_requested {
            return;
        }
        if matches!(status, Status::RefreshRequired) {
            let session = self.session.clone();
            self.jobs.replace(runtime.handle(), async move {
                session.refresh().await.map(|()| Completed::Refresh)
            });
        } else if matches!(self.phase, Phase::Awaiting) && self.clock.now() >= self.next_poll {
            let session = self.session.clone();
            self.jobs.replace(runtime.handle(), async move {
                session.poll_once().await.map(Completed::Poll)
            });
        }
    }
    fn issue_logout(&mut self, runtime: &Handle) {
        if !self.logout_requested || self.jobs.is_active() {
            return;
        }
        let session = self.session.clone();
        self.jobs.replace(runtime, async move {
            session.logout().await.map(|()| Completed::Logout)
        });
    }
    fn completed(&mut self, result: Result<Result<Completed, Error>, tokio::task::JoinError>) {
        if self.logout_requested
            && !matches!(&result, Ok(Ok(Completed::Refresh | Completed::Logout)))
        {
            // An uncertain rotation may have consumed the old credential. A
            // subsequent no-token logout cannot prove remote revocation.
            self.logout_requested = false;
            self.revocation_confirmed = Some(false);
            self.link = None;
            self.phase = Phase::Error;
            return;
        }
        match result {
            Ok(Ok(Completed::Link(link))) => {
                self.link = Some(link);
                self.phase = Phase::Awaiting;
                self.next_poll = self.clock.now();
            }
            Ok(Ok(Completed::Poll(PollOutcome::Authorized) | Completed::Refresh)) => {
                self.link = None;
                if !self.logout_requested {
                    self.phase = Phase::SignedIn;
                }
            }
            Ok(Ok(Completed::Poll(
                PollOutcome::WaitUntil(at) | PollOutcome::Pending(at) | PollOutcome::RetryAt(at),
            ))) => {
                self.next_poll = at;
            }
            Ok(Ok(Completed::Logout)) => {
                self.logout_requested = false;
                self.revocation_confirmed = Some(true);
                self.phase = Phase::SignedOut;
            }
            Ok(Err(Error::Expired)) => {
                self.link = None;
                self.phase = Phase::Expired;
            }
            Ok(Err(Error::Denied)) => {
                self.link = None;
                self.phase = Phase::Denied;
            }
            _ => {
                self.link = None;
                self.phase = Phase::Error;
            }
        }
    }
    pub(crate) fn view(&self) -> LoginView<'_> {
        match self.phase {
            Phase::SignedOut => LoginView::SignedOut,
            Phase::Requesting => LoginView::Requesting,
            Phase::Awaiting => match &self.link {
                Some(link) => LoginView::Awaiting {
                    generation: self.generation,
                    user_code: link.user_code.expose(),
                    verification_uri_complete: link.verification_uri_complete.expose(),
                    remaining_seconds: link
                        .expires_at
                        .saturating_sub(self.clock.now())
                        .as_secs()
                        .min(u64::from(u32::MAX)) as u32,
                },
                None => LoginView::Error,
            },
            Phase::SignedIn => LoginView::SignedIn,
            Phase::SigningOut => LoginView::SigningOut,
            Phase::Expired => LoginView::Expired,
            Phase::Denied => LoginView::Denied,
            Phase::Error => LoginView::Error,
        }
    }
}
impl<T: Transport, C: MonotonicClock> Drop for Authentication<T, C> {
    fn drop(&mut self) {
        self.jobs.cancel();
        self.link = None;
        self.session.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use criterion_session::{Endpoint, Request, Response, SecretBody};
    use std::sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    };

    #[derive(Clone)]
    struct Clock(Arc<AtomicU64>);
    impl MonotonicClock for Clock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }
    #[derive(Clone)]
    struct Fixture {
        calls: Arc<Mutex<Vec<(Endpoint, String)>>>,
        refresh_entered: Arc<tokio::sync::Notify>,
        refresh_release: Arc<tokio::sync::Notify>,
        revoke_fails: bool,
        refresh_fails: bool,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                refresh_entered: Arc::new(tokio::sync::Notify::new()),
                refresh_release: Arc::new(tokio::sync::Notify::new()),
                revoke_fails: false,
                refresh_fails: false,
            }
        }
    }
    fn json(body: &str) -> Response {
        Response {
            status: 200,
            body: SecretBody::new(body.as_bytes().to_vec()),
        }
    }
    impl Transport for Fixture {
        async fn post(&self, request: Request) -> Result<Response, Error> {
            // These forms contain only fixed test credentials, never live data.
            let body = String::from_utf8(request.body.expose().to_vec()).unwrap();
            let refresh = body.contains("grant_type=refresh_token");
            self.calls.lock().unwrap().push((request.endpoint, body));
            match request.endpoint {
                Endpoint::DeviceCode => Ok(json(
                    r#"{"device_code":"test-device","user_code":"ABCD-EFGH","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD-EFGH","expires_in":900,"interval":5}"#,
                )),
                Endpoint::Token if refresh => {
                    self.refresh_entered.notify_one();
                    self.refresh_release.notified().await;
                    if self.refresh_fails {
                        return Err(Error::Deadline);
                    }
                    Ok(json(
                        r#"{"access_token":"new-test-access","refresh_token":"rotated-test-refresh","expires_in":3600}"#,
                    ))
                }
                Endpoint::Token => Ok(json(
                    r#"{"access_token":"test-access","refresh_token":"test-refresh","expires_in":3600}"#,
                )),
                Endpoint::Revoke if self.revoke_fails => Err(Error::Deadline),
                Endpoint::Revoke => Ok(json("")),
            }
        }
    }
    fn runtime() -> Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }
    fn linked(runtime: &Runtime, fixture: Fixture) -> Authentication<Fixture, Clock> {
        let clock = Clock(Arc::new(AtomicU64::new(0)));
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            fixture,
            clock.clone(),
        ));
        runtime.block_on(session.start_link()).unwrap();
        clock.0.store(5, Ordering::SeqCst);
        assert_eq!(
            runtime.block_on(session.poll_once()),
            Ok(PollOutcome::Authorized)
        );
        let mut owner = Authentication::with_session(session, clock);
        owner.phase = Phase::SignedIn;
        owner
    }
    fn settle<T: Transport + 'static>(owner: &mut Authentication<T, Clock>, runtime: &Runtime) {
        let result = runtime.block_on(owner.jobs.finish()).unwrap();
        owner.completed(result);
        owner.poll(runtime, false);
    }
    #[test]
    fn logout_waits_for_issued_rotation_then_revokes_new_token() {
        let runtime = runtime();
        let fixture = Fixture::new();
        let mut owner = linked(&runtime, fixture.clone());
        owner.clock.0.store(3605, Ordering::SeqCst);
        owner.poll(&runtime, true);
        runtime.block_on(fixture.refresh_entered.notified());
        owner.logout(runtime.handle());
        fixture.refresh_release.notify_one();
        assert!(
            owner.finish(&runtime),
            "issued rotation and revoke should both settle"
        );
        let calls = fixture.calls.lock().unwrap();
        let revokes: Vec<_> = calls
            .iter()
            .filter(|(endpoint, _)| *endpoint == Endpoint::Revoke)
            .collect();
        assert_eq!(
            revokes.len(),
            1,
            "logout must actually issue remote revocation"
        );
        assert!(revokes[0].1.contains("token=rotated-test-refresh"));
        assert!(matches!(owner.view(), LoginView::SignedOut));
    }
    #[test]
    fn failed_revoke_remains_unconfirmed_after_error_was_published() {
        let runtime = runtime();
        let mut fixture = Fixture::new();
        fixture.revoke_fails = true;
        let mut owner = linked(&runtime, fixture);
        owner.logout(runtime.handle());
        settle(&mut owner, &runtime);
        assert!(matches!(owner.view(), LoginView::Error));
        assert!(
            !owner.finish(&runtime),
            "publishing an error cannot confirm revocation"
        );
    }
    #[test]
    fn uncertain_rotation_never_claims_a_no_token_logout_revoked_remotely() {
        let runtime = runtime();
        let mut fixture = Fixture::new();
        fixture.refresh_fails = true;
        let mut owner = linked(&runtime, fixture.clone());
        owner.clock.0.store(3605, Ordering::SeqCst);
        owner.poll(&runtime, true);
        runtime.block_on(fixture.refresh_entered.notified());
        owner.logout(runtime.handle());
        fixture.refresh_release.notify_one();
        assert!(!owner.finish(&runtime));
        assert!(matches!(owner.view(), LoginView::Error));
        assert!(
            !fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|(endpoint, _)| *endpoint == Endpoint::Revoke)
        );
    }
    #[test]
    fn linking_obeys_issuer_interval_and_background_defers_new_polling() {
        let runtime = runtime();
        let fixture = Fixture::new();
        let clock = Clock(Arc::new(AtomicU64::new(0)));
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            fixture.clone(),
            clock.clone(),
        ));
        let mut owner = Authentication::with_session(session, clock);
        owner.begin(runtime.handle());
        assert!(matches!(owner.view(), LoginView::Requesting));
        settle(&mut owner, &runtime);
        assert!(matches!(
            owner.view(),
            LoginView::Awaiting {
                remaining_seconds: 900,
                ..
            }
        ));
        owner.clock.0.store(5, Ordering::SeqCst);
        owner.poll(&runtime, false);
        assert!(!owner.jobs.is_active());
        assert_eq!(fixture.calls.lock().unwrap().len(), 1);
        owner.poll(&runtime, true);
        settle(&mut owner, &runtime);
        assert!(owner.signed_in());
        assert!(matches!(owner.view(), LoginView::SignedIn));
        assert_eq!(fixture.calls.lock().unwrap().len(), 2);
    }
    #[test]
    fn expired_link_removes_private_instructions_without_an_extra_poll() {
        let runtime = runtime();
        let fixture = Fixture::new();
        let clock = Clock(Arc::new(AtomicU64::new(0)));
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            fixture.clone(),
            clock.clone(),
        ));
        let mut owner = Authentication::with_session(session, clock);
        owner.begin(runtime.handle());
        settle(&mut owner, &runtime);
        owner.clock.0.store(901, Ordering::SeqCst);
        owner.poll(&runtime, true);
        assert!(matches!(owner.view(), LoginView::Expired));
        assert!(owner.link.is_none());
        assert!(!owner.jobs.is_active());
        assert_eq!(fixture.calls.lock().unwrap().len(), 1);
    }
    #[test]
    fn cancel_after_admitted_tokens_preserves_session_before_owner_completion() {
        let runtime = runtime();
        let fixture = Fixture::new();
        let clock = Clock(Arc::new(AtomicU64::new(0)));
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            fixture.clone(),
            clock.clone(),
        ));
        let mut owner = Authentication::with_session(session, clock);
        owner.begin(runtime.handle());
        settle(&mut owner, &runtime);
        owner.clock.0.store(5, Ordering::SeqCst);
        owner.poll(&runtime, true);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !owner.signed_in() {
            assert!(
                std::time::Instant::now() < deadline,
                "fixture tokens must settle"
            );
            std::thread::yield_now();
        }
        assert!(matches!(owner.view(), LoginView::Awaiting { .. }));
        owner.cancel();
        owner.poll(&runtime, false);
        assert!(matches!(owner.view(), LoginView::SignedIn));
        assert!(owner.link.is_none());
        assert_eq!(
            owner
                .session
                .with_access_token(|token| token == "test-access"),
            Ok(true)
        );
        assert_eq!(fixture.calls.lock().unwrap().len(), 2);
    }
    #[test]
    fn failed_rotation_already_settled_before_logout_remains_unconfirmed() {
        let runtime = runtime();
        let mut fixture = Fixture::new();
        fixture.refresh_fails = true;
        let mut owner = linked(&runtime, fixture.clone());
        owner.clock.0.store(3605, Ordering::SeqCst);
        owner.poll(&runtime, true);
        runtime.block_on(fixture.refresh_entered.notified());
        fixture.refresh_release.notify_one();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while owner.session.status() != Status::ReauthenticationRequired {
            assert!(
                std::time::Instant::now() < deadline,
                "controlled failed refresh did not settle its session"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        owner.logout(runtime.handle());
        assert!(!owner.finish(&runtime));
        assert!(matches!(owner.view(), LoginView::Error));
        assert!(
            !fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|(endpoint, _)| *endpoint == Endpoint::Revoke)
        );
    }
}
