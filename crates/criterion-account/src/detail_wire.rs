//! Bounded application admission of proved root UI metadata. Custom rating,
//! playable paywall and license-expiry encodings remain outside this projection.
//! Children retain only exact native summary fields; their other metadata and
//! nested groups are discarded. Preflight still bounds the entire raw payload.
use crate::{
    Error, MediaKind, NativeDetail, NativeDetailMetadata, NativeFeatured, NativeGenericPlaylist,
    NativePlaylist, NativePlaylistKey, NativeSeason, NativeSeasonsPlaylist, Response,
    native_wire::{EpisodeSummaryWire, SummaryWire},
    object::Object,
};
use criterion_provider::MediaId;
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use std::{collections::HashSet, fmt, marker::PhantomData};

pub(crate) const MAX_BODY: usize = 512 * 1024;
const MAX_ITEMS: usize = 512;
const MAX_TEXT: usize = 256 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_PLAYLISTS: usize = 32;
const MAX_SEASONS: usize = 64;
const MAX_RETAINED: usize = 512 * 1024;

// These are conservative application limits, not native SDK/server claims.
// Preflight scans every string value and raw native marker before projection,
// including unknown fields and nested DTO data that the owned model discards.
#[derive(Default)]
struct Budget {
    depth: usize,
    media: usize,
    playlists: usize,
    seasons: usize,
    text: usize,
}
#[derive(Clone, Copy)]
enum ValueTag {
    Other,
    Playlist,
}
fn charge<E: de::Error>(count: &mut usize, amount: usize, maximum: usize) -> Result<(), E> {
    *count = count
        .checked_add(amount)
        .ok_or_else(|| E::custom("aggregate policy"))?;
    if *count > maximum {
        return Err(E::custom("aggregate policy"));
    }
    Ok(())
}
struct ScanSeed<'a>(&'a mut Budget);
impl<'de> DeserializeSeed<'de> for ScanSeed<'_> {
    type Value = ValueTag;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(ScanVisitor(self.0))
    }
}
struct ScanVisitor<'a>(&'a mut Budget);
impl<'de> Visitor<'de> for ScanVisitor<'_> {
    type Value = ValueTag;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded native JSON")
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        charge(&mut self.0.text, value.len(), MAX_TEXT)?;
        Ok(if matches!(value, "GENERIC_PLAYLIST" | "seasons") {
            ValueTag::Playlist
        } else {
            ValueTag::Other
        })
    }
    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(ValueTag::Other)
    }
    fn visit_i64<E: de::Error>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(ValueTag::Other)
    }
    fn visit_u64<E: de::Error>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(ValueTag::Other)
    }
    fn visit_f64<E: de::Error>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(ValueTag::Other)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(ValueTag::Other)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        charge(&mut self.0.depth, 1, MAX_DEPTH)?;
        while sequence.next_element_seed(ScanSeed(self.0))?.is_some() {}
        self.0.depth -= 1;
        Ok(ValueTag::Other)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        charge(&mut self.0.depth, 1, MAX_DEPTH)?;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "mediaid" => charge(&mut self.0.media, 1, MAX_ITEMS)?,
                "season_number" => charge(&mut self.0.seasons, 1, MAX_SEASONS)?,
                _ => {}
            }
            let value = map.next_value_seed(ScanSeed(self.0))?;
            if key == "type" && matches!(value, ValueTag::Playlist) {
                charge(&mut self.0.playlists, 1, MAX_PLAYLISTS)?;
            }
        }
        self.0.depth -= 1;
        Ok(ValueTag::Other)
    }
}
fn preflight(body: &[u8]) -> Result<(), Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    // Serde's fixed recursion guard stays enabled as an additional boundary.
    ScanSeed(&mut Budget::default())
        .deserialize(&mut deserializer)
        .and_then(|_| deserializer.end())
        .map_err(|_| Error::InvalidResponse)
}

struct Text<const PROSE: bool>(String);
impl<'de, const PROSE: bool> Deserialize<'de> for Text<PROSE> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StringVisitor<const PROSE: bool>;
        impl<const PROSE: bool> Visitor<'_> for StringVisitor<PROSE> {
            type Value = Text<PROSE>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("bounded native text")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > MAX_TEXT
                    || value.chars().any(|character| {
                        character.is_control()
                            && !(PROSE && matches!(character, '\n' | '\r' | '\t'))
                    })
                {
                    return Err(E::custom("text policy"));
                }
                Ok(Text(value.into()))
            }
        }
        deserializer.deserialize_str(StringVisitor::<PROSE>)
    }
}
impl<const PROSE: bool> From<Text<PROSE>> for String {
    fn from(value: Text<PROSE>) -> Self {
        value.0
    }
}
type Label = Text<false>;
type Prose = Text<true>;

struct Items<T>(Vec<T>);
impl<T> Default for Items<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Items<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ListVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for ListVisitor<T> {
            type Value = Items<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("bounded native list")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values =
                    Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_ITEMS));
                while let Some(value) = sequence.next_element()? {
                    if values.len() == MAX_ITEMS {
                        return Err(de::Error::custom("list policy"));
                    }
                    values.push(value);
                }
                Ok(Items(values))
            }
        }
        deserializer.deserialize_seq(ListVisitor(PhantomData))
    }
}
impl<const PROSE: bool> From<Items<Text<PROSE>>> for Vec<String> {
    fn from(value: Items<Text<PROSE>>) -> Self {
        value.0.into_iter().map(Into::into).collect()
    }
}
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

macro_rules! metadata_wire {
    ($name:ident, [$($sort_attr:meta),*], {$($(#[$attr:meta])* $field:ident: $type:ty),* $(,)?}) => {
        #[derive(Deserialize)]
        struct $name {
            #[serde(default)] deeplink: Option<Label>,
            #[serde(default, deserialize_with = "present")] description: Option<Prose>,
            #[serde(default, deserialize_with = "present")] description_long: Option<Prose>,
            #[serde(default, deserialize_with = "present")] description_medium: Option<Prose>,
            #[serde(default, deserialize_with = "present")] description_pull_quote: Option<Prose>,
            #[serde(default, deserialize_with = "present")] description_staff: Option<Prose>,
            #[serde(default, deserialize_with = "present")] logo: Option<bool>,
            #[serde(default, deserialize_with = "present", $($sort_attr),*)] is_first_tab_sortable: Option<bool>,
            $(#[serde(default)] $(#[$attr])* $field: $type),*
        }
        impl $name {
            fn owned(self) -> (NativeDetailMetadata, Option<bool>) {
                (NativeDetailMetadata {
                    deeplink: self.deeplink.map(Into::into),
                    description: self.description.map(Into::into),
                    description_long: self.description_long.map(Into::into),
                    description_medium: self.description_medium.map(Into::into),
                    description_pull_quote: self.description_pull_quote.map(Into::into),
                    description_staff: self.description_staff.map(Into::into),
                    logo: self.logo,
                    $($field: self.$field.map(Into::into),)*
                    ..NativeDetailMetadata::default()
                }, self.is_first_tab_sortable)
            }
        }
    };
}
metadata_wire!(FilmWire, [], {
    #[serde(deserialize_with = "present")] director: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] starring: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] country: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] language: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] commentary_tracks: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] genre: Option<Label>,
    #[serde(deserialize_with = "present")] genre_2: Option<Label>,
    #[serde(deserialize_with = "present")] trailer: Option<Label>,
    #[serde(deserialize_with = "present")] introduction_primary: Option<Label>,
    #[serde(deserialize_with = "present")] playlist_supplements: Option<Label>,
    #[serde(deserialize_with = "present")] playlist_related: Option<Label>,
    #[serde(deserialize_with = "present")] playlist_categories_appears_in: Option<Label>,
    #[serde(deserialize_with = "present")] playlist_collections_appears_in: Option<Label>,
    #[serde(deserialize_with = "present")] content_warnings: Option<Label>,
});
metadata_wire!(CategoryWire, [], {
    #[serde(deserialize_with = "present")] playlist_primary: Option<Label>,
    #[serde(deserialize_with = "present")] teaser: Option<Label>,
    #[serde(deserialize_with = "present")] introduction_primary: Option<Label>,
});
metadata_wire!(CollectionWire, [], {
    #[serde(deserialize_with = "present")] teaser: Option<Label>,
    #[serde(deserialize_with = "present")] introduction_primary: Option<Label>,
    collection_count: Option<i32>,
});
metadata_wire!(FranchiseWire, [], {
    #[serde(deserialize_with = "present")] teaser: Option<Label>,
    #[serde(deserialize_with = "present")] introduction_primary: Option<Label>,
});
metadata_wire!(SeriesWire, [], {
    director: Option<Items<Label>>,
    starring: Option<Items<Label>>,
    trailer: Option<Label>,
    introduction_primary: Option<Label>,
    #[serde(deserialize_with = "present")] country: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] language: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] content_warnings: Option<Label>,
});
metadata_wire!(OriginalWire, [], {
    #[serde(deserialize_with = "present")] director: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] starring: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] country: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] language: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] commentary_tracks: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] franchise_id: Option<Label>,
    #[serde(deserialize_with = "present")] trailer: Option<Label>,
    #[serde(deserialize_with = "present")] introduction_primary: Option<Label>,
    #[serde(deserialize_with = "present")] content_warnings: Option<Label>,
});
metadata_wire!(EpisodeWire, [skip_deserializing], {
    director: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] starring: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] country: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] language: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] content_warnings: Option<Label>,
});
metadata_wire!(SupplementWire, [], {
    #[serde(deserialize_with = "present")] director: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] starring: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] country: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] language: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] commentary_tracks: Option<Items<Label>>,
    #[serde(deserialize_with = "present")] supplement_type: Option<Label>,
    #[serde(deserialize_with = "present")] content_warnings: Option<Label>,
});
#[derive(Deserialize)]
struct LiveWire {
    #[serde(default)]
    deeplink: Option<Label>,
    #[serde(default, deserialize_with = "present")]
    description: Option<Prose>,
    #[serde(default, deserialize_with = "present")]
    paywall: Option<Label>,
}
fn fields<T: for<'de> Deserialize<'de>>(response: &Response) -> Result<T, Error> {
    serde_json::from_slice::<Object<T>>(response.body.expose())
        .map(|value| value.0)
        .map_err(|_| Error::InvalidResponse)
}

#[derive(Clone, Copy, Default, Deserialize)]
enum KeyWire {
    #[serde(rename = "playlist_films_appears_in")]
    FilmsAppearsIn,
    #[serde(rename = "playlist_supplements")]
    Supplements,
    #[serde(rename = "playlist_collections_appears_in")]
    CollectionsAppearsIn,
    #[serde(rename = "playlist_categories_appears_in")]
    CategoriesAppearsIn,
    #[serde(rename = "playlist_primary")]
    Primary,
    #[serde(rename = "playlist_collections")]
    Collections,
    #[serde(rename = "playlist_related")]
    Related,
    #[default]
    #[serde(rename = "playlist_other")]
    Other,
}
impl From<KeyWire> for NativePlaylistKey {
    fn from(value: KeyWire) -> Self {
        match value {
            KeyWire::FilmsAppearsIn => Self::FilmsAppearsIn,
            KeyWire::Supplements => Self::Supplements,
            KeyWire::CollectionsAppearsIn => Self::CollectionsAppearsIn,
            KeyWire::CategoriesAppearsIn => Self::CategoriesAppearsIn,
            KeyWire::Primary => Self::Primary,
            KeyWire::Collections => Self::Collections,
            KeyWire::Related => Self::Related,
            KeyWire::Other => Self::Other,
        }
    }
}
#[derive(Deserialize)]
struct GenericWire {
    title: Label,
    #[serde(rename = "playlistId")]
    playlist_id: Label,
    #[serde(default)]
    key: KeyWire,
    #[serde(default)]
    playlist: Items<Object<SummaryWire>>,
}
#[derive(Deserialize)]
#[serde(tag = "type")]
enum PlaylistWire {
    #[serde(rename = "GENERIC_PLAYLIST")]
    Generic(GenericWire),
    #[serde(rename = "seasons")]
    Seasons(SeasonsWire),
}
#[derive(Deserialize)]
struct SeasonsWire {
    title: Label,
    playlist: Items<Object<SeasonWire>>,
}
#[derive(Deserialize)]
struct SeasonWire {
    season_number: i32,
    season_title: Label,
    #[serde(default)]
    season_description: Option<Prose>,
    // The native list uses concrete EpisodeDto, not the tagged MediaDto union.
    #[serde(default)]
    episodes: Items<Object<EpisodeSummaryWire>>,
    #[serde(default)]
    episode_count: i32,
}
#[derive(Deserialize)]
struct PlaylistsWire {
    #[serde(default)]
    playlists: Option<Items<Object<PlaylistWire>>>,
}
#[derive(Deserialize)]
struct FeaturedWire {
    #[serde(default, deserialize_with = "present")]
    title: Option<Label>,
    #[serde(default)]
    playlist: Items<Object<SummaryWire>>,
}
#[derive(Deserialize)]
struct FeaturedRootWire {
    #[serde(default)]
    featured: Option<Object<FeaturedWire>>,
}
fn children<T: Into<crate::MediaSummary>>(
    values: Items<Object<T>>,
) -> (Vec<crate::MediaSummary>, usize) {
    let raw_count = values.0.len();
    let mut seen = HashSet::with_capacity(raw_count);
    let children = values
        .0
        .into_iter()
        .map(|value| value.0.into())
        .filter(|media: &crate::MediaSummary| seen.insert(media.id.clone()))
        .collect();
    (children, raw_count)
}
impl From<PlaylistWire> for NativePlaylist {
    fn from(value: PlaylistWire) -> Self {
        match value {
            PlaylistWire::Generic(value) => {
                let (children, raw_child_count) = children(value.playlist);
                Self::Generic(NativeGenericPlaylist {
                    title: value.title.0,
                    playlist_id: value.playlist_id.0,
                    key: value.key.into(),
                    children,
                    raw_child_count,
                })
            }
            PlaylistWire::Seasons(value) => Self::Seasons(NativeSeasonsPlaylist {
                title: value.title.0,
                seasons: value
                    .playlist
                    .0
                    .into_iter()
                    .map(|value| {
                        let value = value.0;
                        let (episodes, raw_episode_count) = children(value.episodes);
                        NativeSeason {
                            number: value.season_number,
                            title: value.season_title.0,
                            description: value.season_description.map(Into::into),
                            episodes,
                            raw_episode_count,
                            episode_count: value.episode_count,
                        }
                    })
                    .collect(),
            }),
        }
    }
}

pub(crate) fn detail(response: &Response, expected: &MediaId) -> Result<NativeDetail, Error> {
    if response.status != 200 {
        return Err(Error::HttpStatus(response.status));
    }
    if response.body.expose().len() > MAX_BODY {
        return Err(Error::ResponseTooLarge);
    }
    preflight(response.body.expose())?;
    let media: crate::MediaSummary =
        serde_json::from_slice::<Object<SummaryWire>>(response.body.expose())
            .map(|value| value.0.into())
            .map_err(|_| Error::InvalidResponse)?;
    let (metadata, sortable) = match media.kind {
        MediaKind::Category => fields::<CategoryWire>(response)?.owned(),
        MediaKind::Collection => fields::<CollectionWire>(response)?.owned(),
        MediaKind::Series => fields::<SeriesWire>(response)?.owned(),
        MediaKind::Original => fields::<OriginalWire>(response)?.owned(),
        MediaKind::Episode => fields::<EpisodeWire>(response)?.owned(),
        MediaKind::Franchise => fields::<FranchiseWire>(response)?.owned(),
        MediaKind::Film => fields::<FilmWire>(response)?.owned(),
        MediaKind::Supplement => fields::<SupplementWire>(response)?.owned(),
        MediaKind::Live => {
            let value = fields::<LiveWire>(response)?;
            (
                NativeDetailMetadata {
                    deeplink: value.deeplink.map(Into::into),
                    description: value.description.map(Into::into),
                    paywall: value.paywall.map(Into::into),
                    ..NativeDetailMetadata::default()
                },
                None,
            )
        }
    };
    let playlists = if matches!(media.kind, MediaKind::Episode | MediaKind::Live) {
        Vec::new()
    } else {
        fields::<PlaylistsWire>(response)?
            .playlists
            .map_or_else(Vec::new, |values| {
                values.0.into_iter().map(|value| value.0.into()).collect()
            })
    };
    let featured = if matches!(
        media.kind,
        MediaKind::Category | MediaKind::Collection | MediaKind::Franchise
    ) {
        fields::<FeaturedRootWire>(response)?.featured.map(|value| {
            let (children, raw_child_count) = children(value.0.playlist);
            NativeFeatured {
                title: value.0.title.map(Into::into),
                children,
                raw_child_count,
            }
        })
    } else {
        None
    };
    let result = NativeDetail {
        media,
        metadata,
        playlists,
        featured,
        is_first_tab_sortable: sortable,
    };
    if &result.media.id != expected {
        return Err(Error::InvalidResponse);
    }
    if result.estimated_bytes() > MAX_RETAINED {
        return Err(Error::InvalidResponse);
    }
    Ok(result)
}
