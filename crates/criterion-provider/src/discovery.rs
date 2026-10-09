//! HTML response admission and ordered page-level resource bounds.
use crate::{
    Error, MAX_RESPONSE_BYTES, Response, discovery_model::DiscoveryPage, discovery_wire, flight,
};

pub(super) fn parse(response: Response) -> Result<DiscoveryPage, Error> {
    if !(200..300).contains(&response.status) {
        return Err(Error::HttpStatus(response.status));
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(Error::ResponseTooLarge);
    }
    if response.content_type.len() > 1024
        || !response
            .content_type
            .split(';')
            .next()
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/html"))
    {
        return Err(Error::InvalidResponse);
    }
    let serde_json::Value::Array(values) = flight::blocks(&response.body)? else {
        return Err(Error::InvalidResponse);
    };
    if values.is_empty() || values.len() > 128 {
        return Err(Error::InvalidResponse);
    }
    let mut blocks = Vec::new();
    let mut total_cards = 0;
    for value in values {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_u64)
            .ok_or(Error::InvalidResponse)?;
        // The inspected web renderer skips unknown types. Every known block
        // is projected or fails; missing known metadata never invents content.
        if !matches!(kind, 20 | 21 | 22 | 28) {
            continue;
        }
        let id = value
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .and_then(|id| u32::try_from(id).ok())
            .ok_or(Error::InvalidResponse)?;
        if kind == 20 {
            total_cards += value
                .get("playlist")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            if total_cards > 4096 {
                return Err(Error::InvalidResponse);
            }
        }
        blocks.push(discovery_wire::block(id, kind, value)?);
    }
    if blocks.is_empty() {
        return Err(Error::InvalidResponse);
    }
    Ok(DiscoveryPage { blocks })
}
