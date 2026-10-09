//! Bounded public-artwork decoding; network and GPU ownership remain explicit.
#![forbid(unsafe_code)]

mod decode;
mod loader;
mod source;
mod webp;
pub use decode::decode_artwork;
pub use loader::ArtworkLoader;
pub use source::ArtworkSource;

/// Bounded display roles; cards and full-canvas backgrounds have distinct
/// request/output sizes and cannot share a cached source identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageRole {
    Card,
    Backdrop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtworkError {
    InvalidSource,
    UnsupportedFormat,
    InvalidImage,
    EncodedTooLarge,
    DecodeLimit,
    Busy,
    Deadline,
    Unavailable,
    HttpStatus(u16),
}

impl std::fmt::Display for ArtworkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSource => "invalid public artwork source",
            Self::UnsupportedFormat => "unsupported public artwork format",
            Self::InvalidImage => "invalid public artwork image",
            Self::EncodedTooLarge => "public artwork exceeds encoded size limit",
            Self::DecodeLimit => "public artwork exceeds decode limits",
            Self::Busy => "public artwork loader is busy",
            Self::Deadline => "public artwork request deadline exceeded",
            Self::Unavailable => "public artwork unavailable",
            Self::HttpStatus(_) => "public artwork request rejected",
        })
    }
}
impl std::error::Error for ArtworkError {}

pub struct DecodedArtwork {
    pub(crate) dimensions: [usize; 2],
    pub(crate) rgba: Vec<u8>,
}

impl DecodedArtwork {
    /// The bounded dimensions expected by the renderer's RGBA admission seam.
    pub fn dimensions(&self) -> [usize; 2] {
        self.dimensions
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }
}

impl std::fmt::Debug for DecodedArtwork {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DecodedArtwork([redacted])")
    }
}
