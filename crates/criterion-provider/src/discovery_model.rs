//! Typed ordered discovery metadata and validated content/artwork targets.
use crate::{Error, MediaId, MediaSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryRoute {
    Home,
    New,
    Discover(Slug),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaRoute {
    Film,
    Collection,
    Category,
    Supplement,
    Series,
    Original,
    Live,
    Franchise,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Slug(String);

#[derive(Clone, PartialEq, Eq)]
pub enum ContentTarget {
    Media {
        route: MediaRoute,
        id: MediaId,
        slug: Slug,
    },
    Discover(Slug),
    Home,
    New,
    AllFilms,
    MyList,
    Subscribe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageLabel {
    Landscape,
    Regalia,
    Portrait,
    Edition,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EditorialImage(url::Url);

#[derive(Clone, PartialEq, Eq)]
pub struct ResponsiveImage {
    pub width: u16,
    pub image: EditorialImage,
}

#[derive(Clone, PartialEq)]
pub struct DiscoveryCard {
    pub media: MediaSummary,
    pub target: ContentTarget,
}

#[derive(Clone, PartialEq)]
pub struct DiscoveryPage {
    pub blocks: Vec<DiscoveryBlock>,
}

#[derive(Clone, PartialEq)]
pub enum DiscoveryBlock {
    Slideshow {
        id: u32,
        slides: Vec<DiscoverySlide>,
    },
    Rail {
        id: u32,
        header: Option<String>,
        cta: Option<String>,
        target: Option<ContentTarget>,
        opens_new_window: bool,
        source: RailSource,
        cards: Vec<DiscoveryCard>,
        image_label: ImageLabel,
        presentation: GalleryPresentation,
    },
    Navigation {
        id: u32,
        header: Option<String>,
        items: Vec<DiscoveryNavItem>,
        presentation: GalleryPresentation,
    },
    Banner {
        id: u32,
        alt: Option<String>,
        target: Option<ContentTarget>,
        opens_new_window: bool,
        subscription_promo: bool,
        artwork: DiscoveryArtwork,
    },
}

#[derive(Clone, PartialEq)]
pub struct DiscoverySlide {
    pub id: u32,
    pub title: Option<String>,
    pub title_prefix: Option<String>,
    pub cta: Option<String>,
    pub target: Option<ContentTarget>,
    pub opens_new_window: bool,
    pub artwork: DiscoveryArtwork,
}

#[derive(Clone, PartialEq)]
pub struct DiscoveryNavItem {
    pub id: u32,
    pub label: String,
    pub target: ContentTarget,
    pub opens_new_window: bool,
    pub artwork: DiscoveryArtwork,
}

#[derive(Clone, PartialEq)]
pub struct DiscoveryArtwork {
    pub desktop: Vec<ResponsiveImage>,
    pub mobile: Vec<ResponsiveImage>,
    pub logo: Option<EditorialImage>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum RailSource {
    Provided { feed_id: Option<String> },
    Watchlist,
    ContinueWatching,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GalleryLayout {
    Rail,
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GalleryPresentation {
    pub aspect_ratio_percent: f32,
    pub cards_per_view: u8,
    pub layout: GalleryLayout,
    pub variant: u8,
}

impl Slug {
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(Error::InvalidRequest);
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ContentTarget {
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.len() > 512
            || value.contains(['%', '?', '#', '\\'])
            || value.chars().any(char::is_control)
        {
            return Err(Error::InvalidRequest);
        }
        let path = value
            .strip_prefix("https://www.criterionchannel.com")
            .unwrap_or(value);
        match path {
            "/" => return Ok(Self::Home),
            "/new" => return Ok(Self::New),
            "/all-films" => return Ok(Self::AllFilms),
            "/my-list" => return Ok(Self::MyList),
            "/subscribe" => return Ok(Self::Subscribe),
            _ => (),
        }
        let segments: Vec<_> = path.split('/').collect();
        if segments.len() == 3 && segments[0].is_empty() && segments[1] == "discover" {
            return Ok(Self::Discover(Slug::new(segments[2])?));
        }
        if segments.len() != 4 || !segments[0].is_empty() {
            return Err(Error::InvalidRequest);
        }
        let route = match segments[1] {
            "films" => MediaRoute::Film,
            "collections" => MediaRoute::Collection,
            "categories" => MediaRoute::Category,
            "supplements" => MediaRoute::Supplement,
            "series" => MediaRoute::Series,
            "originals" => MediaRoute::Original,
            "live" => MediaRoute::Live,
            "franchises" => MediaRoute::Franchise,
            _ => return Err(Error::InvalidRequest),
        };
        Ok(Self::Media {
            route,
            id: MediaId::new(segments[2])?,
            slug: Slug::new(segments[3])?,
        })
    }

    pub fn url(&self) -> Result<url::Url, Error> {
        let path = match self {
            Self::Home => "/".into(),
            Self::New => "/new".into(),
            Self::AllFilms => "/all-films".into(),
            Self::MyList => "/my-list".into(),
            Self::Subscribe => "/subscribe".into(),
            Self::Discover(slug) => format!("/discover/{}", slug.as_str()),
            Self::Media { route, id, slug } => format!(
                "/{}/{}/{}",
                match route {
                    MediaRoute::Film => "films",
                    MediaRoute::Collection => "collections",
                    MediaRoute::Category => "categories",
                    MediaRoute::Supplement => "supplements",
                    MediaRoute::Series => "series",
                    MediaRoute::Original => "originals",
                    MediaRoute::Live => "live",
                    MediaRoute::Franchise => "franchises",
                },
                id.as_str(),
                slug.as_str()
            ),
        };
        url::Url::parse(&format!("https://www.criterionchannel.com{path}"))
            .map_err(|_| Error::InvalidRequest)
    }
}

impl ImageLabel {
    pub(super) fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "default_16x9" => Ok(Self::Landscape),
            "regalia_16x9" => Ok(Self::Regalia),
            "default_2x3" => Ok(Self::Portrait),
            "default_bluray" => Ok(Self::Edition),
            _ => Err(Error::InvalidResponse),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Landscape => "default_16x9",
            Self::Regalia => "regalia_16x9",
            Self::Portrait => "default_2x3",
            Self::Edition => "default_bluray",
        }
    }
    /// Uses the admitted row label; availability belongs to the image consumer.
    pub fn url(self, id: &MediaId) -> Result<url::Url, Error> {
        url::Url::parse(&format!(
            "https://img.jwplayer.com/v1/media/{}/images/{}.webp?width=480",
            id.as_str(),
            self.as_str()
        ))
        .map_err(|_| Error::InvalidRequest)
    }
}

impl EditorialImage {
    /// Validates raw source parts before URL normalization can hide traversal.
    pub fn new(base: &str, filename: &str) -> Result<Self, Error> {
        let suffix = base
            .strip_prefix("https://cc.criterion.com/uploads/storyBlocks/")
            .ok_or(Error::InvalidRequest)?;
        if base.len() > 256
            || !(suffix == "thumbnails/"
                || suffix.strip_suffix("/thumbnails/").is_some_and(|id| {
                    !id.is_empty() && id.len() <= 10 && id.bytes().all(|b| b.is_ascii_digit())
                }))
            || filename.len() > 256
            || !filename.strip_suffix(".webp").is_some_and(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b','))
            })
        {
            return Err(Error::InvalidRequest);
        }
        let url =
            url::Url::parse(&format!("{base}{filename}")).map_err(|_| Error::InvalidRequest)?;
        Ok(Self(url))
    }
    pub fn url(&self) -> &url::Url {
        &self.0
    }
}

impl DiscoveryBlock {
    pub fn id(&self) -> u32 {
        match self {
            Self::Slideshow { id, .. }
            | Self::Rail { id, .. }
            | Self::Navigation { id, .. }
            | Self::Banner { id, .. } => *id,
        }
    }
}

macro_rules! redacted_debug {
    ($($name:ty),+ $(,)?) => { $(impl std::fmt::Debug for $name {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(concat!(stringify!($name), "([redacted])"))
        }
    })+ };
}

redacted_debug!(
    Slug,
    ContentTarget,
    EditorialImage,
    ResponsiveImage,
    DiscoveryCard,
    DiscoveryPage,
    DiscoveryBlock,
    DiscoverySlide,
    DiscoveryNavItem,
    DiscoveryArtwork,
    RailSource
);
