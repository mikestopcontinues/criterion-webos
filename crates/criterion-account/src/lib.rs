//! Native middleware bootstrap, anonymous Detail and bounded subscriber capabilities.
use std::future::Future;
mod client;
mod detail;
mod detail_wire;
mod model;
mod native_wire;
mod object;
mod transport;
mod wire;
pub use client::AccountClient;
pub use criterion_session::SecretBody;
pub use detail::{
    NativeDetail, NativeDetailMetadata, NativeFeatured, NativeGenericPlaylist, NativePlaylist,
    NativePlaylistKey, NativeSeason, NativeSeasonsPlaylist,
};
pub use model::{
    ContinueWatching, MediaKind, MediaSummary, MyListIds, NativeEntitlement, PagingInfo, Position,
    SyncReceipt, TypeCount, WatchList, WatchListContentType, WatchListFilter, WatchListRequest,
    WriteFailure, WriteStatus,
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
pub enum SubscriberTarget {
    Entitlement {
        region: Region,
        captured_unix_time_ms: i64,
    },
    MyListIds(Region),
    ContinueWatching(Region),
    WatchList {
        region: Region,
        request: WatchListRequest,
    },
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
/// Single-purpose native Detail authorization. No subscriber token is carried.
pub struct BootstrapAuthorization {
    pub(crate) value: reqwest::header::HeaderValue,
}
impl BootstrapAuthorization {
    pub fn header(&self) -> &reqwest::header::HeaderValue {
        &self.value
    }
}
impl std::fmt::Debug for BootstrapAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BootstrapAuthorization([redacted])")
    }
}
#[derive(Debug)]
pub enum Request {
    Bootstrap,
    Detail {
        region: Region,
        media_id: criterion_provider::MediaId,
        authorization: BootstrapAuthorization,
    },
    Subscriber {
        target: SubscriberTarget,
        credentials: Credentials,
    },
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

#[cfg(test)]
mod detail_tests;

#[cfg(test)]
mod detail_client_tests;

#[cfg(test)]
mod entitlement_tests;
