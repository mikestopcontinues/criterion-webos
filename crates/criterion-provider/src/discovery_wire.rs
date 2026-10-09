//! Bounded projection of each supported story block; unknown fields stay private.
use crate::{
    Error, MediaId, MediaKind,
    discovery_model::*,
    wire::{WireMedia, check_text},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct WireRail {
    header: Option<String>,
    cta: Option<String>,
    link: Option<String>,
    #[serde(rename = "linkTarget", default)]
    link_target: u8,
    #[serde(rename = "playlistType")]
    playlist_type: String,
    #[serde(rename = "playlistID")]
    playlist_id: Option<String>,
    #[serde(rename = "imageJWLabel")]
    image_label: String,
    #[serde(flatten)]
    gallery: WireGallery,
    playlist: Option<Vec<WireCard>>,
}

#[derive(Deserialize)]
struct WireCard {
    deeplink: String,
    #[serde(flatten)]
    media: WireMedia,
}

#[derive(Deserialize)]
struct WireSlide {
    id: u32,
    header1: Option<String>,
    header2: Option<String>,
    link: Option<String>,
    cta: Option<String>,
    #[serde(rename = "linkTarget", default)]
    link_target: u8,
    #[serde(rename = "logoImage")]
    logo: Option<String>,
    thumbs: std::collections::BTreeMap<String, String>,
    #[serde(rename = "thumbsMobile", default)]
    mobile: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct WireGallery {
    #[serde(rename = "imageAspectRatio")]
    aspect: f32,
    #[serde(rename = "galleryPageNum")]
    cards_per_view: u8,
    #[serde(rename = "galleryWrap")]
    wrap: u8,
    #[serde(default)]
    variant: u8,
}

impl WireGallery {
    fn project(self) -> Result<GalleryPresentation, Error> {
        if !self.aspect.is_finite()
            || !(1.0..=300.0).contains(&self.aspect)
            || !(1..=20).contains(&self.cards_per_view)
            || self.variant > 1
        {
            return Err(Error::InvalidResponse);
        }
        let layout = match self.wrap {
            0 => GalleryLayout::Rail,
            1 => GalleryLayout::Grid,
            _ => return Err(Error::InvalidResponse),
        };
        Ok(GalleryPresentation {
            aspect_ratio_percent: self.aspect,
            cards_per_view: self.cards_per_view,
            layout,
            variant: self.variant,
        })
    }
}

#[derive(Deserialize)]
struct WireNavigation {
    header: Option<String>,
    #[serde(rename = "navImagePath")]
    base: Option<String>,
    nav: Vec<WireNavItem>,
    #[serde(flatten)]
    gallery: WireGallery,
}

#[derive(Deserialize)]
struct WireNavItem {
    id: u32,
    cta: String,
    link: String,
    #[serde(rename = "linkTarget", default)]
    link_target: u8,
    #[serde(default)]
    thumbs: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct WireBanner {
    #[serde(rename = "imageAlt")]
    alt: Option<String>,
    link: Option<String>,
    #[serde(rename = "linkTarget", default)]
    link_target: u8,
    #[serde(rename = "subscriptionPromo", default)]
    promo: u8,
    #[serde(rename = "imagePath")]
    base: String,
    thumbs: std::collections::BTreeMap<String, String>,
    #[serde(rename = "thumbsMobile", default)]
    mobile: std::collections::BTreeMap<String, String>,
}

pub(super) fn block(id: u32, kind: u64, value: serde_json::Value) -> Result<DiscoveryBlock, Error> {
    match kind {
        28 => {
            let base = value
                .get("slideImagePath")
                .and_then(serde_json::Value::as_str)
                .ok_or(Error::InvalidResponse)?;
            let slides: Vec<WireSlide> =
                serde_json::from_value(value.get("slides").cloned().ok_or(Error::InvalidResponse)?)
                    .map_err(|_| Error::InvalidResponse)?;
            if slides.is_empty() || slides.len() > 32 {
                return Err(Error::InvalidResponse);
            }
            let slides = slides
                .into_iter()
                .map(|slide| {
                    if slide.thumbs.is_empty() {
                        return Err(Error::InvalidResponse);
                    }
                    let title = optional_text(slide.header2)?;
                    let title_prefix = optional_text(slide.header1)?;
                    let target = slide
                        .link
                        .map(|link| ContentTarget::parse(&link).map_err(|_| Error::InvalidResponse))
                        .transpose()?;
                    let artwork = artwork(Some(base), slide.thumbs, slide.mobile, slide.logo)?;
                    Ok(DiscoverySlide {
                        id: slide.id,
                        title,
                        title_prefix,
                        cta: optional_text(slide.cta)?,
                        target,
                        opens_new_window: flag(slide.link_target)?,
                        artwork,
                    })
                })
                .collect::<Result<_, Error>>()?;
            Ok(DiscoveryBlock::Slideshow { id, slides })
        }
        20 => {
            let rail: WireRail =
                serde_json::from_value(value).map_err(|_| Error::InvalidResponse)?;
            let playlist = match rail.playlist_type.as_str() {
                "playlist" => rail.playlist.ok_or(Error::InvalidResponse)?,
                _ => rail.playlist.unwrap_or_default(),
            };
            if playlist.len() > 512 {
                return Err(Error::InvalidResponse);
            }
            let cards: Vec<DiscoveryCard> = playlist.into_iter().map(|card| {
                    let target = ContentTarget::parse(&card.deeplink).map_err(|_| Error::InvalidResponse)?;
                    let media = card.media.into_summary()?;
                    let expected_route = match media.kind {
                        MediaKind::Film => MediaRoute::Film,
                        MediaKind::Collection => MediaRoute::Collection,
                        MediaKind::Category => MediaRoute::Category,
                        MediaKind::Supplement => MediaRoute::Supplement,
                        MediaKind::Series => MediaRoute::Series,
                        MediaKind::Live => MediaRoute::Live,
                    };
                    if !matches!(&target, ContentTarget::Media {route, id, ..} if *route == expected_route && *id == media.id) { return Err(Error::InvalidResponse); }
                    Ok(DiscoveryCard { media, target })
                }).collect::<Result<_,Error>>()?;
            let source = match rail.playlist_type.as_str() {
                "playlist" => {
                    if let Some(feed_id) = &rail.playlist_id {
                        MediaId::new(feed_id).map_err(|_| Error::InvalidResponse)?;
                    }
                    RailSource::Provided {
                        feed_id: rail.playlist_id,
                    }
                }
                "watchlist" if cards.is_empty() => RailSource::Watchlist,
                "continueWatching" if cards.is_empty() => RailSource::ContinueWatching,
                _ => return Err(Error::InvalidResponse),
            };
            Ok(DiscoveryBlock::Rail {
                id,
                header: optional_text(rail.header)?,
                cta: optional_text(rail.cta)?,
                target: optional_target(rail.link)?,
                opens_new_window: flag(rail.link_target)?,
                source,
                cards,
                image_label: ImageLabel::parse(&rail.image_label)?,
                presentation: rail.gallery.project()?,
            })
        }
        21 => {
            let nav: WireNavigation =
                serde_json::from_value(value).map_err(|_| Error::InvalidResponse)?;
            if nav.nav.is_empty() || nav.nav.len() > 128 {
                return Err(Error::InvalidResponse);
            }
            let items = nav
                .nav
                .into_iter()
                .map(|item| {
                    check_text(&item.cta, 512, false)?;
                    if item.cta.trim().is_empty() {
                        return Err(Error::InvalidResponse);
                    }
                    Ok(DiscoveryNavItem {
                        id: item.id,
                        label: item.cta,
                        target: ContentTarget::parse(&item.link)
                            .map_err(|_| Error::InvalidResponse)?,
                        opens_new_window: flag(item.link_target)?,
                        artwork: artwork(
                            nav.base.as_deref(),
                            item.thumbs,
                            Default::default(),
                            None,
                        )?,
                    })
                })
                .collect::<Result<_, Error>>()?;
            Ok(DiscoveryBlock::Navigation {
                id,
                header: optional_text(nav.header)?,
                items,
                presentation: nav.gallery.project()?,
            })
        }
        22 => {
            let banner: WireBanner =
                serde_json::from_value(value).map_err(|_| Error::InvalidResponse)?;
            if banner.thumbs.is_empty() {
                return Err(Error::InvalidResponse);
            }
            Ok(DiscoveryBlock::Banner {
                id,
                alt: optional_text(banner.alt)?,
                target: optional_target(banner.link)?,
                opens_new_window: flag(banner.link_target)?,
                subscription_promo: flag(banner.promo)?,
                artwork: artwork(Some(&banner.base), banner.thumbs, banner.mobile, None)?,
            })
        }
        _ => Err(Error::InvalidResponse),
    }
}

fn optional_text(value: Option<String>) -> Result<Option<String>, Error> {
    value
        .map(|value| {
            check_text(&value, 512, false)?;
            Ok(value)
        })
        .transpose()
}

fn optional_target(value: Option<String>) -> Result<Option<ContentTarget>, Error> {
    value
        .map(|value| ContentTarget::parse(&value).map_err(|_| Error::InvalidResponse))
        .transpose()
}

fn flag(value: u8) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::InvalidResponse),
    }
}

fn artwork(
    base: Option<&str>,
    desktop: std::collections::BTreeMap<String, String>,
    mobile: std::collections::BTreeMap<String, String>,
    logo: Option<String>,
) -> Result<DiscoveryArtwork, Error> {
    fn images(
        base: Option<&str>,
        values: std::collections::BTreeMap<String, String>,
    ) -> Result<Vec<ResponsiveImage>, Error> {
        if values.len() > 16 {
            return Err(Error::InvalidResponse);
        }
        let mut images = values
            .into_iter()
            .map(|(width, filename)| {
                if width.len() > 4 || !width.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(Error::InvalidResponse);
                }
                let width = width.parse::<u16>().map_err(|_| Error::InvalidResponse)?;
                if !(1..=4096).contains(&width) {
                    return Err(Error::InvalidResponse);
                }
                let image = EditorialImage::new(base.ok_or(Error::InvalidResponse)?, &filename)
                    .map_err(|_| Error::InvalidResponse)?;
                Ok(ResponsiveImage { width, image })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        images.sort_by_key(|image| image.width);
        Ok(images)
    }
    let logo = logo
        .map(|name| {
            EditorialImage::new(base.ok_or(Error::InvalidResponse)?, &name)
                .map_err(|_| Error::InvalidResponse)
        })
        .transpose()?;
    Ok(DiscoveryArtwork {
        desktop: images(base, desktop)?,
        mobile: images(base, mobile)?,
        logo,
    })
}
