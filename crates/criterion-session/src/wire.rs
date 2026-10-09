use crate::{CLIENT_ID, Error, Response, SCOPE, Secret, SecretBody};
use serde::Deserialize;
use std::time::Duration;

pub(crate) const MAX_BODY: usize = 65_536;
pub(crate) const MAX_TOKEN: usize = 16_384;

pub(crate) fn form(fields: &[(&str, &str)]) -> SecretBody {
    let mut encoded = url::form_urlencoded::Serializer::new(String::with_capacity(MAX_BODY));
    for (key, value) in fields {
        encoded.append_pair(key, value);
    }
    SecretBody::new(encoded.finish().into_bytes())
}
pub(crate) fn device_form() -> SecretBody {
    form(&[("client_id", CLIENT_ID), ("scope", SCOPE)])
}
pub(crate) fn poll_form(code: &Secret) -> SecretBody {
    form(&[
        ("client_id", CLIENT_ID),
        ("device_code", code.expose()),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
    ])
}
pub(crate) fn refresh_form(token: &Secret) -> SecretBody {
    form(&[
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", token.expose()),
        ("scope", SCOPE),
    ])
}
pub(crate) fn revoke_form(token: &Secret) -> SecretBody {
    form(&[("client_id", CLIENT_ID), ("token", token.expose())])
}
#[derive(Deserialize)]
pub(crate) struct Device {
    pub device_code: Secret,
    pub user_code: Secret,
    pub verification_uri_complete: Secret,
    pub expires_in: u64,
    pub interval: u64,
}
#[derive(Deserialize)]
pub(crate) struct Tokens {
    pub access_token: Secret,
    pub refresh_token: Secret,
    pub expires_in: u64,
}
#[derive(Deserialize)]
pub(crate) struct Refreshed {
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    pub expires_in: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Pending,
    SlowDown,
    Denied,
    Expired,
    InvalidGrant,
    Other,
}
#[derive(Deserialize)]
struct FailureWire {
    error: Secret,
}
pub(crate) fn failure(response: &Response) -> Result<Failure, Error> {
    if response.status == 429 || response.status >= 500 {
        return Err(Error::Unavailable);
    }
    if !matches!(response.status, 400 | 401 | 403) {
        return Err(Error::HttpStatus(response.status));
    }
    let parsed: FailureWire = parse(response)?;
    Ok(match parsed.error.expose() {
        "authorization_pending" => Failure::Pending,
        "slow_down" => Failure::SlowDown,
        "access_denied" => Failure::Denied,
        "expired_token" => Failure::Expired,
        "invalid_grant" => Failure::InvalidGrant,
        _ => Failure::Other,
    })
}
pub(crate) fn parse<T: for<'de> Deserialize<'de>>(response: &Response) -> Result<T, Error> {
    if response.body.expose().len() > MAX_BODY {
        return Err(Error::ResponseTooLarge);
    }
    serde_json::from_slice(response.body.expose()).map_err(|_| Error::InvalidResponse)
}
pub(crate) fn parse_device(response: &Response) -> Result<Device, Error> {
    if response.status != 200 {
        return Err(Error::HttpStatus(response.status));
    }
    let data: Device = parse(response)?;
    if !data.device_code.valid(4096)
        || !data.user_code.valid(128)
        || data.expires_in == 0
        || data.expires_in > 3600
        || data.interval == 0
        || data.interval > 60
        || !verification_uri(&data.verification_uri_complete)
    {
        return Err(Error::InvalidResponse);
    }
    Ok(data)
}
fn verification_uri(uri: &Secret) -> bool {
    if !uri.valid(2048) {
        return false;
    }
    let Ok(parsed) = url::Url::parse(uri.expose()) else {
        return false;
    };
    parsed.origin().ascii_serialization() == "https://login.criterion.com"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none()
        && parsed.fragment().is_none()
        && parsed.path() == "/activate"
        && parsed.query().is_none_or(|query| query.len() <= 512)
}
pub(crate) fn parse_tokens(response: &Response) -> Result<Tokens, Error> {
    let data: Tokens = parse(response)?;
    if !data.access_token.valid(MAX_TOKEN)
        || !data.refresh_token.valid(MAX_TOKEN)
        || data.expires_in == 0
        || data.expires_in > 86_400
    {
        return Err(Error::InvalidResponse);
    }
    Ok(data)
}
pub(crate) fn parse_refresh(response: &Response) -> Result<Refreshed, Error> {
    let data: Refreshed = parse(response)?;
    if !data.access_token.valid(MAX_TOKEN)
        || data
            .refresh_token
            .as_ref()
            .is_some_and(|token| !token.valid(MAX_TOKEN))
        || data.expires_in == 0
        || data.expires_in > 86_400
    {
        return Err(Error::InvalidResponse);
    }
    Ok(data)
}
pub(crate) fn after(now: Duration, seconds: u64) -> Result<Duration, Error> {
    now.checked_add(Duration::from_secs(seconds))
        .ok_or(Error::InvalidResponse)
}
