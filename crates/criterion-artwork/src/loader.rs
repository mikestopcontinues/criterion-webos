use crate::{
    ArtworkError, ArtworkSource, DecodedArtwork,
    decode::{MAX_ENCODED_BYTES, decode_artwork},
};
use std::sync::Arc;
use std::time::Duration;

/// Public-image loader with finite admission and an owned blocking decode.
/// Use one instance per runtime. Excess work returns Busy rather than queueing.
pub struct ArtworkLoader {
    client: reqwest::Client,
    network: tokio::sync::Semaphore,
    decode: Arc<tokio::sync::Semaphore>,
    #[cfg(test)]
    origin: Option<url::Url>,
}

impl ArtworkLoader {
    pub fn new() -> Result<Self, ArtworkError> {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        Self::build(roots, Duration::from_secs(10))
    }

    /// Poll on a Tokio runtime. Dropping this future cancels network publication;
    /// an issued decode settles while retaining its permit. The application must
    /// reject obsolete generations before admitting the returned pixels to egui.
    pub async fn load(&self, source: &ArtworkSource) -> Result<DecodedArtwork, ArtworkError> {
        let target = source.url()?;
        let role = source.role();
        let _network = self.network.try_acquire().map_err(|_| ArtworkError::Busy)?;
        #[cfg(test)]
        let target = if let Some(origin) = &self.origin {
            let mut mapped = origin.clone();
            mapped.set_path(target.path());
            mapped.set_query(target.query());
            mapped
        } else {
            target
        };
        let mut response = self
            .client
            .get(target)
            .header(reqwest::header::ACCEPT, "image/webp,image/jpeg,image/png")
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(ArtworkError::HttpStatus(status));
        }
        if response
            .headers()
            .get_all(reqwest::header::CONTENT_ENCODING)
            .iter()
            .any(|value| {
                !value
                    .to_str()
                    .is_ok_and(|value| value.trim().eq_ignore_ascii_case("identity"))
            })
        {
            return Err(ArtworkError::UnsupportedFormat);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ENCODED_BYTES as u64)
        {
            return Err(ArtworkError::EncodedTooLarge);
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 128)
            .ok_or(ArtworkError::UnsupportedFormat)?
            .to_owned();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > MAX_ENCODED_BYTES - body.len() {
                return Err(ArtworkError::EncodedTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let permit = self
            .decode
            .clone()
            .try_acquire_owned()
            .map_err(|_| ArtworkError::Busy)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            decode_artwork(role, &content_type, &body)
        })
        .await
        .map_err(|_| ArtworkError::Unavailable)?
    }

    fn build(roots: rustls::RootCertStore, deadline: Duration) -> Result<Self, ArtworkError> {
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| ArtworkError::Unavailable)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
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
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .user_agent(concat!("Criterion-Unofficial/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| ArtworkError::Unavailable)?;
        Ok(Self {
            client,
            network: tokio::sync::Semaphore::new(2),
            decode: Arc::new(tokio::sync::Semaphore::new(1)),
            #[cfg(test)]
            origin: None,
        })
    }

    #[cfg(test)]
    fn for_test(origin: url::Url, roots: rustls::RootCertStore, deadline: Duration) -> Self {
        assert_eq!(origin.scheme(), "https");
        assert_eq!(origin.host_str(), Some("127.0.0.1"));
        let mut loader = Self::build(roots, deadline).unwrap();
        loader.origin = Some(origin);
        loader
    }
}

fn request_error(error: reqwest::Error) -> ArtworkError {
    if error.is_timeout() {
        ArtworkError::Deadline
    } else {
        ArtworkError::Unavailable
    }
}

#[cfg(test)]
mod tests;
