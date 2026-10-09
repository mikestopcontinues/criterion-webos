use super::*;
use crate::ImageRole;
use criterion_provider::{ImageLabel, MediaId};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};

fn media() -> ArtworkSource {
    ArtworkSource::Media {
        id: MediaId::new("qvwT6mJ4").unwrap(),
        label: ImageLabel::Landscape,
        role: ImageRole::Card,
    }
}

struct Server {
    origin: url::Url,
    roots: rustls::RootCertStore,
    thread: Option<thread::JoinHandle<()>>,
    request: Arc<Mutex<Vec<u8>>>,
}

impl Server {
    fn start(name: &str, response: Vec<u8>) -> Self {
        Self::delayed(name, response, 0, Duration::ZERO)
    }
    fn delayed(name: &str, response: Vec<u8>, split: usize, delay: Duration) -> Self {
        let signing = rcgen::generate_simple_self_signed(vec![name.to_owned()]).unwrap();
        let certificate = signing.cert.der().clone();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate.clone()).unwrap();
        let private = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(signing.signing_key.serialize_der()),
        );
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], private)
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let request = Arc::new(Mutex::new(Vec::new()));
        let captured_request = request.clone();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut stream = rustls::StreamOwned::new(
                rustls::ServerConnection::new(Arc::new(config)).unwrap(),
                stream,
            );
            let mut request = [0_u8; 4096];
            if let Ok(count) = stream.read(&mut request) {
                captured_request
                    .lock()
                    .unwrap()
                    .extend_from_slice(&request[..count]);
                if split > 0 {
                    let _ = stream.write_all(&response[..split]);
                    let _ = stream.flush();
                }
                thread::sleep(delay);
                let _ = stream.write_all(&response[split..]);
                let _ = stream.flush();
            }
        });
        Self {
            origin: url::Url::parse(&format!("https://127.0.0.1:{port}")).unwrap(),
            roots,
            thread: Some(handle),
            request,
        }
    }
    fn finish(mut self) {
        self.thread.take().unwrap().join().unwrap();
    }
}

fn response(status: &str, mime: &str, body: &[u8], headers: &str) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

#[tokio::test]
async fn valid_public_image_crosses_verified_local_tls_and_reaches_bounded_rgba_output() {
    let server = Server::start(
        "127.0.0.1",
        response(
            "200 OK",
            "image/png",
            include_bytes!("../../tests/fixtures/two-pixels.png"),
            "",
        ),
    );
    let loader = ArtworkLoader::for_test(
        server.origin.clone(),
        server.roots.clone(),
        Duration::from_secs(1),
    );
    let result = loader.load(&media()).await.unwrap();
    assert_eq!(result.dimensions(), [2, 1]);
    assert_eq!(result.rgba(), &[255, 0, 0, 255, 0, 255, 0, 128]);
    server.finish();
}

#[tokio::test]
async fn response_codings_other_than_identity_are_not_decoded_or_admitted() {
    let server = Server::start(
        "127.0.0.1",
        response(
            "200 OK",
            "image/png",
            include_bytes!("../../tests/fixtures/two-pixels.png"),
            "Content-Encoding: gzip\r\n",
        ),
    );
    let loader = ArtworkLoader::for_test(
        server.origin.clone(),
        server.roots.clone(),
        Duration::from_secs(1),
    );
    assert_eq!(
        loader.load(&media()).await.unwrap_err(),
        ArtworkError::UnsupportedFormat
    );
    server.finish();
}

#[tokio::test]
async fn tls_requires_both_a_trusted_certificate_and_its_matching_hostname() {
    for (name, trust) in [("127.0.0.1", false), ("wrong.example", true)] {
        let server = Server::start(
            name,
            response(
                "200 OK",
                "image/png",
                include_bytes!("../../tests/fixtures/two-pixels.png"),
                "",
            ),
        );
        let roots = if trust {
            server.roots.clone()
        } else {
            rustls::RootCertStore::empty()
        };
        let loader = ArtworkLoader::for_test(server.origin.clone(), roots, Duration::from_secs(1));
        assert_eq!(
            loader.load(&media()).await.unwrap_err(),
            ArtworkError::Unavailable
        );
        server.finish();
    }
}

#[tokio::test]
async fn redirect_status_is_rejected_without_contacting_its_destination() {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let location = format!(
        "Location: https://127.0.0.1:{}/signed?token=private\r\n",
        destination.local_addr().unwrap().port()
    );
    let server = Server::start(
        "127.0.0.1",
        response("302 Found", "image/png", b"", &location),
    );
    let loader = ArtworkLoader::for_test(
        server.origin.clone(),
        server.roots.clone(),
        Duration::from_secs(1),
    );
    let error = loader.load(&media()).await.unwrap_err();
    assert_eq!(error, ArtworkError::HttpStatus(302));
    assert!(!format!("{error:?} {error}").contains("private"));
    assert!(destination.accept().is_err());
    server.finish();
}

#[tokio::test]
async fn encoded_body_cap_applies_to_declared_lengths_and_chunked_streams() {
    let declared = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4194305\r\nConnection: close\r\n\r\n".to_vec();
    let mut chunked = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n400001\r\n".to_vec();
    chunked.resize(chunked.len() + 4 * 1024 * 1024 + 1, 0);
    chunked.extend_from_slice(b"\r\n0\r\n\r\n");
    for bytes in [declared, chunked] {
        let server = Server::start("127.0.0.1", bytes);
        let loader = ArtworkLoader::for_test(
            server.origin.clone(),
            server.roots.clone(),
            Duration::from_secs(1),
        );
        assert_eq!(
            loader.load(&media()).await.unwrap_err(),
            ArtworkError::EncodedTooLarge
        );
        server.finish();
    }
}

#[tokio::test]
async fn the_total_deadline_includes_a_stalled_body_after_successful_headers() {
    let bytes = response(
        "200 OK",
        "image/png",
        include_bytes!("../../tests/fixtures/two-pixels.png"),
        "",
    );
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 5;
    let server = Server::delayed("127.0.0.1", bytes, split, Duration::from_millis(150));
    let loader = ArtworkLoader::for_test(
        server.origin.clone(),
        server.roots.clone(),
        Duration::from_millis(50),
    );
    assert_eq!(
        loader.load(&media()).await.unwrap_err(),
        ArtworkError::Deadline
    );
    server.finish();
}

#[tokio::test]
async fn excess_concurrent_loads_fail_immediately_and_cancelled_network_calls_release_admission() {
    let bytes = response(
        "200 OK",
        "image/png",
        include_bytes!("../../tests/fixtures/two-pixels.png"),
        "",
    );
    let server = Server::delayed("127.0.0.1", bytes, 0, Duration::from_millis(150));
    let loader = ArtworkLoader::for_test(
        server.origin.clone(),
        server.roots.clone(),
        Duration::from_millis(70),
    );
    let source = media();
    {
        let first = loader.load(&source);
        let second = loader.load(&source);
        tokio::pin!(first, second);
        tokio::select! {
            biased;
            _ = &mut first => panic!("pending first request completed before concurrency probe"),
            _ = &mut second => panic!("pending second request completed before concurrency probe"),
            _ = async {
                tokio::task::yield_now().await;
                assert_eq!(loader.load(&source).await.unwrap_err(), ArtworkError::Busy);
            } => {}
        }
    }
    // The issued TLS connections may settle after cancellation. A fresh call
    // must be admitted and reach its network deadline, rather than stay Busy.
    assert_ne!(loader.load(&source).await.unwrap_err(), ArtworkError::Busy);
    server.finish();
}

#[tokio::test]
async fn backdrop_requests_full_canvas_public_width_and_preserves_decoded_detail() {
    use image::ImageEncoder;
    let pixels = vec![67u8; 1920 * 1080 * 4];
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&pixels, 1920, 1080, image::ExtendedColorType::Rgba8)
        .unwrap();
    let server = Server::start("127.0.0.1", response("200 OK", "image/png", &encoded, ""));
    let loader = ArtworkLoader::for_test(
        server.origin.clone(),
        server.roots.clone(),
        Duration::from_secs(2),
    );
    let source = ArtworkSource::Media {
        id: MediaId::new("qvwT6mJ4").unwrap(),
        label: ImageLabel::Landscape,
        role: ImageRole::Backdrop,
    };
    let result = loader.load(&source).await.unwrap();
    assert!(
        server.request.lock().unwrap().starts_with(
            b"GET /v1/media/qvwT6mJ4/images/default_16x9.webp?width=1920 HTTP/1.1\r\n"
        )
    );
    assert_eq!(result.dimensions(), [1920, 1080]);
    assert_eq!(result.rgba().len(), 1920 * 1080 * 4);
    server.finish();
}
