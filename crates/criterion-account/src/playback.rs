//! Pure, bounded native response projection. Candidate text conveys no URL,
//! entitlement, origin, CDM or license-request policy.
use crate::{Error, MediaSummary, Response};
use zeroize::Zeroizing;

/// Owned selected metadata and exact private source candidates. No cloning or
/// serialization is provided; candidate access is explicitly borrowed.
pub struct NativePlayback {
    pub media: MediaSummary,
    pub(crate) dash_file: Zeroizing<String>,
    pub(crate) widevine_license: Option<Zeroizing<String>>,
    pub(crate) thumbnail: Option<Zeroizing<String>>,
}
impl NativePlayback {
    pub fn dash_file(&self) -> &str {
        self.dash_file.as_str()
    }
    pub fn widevine_license(&self) -> Option<&str> {
        self.widevine_license.as_deref().map(String::as_str)
    }
    pub fn thumbnail(&self) -> Option<&str> {
        self.thumbnail.as_deref().map(String::as_str)
    }
}
impl std::fmt::Debug for NativePlayback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativePlayback([redacted])")
    }
}

/// Unselected variants are valid under this guarded projection's schema, not
/// malformed input. Selection considers the first playlist and its first DASH.
#[derive(Debug)]
pub enum NativePlaybackSelection {
    Selected(NativePlayback),
    EmptyPlaylist,
    NoDash,
}
impl NativePlaybackSelection {
    /// No request is issued. The response's metadata identity is not compared
    /// with any request or substituted for the stream chosen by its caller.
    pub fn from_response(response: &Response) -> Result<Self, Error> {
        crate::playback_wire::decode(response)
    }
}
