//! A bounded recognizer for the observed data-only script and named element.
//! This does not execute JavaScript or decode general Flight references/records.
use crate::{Error, wire::check_json_limits};
use serde_json::Value;
use std::collections::HashSet;

const MARKER: &str = "self.__next_f.push(";
// Captured Home: 14 frames, largest decoded frame 165,138 bytes, 193,152
// combined bytes, largest record 165,137 bytes and model depth 8. The
// margins below permit bounded growth; HTML has a separate 2 MiB cap.
const MAX_FRAME_BYTES: usize = 512 * 1024;
const MAX_STREAM_BYTES: usize = 1024 * 1024;
const MAX_RECORD_BYTES: usize = 512 * 1024;

pub(super) fn blocks(html: &[u8]) -> Result<Value, Error> {
    let html = std::str::from_utf8(html).map_err(|_| Error::InvalidResponse)?;
    let mut stream = String::new();
    let mut frames = 0;
    let mut pos = 0;
    let mut template_depth = 0_usize;
    while let Some(offset) = html[pos..].find('<') {
        pos += offset;
        if html[pos..].starts_with("<!--") {
            pos += html[pos..].find("-->").ok_or(Error::InvalidResponse)? + 3;
            continue;
        }
        if html[pos..].starts_with("<![CDATA[") {
            pos += html[pos..].find("]]>").ok_or(Error::InvalidResponse)? + 3;
            continue;
        }
        let end = tag_end(html, pos)?;
        let tag = &html[pos + 1..end];
        let closing = tag.starts_with('/');
        let tag = tag.strip_prefix('/').unwrap_or(tag);
        let name_end = tag
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = &tag[..name_end];
        let is_script = name.eq_ignore_ascii_case("script") && !closing;
        pos = end + 1;
        if name.eq_ignore_ascii_case("template") {
            template_depth = if closing {
                template_depth
                    .checked_sub(1)
                    .ok_or(Error::InvalidResponse)?
            } else {
                template_depth + 1
            };
            if template_depth > 32 {
                return Err(Error::InvalidResponse);
            }
        }
        if !closing && name.eq_ignore_ascii_case("plaintext") {
            break;
        }
        if !closing
            && [
                "textarea", "title", "style", "xmp", "iframe", "noembed", "noframes", "noscript",
            ]
            .iter()
            .any(|tag| name.eq_ignore_ascii_case(tag))
        {
            let closing = format!("</{name}>");
            pos += html.as_bytes()[pos..]
                .windows(closing.len())
                .position(|s| s.eq_ignore_ascii_case(closing.as_bytes()))
                .ok_or(Error::InvalidResponse)?
                + closing.len();
            continue;
        }
        if !is_script {
            continue;
        }
        let close = html.as_bytes()[pos..]
            .windows(9)
            .position(|s| s.eq_ignore_ascii_case(b"</script>"))
            .ok_or(Error::InvalidResponse)?
            + pos;
        let script = html[pos..close].trim();
        pos = close + 9;
        if template_depth != 0 {
            continue;
        }
        if !script.contains(MARKER) {
            continue;
        }
        frames += 1;
        if frames > 128 {
            return Err(Error::InvalidResponse);
        }
        let statement = script.strip_suffix(';').unwrap_or(script).trim_end();
        let encoded = statement
            .strip_prefix(MARKER)
            .ok_or(Error::InvalidResponse)?
            .strip_suffix(')')
            .ok_or(Error::InvalidResponse)?;
        if encoded.len() > MAX_STREAM_BYTES {
            return Err(Error::InvalidResponse);
        }
        check_json_limits(encoded.as_bytes(), 4, 16, MAX_STREAM_BYTES)?;
        let frame: Value = serde_json::from_str(encoded).map_err(|_| Error::InvalidResponse)?;
        let frame = frame.as_array().ok_or(Error::InvalidResponse)?;
        match frame.first().and_then(Value::as_u64) {
            Some(0) if frame.len() == 1 => (),
            Some(2) if frame.len() == 2 && frame[1].is_null() => (),
            Some(1) if frame.len() == 2 => {
                let data = frame[1].as_str().ok_or(Error::InvalidResponse)?;
                if data.len() > MAX_FRAME_BYTES || data.len() > MAX_STREAM_BYTES - stream.len() {
                    return Err(Error::InvalidResponse);
                }
                stream.push_str(data);
            }
            _ => return Err(Error::InvalidResponse),
        }
    }
    if template_depth != 0 || !stream.ends_with('\n') {
        return Err(Error::InvalidResponse);
    }
    let mut imports = Vec::new();
    let mut models = Vec::new();
    let mut ids = HashSet::new();
    for (index, line) in stream.split_terminator('\n').enumerate() {
        if index >= 2048 || line.len() > MAX_RECORD_BYTES {
            return Err(Error::InvalidResponse);
        }
        let (id, payload) = line.split_once(':').ok_or(Error::InvalidResponse)?;
        // Current resource-hint records have no ID. Other non-JSON payloads are
        // outside this recognizer; text/binary length framing is not admitted.
        if id.is_empty() && payload.starts_with("HL[") {
            if !record_json(&payload[2..])?.is_array() {
                return Err(Error::InvalidResponse);
            }
            continue;
        }
        if !canonical_id(id) || !ids.insert(id) {
            return Err(Error::InvalidResponse);
        }
        if let Some(import) = payload.strip_prefix('I') {
            let value = record_json(import)?;
            let array = value.as_array().ok_or(Error::InvalidResponse)?;
            if array.len() != 3
                || array[0].as_u64().is_none()
                || !array[1]
                    .as_array()
                    .is_some_and(|chunks| chunks.len() <= 64 && chunks.iter().all(Value::is_string))
                || !array[2].is_string()
            {
                return Err(Error::InvalidResponse);
            }
            if array[2].as_str() == Some("LanderStoryBlocks") {
                imports.push(format!("$L{id}"));
            }
        } else if payload.starts_with('[') || payload.starts_with('{') {
            models.push(record_json(payload)?);
        } else {
            // Admit only bounded JSON primitives here. Unrecognized Flight
            // tags/length framing change this narrow contract and fail closed.
            record_json(payload)?;
        }
    }
    if imports.len() != 1 {
        return Err(Error::InvalidResponse);
    }
    let mut matches = Vec::new();
    for model in &models {
        find_elements(model, &imports[0], &mut matches)?;
    }
    if matches.len() != 1 {
        return Err(Error::InvalidResponse);
    }
    let element = matches[0].as_array().ok_or(Error::InvalidResponse)?;
    if element.len() != 4 {
        return Err(Error::InvalidResponse);
    }
    let blocks = element[3]
        .as_object()
        .and_then(|props| props.get("blocks"))
        .filter(|blocks| blocks.is_array())
        .ok_or(Error::InvalidResponse)?;
    Ok(blocks.clone())
}

fn tag_end(html: &str, start: usize) -> Result<usize, Error> {
    let mut quote = None;
    for (offset, byte) in html.as_bytes()[start + 1..].iter().copied().enumerate() {
        if offset > 4096 {
            return Err(Error::InvalidResponse);
        }
        match (quote, byte) {
            (Some(q), b) if q == b => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Ok(start + offset + 1),
            _ => (),
        }
    }
    Err(Error::InvalidResponse)
}

fn record_json(record: &str) -> Result<Value, Error> {
    check_json_limits(record.as_bytes(), 32, 65_536, 65_536)?;
    serde_json::from_str::<crate::record_json::StrictValue>(record)
        .map(|value| value.0)
        .map_err(|_| Error::InvalidResponse)
}

// The current client decodes $L references numerically in base 16. Canonical
// lowercase i32 spellings avoid zero/case/overflow chunk aliases without
// interpreting unrelated Flight values or fixing build-local record numbers.
fn canonical_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 8
        && (id.len() == 1 || !id.starts_with('0'))
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && u32::from_str_radix(id, 16).is_ok_and(|id| id <= i32::MAX as u32)
}

fn find_elements<'a>(
    value: &'a Value,
    target: &str,
    matches: &mut Vec<&'a Value>,
) -> Result<(), Error> {
    match value {
        Value::Array(array) => {
            if array.first().and_then(Value::as_str) == Some("$") {
                if array
                    .get(1)
                    .and_then(Value::as_str)
                    .and_then(|s| s.strip_prefix("$L"))
                    .is_some_and(|reference| !canonical_id(reference))
                {
                    return Err(Error::InvalidResponse);
                }
                if array.get(1).and_then(Value::as_str) == Some(target) {
                    matches.push(value);
                    if matches.len() > 1 {
                        return Err(Error::InvalidResponse);
                    }
                }
            }
            for value in array {
                find_elements(value, target, matches)?;
            }
        }
        Value::Object(object) => {
            for value in object.values() {
                find_elements(value, target, matches)?;
            }
        }
        _ => (),
    }
    Ok(())
}
