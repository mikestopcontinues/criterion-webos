//! Contract42a5cade guarded projection, not the full native MediaDto serializer.
//! Optional full metadata and nonempty OffsetDateTime syntax remain unowned.
use crate::{
    Error, NativePlayback, NativePlaybackSelection, Response, native_wire::SummaryWire,
    object::Object, playback_number,
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;
use zeroize::Zeroizing;

const MAX_BODY: usize = 512 * 1024;
// Conservative application limits, not native SDK/server guarantees.
const MAX_DEPTH: usize = 64;
const MAX_ITEMS: usize = 512;
const MAX_RETAINED: usize = 512 * 1024;

// RawValue preserves even enormous JSON number exponents without a floating
// JSON conversion. This Serde walk bounds all content, including discarded
// metadata and tracks after the native first-match stop; it adds no shape rules.
struct Scan(usize);
impl<'de> DeserializeSeed<'de> for Scan {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        let raw = <&RawValue>::deserialize(deserializer)?;
        scan(raw, self.0).map_err(de::Error::custom)
    }
}
impl<'de> Visitor<'de> for Scan {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded native JSON")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        while sequence.next_element_seed(Scan(self.0))?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if key.len() > playback_number::MAX_PRIMITIVE {
                return Err(de::Error::custom("primitive policy"));
            }
            map.next_value_seed(Scan(self.0))?;
        }
        Ok(())
    }
}
fn scan(raw: &RawValue, depth: usize) -> Result<(), Error> {
    let mut deserializer = serde_json::Deserializer::from_str(raw.get());
    match raw.get().as_bytes().first() {
        Some(b'[' | b'{') if depth == MAX_DEPTH => Err(Error::InvalidResponse),
        Some(b'[') => deserializer
            .deserialize_seq(Scan(depth + 1))
            .map_err(|_| Error::InvalidResponse),
        Some(b'{') => deserializer
            .deserialize_map(Scan(depth + 1))
            .map_err(|_| Error::InvalidResponse),
        _ => {
            let text = primitive(raw)?;
            if text.len() > playback_number::MAX_PRIMITIVE {
                return Err(Error::InvalidResponse);
            }
            Ok(())
        }
    }
}

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<&'de RawValue>, D::Error> {
    <&RawValue>::deserialize(deserializer).map(Some)
}
struct Items<'a>(Vec<&'a RawValue>);
impl<'de: 'a, 'a> Deserialize<'de> for Items<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bounded;
        impl<'de> Visitor<'de> for Bounded {
            type Value = Items<'de>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("bounded native array")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(raw) = sequence.next_element::<&RawValue>()? {
                    if values.len() == MAX_ITEMS {
                        return Err(de::Error::custom("item policy"));
                    }
                    values.push(raw);
                }
                Ok(Items(values))
            }
        }
        deserializer.deserialize_seq(Bounded)
    }
}
#[derive(Deserialize)]
struct Root<'a> {
    #[serde(borrow)]
    playlist: Items<'a>,
    #[serde(default, borrow, deserialize_with = "present")]
    cast_token: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    license_end_date_time: Option<&'a RawValue>,
}
#[derive(Deserialize)]
struct Item<'a> {
    #[serde(borrow)]
    sources: Items<'a>,
    #[serde(default, borrow, deserialize_with = "present")]
    tracks: Option<&'a RawValue>,
    #[serde(rename = "contentType", borrow)]
    content_type: &'a RawValue,
    #[serde(borrow)]
    mediaid: &'a RawValue,
    #[serde(borrow)]
    title: &'a RawValue,
    #[serde(default, borrow, deserialize_with = "present")]
    duration: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    release_date: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    series_id: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    series_title: Option<&'a RawValue>,
}
#[derive(Serialize)]
struct Summary<'a> {
    #[serde(rename = "contentType")]
    content_type: &'a RawValue,
    mediaid: &'a RawValue,
    title: &'a RawValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<Duration<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_date: Option<&'a RawValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    series_id: Option<&'a RawValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    series_title: Option<&'a RawValue>,
}
#[derive(Serialize)]
#[serde(untagged)]
enum Duration<'a> {
    Original(&'a RawValue),
    Normalized(i32),
}
#[derive(Deserialize)]
struct Source<'a> {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, borrow, deserialize_with = "present")]
    file: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    drm: Option<&'a RawValue>,
}
#[derive(Deserialize)]
struct Drm<'a> {
    #[serde(borrow)]
    widevine: &'a RawValue,
}
#[derive(Deserialize)]
struct Widevine<'a> {
    #[serde(borrow)]
    url: &'a RawValue,
}
#[derive(Deserialize)]
struct Track<'a> {
    #[serde(default, borrow, deserialize_with = "present")]
    kind: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    file: Option<&'a RawValue>,
}
fn ordinary_string(raw: &RawValue) -> Result<Zeroizing<String>, Error> {
    serde_json::from_str::<String>(raw.get())
        .map(Zeroizing::new)
        .map_err(|_| Error::InvalidResponse)
}
fn primitive(raw: &RawValue) -> Result<Zeroizing<String>, Error> {
    match raw.get().as_bytes().first() {
        Some(b'"') => ordinary_string(raw),
        Some(b'[' | b'{') => Err(Error::InvalidResponse),
        _ => Ok(Zeroizing::new(raw.get().into())),
    }
}
struct Dash {
    file: Zeroizing<String>,
    widevine_license: Option<Zeroizing<String>>,
}
fn source(raw: &RawValue) -> Result<Option<Dash>, Error> {
    let Object(source): Object<Source<'_>> =
        serde_json::from_str(raw.get()).map_err(|_| Error::InvalidResponse)?;
    match source.kind.as_str() {
        "audio/mp4" | "application/vnd.apple.mpegurl" => Ok(None),
        "application/dash+xml" => {
            let file = ordinary_string(source.file.ok_or(Error::InvalidResponse)?)?;
            let license = match source.drm {
                None => None,
                Some(raw) if raw.get() == "null" => None,
                Some(raw) => {
                    let Object(drm): Object<Drm<'_>> =
                        serde_json::from_str(raw.get()).map_err(|_| Error::InvalidResponse)?;
                    let Object(widevine): Object<Widevine<'_>> =
                        serde_json::from_str(drm.widevine.get())
                            .map_err(|_| Error::InvalidResponse)?;
                    Some(ordinary_string(widevine.url)?)
                }
            };
            Ok(Some(Dash {
                file,
                widevine_license: license,
            }))
        }
        _ => Err(Error::InvalidResponse),
    }
}
struct Tracks;
impl<'de> Visitor<'de> for Tracks {
    type Value = Option<Zeroizing<String>>;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("native tracks array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut matched = false;
        let mut thumbnail = None;
        while let Some(raw) = sequence.next_element::<&RawValue>()? {
            if matched {
                continue;
            }
            let Object(track): Object<Track<'_>> =
                serde_json::from_str(raw.get()).map_err(de::Error::custom)?;
            let Some(kind) = track.kind.filter(|raw| raw.get() != "null") else {
                continue;
            };
            if primitive(kind).map_err(de::Error::custom)?.as_str() == "thumbnails" {
                matched = true;
                thumbnail = track
                    .file
                    .filter(|raw| raw.get() != "null")
                    .map(primitive)
                    .transpose()
                    .map_err(de::Error::custom)?;
            }
        }
        Ok(thumbnail)
    }
}
fn item(raw: &RawValue, count: &mut usize) -> Result<Option<NativePlayback>, Error> {
    let Object(item): Object<Item<'_>> =
        serde_json::from_str(raw.get()).map_err(|_| Error::InvalidResponse)?;
    *count = count
        .checked_add(item.sources.0.len())
        .ok_or(Error::InvalidResponse)?;
    if *count > MAX_ITEMS {
        return Err(Error::InvalidResponse);
    }
    let mut dash = None;
    for raw in item.sources.0 {
        let candidate = source(raw)?;
        if dash.is_none() {
            dash = candidate;
        }
    }
    let thumbnail = item
        .tracks
        .map(|raw| {
            let mut deserializer = serde_json::Deserializer::from_str(raw.get());
            deserializer
                .deserialize_seq(Tracks)
                .map_err(|_| Error::InvalidResponse)
        })
        .transpose()?
        .flatten();
    // This guard/normalization deliberately precedes subtype dispatch, including
    // native containers that have no duration field. Failed parses stay original.
    let duration = item
        .duration
        .map(|raw| {
            let content = primitive(raw)?;
            Ok(match playback_number::parse(&content) {
                Some(value) => Duration::Normalized(value as i32),
                None => Duration::Original(raw),
            })
        })
        .transpose()?;
    // Feed only subtype-owned fields into the existing guarded summary decoder.
    // Its internally tagged buffer eagerly parses numeric values, so forwarding
    // a discarded key could falsely reject an otherwise valid raw primitive.
    let kind = ordinary_string(item.content_type)?;
    let timed = matches!(
        kind.as_str(),
        "film" | "original" | "episode" | "live" | "supplement"
    );
    let dated = matches!(
        kind.as_str(),
        "film" | "original" | "episode" | "series" | "supplement"
    );
    let episode = kind.as_str() == "episode";
    let summary = Summary {
        content_type: item.content_type,
        mediaid: item.mediaid,
        title: item.title,
        duration: timed.then_some(duration).flatten(),
        release_date: dated.then_some(item.release_date).flatten(),
        series_id: episode.then_some(item.series_id).flatten(),
        series_title: episode.then_some(item.series_title).flatten(),
    };
    let bytes = Zeroizing::new(serde_json::to_vec(&summary).map_err(|_| Error::InvalidResponse)?);
    let media: SummaryWire = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse)?;
    Ok(dash.map(|dash| NativePlayback {
        media: media.into(),
        dash_file: dash.file,
        widevine_license: dash.widevine_license,
        thumbnail,
    }))
}
pub(crate) fn decode(response: &Response) -> Result<NativePlaybackSelection, Error> {
    if response.status != 200 {
        return Err(Error::HttpStatus(response.status));
    }
    if response.body.expose().len() > MAX_BODY {
        return Err(Error::ResponseTooLarge);
    }
    let raw: &RawValue =
        serde_json::from_slice(response.body.expose()).map_err(|_| Error::InvalidResponse)?;
    scan(raw, 0)?;
    let Object(root): Object<Root<'_>> =
        serde_json::from_slice(response.body.expose()).map_err(|_| Error::InvalidResponse)?;
    // Supplied nullable strings are type-validated, then intentionally discarded.
    // Nonempty license date syntax and unowned full MediaDto metadata are excluded.
    for raw in [root.cast_token, root.license_end_date_time]
        .into_iter()
        .flatten()
    {
        if raw.get() != "null" {
            // In particular, never create an ordinary owned cast-token copy.
            drop(ordinary_string(raw)?);
        }
    }
    let empty = root.playlist.0.is_empty();
    let mut count = root.playlist.0.len();
    let mut first = None;
    for (index, raw) in root.playlist.0.into_iter().enumerate() {
        let selected = item(raw, &mut count)?;
        if index == 0 {
            first = selected;
        }
    }
    if let Some(playback) = &first {
        // Only one fixed-size projection is retained. Charge owned capacities;
        // other playlist/source candidates have already been zeroized on drop.
        let owned = std::mem::size_of::<NativePlayback>()
            + playback.media.id.as_str().len()
            + playback.media.title.capacity()
            + playback
                .media
                .series_id
                .as_ref()
                .map_or(0, |id| id.as_str().len())
            + playback
                .media
                .series_title
                .as_ref()
                .map_or(0, String::capacity)
            + playback.dash_file.capacity()
            + playback
                .widevine_license
                .as_ref()
                .map_or(0, |text| text.capacity())
            + playback
                .thumbnail
                .as_ref()
                .map_or(0, |text| text.capacity());
        if owned > MAX_RETAINED {
            return Err(Error::InvalidResponse);
        }
    }
    Ok(match first {
        Some(playback) => NativePlaybackSelection::Selected(playback),
        None if empty => NativePlaybackSelection::EmptyPlaylist,
        None => NativePlaybackSelection::NoDash,
    })
}
