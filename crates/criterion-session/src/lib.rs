//! Bounded Criterion native device authorization and subscriber session ownership.
use std::future::Future;
use std::time::Duration;
mod lifecycle;
mod secret;
mod session;
mod storage;
mod transport;
mod wire;
pub use lifecycle::PersistentSession;
pub use secret::{Secret, SecretBody};
pub use session::Session;
pub use storage::{SecureSessionStore, StoredSession};
pub use transport::HttpTransport;

pub const ISSUER: &str = "https://login.criterion.com/";
pub const CLIENT_ID: &str = "trliIpKF50NOVoCOBBPWO0Z51xjg1VBd";
pub const SCOPE: &str = "openid profile email offline_access";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    InvalidConfiguration,
    InvalidRequest,
    InvalidResponse,
    ResponseTooLarge,
    Deadline,
    HttpStatus(u16),
    Busy,
    NoSession,
    Expired,
    Stale,
    Denied,
    Disposed,
    ClockRegression,
    PollLimit,
    ReauthenticationRequired,
    RevocationUnconfirmed,
    StorageUnconfirmed,
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub trait MonotonicClock: Send + Sync {
    fn now(&self) -> Duration;
}
#[derive(Clone, Debug)]
pub struct SystemClock(std::time::Instant);
impl Default for SystemClock {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}
impl MonotonicClock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
    DeviceCode,
    Token,
    Revoke,
}
#[derive(Debug)]
pub struct Request {
    pub endpoint: Endpoint,
    pub body: SecretBody,
}
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: SecretBody,
}
pub trait Transport: Send + Sync {
    fn post(&self, request: Request) -> impl Future<Output = Result<Response, Error>> + Send;
}
#[derive(Debug)]
pub struct Configuration;
impl Configuration {
    pub fn production() -> Self {
        Self
    }
}
#[derive(Debug)]
pub struct LinkInstructions {
    pub user_code: Secret,
    pub verification_uri_complete: Secret,
    pub expires_at: Duration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollOutcome {
    WaitUntil(Duration),
    Pending(Duration),
    RetryAt(Duration),
    Authorized,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    SignedOut,
    Linking {
        expires_at: Duration,
        next_poll_at: Duration,
    },
    SignedIn {
        expires_at: Duration,
    },
    RefreshRequired,
    Cancelled,
    Denied,
    Expired,
    ReauthenticationRequired,
    Disposed,
}
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod transport_tests;
