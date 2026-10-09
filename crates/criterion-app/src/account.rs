// SPDX-License-Identifier: GPL-3.0-or-later
//! Native My List reads. The application owns Session epochs and UI
//! privacy; departed generations never publish or start replacement reads.
use crate::jobs::Jobs;
use criterion_account::{AccountClient, Error, WatchList};
use criterion_session::{MonotonicClock, Session, SystemClock};
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime};

#[derive(Debug)]
pub(crate) struct LoadedAccount {
    generation: u64,
    session_generation: u64,
    pub(crate) watch_list: WatchList,
}
impl LoadedAccount {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn session_generation(&self) -> u64 {
        self.session_generation
    }
}
pub(crate) struct Accounts<
    A: criterion_account::Transport = criterion_account::HttpTransport,
    S: criterion_session::Transport = criterion_session::HttpTransport,
    C: MonotonicClock = SystemClock,
> {
    account: Arc<AccountClient<A>>,
    session: Arc<Session<S, C>>,
    jobs: Jobs<Result<LoadedAccount, Error>>,
    generation: u64,
    session_generation: Option<u64>,
    pending: Option<Intent>,
    requested: Option<Intent>,
    foreground: bool,
    disposed: bool,
}
#[derive(Clone, Copy)]
struct Intent {
    generation: u64,
    session_generation: u64,
}
impl Accounts {
    pub(crate) fn new(
        session: Arc<Session<criterion_session::HttpTransport, SystemClock>>,
    ) -> Result<Self, Error> {
        Ok(Self::from_parts(
            Arc::new(AccountClient::with_transport(
                criterion_account::HttpTransport::new()?,
            )),
            session,
        ))
    }
}
impl<
    A: criterion_account::Transport + 'static,
    S: criterion_session::Transport + 'static,
    C: MonotonicClock + 'static,
> Accounts<A, S, C>
{
    pub(crate) fn from_parts(account: Arc<AccountClient<A>>, session: Arc<Session<S, C>>) -> Self {
        Self {
            account,
            session,
            jobs: Jobs::new(),
            generation: 0,
            session_generation: None,
            pending: None,
            requested: None,
            foreground: false,
            disposed: false,
        }
    }
    /// The application advances its monotonic Session epoch before new auth
    /// intent, logout or invalidation. Token bytes are not a lifecycle identity.
    /// Before an active poll and while backgrounded this records only the latest
    /// intent, without contact.
    pub(crate) fn request_shelf(
        &mut self,
        runtime: &Handle,
        session_generation: u64,
    ) -> Result<u64, Error> {
        if self.disposed {
            return Err(Error::Disposed);
        }
        if self
            .session_generation
            .is_some_and(|epoch| session_generation < epoch)
        {
            return Err(Error::Stale);
        }
        self.observe_epoch(session_generation);
        if let Err(error) = self.session.with_access_token(|_| ()) {
            self.retire();
            return Err(Error::Session(error));
        }
        let Some(generation) = self.generation.checked_add(1) else {
            self.retire();
            return Err(Error::Unavailable);
        };
        self.generation = generation;
        let intent = Intent {
            generation,
            session_generation,
        };
        self.pending = Some(intent);
        self.requested = Some(intent);
        if self.foreground {
            self.issue(runtime);
        }
        Ok(generation)
    }
    pub(crate) fn poll(
        &mut self,
        runtime: &Runtime,
        active: bool,
        session_generation: u64,
    ) -> Option<Result<LoadedAccount, Error>> {
        if self.disposed {
            return None;
        }
        // A retiring Jobs may start its pending successor when drained. Clear
        // departed work before joining so no inactive/old-epoch read can start.
        if !active && self.foreground {
            self.background();
        }
        self.foreground = active;
        if self.observe_epoch(session_generation) {
            let _ = runtime.block_on(self.jobs.take_ready());
            return None;
        }
        if self.requested.is_some()
            && let Err(error) = self.session.with_access_token(|_| ())
        {
            self.retire();
            let _ = runtime.block_on(self.jobs.take_ready());
            return active.then_some(Err(Error::Session(error)));
        }
        if active {
            self.issue(runtime.handle());
        }
        let completed = runtime.block_on(self.jobs.take_ready())?;
        let intent = self.requested.take()?;
        if !active {
            return None;
        }
        if let Err(error) = self.session.with_access_token(|_| ()) {
            return Some(Err(Error::Session(error)));
        }
        Some(match completed {
            Ok(Ok(loaded))
                if loaded.generation == intent.generation
                    && loaded.session_generation == session_generation =>
            {
                Ok(loaded)
            }
            Ok(Ok(_)) => Err(Error::Stale),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(Error::Unavailable),
        })
    }
    fn issue(&mut self, runtime: &Handle) {
        let Some(intent) = self.pending.take() else {
            return;
        };
        let account = self.account.clone();
        let session = self.session.clone();
        self.jobs.replace(runtime, async move {
            session.with_access_token(|_| ()).map_err(Error::Session)?;
            match account.region() {
                Ok(_) => {}
                Err(Error::NoBootstrap) => {
                    account.bootstrap().await?;
                }
                Err(error) => return Err(error),
            }
            let watch_list = account.watch_list(&session).await?;
            Ok(LoadedAccount {
                generation: intent.generation,
                session_generation: intent.session_generation,
                watch_list,
            })
        });
    }
    fn retire(&mut self) {
        self.pending = None;
        self.requested = None;
        self.jobs.cancel();
    }
    /// Record every observed root epoch, including failed preflight and idle
    /// polling. An older input never lowers the retained high-water value.
    fn observe_epoch(&mut self, epoch: u64) -> bool {
        let Some(previous) = self.session_generation else {
            self.session_generation = Some(epoch);
            return false;
        };
        if previous == epoch {
            return false;
        }
        self.retire();
        self.session_generation = Some(previous.max(epoch));
        true
    }
    pub(crate) fn background(&mut self) {
        self.foreground = false;
        self.retire();
    }
    /// Dispose read ownership without creating a runtime or changing Session.
    /// Callers cannot use this owner as an issued-write settlement mechanism.
    pub(crate) fn dispose(&mut self, runtime: &Runtime) {
        self.disposed = true;
        self.foreground = false;
        self.retire();
        if self.jobs.is_active() {
            let _ = runtime.block_on(self.jobs.finish());
        }
        self.account.dispose();
    }
}
impl<A: criterion_account::Transport, S: criterion_session::Transport, C: MonotonicClock> Drop
    for Accounts<A, S, C>
{
    fn drop(&mut self) {
        self.jobs.cancel();
        self.account.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use criterion_account::{Region, Request, Response, SecretBody, Target};
    use criterion_session::{Configuration, Endpoint};
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::sync::Notify;

    #[derive(Clone)]
    struct Clock(Arc<AtomicU64>);
    impl MonotonicClock for Clock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }
    struct Issuer;
    impl criterion_session::Transport for Issuer {
        async fn post(
            &self,
            request: criterion_session::Request,
        ) -> Result<criterion_session::Response, criterion_session::Error> {
            let body = match request.endpoint {
                Endpoint::DeviceCode => br#"{"device_code":"synthetic-device","user_code":"ABCD","verification_uri_complete":"https://login.criterion.com/activate?user_code=ABCD","expires_in":900,"interval":5}"#.to_vec(),
                Endpoint::Token => br#"{"access_token":"synthetic-same-token","refresh_token":"synthetic-refresh","expires_in":3600}"#.to_vec(),
                Endpoint::Revoke => b"{}".to_vec(),
            };
            Ok(criterion_session::Response {
                status: 200,
                body: SecretBody::new(body),
            })
        }
    }
    struct Retire(Arc<AtomicUsize>);
    impl Drop for Retire {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[derive(Clone)]
    struct Middleware {
        calls: Arc<Mutex<Vec<Target>>>,
        hold_at: Option<usize>,
        fail_at: Option<usize>,
        entered: Arc<AtomicUsize>,
        retired: Arc<AtomicUsize>,
        release: Arc<Notify>,
    }
    impl Middleware {
        fn new(hold_at: Option<usize>, fail_at: Option<usize>) -> Self {
            Self {
                calls: Arc::default(),
                hold_at,
                fail_at,
                entered: Arc::default(),
                retired: Arc::default(),
                release: Arc::default(),
            }
        }
    }
    impl criterion_account::Transport for Middleware {
        async fn send(&self, request: Request) -> Result<Response, Error> {
            let count = {
                let mut calls = self.calls.lock().unwrap();
                calls.push(request.target.clone());
                calls.len()
            };
            if self.hold_at == Some(count) {
                let _retire = Retire(self.retired.clone());
                self.entered.fetch_add(1, Ordering::SeqCst);
                self.release.notified().await;
            }
            if self.fail_at == Some(count) {
                return Err(Error::Unavailable);
            }
            let body = match request.target {
                Target::Bootstrap => {
                    assert!(request.credentials.is_none());
                    br#"{"country":"US","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#.to_vec()
                }
                Target::WatchList(Region::Us) => br#"{"paging":{"page_limit":60},"type_counts":{"film":1},"playlist":[{"contentType":"film","mediaid":"AbCd1234","title":"Synthetic private film"}]}"#.to_vec(),
                _ => panic!("unexpected typed request"),
            };
            Ok(Response {
                status: 200,
                body: SecretBody::new(body),
            })
        }
    }
    type Fixture = Accounts<Middleware, Issuer, Clock>;
    fn runtime() -> Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }
    fn pump(runtime: &Runtime) {
        runtime.block_on(async {
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
        });
    }
    fn fixture(
        runtime: &Runtime,
        hold_at: Option<usize>,
        fail_at: Option<usize>,
        signed_in: bool,
    ) -> (
        Fixture,
        Middleware,
        Arc<Session<Issuer, Clock>>,
        Arc<AtomicU64>,
    ) {
        let time = Arc::new(AtomicU64::new(0));
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            Issuer,
            Clock(time.clone()),
        ));
        if signed_in {
            runtime.block_on(session.start_link()).unwrap();
            time.store(5, Ordering::SeqCst);
            runtime.block_on(session.poll_once()).unwrap();
        }
        let middleware = Middleware::new(hold_at, fail_at);
        let account = Arc::new(AccountClient::with_transport(middleware.clone()));
        let mut owner = Accounts::from_parts(account, session.clone());
        assert!(owner.poll(runtime, true, 0).is_none());
        (owner, middleware, session, time)
    }
    fn result(owner: &mut Fixture, runtime: &Runtime, epoch: u64) -> Result<LoadedAccount, Error> {
        for _ in 0..32 {
            pump(runtime);
            if let Some(result) = owner.poll(runtime, true, epoch) {
                return result;
            }
        }
        panic!("bounded owner job did not publish");
    }
    #[test]
    fn my_list_fetches_only_watch_list_and_reuses_admitted_bootstrap() {
        let runtime = runtime();
        let (mut owner, middleware, _, _) = fixture(&runtime, None, None, true);
        let generation = owner.request_shelf(runtime.handle(), 7).unwrap();
        let loaded = result(&mut owner, &runtime, 7).unwrap();
        assert_eq!(loaded.generation(), generation);
        assert_eq!(loaded.session_generation(), 7);
        assert_eq!(loaded.watch_list.playlist[0].duration, None);
        assert!(
            !format!("{loaded:?}").contains("Synthetic")
                && !format!("{loaded:?}").contains("AbCd1234")
        );
        assert_eq!(
            *middleware.calls.lock().unwrap(),
            [Target::Bootstrap, Target::WatchList(Region::Us)]
        );
        owner.request_shelf(runtime.handle(), 7).unwrap();
        result(&mut owner, &runtime, 7).unwrap();
        assert_eq!(
            *middleware.calls.lock().unwrap(),
            [
                Target::Bootstrap,
                Target::WatchList(Region::Us),
                Target::WatchList(Region::Us)
            ]
        );
        owner.dispose(&runtime);
    }
    #[test]
    fn watch_list_failure_returns_error_without_retry() {
        let runtime = runtime();
        let (mut owner, middleware, _, _) = fixture(&runtime, None, Some(2), true);
        owner.request_shelf(runtime.handle(), 1).unwrap();
        assert!(matches!(
            result(&mut owner, &runtime, 1),
            Err(Error::Unavailable)
        ));
        for _ in 0..4 {
            pump(&runtime);
            assert!(owner.poll(&runtime, true, 1).is_none());
        }
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        owner.dispose(&runtime);
    }
    #[test]
    fn latest_replacement_waits_for_retirement_and_only_latest_generation_publishes() {
        let runtime = runtime();
        let (mut owner, middleware, _, _) = fixture(&runtime, Some(2), None, true);
        owner.request_shelf(runtime.handle(), 4).unwrap();
        pump(&runtime);
        assert_eq!(middleware.entered.load(Ordering::SeqCst), 1);
        owner.request_shelf(runtime.handle(), 4).unwrap();
        let latest = owner.request_shelf(runtime.handle(), 4).unwrap();
        let loaded = result(&mut owner, &runtime, 4).unwrap();
        assert_eq!(loaded.generation(), latest);
        assert_eq!(middleware.retired.load(Ordering::SeqCst), 1);
        assert_eq!(middleware.calls.lock().unwrap().len(), 3);
        assert!(owner.poll(&runtime, true, 4).is_none());
        owner.dispose(&runtime);
    }
    #[test]
    fn background_retires_reads_and_defers_latest_new_intent_until_foreground() {
        let runtime = runtime();
        let (mut owner, middleware, _, _) = fixture(&runtime, Some(2), None, true);
        owner.request_shelf(runtime.handle(), 3).unwrap();
        pump(&runtime);
        assert_eq!(middleware.entered.load(Ordering::SeqCst), 1);
        owner.background();
        for _ in 0..4 {
            pump(&runtime);
            assert!(owner.poll(&runtime, false, 3).is_none());
        }
        assert_eq!(middleware.retired.load(Ordering::SeqCst), 1);
        let latest = owner.request_shelf(runtime.handle(), 3).unwrap();
        for _ in 0..4 {
            pump(&runtime);
            assert!(owner.poll(&runtime, false, 3).is_none());
        }
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        assert_eq!(
            result(&mut owner, &runtime, 3).unwrap().generation(),
            latest
        );
        assert_eq!(middleware.calls.lock().unwrap().len(), 3);
        owner.dispose(&runtime);
    }
    #[test]
    fn root_epoch_denies_same_token_reauthentication_and_finished_unpublished_payload() {
        let runtime = runtime();
        let (mut owner, middleware, session, time) = fixture(&runtime, None, None, true);
        owner.request_shelf(runtime.handle(), 9).unwrap();
        pump(&runtime);
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        runtime.block_on(session.logout()).unwrap();
        runtime.block_on(session.start_link()).unwrap();
        time.store(10, Ordering::SeqCst);
        runtime.block_on(session.poll_once()).unwrap();
        assert!(owner.poll(&runtime, true, 10).is_none());
        pump(&runtime);
        assert!(owner.poll(&runtime, true, 10).is_none());
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        owner.request_shelf(runtime.handle(), 10).unwrap();
        assert_eq!(
            result(&mut owner, &runtime, 10)
                .unwrap()
                .session_generation(),
            10
        );
        owner.dispose(&runtime);
    }
    #[test]
    fn signed_out_expired_and_disposed_session_admit_no_new_account_calls() {
        let runtime = runtime();
        let (mut owner, middleware, session, time) = fixture(&runtime, None, None, false);
        assert!(matches!(
            owner.request_shelf(runtime.handle(), 1),
            Err(Error::Session(criterion_session::Error::NoSession))
        ));
        assert!(middleware.calls.lock().unwrap().is_empty());
        runtime.block_on(session.start_link()).unwrap();
        time.store(5, Ordering::SeqCst);
        runtime.block_on(session.poll_once()).unwrap();
        time.store(3605, Ordering::SeqCst);
        assert!(matches!(
            owner.request_shelf(runtime.handle(), 2),
            Err(Error::Session(criterion_session::Error::Expired))
        ));
        session.dispose();
        assert!(matches!(
            owner.request_shelf(runtime.handle(), 3),
            Err(Error::Session(criterion_session::Error::Disposed))
        ));
        assert!(middleware.calls.lock().unwrap().is_empty());
        owner.dispose(&runtime);
    }
    #[test]
    fn dispose_joins_retiring_reads_clears_bootstrap_and_prevents_future_work() {
        let runtime = runtime();
        let (mut owner, middleware, session, _) = fixture(&runtime, Some(2), None, true);
        let account = owner.account.clone();
        owner.request_shelf(runtime.handle(), 2).unwrap();
        pump(&runtime);
        assert_eq!(middleware.entered.load(Ordering::SeqCst), 1);
        owner.dispose(&runtime);
        assert_eq!(middleware.retired.load(Ordering::SeqCst), 1);
        assert_eq!(account.region(), Err(Error::Disposed));
        assert!(session.with_access_token(|_| ()).is_ok());
        assert_eq!(
            owner.request_shelf(runtime.handle(), 2),
            Err(Error::Disposed)
        );
        assert!(owner.poll(&runtime, true, 2).is_none());
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
    }
    #[test]
    fn production_constructor_creates_no_session_or_network_work() {
        let session = Arc::new(Session::with_transport(
            Configuration::production(),
            criterion_session::HttpTransport::new().unwrap(),
            SystemClock::default(),
        ));
        let owner = Accounts::new(session.clone()).unwrap();
        assert_eq!(owner.account.region(), Err(Error::NoBootstrap));
        assert_eq!(session.status(), criterion_session::Status::SignedOut);
    }
    #[test]
    fn finished_private_shelf_is_denied_if_session_expires_or_logs_out_before_poll() {
        for logout in [false, true] {
            let runtime = runtime();
            let (mut owner, middleware, session, time) = fixture(&runtime, None, None, true);
            owner.request_shelf(runtime.handle(), 8).unwrap();
            pump(&runtime);
            assert_eq!(middleware.calls.lock().unwrap().len(), 2);
            let expected = if logout {
                runtime.block_on(session.logout()).unwrap();
                criterion_session::Error::NoSession
            } else {
                time.store(3605, Ordering::SeqCst);
                criterion_session::Error::Expired
            };
            assert!(
                matches!(owner.poll(&runtime, true, 8), Some(Err(Error::Session(error))) if error == expected)
            );
            pump(&runtime);
            assert!(owner.poll(&runtime, true, 8).is_none());
            assert_eq!(middleware.calls.lock().unwrap().len(), 2);
            owner.dispose(&runtime);
        }
    }
    #[test]
    fn background_discards_finished_payload_without_automatic_reload_on_foreground() {
        let runtime = runtime();
        let (mut owner, middleware, _, _) = fixture(&runtime, None, None, true);
        owner.request_shelf(runtime.handle(), 5).unwrap();
        pump(&runtime);
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        assert!(owner.poll(&runtime, false, 5).is_none());
        pump(&runtime);
        assert!(owner.poll(&runtime, true, 5).is_none());
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        owner.dispose(&runtime);
    }
    #[test]
    fn session_departure_during_watch_list_retires_transport_and_denies_publication() {
        let runtime = runtime();
        let (mut owner, middleware, session, _) = fixture(&runtime, Some(2), None, true);
        owner.request_shelf(runtime.handle(), 6).unwrap();
        pump(&runtime);
        assert_eq!(middleware.entered.load(Ordering::SeqCst), 1);
        runtime.block_on(session.logout()).unwrap();
        assert!(matches!(
            owner.poll(&runtime, true, 6),
            Some(Err(Error::Session(criterion_session::Error::NoSession)))
        ));
        pump(&runtime);
        assert!(owner.poll(&runtime, true, 6).is_none());
        assert_eq!(middleware.retired.load(Ordering::SeqCst), 1);
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        owner.dispose(&runtime);
    }
    #[test]
    fn fresh_poll_records_session_epoch_before_an_older_request_can_start() {
        let runtime = runtime();
        let (mut owner, middleware, _, _) = fixture(&runtime, None, None, true);
        assert!(owner.poll(&runtime, true, 10).is_none());
        assert_eq!(owner.request_shelf(runtime.handle(), 9), Err(Error::Stale));
        assert!(middleware.calls.lock().unwrap().is_empty());
        owner.request_shelf(runtime.handle(), 10).unwrap();
        assert_eq!(
            result(&mut owner, &runtime, 10)
                .unwrap()
                .session_generation(),
            10
        );
        owner.dispose(&runtime);
    }
    #[test]
    fn failed_higher_epoch_request_still_denies_an_older_request_after_relink() {
        let runtime = runtime();
        let (mut owner, middleware, session, time) = fixture(&runtime, None, None, false);
        assert!(matches!(
            owner.request_shelf(runtime.handle(), 10),
            Err(Error::Session(criterion_session::Error::NoSession))
        ));
        runtime.block_on(session.start_link()).unwrap();
        time.store(5, Ordering::SeqCst);
        runtime.block_on(session.poll_once()).unwrap();
        assert_eq!(owner.request_shelf(runtime.handle(), 9), Err(Error::Stale));
        assert!(middleware.calls.lock().unwrap().is_empty());
        owner.request_shelf(runtime.handle(), 10).unwrap();
        assert_eq!(
            result(&mut owner, &runtime, 10)
                .unwrap()
                .session_generation(),
            10
        );
        owner.dispose(&runtime);
    }
    #[test]
    fn rejected_old_request_preserves_the_admitted_current_read() {
        let runtime = runtime();
        let (mut owner, middleware, _, _) = fixture(&runtime, Some(2), None, true);
        let current = owner.request_shelf(runtime.handle(), 10).unwrap();
        pump(&runtime);
        assert_eq!(middleware.entered.load(Ordering::SeqCst), 1);
        assert_eq!(owner.request_shelf(runtime.handle(), 9), Err(Error::Stale));
        middleware.release.notify_one();
        let loaded = result(&mut owner, &runtime, 10).unwrap();
        assert_eq!(loaded.generation(), current);
        assert_eq!(loaded.session_generation(), 10);
        assert_eq!(middleware.calls.lock().unwrap().len(), 2);
        owner.dispose(&runtime);
    }
    #[test]
    fn new_owner_waits_for_explicit_foreground_before_starting_queued_reads() {
        let runtime = runtime();
        let (_, middleware, session, _) = fixture(&runtime, None, None, true);
        let mut owner = Accounts::from_parts(
            Arc::new(AccountClient::with_transport(middleware.clone())),
            session,
        );
        let generation = owner.request_shelf(runtime.handle(), 3).unwrap();
        pump(&runtime);
        assert!(middleware.calls.lock().unwrap().is_empty());
        assert!(owner.poll(&runtime, false, 3).is_none());
        pump(&runtime);
        assert!(middleware.calls.lock().unwrap().is_empty());
        assert_eq!(
            result(&mut owner, &runtime, 3).unwrap().generation(),
            generation
        );
        owner.dispose(&runtime);
    }
}
