use crate::ArtworkError;
use criterion_provider::{EditorialImage, ImageLabel, MediaId};

#[derive(Clone, PartialEq, Eq)]
pub enum ArtworkSource {
    Media { id: MediaId, label: ImageLabel },
    Editorial(EditorialImage),
}

impl ArtworkSource {
    pub(crate) fn url(&self) -> Result<url::Url, ArtworkError> {
        let target = match self {
            Self::Media { id, label } => label.url(id).map_err(|_| ArtworkError::InvalidSource),
            Self::Editorial(image) => Ok(image.url().clone()),
        }?;
        check_url(&target)?;
        Ok(target)
    }
}

fn check_url(target: &url::Url) -> Result<(), ArtworkError> {
    if target.scheme() != "https"
        || target.port().is_some()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.fragment().is_some()
    {
        return Err(ArtworkError::InvalidSource);
    }
    let path: Vec<_> = target.path().split('/').collect();
    let valid = match target.host_str() {
        Some("img.jwplayer.com") => {
            path.len() == 6
                && path[0].is_empty()
                && path[1] == "v1"
                && path[2] == "media"
                && MediaId::new(path[3]).is_ok()
                && path[4] == "images"
                && matches!(
                    path[5],
                    "default_16x9.webp"
                        | "regalia_16x9.webp"
                        | "default_2x3.webp"
                        | "default_bluray.webp"
                )
                && target.query() == Some("width=480")
        }
        Some("cc.criterion.com") => {
            let base = path.len() == 5 && path[3] == "thumbnails"
                || path.len() == 6
                    && !path[3].is_empty()
                    && path[3].len() <= 10
                    && path[3].bytes().all(|byte| byte.is_ascii_digit())
                    && path[4] == "thumbnails";
            base && path[0].is_empty()
                && path[1] == "uploads"
                && path[2] == "storyBlocks"
                && target.query().is_none()
                && path.last().is_some_and(|file| {
                    file.len() <= 256
                        && file.strip_suffix(".webp").is_some_and(|name| {
                            !name.is_empty()
                                && name.bytes().all(|byte| {
                                    byte.is_ascii_alphanumeric()
                                        || matches!(byte, b'_' | b'-' | b',')
                                })
                        })
                })
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ArtworkError::InvalidSource)
    }
}

impl std::fmt::Debug for ArtworkSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ArtworkSource([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_unknown_origin_credentials_paths_queries_and_fragments_are_not_request_targets() {
        for target in [
            "http://img.jwplayer.com/v1/media/qvwT6mJ4/images/default_16x9.webp?width=480",
            "https://img.jwplayer.com.attacker.invalid/v1/media/qvwT6mJ4/images/default_16x9.webp?width=480",
            "https://user:secret@img.jwplayer.com/v1/media/qvwT6mJ4/images/default_16x9.webp?width=480",
            "https://img.jwplayer.com:444/v1/media/qvwT6mJ4/images/default_16x9.webp?width=480",
            "https://img.jwplayer.com/v1/media/qvwT6mJ4/images/default_16x9.webp?width=480&token=secret",
            "https://img.jwplayer.com/v1/media/qvwT6mJ4/images/unknown.webp?width=480",
            "https://img.jwplayer.com/v1/media/qvwT6mJ4/images/default_16x9.webp?width=480#fragment",
            "https://cc.criterion.com/uploads/storyBlocks/123/thumbnails/asset.webp?token=secret",
            "https://cc.criterion.com/uploads/storyBlocks/no-number/thumbnails/asset.webp",
            "https://cc.criterion.com/uploads/storyBlocks/123/thumbnails/asset%2fsecret.webp",
            "https://cc.criterion.com/uploads/storyBlocks/123/thumbnails/deeper/asset.webp",
        ] {
            assert_eq!(
                check_url(&url::Url::parse(target).unwrap()),
                Err(ArtworkError::InvalidSource)
            );
        }
    }

    #[test]
    fn admitted_provider_artwork_types_retain_only_the_exact_public_image_families() {
        let id = MediaId::new("qvwT6mJ4").unwrap();
        for label in [
            ImageLabel::Landscape,
            ImageLabel::Regalia,
            ImageLabel::Portrait,
            ImageLabel::Edition,
        ] {
            assert!(
                ArtworkSource::Media {
                    id: id.clone(),
                    label
                }
                .url()
                .is_ok()
            );
        }
        for base in [
            "https://cc.criterion.com/uploads/storyBlocks/thumbnails/",
            "https://cc.criterion.com/uploads/storyBlocks/123/thumbnails/",
        ] {
            assert!(
                ArtworkSource::Editorial(EditorialImage::new(base, "image_01-asset.webp").unwrap())
                    .url()
                    .is_ok()
            );
        }
        assert_eq!(
            format!(
                "{:?}",
                ArtworkSource::Media {
                    id,
                    label: ImageLabel::Landscape
                }
            ),
            "ArtworkSource([redacted])"
        );
    }

    #[test]
    fn provider_admitted_shorts_banner_comma_remains_a_valid_request_target() {
        let image = EditorialImage::new(
            "https://cc.criterion.com/uploads/storyBlocks/thumbnails/",
            "DB_Romvari,S_Banner_Wide_320x0.webp",
        )
        .unwrap();
        assert!(ArtworkSource::Editorial(image).url().is_ok());
    }
}
