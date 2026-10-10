//! Owned native metadata, separate from the public website detail projection.
use crate::MediaSummary;

#[derive(Clone, Default, PartialEq)]
pub struct NativeDetailMetadata {
    pub deeplink: Option<String>,
    pub description: Option<String>,
    pub description_long: Option<String>,
    pub description_medium: Option<String>,
    pub description_pull_quote: Option<String>,
    pub description_staff: Option<String>,
    pub director: Option<Vec<String>>,
    pub starring: Option<Vec<String>>,
    pub country: Option<Vec<String>>,
    pub language: Option<Vec<String>>,
    pub commentary_tracks: Option<Vec<String>>,
    /// The proved plain String field on Live only; no entitlement interpretation.
    pub paywall: Option<String>,
    pub content_warnings: Option<String>,
    pub introduction_primary: Option<String>,
    pub trailer: Option<String>,
    pub teaser: Option<String>,
    pub playlist_primary: Option<String>,
    pub playlist_supplements: Option<String>,
    pub playlist_related: Option<String>,
    pub playlist_categories_appears_in: Option<String>,
    pub playlist_collections_appears_in: Option<String>,
    pub genre: Option<String>,
    pub genre_2: Option<String>,
    pub franchise_id: Option<String>,
    pub supplement_type: Option<String>,
    pub collection_count: Option<i32>,
    pub logo: Option<bool>,
}
impl std::fmt::Debug for NativeDetailMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeDetailMetadata([redacted])")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativePlaylistKey {
    Supplements,
    FilmsAppearsIn,
    CollectionsAppearsIn,
    CategoriesAppearsIn,
    Primary,
    Collections,
    Related,
    #[default]
    Other,
}

#[derive(Clone, PartialEq)]
pub struct NativeGenericPlaylist {
    pub title: String,
    pub playlist_id: String,
    pub key: NativePlaylistKey,
    /// First exact identifier per individual source child list, in source order.
    pub children: Vec<MediaSummary>,
    /// Supplied list cardinality before duplicate identifiers are removed.
    pub raw_child_count: usize,
}
#[derive(Clone, PartialEq)]
pub struct NativeSeason {
    pub number: i32,
    pub title: String,
    pub description: Option<String>,
    pub episodes: Vec<MediaSummary>,
    pub raw_episode_count: usize,
    /// Supplied native Int32 count; independent of admitted episode cardinality.
    pub episode_count: i32,
}
#[derive(Clone, PartialEq)]
pub struct NativeSeasonsPlaylist {
    pub title: String,
    pub seasons: Vec<NativeSeason>,
}
#[derive(Clone, PartialEq)]
pub enum NativePlaylist {
    Generic(NativeGenericPlaylist),
    Seasons(NativeSeasonsPlaylist),
}
#[derive(Clone, PartialEq)]
pub struct NativeFeatured {
    pub title: Option<String>,
    pub children: Vec<MediaSummary>,
    pub raw_child_count: usize,
}

#[derive(Clone, PartialEq)]
pub struct NativeDetail {
    pub media: MediaSummary,
    pub metadata: NativeDetailMetadata,
    pub playlists: Vec<NativePlaylist>,
    pub featured: Option<NativeFeatured>,
    pub is_first_tab_sortable: Option<bool>,
}
impl NativeDetail {
    /// Native Series display selection: first Seasons group, then every Generic
    /// group in source order. The owned playlists retain every supplied group.
    pub fn series_display_playlists(&self) -> impl Iterator<Item = &NativePlaylist> {
        self.playlists
            .iter()
            .find(|playlist| matches!(playlist, NativePlaylist::Seasons(_)))
            .into_iter()
            .chain(
                self.playlists
                    .iter()
                    .filter(|playlist| matches!(playlist, NativePlaylist::Generic(_))),
            )
    }

    /// Owned model inline storage and allocation capacities. This excludes
    /// allocator overhead, input bodies, transient decoder storage and RSS.
    /// The validated MediaId constructor owns an exact eight-byte String.
    pub fn estimated_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>()
            + media_allocated(&self.media)
            + self.metadata.allocated()
            + self.playlists.capacity() * std::mem::size_of::<NativePlaylist>();
        for playlist in &self.playlists {
            bytes += match playlist {
                NativePlaylist::Generic(value) => {
                    value.title.capacity()
                        + value.playlist_id.capacity()
                        + media_list_allocated(&value.children)
                }
                NativePlaylist::Seasons(value) => {
                    value.title.capacity()
                        + value.seasons.capacity() * std::mem::size_of::<NativeSeason>()
                        + value
                            .seasons
                            .iter()
                            .map(|season| {
                                season.title.capacity()
                                    + text_allocated(&season.description)
                                    + media_list_allocated(&season.episodes)
                            })
                            .sum::<usize>()
                }
            };
        }
        if let Some(featured) = &self.featured {
            bytes += text_allocated(&featured.title) + media_list_allocated(&featured.children);
        }
        bytes
    }
}
fn text_allocated(value: &Option<String>) -> usize {
    value.as_ref().map_or(0, String::capacity)
}
fn media_allocated(value: &MediaSummary) -> usize {
    value.id.as_str().len()
        + value.title.capacity()
        + value.series_id.as_ref().map_or(0, |id| id.as_str().len())
        + text_allocated(&value.series_title)
}
fn media_list_allocated(values: &Vec<MediaSummary>) -> usize {
    values.capacity() * std::mem::size_of::<MediaSummary>()
        + values.iter().map(media_allocated).sum::<usize>()
}
impl NativeDetailMetadata {
    fn allocated(&self) -> usize {
        let texts = [
            &self.deeplink,
            &self.description,
            &self.description_long,
            &self.description_medium,
            &self.description_pull_quote,
            &self.description_staff,
            &self.paywall,
            &self.content_warnings,
            &self.introduction_primary,
            &self.trailer,
            &self.teaser,
            &self.playlist_primary,
            &self.playlist_supplements,
            &self.playlist_related,
            &self.playlist_categories_appears_in,
            &self.playlist_collections_appears_in,
            &self.genre,
            &self.genre_2,
            &self.franchise_id,
            &self.supplement_type,
        ];
        let lists = [
            &self.director,
            &self.starring,
            &self.country,
            &self.language,
            &self.commentary_tracks,
        ];
        texts.into_iter().map(text_allocated).sum::<usize>()
            + lists
                .into_iter()
                .filter_map(Option::as_ref)
                .map(|values| {
                    values.capacity() * std::mem::size_of::<String>()
                        + values.iter().map(String::capacity).sum::<usize>()
                })
                .sum::<usize>()
    }
}
impl std::fmt::Debug for NativeDetail {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeDetail")
            .field("kind", &self.media.kind)
            .field("playlist_count", &self.playlists.len())
            .field("featured_present", &self.featured.is_some())
            .finish_non_exhaustive()
    }
}
