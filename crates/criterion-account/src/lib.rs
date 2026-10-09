//! Native middleware bootstrap and bounded subscriber-account capabilities.
use std::future::Future;
mod client;
mod model;
mod native_wire;
mod transport;
mod wire;
pub use client::AccountClient;
pub use criterion_session::SecretBody;
pub use model::{
    ContinueWatching, MediaKind, MediaSummary, MyListIds, PagingInfo, Position, SyncReceipt,
    TypeCount, WatchList, WatchListContentType, WriteFailure, WriteStatus,
};
pub use transport::HttpTransport;
pub use wire::{BOOTSTRAP_URL, CA_BASE, US_BASE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    InvalidRequest,
    InvalidResponse,
    HttpStatus(u16),
    ResponseTooLarge,
    Deadline,
    Busy,
    Stale,
    Disposed,
    NoBootstrap,
    UnsupportedRegion,
    ReconciliationRequired,
    Session(criterion_session::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Us,
    Ca,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Bootstrap,
    MyListIds(Region),
    ContinueWatching(Region),
    WatchList(Region),
    AddWatchList {
        region: Region,
        media_id: criterion_provider::MediaId,
        content_type: WatchListContentType,
    },
    RemoveWatchList {
        region: Region,
        media_id: criterion_provider::MediaId,
    },
}
pub struct Credentials {
    pub(crate) bootstrap: reqwest::header::HeaderValue,
    pub(crate) subscriber: reqwest::header::HeaderValue,
}
impl std::fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Credentials([redacted])")
    }
}
impl Credentials {
    pub fn bootstrap(&self) -> &reqwest::header::HeaderValue {
        &self.bootstrap
    }
    pub fn subscriber(&self) -> &reqwest::header::HeaderValue {
        &self.subscriber
    }
}
#[derive(Debug)]
pub struct Request {
    pub target: Target,
    pub credentials: Option<Credentials>,
}
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: SecretBody,
}
pub trait Transport: Send + Sync {
    /// Busy and InvalidRequest must mean no HTTP contact. Once a request may
    /// have been delivered, implementations return another coarse error.
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, Error>> + Send;
}
#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod native_tests;
#[cfg(test)]
mod transport_tests;
#[cfg(test)]
mod write_tests;
