use crate::{Error, MediaDetail, MediaId, MediaKind, MediaSummary, Playlist};
use serde::Deserialize;

pub(super) fn check_json_bounds(body: &[u8]) -> Result<(), Error> {
    let mut depth = 0_usize;
    let mut tokens = 0_usize;
    let mut string_start = None;
    let mut escaped = false;
    for (index, byte) in body.iter().copied().enumerate() {
        if let Some(start) = string_start {
            if index - start > 65_536 {
                return Err(Error::InvalidResponse);
            }
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string_start = None;
            }
            continue;
        }
        match byte {
            b'"' => string_start = Some(index + 1),
            b'{' | b'[' => {
                depth += 1;
                tokens += 1;
                if depth > 16 {
                    return Err(Error::InvalidResponse);
                }
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1).ok_or(Error::InvalidResponse)?;
            }
            b',' | b':' => tokens += 1,
            _ => (),
        }
        if tokens > 32_768 {
            return Err(Error::InvalidResponse);
        }
    }
    Ok(())
}

pub(super) fn check_text(value: &str, limit: usize, multiline: bool) -> Result<(), Error> {
    if value.len() > limit
        || value.chars().any(|character| {
            character.is_control() && !(multiline && matches!(character, '\n' | '\r' | '\t'))
        })
    {
        return Err(Error::InvalidResponse);
    }
    Ok(())
}

fn check_names(values: &[String]) -> Result<(), Error> {
    if values.len() > 128 {
        return Err(Error::InvalidResponse);
    }
    values
        .iter()
        .try_for_each(|value| check_text(value, 512, false))
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let Ok(year) = value[..4].parse::<u16>() else {
        return false;
    };
    let Ok(month) = value[5..7].parse::<u8>() else {
        return false;
    };
    let Ok(day) = value[8..].parse::<u8>() else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    year > 0 && day > 0 && day <= days
}

#[derive(Deserialize)]
pub(super) struct WirePage {
    pub(super) items: Vec<WireMedia>,
    pub(super) total: u32,
    pub(super) paging: WirePaging,
}

#[derive(Deserialize)]
pub(super) struct WirePaging {
    pub(super) next_pagination_key: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct WireSearch {
    pub(super) playlist: Vec<WireMedia>,
    pub(super) type_counts: std::collections::BTreeMap<String, u32>,
}

#[derive(Deserialize)]
pub(super) struct WireOptions {
    #[serde(rename = "filterGroups")]
    pub(super) filter_groups: Vec<WireFilterOptions>,
    #[serde(rename = "sortOptions")]
    pub(super) sort_options: Vec<WireOption>,
}

#[derive(Deserialize)]
pub(super) struct WireFilterOptions {
    pub(super) label: String,
    pub(super) value: String,
    pub(super) options: Vec<WireOption>,
}

#[derive(Deserialize)]
pub(super) struct WireOption {
    pub(super) label: String,
    pub(super) value: String,
}

#[derive(Deserialize)]
pub(super) struct WireMedia {
    pub(super) mediaid: String,
    pub(super) title: String,
    #[serde(rename = "contentType")]
    pub(super) content_type: String,
    pub(super) duration: u32,
    pub(super) release_date: Option<String>,
    pub(super) description_long: Option<String>,
    pub(super) description_medium: Option<String>,
    #[serde(default)]
    pub(super) director: Vec<String>,
    #[serde(default)]
    pub(super) starring: Vec<String>,
    #[serde(default)]
    pub(super) country: Vec<String>,
    #[serde(default)]
    pub(super) language: Vec<String>,
    pub(super) genre: Option<String>,
    pub(super) genre_2: Option<String>,
    pub(super) content_warnings: Option<String>,
    #[serde(default)]
    pub(super) commentary_tracks: Vec<String>,
    #[serde(default)]
    pub(super) playlists: Vec<WirePlaylist>,
    #[serde(default)]
    pub(super) is_first_tab_sortable: bool,
}

#[derive(Deserialize)]
pub(super) struct WirePlaylist {
    pub(super) key: String,
    pub(super) title: String,
    #[serde(rename = "playlistId")]
    pub(super) id: String,
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) playlist: Vec<WireMedia>,
}

impl WireMedia {
    fn validate(&self) -> Result<(), Error> {
        check_text(&self.title, 512, false)?;
        if self.title.trim().is_empty()
            || self.duration > 604_800
            || self
                .release_date
                .as_deref()
                .is_some_and(|date| !date.is_empty() && !valid_date(date))
        {
            return Err(Error::InvalidResponse);
        }
        for names in [
            &self.director,
            &self.starring,
            &self.country,
            &self.language,
            &self.commentary_tracks,
        ] {
            check_names(names)?;
        }
        for description in [&self.description_long, &self.description_medium]
            .into_iter()
            .flatten()
        {
            check_text(description, 65_536, true)?;
        }
        for genre in [&self.genre, &self.genre_2].into_iter().flatten() {
            check_text(genre, 512, false)?;
        }
        if let Some(warnings) = &self.content_warnings {
            check_text(warnings, 8192, true)?;
        }
        if self.playlists.len() > 32
            || self
                .playlists
                .iter()
                .map(|playlist| playlist.playlist.len())
                .sum::<usize>()
                > 1024
        {
            return Err(Error::InvalidResponse);
        }
        for playlist in &self.playlists {
            check_text(&playlist.key, 128, false)?;
            check_text(&playlist.title, 512, false)?;
            if playlist.kind != "GENERIC_PLAYLIST" {
                return Err(Error::InvalidResponse);
            }
            MediaId::new(&playlist.id).map_err(|_| Error::InvalidResponse)?;
            playlist.playlist.iter().try_for_each(WireMedia::validate)?;
        }
        Ok(())
    }

    pub(super) fn into_summary(self) -> Result<MediaSummary, Error> {
        self.validate()?;
        Ok(MediaSummary {
            id: MediaId::new(&self.mediaid).map_err(|_| Error::InvalidResponse)?,
            title: self.title,
            kind: MediaKind::parse(&self.content_type)?,
            duration_seconds: self.duration,
            release_date: self.release_date.filter(|date| !date.is_empty()),
        })
    }

    pub(super) fn into_detail(mut self, requested_id: &MediaId) -> Result<MediaDetail, Error> {
        self.validate()?;
        if self.mediaid != requested_id.as_str() {
            return Err(Error::InvalidResponse);
        }
        let playlists = std::mem::take(&mut self.playlists)
            .into_iter()
            .map(|playlist| {
                if playlist.kind != "GENERIC_PLAYLIST" {
                    return Err(Error::InvalidResponse);
                }
                Ok(Playlist {
                    key: playlist.key,
                    title: playlist.title,
                    id: MediaId::new(&playlist.id).map_err(|_| Error::InvalidResponse)?,
                    items: playlist
                        .playlist
                        .into_iter()
                        .map(WireMedia::into_summary)
                        .collect::<Result<Vec<_>, Error>>()?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(MediaDetail {
            description: self
                .description_long
                .take()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    self.description_medium
                        .take()
                        .filter(|value| !value.trim().is_empty())
                }),
            directors: std::mem::take(&mut self.director),
            starring: std::mem::take(&mut self.starring),
            countries: std::mem::take(&mut self.country),
            languages: std::mem::take(&mut self.language),
            genres: [self.genre.take(), self.genre_2.take()]
                .into_iter()
                .flatten()
                .filter(|value| !value.is_empty())
                .collect(),
            content_warnings: self
                .content_warnings
                .take()
                .filter(|value| !value.is_empty()),
            commentary_tracks: std::mem::take(&mut self.commentary_tracks),
            first_playlist_sortable: self.is_first_tab_sortable,
            playlists,
            media: self.into_summary()?,
        })
    }
}
