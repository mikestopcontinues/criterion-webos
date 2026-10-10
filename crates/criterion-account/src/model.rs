use criterion_provider::{MediaId, PageCursor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Category,
    Collection,
    Series,
    Original,
    Episode,
    Franchise,
    Live,
    Film,
    Supplement,
}
/// Verified grouped GET choices, distinct from the POST media content types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WatchListFilter {
    #[default]
    All,
    FilmSeries,
    Collection,
    OriginalFranchise,
    Supplement,
    Category,
}
impl WatchListFilter {
    pub(crate) fn as_str(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::FilmSeries => Some("film_series"),
            Self::Collection => Some("collection"),
            Self::OriginalFranchise => Some("original_franchise"),
            Self::Supplement => Some("supplement"),
            Self::Category => Some("category"),
        }
    }
}
/// One explicitly requested native page. The transport owns the fixed 50-item
/// policy; response page sizes never select a subsequent request limit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchListRequest {
    pub filter: WatchListFilter,
    pub cursor: Option<PageCursor>,
}

/// Exact native request enum; caller selection is explicit rather than an
/// inferred automatic conversion from the media projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchListContentType {
    Film,
    Series,
    Collection,
    Episode,
    Supplement,
    Category,
    Franchise,
    Live,
    Original,
}
impl WatchListContentType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Film => "film",
            Self::Series => "series",
            Self::Collection => "collection",
            Self::Episode => "episode",
            Self::Supplement => "supplement",
            Self::Category => "category",
            Self::Franchise => "franchise",
            Self::Live => "live",
            Self::Original => "original",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncReceipt {
    /// Provider response flag, not a claim that membership was applied.
    pub sync: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteFailure {
    NotIssued(crate::Error),
    Unconfirmed(crate::Error),
}
impl std::fmt::Display for WriteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for WriteFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotIssued(error) | Self::Unconfirmed(error) => Some(error),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WriteStatus {
    #[default]
    Ready,
    Issued,
    Unconfirmed,
}
#[derive(Clone, PartialEq)]
pub struct MediaSummary {
    pub id: MediaId,
    pub title: String,
    pub kind: MediaKind,
    /// Native Float32 value; interpretation depends on the subtype. Admission preserves it.
    pub duration: Option<f32>,
    pub release_date: Option<time::Date>,
    /// Native Episode metadata, distinct from a saved Position override.
    /// Other subtypes have no series association in this summary projection.
    pub series_id: Option<MediaId>,
    pub series_title: Option<String>,
}
impl std::fmt::Debug for MediaSummary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaSummary")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
#[derive(PartialEq, Eq)]
pub struct Position {
    pub media_id: MediaId,
    /// Native signed Int64 wire values. The provider contract owns units and completion rules.
    pub pos: i64,
    pub dur: i64,
    pub commentary_track: Option<String>,
    pub series_id: Option<MediaId>,
    pub series_title: Option<String>,
}
impl std::fmt::Debug for Position {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Position([redacted])")
    }
}
#[derive(PartialEq, Eq)]
pub struct MyListIds {
    pub watchlist: Vec<MediaId>,
    pub positions: Vec<Position>,
}
impl std::fmt::Debug for MyListIds {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MyListIds")
            .field("watchlist_count", &self.watchlist.len())
            .field("position_count", &self.positions.len())
            .finish_non_exhaustive()
    }
}
#[derive(PartialEq)]
pub struct ContinueWatching {
    pub playlist: Vec<MediaSummary>,
    pub positions: Vec<Position>,
}
impl std::fmt::Debug for ContinueWatching {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContinueWatching")
            .field("playlist_count", &self.playlist.len())
            .field("position_count", &self.positions.len())
            .finish_non_exhaustive()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct PagingInfo {
    pub page_limit: i32,
    pub next_pagination_key: Option<PageCursor>,
}
#[derive(PartialEq, Eq)]
pub struct TypeCount {
    pub content_type: String,
    pub count: i32,
}
impl std::fmt::Debug for TypeCount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TypeCount([redacted])")
    }
}
#[derive(PartialEq)]
pub struct WatchList {
    pub paging: PagingInfo,
    pub type_counts: Vec<TypeCount>,
    pub playlist: Vec<MediaSummary>,
}
impl std::fmt::Debug for WatchList {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WatchList")
            .field("playlist_count", &self.playlist.len())
            .field("type_count_count", &self.type_counts.len())
            .field(
                "continuation_present",
                &self.paging.next_pagination_key.is_some(),
            )
            .finish_non_exhaustive()
    }
}
