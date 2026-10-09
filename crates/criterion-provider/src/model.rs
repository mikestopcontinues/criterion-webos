use crate::Error;

#[derive(Clone, PartialEq, Eq, Hash)]
/// A validated, exactly eight-character ASCII alphanumeric media identifier.
pub struct MediaId(String);

impl std::fmt::Debug for MediaId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MediaId([redacted])")
    }
}

impl MediaId {
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(Error::InvalidRequest);
        }
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, PartialEq, Eq)]
/// A provider continuation value, preserved without numeric interpretation.
pub struct PageCursor(String);

impl PageCursor {
    pub(super) fn value(&self) -> &str {
        &self.0
    }

    pub fn new(value: &str) -> Result<Self, Error> {
        if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
            return Err(Error::InvalidRequest);
        }
        Ok(Self(value.into()))
    }
}

impl std::fmt::Debug for PageCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PageCursor([redacted])")
    }
}

pub struct BrowseRequest {
    pub page_limit: u16,
    pub cursor: Option<PageCursor>,
    pub sort: Sort,
    pub direction: SortDirection,
    pub filters: Vec<Filter>,
}

impl Default for BrowseRequest {
    fn default() -> Self {
        Self {
            page_limit: 60,
            cursor: None,
            sort: Sort::Title,
            direction: SortDirection::Ascending,
            filters: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Title,
    Director,
    Year,
    Country,
    Duration,
}

impl Sort {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Director => "director",
            Self::Year => "year",
            Self::Country => "primary_country_slug",
            Self::Duration => "duration",
        }
    }
    pub(super) fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "title" => Ok(Self::Title),
            "director" => Ok(Self::Director),
            "year" => Ok(Self::Year),
            "primary_country_slug" => Ok(Self::Country),
            "duration" => Ok(Self::Duration),
            _ => Err(Error::InvalidResponse),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterGroup {
    Genres,
    Decades,
    Countries,
    Directors,
}

impl FilterGroup {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Genres => "genres",
            Self::Decades => "decades",
            Self::Countries => "countries",
            Self::Directors => "directors",
        }
    }
    pub(super) fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "genres" => Ok(Self::Genres),
            "decades" => Ok(Self::Decades),
            "countries" => Ok(Self::Countries),
            "directors" => Ok(Self::Directors),
            _ => Err(Error::InvalidResponse),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FilterValue(String);

impl std::fmt::Debug for FilterValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FilterValue([redacted])")
    }
}

impl FilterValue {
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .chars()
                .all(|character| character.is_alphanumeric() || character == '-')
        {
            return Err(Error::InvalidRequest);
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub group: FilterGroup,
    pub value: FilterValue,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MediaSummary {
    pub id: MediaId,
    pub title: String,
    pub kind: MediaKind,
    pub duration_seconds: u32,
    pub release_date: Option<String>,
}

impl std::fmt::Debug for MediaSummary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaSummary")
            .field("kind", &self.kind)
            .field("duration_seconds", &self.duration_seconds)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Film,
    Collection,
    Category,
    Supplement,
    Series,
    Live,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosterShape {
    Portrait,
    Landscape,
}

/// Derive the verified 480-wide JW image route. Availability and decoded dimensions
/// belong to the image loader; a missing shape has no inferred crop or fallback.
pub fn poster_url(id: &MediaId, shape: PosterShape) -> Result<url::Url, Error> {
    let label = match shape {
        PosterShape::Portrait => "default_2x3",
        PosterShape::Landscape => "default_16x9",
    };
    url::Url::parse(&format!(
        "https://img.jwplayer.com/v1/media/{}/images/{label}.webp?width=480",
        id.as_str()
    ))
    .map_err(|_| Error::Unavailable)
}

impl MediaKind {
    pub(super) fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "film" => Ok(Self::Film),
            "collection" => Ok(Self::Collection),
            "category" => Ok(Self::Category),
            "supplement" => Ok(Self::Supplement),
            "series" => Ok(Self::Series),
            "live" => Ok(Self::Live),
            _ => Err(Error::InvalidResponse),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CatalogPage {
    pub items: Vec<MediaSummary>,
    pub total: u32,
    pub next_cursor: Option<PageCursor>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SearchResults {
    pub items: Vec<MediaSummary>,
    pub type_counts: Vec<KindCount>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct KindCount {
    pub kind: MediaKind,
    pub count: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BrowseOptions {
    pub filter_groups: Vec<FilterOptions>,
    pub sort_options: Vec<SortOption>,
}

#[derive(PartialEq, Eq)]
pub struct FilterOptions {
    pub group: FilterGroup,
    pub label: String,
    pub options: Vec<FilterOption>,
}

impl std::fmt::Debug for FilterOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FilterOptions")
            .field("group", &self.group)
            .field("options", &self.options.len())
            .finish_non_exhaustive()
    }
}

#[derive(PartialEq, Eq)]
pub struct FilterOption {
    pub label: String,
    pub value: FilterValue,
}

impl std::fmt::Debug for FilterOption {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FilterOption([redacted])")
    }
}

#[derive(PartialEq, Eq)]
pub struct SortOption {
    pub label: String,
    pub sort: Sort,
}

impl std::fmt::Debug for SortOption {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SortOption")
            .field("sort", &self.sort)
            .finish_non_exhaustive()
    }
}

#[derive(PartialEq, Eq)]
pub struct MediaDetail {
    pub media: MediaSummary,
    pub description: Option<String>,
    pub directors: Vec<String>,
    pub starring: Vec<String>,
    pub countries: Vec<String>,
    pub languages: Vec<String>,
    pub genres: Vec<String>,
    pub content_warnings: Option<String>,
    pub commentary_tracks: Vec<String>,
    pub playlists: Vec<Playlist>,
    pub first_playlist_sortable: bool,
    /// Public schedule in supplied order. Non-live media has no schedule.
    pub live_schedule: Vec<LiveProgram>,
}

impl std::fmt::Debug for MediaDetail {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaDetail")
            .field("media", &self.media)
            .field("playlists", &self.playlists.len())
            .field("description_present", &self.description.is_some())
            .field("schedule_entries", &self.live_schedule.len())
            .finish_non_exhaustive()
    }
}

/// Canonical UTC second precision timestamps supplied by the public schedule.
/// No local timezone conversion, trusted-current-time or playback progress is implied.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcTimestamp(String);

impl UtcTimestamp {
    pub fn new(value: &str) -> Result<Self, Error> {
        let bytes = value.as_bytes();
        if bytes.len() != 20
            || bytes[10] != b'T'
            || bytes[13] != b':'
            || bytes[16] != b':'
            || bytes[19] != b'Z'
            || !bytes.iter().enumerate().all(|(index, byte)| {
                matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
            })
            || !crate::wire::valid_date(&value[..10])
            || !value[11..13].parse::<u8>().is_ok_and(|hour| hour < 24)
            || !value[14..16].parse::<u8>().is_ok_and(|minute| minute < 60)
            || !value[17..19].parse::<u8>().is_ok_and(|second| second < 60)
        {
            return Err(Error::InvalidRequest);
        }
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for UtcTimestamp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("UtcTimestamp([redacted])")
    }
}

#[derive(PartialEq, Eq)]
pub struct LiveProgram {
    /// Some observed schedule entries have an empty ID and no catalog target.
    pub media_id: Option<MediaId>,
    pub title: String,
    pub kind: MediaKind,
    pub duration_seconds: u32,
    pub starts_at: UtcTimestamp,
    pub ends_at: UtcTimestamp,
}

impl std::fmt::Debug for LiveProgram {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LiveProgram")
            .field("kind", &self.kind)
            .field("duration_seconds", &self.duration_seconds)
            .field("catalog_target_present", &self.media_id.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(PartialEq, Eq)]
pub struct Playlist {
    pub key: String,
    pub title: String,
    pub id: MediaId,
    pub items: Vec<MediaSummary>,
}

impl std::fmt::Debug for Playlist {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Playlist")
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}
