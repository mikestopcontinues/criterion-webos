//! Bounded projection of the observed anonymous 24/7 schedule; no playback fields.
use crate::{Error, LiveProgram, MediaId, MediaKind, UtcTimestamp, wire::check_text};
use serde::Deserialize;

// Current public response supplies 111 entries. This is an application ceiling,
// not a guarantee about how much future schedule the provider publishes.
const MAX_PROGRAMS: usize = 256;

#[derive(Deserialize)]
pub(super) struct WireProgram {
    mediaid: String,
    title: String,
    #[serde(rename = "contentType")]
    content_type: String,
    duration: u32,
    start_time: String,
    #[serde(rename = "startTime")]
    start_time_camel: String,
    end_time: String,
    #[serde(rename = "endTime")]
    end_time_camel: String,
}

pub(super) fn project(
    kind: MediaKind,
    schedule: Option<Vec<WireProgram>>,
) -> Result<Vec<LiveProgram>, Error> {
    let Some(schedule) = schedule else {
        return if kind == MediaKind::Live {
            Err(Error::InvalidResponse)
        } else {
            Ok(Vec::new())
        };
    };
    if kind != MediaKind::Live || schedule.len() > MAX_PROGRAMS {
        return Err(Error::InvalidResponse);
    }
    let mut programs: Vec<LiveProgram> = Vec::with_capacity(schedule.len());
    for program in schedule {
        check_text(&program.title, 512, false)?;
        if program.title.trim().is_empty()
            || program.content_type != "film"
            || program.duration > 604_800
            || program.start_time != program.start_time_camel
            || program.end_time != program.end_time_camel
        {
            return Err(Error::InvalidResponse);
        }
        let starts_at =
            UtcTimestamp::new(&program.start_time).map_err(|_| Error::InvalidResponse)?;
        let ends_at = UtcTimestamp::new(&program.end_time).map_err(|_| Error::InvalidResponse)?;
        if starts_at >= ends_at
            || programs
                .last()
                .is_some_and(|previous| previous.ends_at > starts_at)
        {
            return Err(Error::InvalidResponse);
        }
        let media_id = if program.mediaid.is_empty() {
            None
        } else {
            Some(MediaId::new(&program.mediaid).map_err(|_| Error::InvalidResponse)?)
        };
        programs.push(LiveProgram {
            media_id,
            title: program.title,
            kind: MediaKind::Film,
            duration_seconds: program.duration,
            starts_at,
            ends_at,
        });
    }
    Ok(programs)
}
