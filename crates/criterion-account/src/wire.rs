use crate::{Error, Region, Response, SecretBody};
use criterion_session::Secret;
use reqwest::header::HeaderValue;
use serde::Deserialize;

pub(crate) const MAX_BODY: usize = 65_536;
pub(crate) const MAX_TOKEN: usize = 16_384;
pub const BOOTSTRAP_URL: &str = "https://mw.criterion.com/api/init";
pub const US_BASE: &str = "https://mw.criterion.com/api/us";
pub const CA_BASE: &str = "https://mw.criterion.com/api/ca";

#[derive(Deserialize)]
struct Bases {
    us: String,
    ca: String,
}
#[derive(Deserialize)]
struct BootstrapWire {
    country: String,
    token: Secret,
    #[serde(rename = "baseUrl")]
    base_url: Bases,
}
pub(crate) struct Bootstrap {
    pub region: Region,
    pub token: Secret,
}

pub(crate) fn bootstrap(response: &Response) -> Result<Bootstrap, Error> {
    if response.status != 200 {
        return Err(Error::HttpStatus(response.status));
    }
    if response.body.expose().len() > MAX_BODY {
        return Err(Error::ResponseTooLarge);
    }
    let data: BootstrapWire =
        serde_json::from_slice(response.body.expose()).map_err(|_| Error::InvalidResponse)?;
    if data.base_url.us != US_BASE || data.base_url.ca != CA_BASE {
        return Err(Error::InvalidResponse);
    }
    valid_token(data.token.expose())?;
    let region = match data.country.as_str() {
        "US" | "us" => Region::Us,
        "CA" | "ca" => Region::Ca,
        _ => return Err(Error::UnsupportedRegion),
    };
    Ok(Bootstrap {
        region,
        token: data.token,
    })
}

fn valid_token(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.len() > MAX_TOKEN
        || !value.bytes().all(|byte| (33..=126).contains(&byte))
    {
        return Err(Error::InvalidResponse);
    }
    Ok(())
}
pub(crate) fn token_header(prefix: &[u8], token: &str) -> Result<HeaderValue, Error> {
    valid_token(token)?;
    let mut bytes = Vec::with_capacity(prefix.len() + token.len());
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(token.as_bytes());
    let private = bytes::Bytes::from_owner(SecretBody::new(bytes));
    // http::HeaderValue validates and retains the Bytes owner instead of
    // copying this application-owned private buffer into a normal allocation.
    let mut header = HeaderValue::from_maybe_shared(private).map_err(|_| Error::InvalidResponse)?;
    header.set_sensitive(true);
    Ok(header)
}
