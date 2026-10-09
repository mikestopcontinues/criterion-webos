use crate::{
    Credentials, Error, Region, Request, Response, SecretBody, Target, Transport,
    WatchListContentType,
};
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

const MAX_REQUEST_BYTES: usize = 256;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);

/// Fixed middleware HTTPS transport with verified static roots, no proxy,
/// redirects, retries or decoding. One active future per instance; excess calls
/// return Busy. The total ten-second deadline includes connection and body reads.
pub struct HttpTransport {
    client: reqwest::Client,
    permit: tokio::sync::Semaphore,
    #[cfg(test)]
    test_origin: Option<url::Url>,
}

impl HttpTransport {
    pub fn new() -> Result<Self, Error> {
        let client = client_builder(REQUEST_DEADLINE)?
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self::with_client(client))
    }

    fn with_client(client: reqwest::Client) -> Self {
        Self {
            client,
            permit: tokio::sync::Semaphore::new(1),
            #[cfg(test)]
            test_origin: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(origin: url::Url, deadline: Duration) -> Result<Self, Error> {
        check_test_origin(&origin)?;
        let client = client_builder(deadline)?
            .build()
            .map_err(|_| Error::Unavailable)?;
        let mut transport = Self::with_client(client);
        transport.test_origin = Some(origin);
        Ok(transport)
    }

    #[cfg(test)]
    pub(crate) fn for_test_roots(
        origin: url::Url,
        roots: rustls::RootCertStore,
        deadline: Duration,
    ) -> Result<Self, Error> {
        check_test_origin(&origin)?;
        let client = client_builder(deadline)?
            .tls_backend_preconfigured(tls_config(roots)?)
            .build()
            .map_err(|_| Error::Unavailable)?;
        let mut transport = Self::with_client(client);
        transport.test_origin = Some(origin);
        Ok(transport)
    }

    fn target(&self, target: &Target) -> Result<url::Url, Error> {
        let target = match target {
            Target::AddWatchList { region, .. } => account_target(*region, "/content/watch-list")?,
            Target::RemoveWatchList { region, media_id } => {
                let mut target = account_target(*region, "/content/watch-list")?;
                target
                    .path_segments_mut()
                    .map_err(|_| Error::InvalidRequest)?
                    .push(media_id.as_str());
                target
            }
            Target::Bootstrap => {
                url::Url::parse(crate::BOOTSTRAP_URL).map_err(|_| Error::InvalidRequest)?
            }
            Target::WatchList(region) => account_target(*region, "/content/watch-list")?,
            Target::MyListIds(region) => account_target(*region, "/content/my-stuff-ids")?,
            Target::ContinueWatching(region) => {
                account_target(*region, "/content/continue-watching")?
            }
        };
        #[cfg(test)]
        if let Some(origin) = &self.test_origin {
            let mut test_target = origin.clone();
            test_target.set_path(target.path());
            test_target.set_query(target.query());
            return Ok(test_target);
        }
        Ok(target)
    }
}

fn account_target(region: Region, path: &str) -> Result<url::Url, Error> {
    let base = match region {
        Region::Us => crate::US_BASE,
        Region::Ca => crate::CA_BASE,
    };
    // The suffixes and regional bases are fixed source-owned values, never
    // payload-selected paths, provider URLs or caller-supplied route strings.
    url::Url::parse(&format!("{base}{path}")).map_err(|_| Error::InvalidRequest)
}

#[derive(serde::Serialize)]
struct AddBody<'a> {
    media_id: &'a str,
    content_type: &'a str,
}

fn add_body(
    media_id: &criterion_provider::MediaId,
    content_type: WatchListContentType,
) -> Result<SecretBody, Error> {
    // Fixed stack storage makes encoder overflow a pre-contact error without
    // reallocating or abandoning any partially encoded private body.
    let mut storage = Zeroizing::new([0_u8; MAX_REQUEST_BYTES]);
    let length = {
        let mut writer = std::io::Cursor::new(&mut storage[..]);
        serde_json::to_writer(
            &mut writer,
            &AddBody {
                media_id: media_id.as_str(),
                content_type: content_type.as_str(),
            },
        )
        .map_err(|_| Error::InvalidRequest)?;
        usize::try_from(writer.position()).map_err(|_| Error::InvalidRequest)?
    };
    if length > MAX_REQUEST_BYTES {
        return Err(Error::InvalidRequest);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_REQUEST_BYTES));
    bytes.extend_from_slice(&storage[..length]);
    Ok(SecretBody::new(std::mem::take(&mut *bytes)))
}

fn valid_credentials(credentials: &Credentials) -> Result<(), Error> {
    if !credentials.bootstrap.is_sensitive() || !credentials.subscriber.is_sensitive() {
        return Err(Error::InvalidRequest);
    }
    let bootstrap = credentials
        .bootstrap
        .to_str()
        .map_err(|_| Error::InvalidRequest)?
        .strip_prefix("Bearer ")
        .ok_or(Error::InvalidRequest)?;
    let subscriber = credentials
        .subscriber
        .to_str()
        .map_err(|_| Error::InvalidRequest)?;
    crate::wire::valid_token(bootstrap).map_err(|_| Error::InvalidRequest)?;
    crate::wire::valid_token(subscriber).map_err(|_| Error::InvalidRequest)
}

fn tls_config(roots: rustls::RootCertStore) -> Result<rustls::ClientConfig, Error> {
    Ok(rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| Error::Unavailable)?
    .with_root_certificates(roots)
    .with_no_client_auth())
}

fn client_builder(deadline: Duration) -> Result<reqwest::ClientBuilder, Error> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    Ok(reqwest::Client::builder()
        .tls_backend_preconfigured(tls_config(roots)?)
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .timeout(deadline)
        .connect_timeout(Duration::from_secs(3).min(deadline))
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(Duration::from_secs(30))
        .http1_max_headers(32)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .user_agent(concat!("Criterion-Unofficial/", env!("CARGO_PKG_VERSION"))))
}

#[cfg(test)]
fn check_test_origin(origin: &url::Url) -> Result<(), Error> {
    if origin.scheme() != "https"
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
        || !matches!(origin.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback())
            && !matches!(origin.host(), Some(url::Host::Ipv6(ip)) if ip.is_loopback())
    {
        return Err(Error::InvalidRequest);
    }
    Ok(())
}

fn json_media_type(headers: &reqwest::header::HeaderMap) -> bool {
    let mut types = headers.get_all(reqwest::header::CONTENT_TYPE).iter();
    let Some(value) = types.next() else {
        return false;
    };
    if types.next().is_some() {
        return false;
    }
    value.to_str().ok().is_some_and(|value| {
        value.len() <= 1024
            && value
                .split(';')
                .next()
                .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
    })
}

fn request_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Deadline
    } else {
        Error::Unavailable
    }
}

impl Transport for HttpTransport {
    async fn send(&self, request: Request) -> Result<Response, Error> {
        // Refuse confused credential roles and malformed private capabilities
        // before taking capacity or constructing any outbound HTTP request.
        let Request {
            target,
            credentials,
        } = request;
        let credentials = match &target {
            Target::Bootstrap if credentials.is_some() => return Err(Error::InvalidRequest),
            Target::Bootstrap => None,
            Target::MyListIds(_)
            | Target::ContinueWatching(_)
            | Target::WatchList(_)
            | Target::AddWatchList { .. }
            | Target::RemoveWatchList { .. } => {
                let credentials = credentials.ok_or(Error::InvalidRequest)?;
                valid_credentials(&credentials)?;
                Some(credentials)
            }
        };
        let (method, body) = match &target {
            Target::AddWatchList {
                media_id,
                content_type,
                ..
            } => (
                reqwest::Method::POST,
                Some(add_body(media_id, *content_type)?),
            ),
            Target::RemoveWatchList { .. } => (reqwest::Method::DELETE, None),
            _ => (reqwest::Method::GET, None),
        };
        let _permit = self.permit.try_acquire().map_err(|_| Error::Busy)?;
        let target = self.target(&target)?;
        let mut outbound = self
            .client
            .request(method, target)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
        if let Some(body) = body {
            // Bytes retains the private zeroizing body owner throughout the
            // request; HTTP/TLS internal copies remain outside this guarantee.
            outbound = outbound
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(bytes::Bytes::from_owner(body));
        }
        if let Some(credentials) = credentials {
            // Move the existing sensitive HeaderValue owners into this request;
            // never store either private value in client-wide default headers.
            outbound = outbound
                .header(reqwest::header::AUTHORIZATION, credentials.bootstrap)
                .header("x-auth-token", credentials.subscriber);
        }
        let mut response = outbound.send().await.map_err(request_error)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(Error::HttpStatus(status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(Error::ResponseTooLarge);
        }
        if !response
            .headers()
            .get_all(reqwest::header::CONTENT_ENCODING)
            .iter()
            .all(|value| {
                value
                    .to_str()
                    .ok()
                    .is_some_and(|encoding| encoding.trim().eq_ignore_ascii_case("identity"))
            })
            || !json_media_type(response.headers())
        {
            return Err(Error::InvalidResponse);
        }
        // Reserve the whole bound so reallocations cannot abandon a partially
        // private body. HTTP/TLS internal scratch remains outside this guarantee.
        let mut body = Zeroizing::new(Vec::with_capacity(MAX_RESPONSE_BYTES));
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
                return Err(Error::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        if body.is_empty() {
            return Err(Error::InvalidResponse);
        }
        Ok(Response {
            status,
            body: SecretBody::new(std::mem::take(&mut *body)),
        })
    }
}
