use crate::{
    Credentials, Error, Region, Request, Target, Transport, WriteFailure, WriteStatus, native_wire,
    wire,
};
use criterion_session::{MonotonicClock, Session};
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Read,
    Write,
}
#[derive(Default)]
struct State {
    bootstrap: Option<wire::Bootstrap>,
    generation: u64,
    in_flight: Option<Operation>,
    write_status: WriteStatus,
    disposed: bool,
}
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|poisoned| {
        let mut guard = poisoned.into_inner();
        guard.bootstrap = None;
        guard.generation = guard.generation.wrapping_add(1);
        if guard.write_status == WriteStatus::Issued {
            guard.write_status = WriteStatus::Unconfirmed;
        }
        guard.in_flight = None;
        state.clear_poison();
        guard
    })
}
struct Reservation<'a> {
    state: &'a Mutex<State>,
    generation: u64,
    operation: Operation,
    complete: bool,
}
impl<'a> Reservation<'a> {
    fn finish(&mut self) -> Result<MutexGuard<'a, State>, Error> {
        self.complete = true;
        let mut state = lock(self.state);
        if state.disposed {
            return Err(Error::Disposed);
        }
        if state.generation != self.generation || state.in_flight != Some(self.operation) {
            return Err(Error::Stale);
        }
        state.in_flight = None;
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
            if self.operation == Operation::Write {
                state.write_status = WriteStatus::Unconfirmed;
            }
            state.generation = state.generation.wrapping_add(1);
            state.in_flight = None;
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
    pub fn write_status(&self) -> crate::WriteStatus {
        lock(&self.state).write_status
    }
    pub async fn add_watch_list<S: criterion_session::Transport, C: MonotonicClock>(
        &self,
        session: &Session<S, C>,
        media_id: &criterion_provider::MediaId,
        content_type: crate::WatchListContentType,
    ) -> Result<crate::SyncReceipt, crate::WriteFailure> {
        self.write(session, |region| Target::AddWatchList {
            region,
            media_id: media_id.clone(),
            content_type,
        })
        .await
    }
    pub async fn remove_watch_list<S: criterion_session::Transport, C: MonotonicClock>(
        &self,
        session: &Session<S, C>,
        media_id: &criterion_provider::MediaId,
    ) -> Result<crate::SyncReceipt, crate::WriteFailure> {
        self.write(session, |region| Target::RemoveWatchList {
            region,
            media_id: media_id.clone(),
        })
        .await
    }
    async fn write<S: criterion_session::Transport, C: MonotonicClock>(
        &self,
        session: &Session<S, C>,
        target: impl FnOnce(Region) -> Target,
    ) -> Result<crate::SyncReceipt, WriteFailure> {
        let (mut reservation, request, subscriber) = self
            .reserve_authenticated(session, target, Operation::Write)
            .map_err(WriteFailure::NotIssued)?;
        let response = self.transport.send(request).await;
        let mut state = reservation.finish().map_err(WriteFailure::Unconfirmed)?;
        // The transport contract reserves these two errors for refusal before
        // contact. Every potentially delivered failure remains uncertain.
        if let Err(error @ (Error::Busy | Error::InvalidRequest)) = &response {
            state.write_status = WriteStatus::Ready;
            return Err(WriteFailure::NotIssued(*error));
        }
        let result =
            Self::admit_response(session, &subscriber, response, native_wire::sync_receipt);
        match result {
            Ok(receipt) => {
                state.write_status = WriteStatus::Ready;
                Ok(receipt)
            }
            Err(error) => {
                state.write_status = WriteStatus::Unconfirmed;
                Err(WriteFailure::Unconfirmed(error))
            }
        }
    }
    fn reserve_bootstrap(&self) -> Result<Reservation<'_>, Error> {
        let mut state = lock(&self.state);
        if state.disposed {
            return Err(Error::Disposed);
        }
        if state.in_flight.is_some() {
            return Err(Error::Busy);
        }
        state.bootstrap = None;
        state.in_flight = Some(Operation::Read);
        Ok(Reservation {
            state: &self.state,
            generation: state.generation,
            operation: Operation::Read,
            complete: false,
        })
    }
    pub async fn bootstrap(&self) -> Result<Region, Error> {
        let mut reservation = self.reserve_bootstrap()?;
        let response = self
            .transport
            .send(Request {
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
    pub async fn watch_list<S: criterion_session::Transport, C: MonotonicClock>(
        &self,
        session: &Session<S, C>,
    ) -> Result<crate::WatchList, Error> {
        self.read(session, Target::WatchList, native_wire::watch_list)
            .await
    }
    async fn read<S: criterion_session::Transport, C: MonotonicClock, R>(
        &self,
        session: &Session<S, C>,
        target: impl FnOnce(Region) -> Target,
        parse: impl FnOnce(&crate::Response) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let (mut reservation, request, subscriber) =
            self.reserve_authenticated(session, target, Operation::Read)?;
        let response = self.transport.send(request).await;
        let _state = reservation.finish()?;
        Self::admit_response(session, &subscriber, response, parse)
    }
    fn reserve_authenticated<S: criterion_session::Transport, C: MonotonicClock>(
        &self,
        session: &Session<S, C>,
        target: impl FnOnce(Region) -> Target,
        operation: Operation,
    ) -> Result<(Reservation<'_>, Request, reqwest::header::HeaderValue), Error> {
        {
            let mut state = lock(&self.state);
            if state.disposed {
                return Err(Error::Disposed);
            }
            if state.in_flight.is_some() {
                return Err(Error::Busy);
            }
            if operation == Operation::Write && state.write_status == WriteStatus::Unconfirmed {
                return Err(Error::ReconciliationRequired);
            }
            let bootstrap = state.bootstrap.as_ref().ok_or(Error::NoBootstrap)?;
            let credentials = Self::headers(bootstrap, session)?;
            let subscriber = credentials.subscriber.clone();
            let request = Request {
                target: target(bootstrap.region),
                credentials: Some(credentials),
            };
            state.in_flight = Some(operation);
            if operation == Operation::Write {
                state.write_status = WriteStatus::Issued;
            }
            let reservation = Reservation {
                state: &self.state,
                generation: state.generation,
                operation,
                complete: false,
            };
            Ok((reservation, request, subscriber))
        }
    }
    fn admit_response<S: criterion_session::Transport, C: MonotonicClock, R>(
        session: &Session<S, C>,
        subscriber: &reqwest::header::HeaderValue,
        response: Result<crate::Response, Error>,
        parse: impl FnOnce(&crate::Response) -> Result<R, Error>,
    ) -> Result<R, Error> {
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
        if state.in_flight.is_some() {
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
        if state.write_status == WriteStatus::Issued {
            state.write_status = WriteStatus::Unconfirmed;
        }
        state.generation = state.generation.wrapping_add(1);
        state.in_flight = None;
    }
    pub fn dispose(&self) {
        let mut state = lock(&self.state);
        if state.write_status == WriteStatus::Issued {
            state.write_status = WriteStatus::Unconfirmed;
        }
        state.generation = state.generation.wrapping_add(1);
        state.in_flight = None;
        state.bootstrap = None;
        state.disposed = true;
    }
}
