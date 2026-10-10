use super::{Db8Transport, FenceError, FenceFuture, FenceState, WriteReservation, store::Store};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    task::{Context, Poll, Waker},
    thread::JoinHandle,
    time::{Duration, Instant},
};

const OPERATION_LIMIT: Duration = Duration::from_secs(8);
struct Value<T> {
    result: Option<Result<T, FenceError>>,
    waker: Option<Waker>,
}
struct Waiting<T>(Arc<Mutex<Value<T>>>);
struct Respond<T>(Arc<Mutex<Value<T>>>);
fn pair<T>() -> (Respond<T>, Waiting<T>) {
    let value = Arc::new(Mutex::new(Value {
        result: None,
        waker: None,
    }));
    (Respond(value.clone()), Waiting(value))
}
impl<T> Respond<T> {
    fn send(&self, result: Result<T, FenceError>) {
        let wake = {
            let mut value = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if value.result.is_some() {
                return;
            }
            value.result = Some(result);
            value.waker.take()
        };
        if let Some(waker) = wake {
            waker.wake();
        }
    }
}
impl<T> Drop for Respond<T> {
    fn drop(&mut self) {
        self.send(Err(FenceError::Unconfirmed));
    }
}
impl<T> Future for Waiting<T> {
    type Output = Result<T, FenceError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut value = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(result) = value.result.take() {
            Poll::Ready(result)
        } else {
            value.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
enum Command {
    State(Instant, Respond<FenceState>),
    Reserve(Instant, Respond<WriteReservation>),
    Complete(Instant, WriteReservation, Respond<()>),
}
pub(super) struct Worker {
    sender: Arc<Mutex<Option<SyncSender<Command>>>>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub(super) fn start(transport: impl Db8Transport) -> Result<Self, FenceError> {
        Self::start_factory(move || Ok(transport))
    }
    pub(super) fn start_factory<T: Db8Transport>(
        factory: impl FnOnce() -> Result<T, FenceError> + Send + 'static,
    ) -> Result<Self, FenceError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (initialized, initialization) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("criterion-write-fence".into())
            .spawn(move || {
                // Native registration/context/slots are created and retired on this worker.
                let transport = match factory() {
                    Ok(transport) => transport,
                    Err(error) => {
                        let _ = initialized.send(Err(error));
                        return;
                    }
                };
                if initialized.send(Ok(())).is_err() {
                    return;
                }
                let mut store = Store::new(transport);
                while let Ok(command) = receiver.recv() {
                    // A worker panic closes the channel; queued responses fail closed on Drop.
                    match command {
                        Command::State(deadline, response) => response.send(store.state(deadline)),
                        Command::Reserve(deadline, response) => {
                            response.send(store.reserve(deadline))
                        }
                        Command::Complete(deadline, reservation, response) => {
                            response.send(store.complete(reservation, deadline))
                        }
                    }
                }
            })
            .map_err(|_| FenceError::Unavailable)?;
        match initialization.recv_timeout(OPERATION_LIMIT) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = thread.join();
                return Err(error);
            }
            Err(_) => {
                // Close the queue and detach a stalled initializer. If it later settles,
                // failed publication drops its transport on its own worker. Native owns
                // a singleton claim, so uncertain initialization cannot be repeated.
                return Err(FenceError::Unconfirmed);
            }
        }
        Ok(Self {
            sender: Arc::new(Mutex::new(Some(sender))),
            thread: Some(thread),
        })
    }
    fn submit<T: Send + 'static>(
        &self,
        command: impl FnOnce(Instant, Respond<T>) -> Command,
    ) -> FenceFuture<T> {
        let deadline = Instant::now() + OPERATION_LIMIT;
        let (response, waiting) = pair();
        let command = command(deadline, response);
        let sender = self.sender.clone();
        Box::pin(async move {
            // Creating an operation captures its deadline; only its first poll may issue I/O.
            let sent = {
                let locked = sender.lock().unwrap_or_else(|e| e.into_inner());
                let Some(sender) = locked.as_ref() else {
                    return Err(FenceError::Unavailable);
                };
                sender.try_send(command)
            };
            if let Err(error) = sent {
                match error {
                    mpsc::TrySendError::Full(command) => match command {
                        Command::State(_, response) => response.send(Err(FenceError::Held)),
                        Command::Reserve(_, response) => response.send(Err(FenceError::Held)),
                        Command::Complete(_, _, response) => {
                            response.send(Err(FenceError::Unconfirmed))
                        }
                    },
                    mpsc::TrySendError::Disconnected(command) => drop(command),
                }
            }
            waiting.await
        })
    }
    pub(super) fn state(&self) -> FenceFuture<FenceState> {
        self.submit(Command::State)
    }
    pub(super) fn reserve(&self) -> FenceFuture<WriteReservation> {
        self.submit(Command::Reserve)
    }
    pub(super) fn complete(&self, reservation: WriteReservation) -> FenceFuture<()> {
        self.submit(|deadline, response| Command::Complete(deadline, reservation, response))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.sender.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
