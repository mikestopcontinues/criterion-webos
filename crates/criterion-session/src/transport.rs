use crate::{Endpoint, Error, Request, Response, SecretBody, Transport};
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);

/// Fixed-origin HTTPS transport for the native subscriber authentication forms.
/// One active request per instance, verified static TLS roots, no redirects,
/// proxies, retries or automatic content decoding, and a total ten-second limit.
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

    fn target(&self, endpoint: Endpoint) -> Result<url::Url, Error> {
        let path = match endpoint {
            Endpoint::DeviceCode => "/oauth/device/code",
            Endpoint::Token => "/oauth/token",
            Endpoint::Revoke => "/oauth/revoke",
        };
        #[cfg(test)]
        if let Some(origin) = &self.test_origin {
            let mut target = origin.clone();
            target.set_path(path);
            return Ok(target);
        }
        url::Url::parse(&format!("https://login.criterion.com{path}"))
            .map_err(|_| Error::InvalidRequest)
    }
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
    let loopback = match origin.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    if origin.scheme() != "https"
        || !loopback
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(Error::InvalidRequest);
    }
    Ok(())
}

fn json_media_type(headers: &reqwest::header::HeaderMap) -> bool {
    let mut values = headers.get_all(reqwest::header::CONTENT_TYPE).iter();
    let Some(value) = values.next() else {
        return false;
    };
    if values.next().is_some() {
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
    async fn post(&self, request: Request) -> Result<Response, Error> {
        if request.body.expose().len() > MAX_REQUEST_BYTES {
            return Err(Error::InvalidRequest);
        }
        let _permit = self.permit.try_acquire().map_err(|_| Error::Busy)?;
        let target = self.target(request.endpoint)?;
        let empty_revoke = matches!(request.endpoint, Endpoint::Revoke);
        // Bytes retains this zeroizing owner through reqwest instead of copying
        // the private form into an ordinary Vec-owned request body.
        let body = bytes::Bytes::from_owner(request.body);
        let mut response = self
            .client
            .post(target)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) || status < 200 {
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
        {
            return Err(Error::InvalidResponse);
        }
        let is_json = json_media_type(response.headers());
        let allow_empty = empty_revoke && status == 200;
        if !is_json && !allow_empty {
            return Err(Error::InvalidResponse);
        }
        // Reserve the full bounded buffer so growth never reallocates a partial
        // secret response into abandoned, nonzeroized allocator storage.
        let mut body = Zeroizing::new(Vec::with_capacity(MAX_RESPONSE_BYTES));
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
                return Err(Error::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        if (body.is_empty() && !allow_empty) || (!body.is_empty() && !is_json) {
            return Err(Error::InvalidResponse);
        }
        Ok(Response {
            status,
            body: SecretBody::new(std::mem::take(&mut *body)),
        })
    }
}
