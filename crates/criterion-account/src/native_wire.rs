//! Minimal native SDK projection. Unowned fields are discarded, including
//! source/license URLs. Native duration/position values retain their wire units.
use crate::{
    ContinueWatching, Error, MediaKind, MediaSummary, MyListIds, Position, Response,
    object::Object, wire::MAX_BODY,
};
use criterion_provider::{MediaId, PageCursor};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use std::{collections::BTreeMap, fmt, marker::PhantomData, sync::LazyLock};

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
struct Text<const N: usize, const EMPTY: bool = false>(String);
impl<'de, const N: usize, const EMPTY: bool> Deserialize<'de> for Text<N, EMPTY> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bounded<const N: usize, const EMPTY: bool>;
        impl<const N: usize, const EMPTY: bool> Visitor<'_> for Bounded<N, EMPTY> {
            type Value = Text<N, EMPTY>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("bounded text")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if (!EMPTY && value.is_empty())
                    || value.len() > N
                    || value.chars().any(char::is_control)
                {
                    return Err(E::custom("text limit"));
                }
                Ok(Text(value.into()))
            }
        }
        deserializer.deserialize_str(Bounded::<N, EMPTY>)
    }
}
// Reuse the application's strict eight-character identifier admission. The
// signed native DTO String descriptor does not establish a universal namespace.
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
    positions: Items<Object<PositionWire>>,
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
#[derive(Deserialize)]
struct Episode {
    mediaid: Id,
    title: Text<1024>,
    #[serde(default, deserialize_with = "duration")]
    duration: Option<f32>,
    #[serde(default, deserialize_with = "date")]
    release_date: Option<time::Date>,
    #[serde(default)]
    series_id: Option<Id>,
    #[serde(default)]
    series_title: Option<Text<1024, true>>,
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
    Episode(Episode),
    Franchise(Common),
    Live(Timed),
    Film(TimedDate),
    Supplement(TimedDate),
}
/// Seasons contain concrete Episode DTOs, without a polymorphic discriminator.
#[derive(Deserialize)]
#[serde(transparent)]
pub(crate) struct EpisodeSummaryWire(Episode);
impl From<EpisodeSummaryWire> for MediaSummary {
    fn from(value: EpisodeSummaryWire) -> Self {
        MediaWire::Episode(value.0).into()
    }
}
/// Shared exact native nine-kind summary decoder for owned child lists.
#[derive(Deserialize)]
#[serde(transparent)]
pub(crate) struct SummaryWire(MediaWire);
impl From<SummaryWire> for MediaSummary {
    fn from(value: SummaryWire) -> Self {
        value.0.into()
    }
}
impl From<MediaWire> for MediaSummary {
    fn from(value: MediaWire) -> Self {
        let (series_id, series_title) = match &value {
            MediaWire::Episode(data) => (
                data.series_id.as_ref().map(|id| id.0.clone()),
                data.series_title.as_ref().map(|text| text.0.clone()),
            ),
            _ => (None, None),
        };
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
            series_id,
            series_title,
        }
    }
}
#[derive(Deserialize)]
struct ContinueWire {
    playlist: Items<Object<MediaWire>>,
    positions: Items<Object<PositionWire>>,
}
fn parse<T: for<'de> Deserialize<'de>>(response: &Response) -> Result<T, Error> {
    if response.status != 200 {
        return Err(Error::HttpStatus(response.status));
    }
    if response.body.expose().len() > MAX_BODY {
        return Err(Error::ResponseTooLarge);
    }
    serde_json::from_slice::<Object<T>>(response.body.expose())
        .map(|value| value.0)
        .map_err(|_| Error::InvalidResponse)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntitlementWire {
    access_granted: bool,
    customer_id: i32,
}
pub(crate) fn entitlement(response: &Response) -> Result<crate::NativeEntitlement, Error> {
    // Only the required native projection is owned. Optional grant/expiry/data
    // fields are discarded; this does not infer their semantics or defaults.
    let data: EntitlementWire = parse(response)?;
    Ok(crate::NativeEntitlement {
        access_granted: data.access_granted,
        customer_id: data.customer_id,
    })
}
pub(crate) fn my_list_ids(response: &Response) -> Result<MyListIds, Error> {
    let data: IdsWire = parse(response)?;
    Ok(MyListIds {
        watchlist: data.watchlist.0.into_iter().map(|id| id.0).collect(),
        positions: data
            .positions
            .0
            .into_iter()
            .map(|value| value.0.into())
            .collect(),
    })
}
pub(crate) fn continue_watching(response: &Response) -> Result<ContinueWatching, Error> {
    let data: ContinueWire = parse(response)?;
    Ok(ContinueWatching {
        playlist: data
            .playlist
            .0
            .into_iter()
            .map(|value| value.0.into())
            .collect(),
        positions: data
            .positions
            .0
            .into_iter()
            .map(|value| value.0.into())
            .collect(),
    })
}
struct Cursor(PageCursor);
impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Text::<512>::deserialize(deserializer)?;
        PageCursor::new(&value.0)
            .map(Self)
            .map_err(|_| de::Error::custom("invalid continuation"))
    }
}
#[derive(Deserialize)]
struct PagingWire {
    page_limit: i32,
    #[serde(default)]
    next_pagination_key: Option<Cursor>,
}
struct TypeCounts(BTreeMap<String, i32>);
impl<'de> Deserialize<'de> for TypeCounts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Counts;
        impl<'de> Visitor<'de> for Counts {
            type Value = TypeCounts;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded count map")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some(key) = map.next_key::<Text<64>>()? {
                    if values.len() == 128 || values.contains_key(&key.0) {
                        return Err(de::Error::custom("count map limit or duplicate"));
                    }
                    values.insert(key.0, map.next_value::<i32>()?);
                }
                Ok(TypeCounts(values))
            }
        }
        deserializer.deserialize_map(Counts)
    }
}
#[derive(Deserialize)]
struct WatchWire {
    paging: Object<PagingWire>,
    type_counts: TypeCounts,
    playlist: Items<Object<MediaWire>>,
}
pub(crate) fn watch_list(response: &Response) -> Result<crate::WatchList, Error> {
    let data: WatchWire = parse(response)?;
    Ok(crate::WatchList {
        paging: crate::PagingInfo {
            page_limit: data.paging.0.page_limit,
            next_pagination_key: data.paging.0.next_pagination_key.map(|value| value.0),
        },
        type_counts: data
            .type_counts
            .0
            .into_iter()
            .map(|(content_type, count)| crate::TypeCount {
                content_type,
                count,
            })
            .collect(),
        playlist: data
            .playlist
            .0
            .into_iter()
            .map(|value| value.0.into())
            .collect(),
    })
}
#[derive(Deserialize)]
struct SyncWire {
    #[serde(default)]
    sync: bool,
}
pub(crate) fn sync_receipt(response: &Response) -> Result<crate::SyncReceipt, Error> {
    let data: SyncWire = parse(response)?;
    Ok(crate::SyncReceipt { sync: data.sync })
}
