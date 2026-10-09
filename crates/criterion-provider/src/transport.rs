use crate::{Error, MAX_RESPONSE_BYTES, MediaId, Request, RequestTransport, Response};
use std::sync::Arc;
use std::time::Duration;

/// Fixed HTTPS catalog origin, verified static roots, no redirects or automatic
/// retries. At most two active requests per instance; excess calls return Busy.
/// Each live request has a ten-second deadline covering connection and body reads.
pub struct HttpTransport {
    client: reqwest::Client,
    permits: tokio::sync::Semaphore,
    #[cfg(test)]
    test_origin: Option<url::Url>,
}

impl HttpTransport {
    pub fn new() -> Result<Self, Error> {
        let client = client_builder(Duration::from_secs(10))?
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            client,
            permits: tokio::sync::Semaphore::new(2),
            #[cfg(test)]
            test_origin: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(origin: url::Url, deadline: Duration) -> Self {
        Self {
            client: client_builder(deadline)
                .unwrap()
                .https_only(false)
                .build()
                .unwrap(),
            permits: tokio::sync::Semaphore::new(2),
            test_origin: Some(origin),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_roots(origin: url::Url, roots: rustls::RootCertStore) -> Self {
        Self {
            client: client_builder(Duration::from_secs(1))
                .unwrap()
                .tls_backend_preconfigured(tls_config(roots).unwrap())
                .https_only(false)
                .build()
                .unwrap(),
            permits: tokio::sync::Semaphore::new(2),
            test_origin: Some(origin),
        }
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
        .pool_max_idle_per_host(2)
        .pool_idle_timeout(Duration::from_secs(30))
        .http1_max_headers(32)
        .gzip(true)
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .user_agent(concat!("Criterion-Unofficial/", env!("CARGO_PKG_VERSION"))))
}

fn check_origin(url: &url::Url) -> Result<(), Error> {
    if url.origin().ascii_serialization() != "https://www.criterionchannel.com"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidRequest);
    }
    match url.path() {
        "/api/all-films/results" | "/api/all-films/filters" | "/api/search" => Ok(()),
        path => path
            .strip_prefix("/api/media/")
            .ok_or(Error::InvalidRequest)
            .and_then(|id| MediaId::new(id).map(|_| ())),
    }
}

fn request_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Deadline
    } else {
        Error::Unavailable
    }
}

impl RequestTransport for HttpTransport {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        check_origin(&request.url)?;
        let _permit = self.permits.try_acquire().map_err(|_| Error::Busy)?;
        let target = request.url;
        #[cfg(test)]
        let target = if let Some(origin) = &self.test_origin {
            let path = target.path().to_owned();
            let query = target.query().map(str::to_owned);
            let mut target = origin.clone();
            target.set_path(&path);
            target.set_query(query.as_deref());
            target
        } else {
            target
        };
        let mut response = self
            .client
            .get(target)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(request_error)?;
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
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .ok_or(Error::InvalidResponse)?
            .to_owned();
        if content_type.len() > 1024 {
            return Err(Error::InvalidResponse);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
                return Err(Error::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Response {
            status,
            content_type,
            body,
        })
    }
}
