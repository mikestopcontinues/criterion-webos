use crate::{
    Configuration, Error, LinkInstructions, MonotonicClock, PollOutcome, SecureSessionStore,
    Session, Status, Transport,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::runtime::Handle;
use tokio::sync::{Mutex, OwnedMutexGuard};

struct Owned<T, C, S> {
    session: Session<T, C>,
    store: S,
    initialized: bool,
    failure: Option<Error>,
    remote_uncertain: bool,
    confirmed: bool,
    linking: bool,
}

impl<T: Transport, C: MonotonicClock, S> Owned<T, C, S> {
    fn observe_status(&mut self) -> Status {
        let status = self.session.status();
        if self.confirmed && status == Status::ReauthenticationRequired {
            self.confirmed = false;
            self.remote_uncertain = true;
        }
        status
    }
    fn status(&mut self, generation: &AtomicU64) -> Status {
        let confirmed = self.confirmed;
        let status = self.observe_status();
        let lost_link = self.linking && !matches!(status, Status::Linking { .. });
        if lost_link {
            self.linking = false;
        }
        if (confirmed && !self.confirmed) || lost_link {
            generation.fetch_add(1, Ordering::SeqCst);
        }
        status
    }
    fn checkpoint(&mut self) -> Result<crate::StoredSession, Error> {
        let result = self.session.checkpoint();
        if result.is_err()
            && self.confirmed
            && self.session.status() == Status::ReauthenticationRequired
        {
            self.confirmed = false;
            self.remote_uncertain = true;
            if matches!(result, Err(Error::NoSession)) {
                return Err(Error::ReauthenticationRequired);
            }
        }
        result
    }
}

enum Command {
    Restore,
    Link,
    Poll,
    Refresh,
}
enum Reply {
    Restored(bool),
    Linked(LinkInstructions),
    Polled(PollOutcome),
    Unit,
}
enum Retirement {
    Stop,
    Logout,
    Invalidate,
}

/// Whole-session ownership for an independently admitted secure store.
/// Active credentials stay volatile after consuming the saved checkpoint.
/// The injected runtime must remain alive until all issued jobs settle.
pub struct PersistentSession<T: Transport, C: MonotonicClock, S: SecureSessionStore> {
    owned: Arc<Mutex<Owned<T, C, S>>>,
    runtime: Handle,
    alive: Arc<AtomicBool>,
    retiring: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

// A runtime abort/panic cannot leave this instance eligible for reuse. This
// local latch makes no claim about recovery from a backend's interrupted write.
struct Job<T: Transport, C: MonotonicClock, S: SecureSessionStore> {
    owned: OwnedMutexGuard<Owned<T, C, S>>,
    storage_pending: bool,
    finished: bool,
}
impl<T: Transport, C: MonotonicClock, S: SecureSessionStore> Drop for Job<T, C, S> {
    fn drop(&mut self) {
        if !self.finished {
            self.owned.session.dispose();
            self.owned.failure = Some(if self.storage_pending {
                Error::StorageUnconfirmed
            } else {
                Error::ReauthenticationRequired
            });
        }
    }
}

impl<T: Transport, C: MonotonicClock, S: SecureSessionStore> Job<T, C, S> {
    fn storage_result<R>(&mut self, result: Result<R, Error>) -> Result<R, Error> {
        self.storage_pending = false;
        result.map_err(|_| {
            self.owned.session.dispose();
            self.owned.failure = Some(Error::StorageUnconfirmed);
            Error::StorageUnconfirmed
        })
    }
    async fn clear(&mut self) -> Result<(), Error> {
        self.storage_pending = true;
        let result = self.owned.store.clear().await;
        self.storage_result(result)
    }
    async fn refresh(&mut self) -> Result<(), Error> {
        let result = self.owned.session.refresh().await;
        if result.is_err() {
            self.owned.remote_uncertain = true;
            self.owned.confirmed = false;
        }
        result
    }
    async fn command(&mut self, command: Command) -> Result<Reply, Error> {
        if let Some(error) = self.owned.failure {
            return Err(error);
        }
        if self.owned.remote_uncertain {
            return Err(Error::ReauthenticationRequired);
        }
        match command {
            Command::Restore => {
                if self.owned.initialized {
                    return Err(Error::Busy);
                }
                self.storage_pending = true;
                let result = self.owned.store.take().await;
                let record = self.storage_result(result)?;
                self.owned.initialized = true;
                let Some(record) = record else {
                    return Ok(Reply::Restored(false));
                };
                self.owned.confirmed = true;
                if let Err(error) = self.owned.session.restore(record) {
                    self.owned.confirmed = false;
                    self.owned.remote_uncertain = true;
                    return Err(error);
                }
                self.refresh().await?;
                Ok(Reply::Restored(true))
            }
            Command::Link => {
                // Re-link is rejected while any confirmed refresh token exists.
                if self.owned.checkpoint().is_ok() {
                    return Err(Error::Busy);
                }
                if self.owned.remote_uncertain {
                    return Err(Error::ReauthenticationRequired);
                }
                self.clear().await?;
                self.owned.initialized = true;
                let result = self.owned.session.start_link().await;
                self.owned.linking = result.is_ok();
                result.map(Reply::Linked)
            }
            Command::Poll => {
                // A grant already observed expired has issued no new request.
                if self.owned.observe_status() == Status::Expired {
                    return Err(Error::Expired);
                }
                if self.owned.remote_uncertain {
                    return Err(Error::ReauthenticationRequired);
                }
                self.clear().await?;
                let result = self.owned.session.poll_once().await;
                if result == Ok(PollOutcome::Authorized) {
                    self.owned.confirmed = true;
                    self.owned.linking = false;
                }
                // These outcomes cannot prove an issued grant remained unused.
                // An expiry between preflight and issuance is conservatively
                // unconfirmed; the raw session intentionally hides that phase.
                if matches!(
                    result,
                    Err(Error::ReauthenticationRequired
                        | Error::ClockRegression
                        | Error::Expired
                        | Error::InvalidResponse)
                ) {
                    self.owned.remote_uncertain = true;
                }
                result.map(Reply::Polled)
            }
            Command::Refresh => {
                // Validate before removal; no rotating request may precede its
                // confirmed crash boundary, even if storage was already empty.
                drop(self.owned.checkpoint()?);
                self.clear().await?;
                self.refresh().await?;
                Ok(Reply::Unit)
            }
        }
    }
    async fn retire(&mut self, retirement: Retirement) -> Result<(), Error> {
        if let Some(error) = self.owned.failure {
            return Err(error);
        }
        let result = match retirement {
            Retirement::Stop => {
                if self.owned.remote_uncertain {
                    self.clear().await?;
                    Err(Error::ReauthenticationRequired)
                } else {
                    match self.owned.checkpoint() {
                        Ok(record) => {
                            self.storage_pending = true;
                            let result = self.owned.store.replace(record).await;
                            self.storage_result(result)
                        }
                        Err(Error::NoSession) => self.clear().await,
                        Err(error) => {
                            self.clear().await?;
                            Err(error)
                        }
                    }
                }
            }
            Retirement::Logout => {
                self.owned.observe_status();
                self.clear().await?;
                let result = self
                    .owned
                    .session
                    .logout()
                    .await
                    .map_err(|_| Error::RevocationUnconfirmed);
                if self.owned.remote_uncertain {
                    Err(Error::RevocationUnconfirmed)
                } else {
                    result
                }
            }
            Retirement::Invalidate => self.clear().await,
        };
        self.owned.session.dispose();
        if let Err(error) = result {
            self.owned.failure = Some(error);
        }
        result
    }
}

impl<T: Transport + 'static, C: MonotonicClock + 'static, S: SecureSessionStore + 'static>
    PersistentSession<T, C, S>
{
    pub fn with_transport(
        config: Configuration,
        transport: T,
        clock: C,
        store: S,
        runtime: Handle,
    ) -> Self {
        Self {
            owned: Arc::new(Mutex::new(Owned {
                session: Session::with_transport(config, transport, clock),
                store,
                initialized: false,
                failure: None,
                remote_uncertain: false,
                confirmed: false,
                linking: false,
            })),
            runtime,
            alive: Arc::new(AtomicBool::new(true)),
            retiring: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Issuance owns the job before returning its reply future. Dropping that
    /// reply cannot cancel a consume or a rotating issuer request.
    fn issue(&self, command: Command) -> impl Future<Output = Result<Reply, Error>> + use<T, C, S> {
        let owned = self
            .available()
            .and_then(|_| self.owned.clone().try_lock_owned().map_err(|_| Error::Busy));
        let alive = self.alive.clone();
        let retiring = self.retiring.clone();
        let mut generation = self.generation.load(Ordering::SeqCst);
        let task = owned.and_then(|owned| {
            if retiring.load(Ordering::SeqCst) {
                return Err(Error::Disposed);
            }
            generation = self
                .generation
                .fetch_add(1, Ordering::SeqCst)
                .wrapping_add(1);
            let mut job = Job {
                owned,
                storage_pending: false,
                finished: false,
            };
            Ok(self.runtime.spawn(async move {
                let result = job.command(command).await;
                job.finished = true;
                if !alive.load(Ordering::SeqCst) && !retiring.load(Ordering::SeqCst) {
                    job.owned.session.dispose();
                }
                if !alive.load(Ordering::SeqCst) || retiring.load(Ordering::SeqCst) {
                    Err(Error::Stale)
                } else {
                    result
                }
            }))
        });
        let reply = self.reply(task, Some(generation));
        // Completed public replies must not keep credentials alive after the
        // public owner and issued jobs have dropped their strong ownership.
        let owned = Arc::downgrade(&self.owned);
        let alive = self.alive.clone();
        let retiring = self.retiring.clone();
        let current = self.generation.clone();
        async move {
            let reply = reply.await?;
            let shared = owned.upgrade().ok_or(Error::Stale)?;
            let mut owned = shared.try_lock().map_err(|_| Error::Stale)?;
            let status = owned.status(&current);
            let live = match &reply {
                Reply::Restored(true) | Reply::Unit | Reply::Polled(PollOutcome::Authorized) => {
                    matches!(status, Status::SignedIn { .. })
                }
                Reply::Restored(false) => matches!(status, Status::SignedOut),
                Reply::Linked(instructions) => {
                    matches!(status, Status::Linking { expires_at, .. } if expires_at == instructions.expires_at)
                }
                Reply::Polled(_) => matches!(status, Status::Linking { .. }),
            };
            if !live
                || retiring.load(Ordering::SeqCst)
                || !alive.load(Ordering::SeqCst)
                || current.load(Ordering::SeqCst) != generation
            {
                return Err(Error::Stale);
            }
            Ok(reply)
        }
    }

    fn reply<R: Send + 'static>(
        &self,
        task: Result<tokio::task::JoinHandle<Result<R, Error>>, Error>,
        generation: Option<u64>,
    ) -> impl Future<Output = Result<R, Error>> + use<T, C, S, R> {
        let owned = Arc::downgrade(&self.owned);
        // Retirement jobs dispose RAM, including on interruption. Retain their
        // settled failure for observation after the public owner drops.
        let retirement_owner = generation.is_none().then(|| self.owned.clone());
        let alive = self.alive.clone();
        let retiring = self.retiring.clone();
        let current = self.generation.clone();
        async move {
            let result = match task {
                Err(error) => Err(error),
                Ok(task) => match task.await {
                    Ok(result) => result,
                    Err(_) => Err(retirement_owner
                        .or_else(|| owned.upgrade())
                        .and_then(|owned| owned.try_lock().ok().and_then(|guard| guard.failure))
                        .unwrap_or(Error::ReauthenticationRequired)),
                },
            };
            if generation.is_some_and(|generation| {
                !alive.load(Ordering::SeqCst)
                    || retiring.load(Ordering::SeqCst)
                    || current.load(Ordering::SeqCst) != generation
            }) {
                Err(Error::Stale)
            } else {
                result
            }
        }
    }

    fn available(&self) -> Result<(), Error> {
        if self.retiring.load(Ordering::SeqCst) {
            return Err(self
                .owned
                .try_lock()
                .ok()
                .and_then(|guard| guard.failure)
                .unwrap_or(Error::Disposed));
        }
        Ok(())
    }
    pub fn restore(&self) -> impl Future<Output = Result<bool, Error>> + use<T, C, S> {
        let reply = self.issue(Command::Restore);
        async move {
            match reply.await? {
                Reply::Restored(value) => Ok(value),
                _ => unreachable!(),
            }
        }
    }
    pub fn start_link(
        &self,
    ) -> impl Future<Output = Result<LinkInstructions, Error>> + use<T, C, S> {
        let reply = self.issue(Command::Link);
        async move {
            match reply.await? {
                Reply::Linked(value) => Ok(value),
                _ => unreachable!(),
            }
        }
    }
    pub fn poll_once(&self) -> impl Future<Output = Result<PollOutcome, Error>> + use<T, C, S> {
        let reply = self.issue(Command::Poll);
        async move {
            match reply.await? {
                Reply::Polled(value) => Ok(value),
                _ => unreachable!(),
            }
        }
    }
    pub fn refresh(&self) -> impl Future<Output = Result<(), Error>> + use<T, C, S> {
        let reply = self.issue(Command::Refresh);
        async move {
            match reply.await? {
                Reply::Unit => Ok(()),
                _ => unreachable!(),
            }
        }
    }

    // The intent is recorded synchronously before returning its reply, denying
    // new borrowing/publication while the sole issued job is joined.
    fn retirement(
        &self,
        retirement: Retirement,
    ) -> impl Future<Output = Result<(), Error>> + use<T, C, S> {
        let task = self
            .retiring
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| Error::Disposed)
            .map(|_| {
                let owned = self.owned.clone();
                self.runtime.spawn(async move {
                    let mut job = Job {
                        owned: owned.lock_owned().await,
                        storage_pending: false,
                        finished: false,
                    };
                    let result = job.retire(retirement).await;
                    job.owned.session.dispose();
                    job.finished = true;
                    result
                })
            });
        self.reply(task, None)
    }
    /// Only this successful, joined operation may leave a current checkpoint.
    pub fn graceful_stop(&self) -> impl Future<Output = Result<(), Error>> + use<T, C, S> {
        self.retirement(Retirement::Stop)
    }
    /// Removal and issuer revocation have separate confirmation requirements.
    pub fn logout(&self) -> impl Future<Output = Result<(), Error>> + use<T, C, S> {
        self.retirement(Retirement::Logout)
    }
    /// Explicit local invalidation removes the checkpoint and erases RAM.
    pub fn invalidate(&self) -> impl Future<Output = Result<(), Error>> + use<T, C, S> {
        self.retirement(Retirement::Invalidate)
    }

    pub fn with_access_token<R>(&self, action: impl FnOnce(&str) -> R) -> Result<R, Error> {
        self.available()?;
        let mut owned = self.owned.try_lock().map_err(|_| Error::Busy)?;
        self.available()?;
        if let Some(error) = owned.failure {
            return Err(error);
        }
        let generation = self.generation.load(Ordering::SeqCst);
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owned.session.with_access_token(action)
        })) {
            Ok(result) => result,
            Err(panic) => {
                owned.session.dispose();
                owned.remote_uncertain |= owned.confirmed;
                owned.confirmed = false;
                self.generation.fetch_add(1, Ordering::SeqCst);
                drop(owned);
                std::panic::resume_unwind(panic);
            }
        };
        owned.status(&self.generation);
        self.available()?;
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(Error::Stale);
        }
        result
    }
    pub fn status(&self) -> Result<Status, Error> {
        self.available()?;
        let mut owned = self.owned.try_lock().map_err(|_| Error::Busy)?;
        self.available()?;
        if let Some(error) = owned.failure {
            return Err(error);
        }
        let status = owned.status(&self.generation);
        self.available()?;
        Ok(status)
    }
}

impl<T: Transport, C: MonotonicClock, S: SecureSessionStore> Drop for PersistentSession<T, C, S> {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        if !self.retiring.load(Ordering::SeqCst)
            && let Ok(owned) = self.owned.try_lock()
        {
            owned.session.dispose();
        }
        // Issued jobs retain their owner until settlement. Drop cannot await a
        // disk barrier, save a checkpoint, or claim durable removal.
    }
}
