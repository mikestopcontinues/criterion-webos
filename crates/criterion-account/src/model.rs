use criterion_provider::MediaId;

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
#[derive(PartialEq)]
pub struct MediaSummary {
    pub id: MediaId,
    pub title: String,
    pub kind: MediaKind,
    /// Native Float32 value; units are not yet independently admitted.
    pub duration: Option<f32>,
    pub release_date: Option<time::Date>,
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
    /// Native signed Int64 values; units/range/cadence remain unverified.
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
