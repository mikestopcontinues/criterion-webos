use crate::{Credentials, Error, Region, Request, Target, Transport, native_wire, wire};
use criterion_session::{MonotonicClock, Session};
use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
struct State {
    bootstrap: Option<wire::Bootstrap>,
    generation: u64,
    in_flight: bool,
    disposed: bool,
}
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|poisoned| {
        let mut guard = poisoned.into_inner();
        guard.bootstrap = None;
        guard.generation = guard.generation.wrapping_add(1);
        guard.in_flight = false;
        state.clear_poison();
        guard
    })
}
struct Reservation<'a> {
    state: &'a Mutex<State>,
    generation: u64,
    complete: bool,
}
impl<'a> Reservation<'a> {
    fn finish(&mut self) -> Result<MutexGuard<'a, State>, Error> {
        self.complete = true;
        let mut state = lock(self.state);
        if state.disposed {
            return Err(Error::Disposed);
        }
        if state.generation != self.generation || !state.in_flight {
            return Err(Error::Stale);
        }
        state.in_flight = false;
        Ok(state)
    }
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        let mut state = lock(self.state);
        if state.generation == self.generation {
            state.generation = state.generation.wrapping_add(1);
            state.in_flight = false;
        }
    }
}
pub struct AccountClient<T> {
    transport: T,
    state: Mutex<State>,
}
impl<T: Transport> AccountClient<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            state: Mutex::new(State::default()),
        }
    }
    fn reserve_bootstrap(&self) -> Result<Reservation<'_>, Error> {
        let mut state = lock(&self.state);
        if state.disposed {
            return Err(Error::Disposed);
        }
        if state.in_flight {
            return Err(Error::Busy);
        }
        state.bootstrap = None;
        state.in_flight = true;
        Ok(Reservation {
            state: &self.state,
            generation: state.generation,
            complete: false,
        })
    }
    pub async fn bootstrap(&self) -> Result<Region, Error> {
        let mut reservation = self.reserve_bootstrap()?;
        let response = self
            .transport
            .get(Request {
                target: Target::Bootstrap,
                credentials: None,
            })
            .await;
        let mut state = reservation.finish()?;
        let bootstrap = wire::bootstrap(&response?)?;
        let region = bootstrap.region;
        state.bootstrap = Some(bootstrap);
        Ok(region)
    }
    pub async fn my_list_ids<S: criterion_session::Transport, C: MonotonicClock>(
        &self,
        session: &Session<S, C>,
    ) -> Result<crate::MyListIds, Error> {
        self.read(session, Target::MyListIds, native_wire::my_list_ids)
            .await
    }
    pub async fn continue_watching<S: criterion_session::Transport, C: MonotonicClock>(
        &self,
        session: &Session<S, C>,
    ) -> Result<crate::ContinueWatching, Error> {
        self.read(
            session,
            Target::ContinueWatching,
            native_wire::continue_watching,
        )
        .await
    }
    async fn read<S: criterion_session::Transport, C: MonotonicClock, R>(
        &self,
        session: &Session<S, C>,
        target: impl FnOnce(Region) -> Target,
        parse: impl FnOnce(&crate::Response) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let (mut reservation, request, subscriber) = {
            let mut state = lock(&self.state);
            if state.disposed {
                return Err(Error::Disposed);
            }
            if state.in_flight {
                return Err(Error::Busy);
            }
            let bootstrap = state.bootstrap.as_ref().ok_or(Error::NoBootstrap)?;
            let credentials = Self::headers(bootstrap, session)?;
            let subscriber = credentials.subscriber.clone();
            let request = Request {
                target: target(bootstrap.region),
                credentials: Some(credentials),
            };
            state.in_flight = true;
            let reservation = Reservation {
                state: &self.state,
                generation: state.generation,
                complete: false,
            };
            (reservation, request, subscriber)
        };
        let response = self.transport.get(request).await;
        let _state = reservation.finish()?;
        // Keep account ownership and the Session borrowed token lock through
        // final admission. A lifecycle change cannot publish the old payload.
        session
            .with_access_token(|token| {
                if token.as_bytes() != subscriber.as_bytes() {
                    return Err(Error::Stale);
                }
                parse(&response?)
            })
            .map_err(|_| Error::Stale)?
    }
    fn headers<S: criterion_session::Transport, C: MonotonicClock>(
        bootstrap: &wire::Bootstrap,
        session: &Session<S, C>,
    ) -> Result<Credentials, Error> {
        let bearer = wire::token_header(b"Bearer ", bootstrap.token.expose())?;
        let subscriber = session
            .with_access_token(|token| wire::token_header(b"", token))
            .map_err(Error::Session)??;
        Ok(Credentials {
            bootstrap: bearer,
            subscriber,
        })
    }
    /// Explicit sensitive headers for the admitted middleware transport seam.
    /// This does not admit any route, entitlement or independent API access.
    pub fn credentials<S: criterion_session::Transport, C: MonotonicClock>(
        &self,
        session: &Session<S, C>,
    ) -> Result<Credentials, Error> {
        let state = lock(&self.state);
        if state.disposed {
            return Err(Error::Disposed);
        }
        if state.in_flight {
            return Err(Error::Busy);
        }
        let bootstrap = state.bootstrap.as_ref().ok_or(Error::NoBootstrap)?;
        Self::headers(bootstrap, session)
    }
    pub fn region(&self) -> Result<Region, Error> {
        let state = lock(&self.state);
        if state.disposed {
            return Err(Error::Disposed);
        }
        state
            .bootstrap
            .as_ref()
            .map(|bootstrap| bootstrap.region)
            .ok_or(Error::NoBootstrap)
    }
    pub fn cancel(&self) {
        let mut state = lock(&self.state);
        state.generation = state.generation.wrapping_add(1);
        state.in_flight = false;
    }
    pub fn dispose(&self) {
        let mut state = lock(&self.state);
        state.generation = state.generation.wrapping_add(1);
        state.in_flight = false;
        state.bootstrap = None;
        state.disposed = true;
    }
}
