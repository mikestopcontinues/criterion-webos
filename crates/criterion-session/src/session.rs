use crate::wire;
use crate::{
    Configuration, Endpoint, Error, LinkInstructions, MonotonicClock, PollOutcome, Request, Secret,
    Status, Transport,
};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

struct Grant {
    code: Secret,
    expires_at: Duration,
    next_poll: Duration,
    interval: u64,
    backoff: u64,
    attempts: u16,
}
struct Access {
    token: Secret,
    expires_at: Duration,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Link,
    Poll,
    Refresh,
    Revoke,
}
struct State {
    grant: Option<Grant>,
    access: Option<Access>,
    refresh: Option<Secret>,
    status: Status,
    generation: u64,
    in_flight: Option<Operation>,
    last_time: Duration,
}
impl Default for State {
    fn default() -> Self {
        Self {
            grant: None,
            access: None,
            refresh: None,
            status: Status::SignedOut,
            generation: 0,
            in_flight: None,
            last_time: Duration::ZERO,
        }
    }
}
impl State {
    fn invalidate(&mut self, status: Status) {
        self.generation = self.generation.wrapping_add(1);
        self.in_flight = None;
        self.grant = None;
        self.access = None;
        self.refresh = None;
        self.status = status;
    }
    fn observe(&mut self, now: Duration) -> Result<(), Error> {
        if now < self.last_time {
            self.invalidate(Status::ReauthenticationRequired);
            self.last_time = now;
            return Err(Error::ClockRegression);
        }
        self.last_time = now;
        Ok(())
    }
}
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|poisoned| {
        let mut guard = poisoned.into_inner();
        guard.invalidate(Status::ReauthenticationRequired);
        state.clear_poison();
        guard
    })
}
// Dropping an issued request future cannot leave a reservation locked forever or
// admit its later result. These HTTP operations are cancellation safe locally;
// remote token-grant completion remains unconfirmed after a dropped request.
struct Reservation<'a> {
    state: &'a Mutex<State>,
    generation: u64,
    operation: Operation,
    complete: bool,
}
impl<'a> Reservation<'a> {
    fn finish(&mut self, now: Duration) -> Result<MutexGuard<'a, State>, Error> {
        self.complete = true;
        let mut state = lock(self.state);
        if state.status == Status::Disposed {
            return Err(Error::Disposed);
        }
        if state.generation != self.generation || state.in_flight != Some(self.operation) {
            return Err(Error::Stale);
        }
        state.in_flight = None;
        state.observe(now)?;
        Ok(state)
    }
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut state = lock(self.state);
        if !self.complete
            && state.generation == self.generation
            && state.in_flight == Some(self.operation)
        {
            state.invalidate(match self.operation {
                Operation::Refresh => Status::ReauthenticationRequired,
                Operation::Revoke => Status::SignedOut,
                _ => Status::Cancelled,
            });
        }
    }
}

pub struct Session<T, C> {
    transport: T,
    clock: C,
    state: Mutex<State>,
}
impl<T: Transport, C: MonotonicClock> Session<T, C> {
    /// The platform owner serializes checkpoint/storage with refresh/logout.
    /// This copy is private and zeroizing, and never grants access by itself.
    pub fn checkpoint(&self) -> Result<crate::StoredSession, Error> {
        let state = self.state()?;
        if state.in_flight.is_some() {
            return Err(Error::Busy);
        }
        let refresh = state.refresh.as_ref().ok_or(Error::NoSession)?;
        Ok(crate::StoredSession {
            refresh_token: Secret(zeroize::Zeroizing::new(refresh.expose().to_owned())),
        })
    }
    pub fn restore(&self, record: crate::StoredSession) -> Result<(), Error> {
        let mut state = self.state()?;
        if state.in_flight.is_some() || state.grant.is_some() || state.refresh.is_some() {
            return Err(Error::Busy);
        }
        state.invalidate(Status::RefreshRequired);
        state.refresh = Some(record.refresh_token);
        Ok(())
    }
    pub fn with_transport(_config: Configuration, transport: T, clock: C) -> Self {
        Self {
            transport,
            clock,
            state: Mutex::new(State::default()),
        }
    }
    fn reserve(&self, state: &mut State, operation: Operation) -> Reservation<'_> {
        state.in_flight = Some(operation);
        Reservation {
            state: &self.state,
            generation: state.generation,
            operation,
            complete: false,
        }
    }
    fn state(&self) -> Result<MutexGuard<'_, State>, Error> {
        let mut state = lock(&self.state);
        if state.status == Status::Disposed {
            return Err(Error::Disposed);
        }
        state.observe(self.clock.now())?;
        Ok(state)
    }
    pub async fn start_link(&self) -> Result<LinkInstructions, Error> {
        let (mut reservation, started) = {
            let mut state = self.state()?;
            if state.in_flight.is_some() {
                return Err(Error::Busy);
            }
            if state.refresh.is_some() {
                return Err(Error::Busy);
            }
            state.invalidate(Status::SignedOut);
            let started = state.last_time;
            (self.reserve(&mut state, Operation::Link), started)
        };
        let response = self
            .transport
            .post(Request {
                endpoint: Endpoint::DeviceCode,
                body: wire::device_form(),
            })
            .await;
        let mut state = reservation.finish(self.clock.now())?;
        let data = wire::parse_device(&response?)?;
        let expires_at = wire::after(started, data.expires_in)?;
        if state.last_time >= expires_at {
            state.status = Status::Expired;
            return Err(Error::Expired);
        }
        let next_poll = state
            .last_time
            .saturating_add(Duration::from_secs(data.interval))
            .min(expires_at);
        state.grant = Some(Grant {
            code: data.device_code,
            expires_at,
            next_poll,
            interval: data.interval,
            backoff: data.interval,
            attempts: 0,
        });
        state.status = Status::Linking {
            expires_at,
            next_poll_at: next_poll,
        };
        Ok(LinkInstructions {
            user_code: data.user_code,
            verification_uri_complete: data.verification_uri_complete,
            expires_at,
        })
    }
    pub async fn poll_once(&self) -> Result<PollOutcome, Error> {
        let (mut reservation, request, started) = {
            let mut state = self.state()?;
            if state.in_flight.is_some() {
                return Err(Error::Busy);
            }
            let now = state.last_time;
            let grant = state.grant.as_mut().ok_or(Error::NoSession)?;
            if now >= grant.expires_at {
                state.invalidate(Status::Expired);
                return Err(Error::Expired);
            }
            if now < grant.next_poll {
                return Ok(PollOutcome::WaitUntil(grant.next_poll));
            }
            if grant.attempts >= 1024 {
                state.invalidate(Status::Expired);
                return Err(Error::PollLimit);
            }
            grant.attempts += 1;
            let request = Request {
                endpoint: Endpoint::Token,
                body: wire::poll_form(&grant.code),
            };
            (self.reserve(&mut state, Operation::Poll), request, now)
        };
        let response = self.transport.post(request).await;
        let mut state = reservation.finish(self.clock.now())?;
        let expires_at = state.grant.as_ref().ok_or(Error::NoSession)?.expires_at;
        if state.last_time >= expires_at {
            state.invalidate(Status::Expired);
            return Err(Error::Expired);
        }
        let response = match response {
            Ok(response) => response,
            Err(Error::Unavailable | Error::Deadline) => {
                return Self::schedule(&mut state, true, false);
            }
            Err(error) => {
                state.invalidate(Status::Cancelled);
                return Err(error);
            }
        };
        if response.status == 200 {
            let result = wire::parse_tokens(&response).and_then(|tokens| {
                wire::after(started, tokens.expires_in).map(|expiry| (tokens, expiry))
            });
            let (tokens, expires_at) = match result {
                Ok(value) => value,
                Err(_) => {
                    state.invalidate(Status::ReauthenticationRequired);
                    return Err(Error::ReauthenticationRequired);
                }
            };
            if state.last_time >= expires_at {
                state.invalidate(Status::Expired);
                return Err(Error::Expired);
            }
            state.refresh = Some(tokens.refresh_token);
            state.access = Some(Access {
                token: tokens.access_token,
                expires_at,
            });
            state.grant = None;
            state.status = Status::SignedIn { expires_at };
            return Ok(PollOutcome::Authorized);
        }
        match wire::failure(&response) {
            Ok(wire::Failure::Pending) => Self::schedule(&mut state, false, false),
            Ok(wire::Failure::SlowDown) => Self::schedule(&mut state, false, true),
            Ok(wire::Failure::Denied) => {
                state.invalidate(Status::Denied);
                Err(Error::Denied)
            }
            Ok(wire::Failure::Expired) => {
                state.invalidate(Status::Expired);
                Err(Error::Expired)
            }
            Err(Error::Unavailable) => Self::schedule(&mut state, true, false),
            _ => {
                state.invalidate(Status::Cancelled);
                Err(Error::InvalidResponse)
            }
        }
    }
    fn schedule(state: &mut State, transient: bool, slow_down: bool) -> Result<PollOutcome, Error> {
        let grant = state.grant.as_mut().ok_or(Error::NoSession)?;
        if slow_down {
            if grant.interval > 115 {
                state.invalidate(Status::Expired);
                return Err(Error::PollLimit);
            }
            grant.interval += 5;
        }
        grant.backoff = if transient {
            grant.backoff.max(grant.interval).saturating_mul(2).min(120)
        } else {
            grant.interval
        };
        grant.next_poll = state
            .last_time
            .saturating_add(Duration::from_secs(grant.backoff))
            .min(grant.expires_at);
        state.status = Status::Linking {
            expires_at: grant.expires_at,
            next_poll_at: grant.next_poll,
        };
        Ok(if transient {
            PollOutcome::RetryAt(grant.next_poll)
        } else {
            PollOutcome::Pending(grant.next_poll)
        })
    }

    /// Synchronous borrowed access for the provider's header-construction seam.
    /// The callback must not re-enter this session or perform blocking work.
    pub fn with_access_token<R>(&self, action: impl FnOnce(&str) -> R) -> Result<R, Error> {
        let state = self.state()?;
        let access = state.access.as_ref().ok_or(Error::NoSession)?;
        if state.last_time >= access.expires_at {
            return Err(Error::Expired);
        }
        Ok(action(access.token.expose()))
    }
    pub fn cancel(&self) {
        let mut state = lock(&self.state);
        if state.status == Status::Disposed {
            return;
        }
        if state.in_flight == Some(Operation::Refresh) {
            state.invalidate(Status::ReauthenticationRequired);
        } else if state.access.is_none() {
            state.invalidate(Status::Cancelled);
        }
    }
    pub fn dispose(&self) {
        lock(&self.state).invalidate(Status::Disposed);
    }
    pub fn status(&self) -> Status {
        let mut state = lock(&self.state);
        if state.status == Status::Disposed {
            return Status::Disposed;
        }
        let _ = state.observe(self.clock.now());
        if state
            .grant
            .as_ref()
            .is_some_and(|grant| state.last_time >= grant.expires_at)
        {
            state.invalidate(Status::Expired);
        } else if state
            .access
            .as_ref()
            .is_some_and(|access| state.last_time >= access.expires_at)
        {
            state.status = Status::RefreshRequired;
        }
        state.status
    }
    pub async fn refresh(&self) -> Result<(), Error> {
        let (mut reservation, request, started) = {
            let mut state = self.state()?;
            if state.in_flight.is_some() {
                return Err(Error::Busy);
            }
            let refresh = state.refresh.as_ref().ok_or(Error::NoSession)?;
            let request = Request {
                endpoint: Endpoint::Token,
                body: wire::refresh_form(refresh),
            };
            let started = state.last_time;
            (
                self.reserve(&mut state, Operation::Refresh),
                request,
                started,
            )
        };
        let response = self.transport.post(request).await;
        let mut state = reservation.finish(self.clock.now())?;
        let result = response.and_then(|response| {
            if response.status != 200 {
                return Err(Error::ReauthenticationRequired);
            }
            wire::parse_refresh(&response)
        });
        let refreshed = match result {
            Ok(tokens) => tokens,
            Err(_) => {
                state.invalidate(Status::ReauthenticationRequired);
                return Err(Error::ReauthenticationRequired);
            }
        };
        let expires_at = match wire::after(started, refreshed.expires_in) {
            Ok(expires) => expires,
            Err(_) => {
                state.invalidate(Status::ReauthenticationRequired);
                return Err(Error::ReauthenticationRequired);
            }
        };
        if state.last_time >= expires_at {
            state.invalidate(Status::ReauthenticationRequired);
            return Err(Error::ReauthenticationRequired);
        }
        if let Some(refresh) = refreshed.refresh_token {
            state.refresh = Some(refresh);
        }
        state.access = Some(Access {
            token: refreshed.access_token,
            expires_at,
        });
        state.status = Status::SignedIn { expires_at };
        Ok(())
    }
    /// Local credentials are removed immediately. A failed/overlapped revoke is
    /// explicitly unconfirmed; it is never automatically retried.
    pub async fn logout(&self) -> Result<(), Error> {
        let (mut reservation, request) = {
            let mut state = self.state()?;
            if state.in_flight.is_some() {
                state.invalidate(Status::SignedOut);
                return Err(Error::RevocationUnconfirmed);
            }
            let request = state.refresh.as_ref().map(|refresh| Request {
                endpoint: Endpoint::Revoke,
                body: wire::revoke_form(refresh),
            });
            state.invalidate(Status::SignedOut);
            let Some(request) = request else {
                return Ok(());
            };
            (self.reserve(&mut state, Operation::Revoke), request)
        };
        let response = self.transport.post(request).await;
        let _state = reservation
            .finish(self.clock.now())
            .map_err(|_| Error::RevocationUnconfirmed)?;
        match response {
            Ok(response) if response.status == 200 => Ok(()),
            _ => Err(Error::RevocationUnconfirmed),
        }
    }
}
