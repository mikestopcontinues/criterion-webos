//! Minimal native SDK projection. Unowned fields are discarded, including
//! source/license URLs. Native duration/position values retain their wire units.
use crate::{
    ContinueWatching, Error, MediaKind, MediaSummary, MyListIds, Position, Response, wire::MAX_BODY,
};
use criterion_provider::MediaId;
use serde::{
    Deserialize, Deserializer,
    de::{self, SeqAccess, Visitor},
};
use std::{fmt, marker::PhantomData, sync::LazyLock};

const MAX_ITEMS: usize = 512;
struct Items<T>(Vec<T>);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Items<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bounded<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Bounded<T> {
            type Value = Items<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded sequence")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values =
                    Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_ITEMS));
                while let Some(value) = sequence.next_element()? {
                    if values.len() == MAX_ITEMS {
                        return Err(de::Error::custom("sequence limit"));
                    }
                    values.push(value);
                }
                Ok(Items(values))
            }
        }
        deserializer.deserialize_seq(Bounded(PhantomData))
    }
}
struct Text<const N: usize>(String);
impl<'de, const N: usize> Deserialize<'de> for Text<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bounded<const N: usize>;
        impl<const N: usize> Visitor<'_> for Bounded<N> {
            type Value = Text<N>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("bounded nonempty text")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.is_empty() || value.len() > N || value.chars().any(char::is_control) {
                    return Err(E::custom("text limit"));
                }
                Ok(Text(value.into()))
            }
        }
        deserializer.deserialize_str(Bounded::<N>)
    }
}
struct Id(MediaId);
impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Text::<8>::deserialize(deserializer)?;
        MediaId::new(&value.0)
            .map(Self)
            .map_err(|_| de::Error::custom("invalid media identifier"))
    }
}
#[derive(Deserialize)]
struct PositionWire {
    media_id: Id,
    pos: i64,
    dur: i64,
    #[serde(default)]
    commentary_track: Option<Text<128>>,
    #[serde(default)]
    series_id: Option<Id>,
    #[serde(default)]
    series_title: Option<Text<1024>>,
}
impl From<PositionWire> for Position {
    fn from(value: PositionWire) -> Self {
        Self {
            media_id: value.media_id.0,
            pos: value.pos,
            dur: value.dur,
            commentary_track: value.commentary_track.map(|text| text.0),
            series_id: value.series_id.map(|id| id.0),
            series_title: value.series_title.map(|text| text.0),
        }
    }
}
#[derive(Deserialize)]
struct IdsWire {
    watchlist: Items<Id>,
    positions: Items<PositionWire>,
}
#[derive(Deserialize)]
struct Common {
    mediaid: Id,
    title: Text<1024>,
}
#[derive(Deserialize)]
struct Dated {
    mediaid: Id,
    title: Text<1024>,
    #[serde(default, deserialize_with = "date")]
    release_date: Option<time::Date>,
}
#[derive(Deserialize)]
struct Timed {
    mediaid: Id,
    title: Text<1024>,
    #[serde(default, deserialize_with = "duration")]
    duration: Option<f32>,
}
#[derive(Deserialize)]
struct TimedDate {
    mediaid: Id,
    title: Text<1024>,
    #[serde(default, deserialize_with = "duration")]
    duration: Option<f32>,
    #[serde(default, deserialize_with = "date")]
    release_date: Option<time::Date>,
}
fn duration<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f32>, D::Error> {
    // Present null is invalid in the native nonnullable Float32 descriptor;
    // omitted duration remains absent rather than acquiring an invented zero.
    let value = f32::deserialize(deserializer)?;
    if !value.is_finite() || value < 0.0 {
        return Err(de::Error::custom("invalid duration"));
    }
    Ok(Some(value))
}
fn date<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<time::Date>, D::Error> {
    static FORMAT: LazyLock<time::format_description::FormatDescriptionV3<'static>> =
        LazyLock::new(|| {
            time::format_description::parse_borrowed::<3>("[year]-[month]-[day]")
                .expect("fixed date format")
        });
    let Some(value) = Option::<Text<10>>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let bytes = value.0.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return Err(de::Error::custom("invalid date"));
    }
    time::Date::parse(&value.0, &*FORMAT)
        .map(Some)
        .map_err(|_| de::Error::custom("invalid date"))
}
#[derive(Deserialize)]
#[serde(tag = "contentType", rename_all = "lowercase")]
enum MediaWire {
    Category(Common),
    Collection(Common),
    Series(Dated),
    Original(TimedDate),
    Episode(TimedDate),
    Franchise(Common),
    Live(Timed),
    Film(TimedDate),
    Supplement(TimedDate),
}
impl From<MediaWire> for MediaSummary {
    fn from(value: MediaWire) -> Self {
        let (id, title, kind, duration, release_date) = match value {
            MediaWire::Category(data) => {
                (data.mediaid, data.title, MediaKind::Category, None, None)
            }
            MediaWire::Collection(data) => {
                (data.mediaid, data.title, MediaKind::Collection, None, None)
            }
            MediaWire::Franchise(data) => {
                (data.mediaid, data.title, MediaKind::Franchise, None, None)
            }
            MediaWire::Series(data) => (
                data.mediaid,
                data.title,
                MediaKind::Series,
                None,
                data.release_date,
            ),
            MediaWire::Live(data) => (
                data.mediaid,
                data.title,
                MediaKind::Live,
                data.duration,
                None,
            ),
            MediaWire::Original(data) => (
                data.mediaid,
                data.title,
                MediaKind::Original,
                data.duration,
                data.release_date,
            ),
            MediaWire::Episode(data) => (
                data.mediaid,
                data.title,
                MediaKind::Episode,
                data.duration,
                data.release_date,
            ),
            MediaWire::Film(data) => (
                data.mediaid,
                data.title,
                MediaKind::Film,
                data.duration,
                data.release_date,
            ),
            MediaWire::Supplement(data) => (
                data.mediaid,
                data.title,
                MediaKind::Supplement,
                data.duration,
                data.release_date,
            ),
        };
        Self {
            id: id.0,
            title: title.0,
            kind,
            duration,
            release_date,
        }
    }
}
#[derive(Deserialize)]
struct ContinueWire {
    playlist: Items<MediaWire>,
    positions: Items<PositionWire>,
}
fn parse<T: for<'de> Deserialize<'de>>(response: &Response) -> Result<T, Error> {
    if response.status != 200 {
        return Err(Error::HttpStatus(response.status));
    }
    if response.body.expose().len() > MAX_BODY {
        return Err(Error::ResponseTooLarge);
    }
    serde_json::from_slice(response.body.expose()).map_err(|_| Error::InvalidResponse)
}
pub(crate) fn my_list_ids(response: &Response) -> Result<MyListIds, Error> {
    let data: IdsWire = parse(response)?;
    Ok(MyListIds {
        watchlist: data.watchlist.0.into_iter().map(|id| id.0).collect(),
        positions: data.positions.0.into_iter().map(Into::into).collect(),
    })
}
pub(crate) fn continue_watching(response: &Response) -> Result<ContinueWatching, Error> {
    let data: ContinueWire = parse(response)?;
    Ok(ContinueWatching {
        playlist: data.playlist.0.into_iter().map(Into::into).collect(),
        positions: data.positions.0.into_iter().map(Into::into).collect(),
    })
}
