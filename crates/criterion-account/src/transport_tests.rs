use crate::{
    Credentials, Error, HttpTransport, Region, Request, Target, Transport, WatchListContentType,
};
use reqwest::header::HeaderValue;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

type TlsStream = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

#[derive(Debug)]
struct Received {
    head: String,
    body: Vec<u8>,
}

struct Server {
    origin: url::Url,
    roots: rustls::RootCertStore,
    connections: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<Received>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn new(
        name: &str,
        handler: impl Fn(&mut TlsStream, &AtomicBool) + Send + Sync + 'static,
    ) -> Self {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec![name.to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate.clone()).unwrap();
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()),
        );
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::aws_lc_rs::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![certificate], key)
            .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin =
            url::Url::parse(&format!("https://{}", listener.local_addr().unwrap())).unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_connections = connections.clone();
        let thread_requests = requests.clone();
        let handler = Arc::new(handler);
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(4);
            while !thread_stop.load(Ordering::Acquire) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((stream, _)) => {
                        thread_connections.fetch_add(1, Ordering::Release);
                        let config = config.clone();
                        let requests = thread_requests.clone();
                        let stop = thread_stop.clone();
                        let handler = handler.clone();
                        workers.push(std::thread::spawn(move || {
                            stream
                                .set_read_timeout(Some(Duration::from_millis(500)))
                                .unwrap();
                            stream
                                .set_write_timeout(Some(Duration::from_millis(500)))
                                .unwrap();
                            let mut tls = rustls::StreamOwned::new(
                                rustls::ServerConnection::new(config).unwrap(),
                                stream,
                            );
                            if let Some(request) = read_request(&mut tls) {
                                requests.lock().unwrap().push(request);
                                handler(&mut tls, &stop);
                                tls.conn.send_close_notify();
                                let _ = tls.flush();
                            }
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => panic!("local TLS fixture accept failed"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            origin,
            roots,
            connections,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn json(status: u16, body: &'static [u8]) -> Self {
        Self::new("127.0.0.1", move |tls, _| {
            let _ = write!(
                tls,
                "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = tls.write_all(body);
        })
    }

    fn transport(&self, deadline: Duration) -> HttpTransport {
        HttpTransport::for_test_roots(self.origin.clone(), self.roots.clone(), deadline).unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn read_request(stream: &mut TlsStream) -> Option<Received> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let end = loop {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 || bytes.len() + count > 72 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() > 40 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8(bytes[..end].to_vec()).ok()?;
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if length > 64 * 1024 {
        return None;
    }
    while bytes.len() - end < length {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Some(Received {
        head,
        body: bytes[end..end + length].to_vec(),
    })
}

fn request() -> Request {
    Request {
        target: Target::Bootstrap,
        credentials: None,
    }
}

#[tokio::test]
async fn verified_tls_gets_fixed_bootstrap_without_credentials_and_returns_private_json() {
    let server = Server::json(200, br#"{"token":"synthetic-bootstrap-result"}"#);
    let response = server
        .transport(Duration::from_secs(1))
        .send(request())
        .await
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(
        response.body.expose(),
        br#"{"token":"synthetic-bootstrap-result"}"#
    );
    assert!(!format!("{response:?}").contains("synthetic-bootstrap-result"));
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].head.starts_with("GET /api/init HTTP/1.1\r\n"));
    let headers = requests[0].head.to_ascii_lowercase();
    assert!(headers.contains("accept: application/json\r\n"));
    assert!(headers.contains("accept-encoding: identity\r\n"));
    assert!(!headers.contains("authorization:"));
    assert!(!headers.contains("x-auth-token:"));
    assert!(!headers.contains("cookie:"));
    assert!(requests[0].body.is_empty());
}

#[tokio::test]
async fn bootstrap_credentials_are_rejected_before_any_network_contact() {
    let server = Server::json(200, b"{}");
    let private_request = Request {
        target: Target::Bootstrap,
        credentials: Some(Credentials {
            bootstrap: HeaderValue::from_static("Bearer fixture-bootstrap-secret"),
            subscriber: HeaderValue::from_static("fixture-subscriber-secret"),
        }),
    };
    assert!(!format!("{private_request:?}").contains("fixture-bootstrap-secret"));
    assert!(!format!("{private_request:?}").contains("fixture-subscriber-secret"));
    assert_eq!(
        server
            .transport(Duration::from_secs(1))
            .send(private_request)
            .await
            .err(),
        Some(Error::InvalidRequest)
    );
    assert_eq!(server.connections.load(Ordering::Acquire), 0);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn untrusted_and_wrong_hostname_certificates_fail_before_http_delivery() {
    let untrusted = Server::json(200, b"{}");
    let transport =
        HttpTransport::for_test(untrusted.origin.clone(), Duration::from_secs(1)).unwrap();
    assert_eq!(
        transport.send(request()).await.err(),
        Some(Error::Unavailable)
    );
    assert!(untrusted.requests.lock().unwrap().is_empty());
    let wrong_host = Server::new("wrong.example", |tls, _| {
        let _ = tls.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
        );
    });
    assert_eq!(
        wrong_host
            .transport(Duration::from_secs(1))
            .send(request())
            .await
            .err(),
        Some(Error::Unavailable)
    );
    assert!(wrong_host.requests.lock().unwrap().is_empty());
}

#[test]
fn injected_test_origins_require_https_loopback_without_credentials_path_query_or_fragment() {
    for origin in [
        "http://127.0.0.1:443/",
        "https://name@127.0.0.1:443/",
        "https://name:secret@127.0.0.1:443/",
        "https://127.0.0.1:443/private",
        "https://127.0.0.1:443/?q=value",
        "https://127.0.0.1:443/#fragment",
        "https://remote.example/",
        "https://192.0.2.1:443/",
    ] {
        assert!(matches!(
            HttpTransport::for_test(url::Url::parse(origin).unwrap(), Duration::from_secs(1)),
            Err(Error::InvalidRequest)
        ));
    }
}

#[tokio::test]
async fn redirects_return_status_and_never_contact_the_destination() {
    let target = Server::json(200, b"{}");
    let location = target.origin.to_string();
    let server = Server::new("127.0.0.1", move |tls, _| {
        let _ = write!(
            tls,
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}private?synthetic-secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    });
    let result = server
        .transport(Duration::from_secs(1))
        .send(request())
        .await;
    assert_eq!(result.err(), Some(Error::HttpStatus(307)));
    assert_eq!(target.connections.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn non_success_status_does_not_publish_the_private_error_body_or_retry() {
    for status in [400, 401, 403, 429, 500, 503] {
        let server = Server::json(status, br#"{"private":"synthetic-private-error"}"#);
        let error = server
            .transport(Duration::from_secs(1))
            .send(request())
            .await
            .err()
            .unwrap();
        assert_eq!(error, Error::HttpStatus(status));
        assert!(!format!("{error:?} {error}").contains("synthetic-private-error"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn json_content_type_allows_case_and_parameters_but_rejects_missing_ambiguous_or_other_media()
{
    let good = Server::new("127.0.0.1", |tls, _| {
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: Application/JSON; charset=utf-8\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    });
    assert_eq!(
        good.transport(Duration::from_secs(1))
            .send(request())
            .await
            .unwrap()
            .body
            .expose(),
        b"{}"
    );
    for header in [
        "",
        "Content-Type: text/html\r\n",
        "Content-Type: application/jsonp\r\n",
        "Content-Type: application/json\r\nContent-Type: application/json\r\n",
        "Content-Type: application/json\r\nContent-Type: text/html\r\n",
    ] {
        let server = Server::new("127.0.0.1", move |tls, _| {
            let _ = write!(
                tls,
                "HTTP/1.1 200 OK\r\n{header}Content-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            );
        });
        assert_eq!(
            server
                .transport(Duration::from_secs(1))
                .send(request())
                .await
                .err(),
            Some(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn encoded_bodies_are_rejected_instead_of_decoded_or_reinterpreted() {
    for encoding in ["gzip", "br", "deflate", "zstd", "identity, gzip"] {
        let server = Server::new("127.0.0.1", move |tls, _| {
            let _ = write!(
                tls,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: {encoding}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            );
        });
        assert_eq!(
            server
                .transport(Duration::from_secs(1))
                .send(request())
                .await
                .err(),
            Some(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn advertised_and_chunked_body_lengths_are_bounded_at_sixty_four_kibibytes() {
    let advertised = Server::new("127.0.0.1", |tls, _| {
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 65537\r\nConnection: close\r\n\r\n");
    });
    assert_eq!(
        advertised
            .transport(Duration::from_secs(1))
            .send(request())
            .await
            .err(),
        Some(Error::ResponseTooLarge)
    );
    let chunked = Server::new("127.0.0.1", |tls, _| {
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
        let chunk = [b' '; 16_384];
        for _ in 0..5 {
            if write!(tls, "4000\r\n")
                .and_then(|()| tls.write_all(&chunk))
                .and_then(|()| tls.write_all(b"\r\n"))
                .is_err()
            {
                return;
            }
        }
        let _ = tls.write_all(b"0\r\n\r\n");
    });
    assert_eq!(
        chunked
            .transport(Duration::from_secs(1))
            .send(request())
            .await
            .err(),
        Some(Error::ResponseTooLarge)
    );
}

#[tokio::test]
async fn exactly_sixty_four_kibibytes_of_private_json_are_returned_without_truncation() {
    let server = Server::new("127.0.0.1", |tls, _| {
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 65536\r\nConnection: close\r\n\r\n");
        let _ = tls.write_all(br#"{"v":""#);
        let filler = [b'x'; 65_528];
        let _ = tls.write_all(&filler);
        let _ = tls.write_all(br#""}"#);
    });
    let response = server
        .transport(Duration::from_secs(1))
        .send(request())
        .await
        .unwrap();
    assert_eq!(response.body.expose().len(), 65_536);
    assert!(response.body.expose().starts_with(br#"{"v":""#));
    assert!(response.body.expose().ends_with(br#""}"#));
}

#[tokio::test]
async fn close_delimited_oversized_and_truncated_bodies_do_not_publish_partial_private_data() {
    let oversized = Server::new("127.0.0.1", |tls, _| {
        let _ = tls.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
        );
        let _ = tls.write_all(&[b'x'; 65_537]);
    });
    assert_eq!(
        oversized
            .transport(Duration::from_secs(1))
            .send(request())
            .await
            .err(),
        Some(Error::ResponseTooLarge)
    );
    let truncated = Server::new("127.0.0.1", |tls, _| {
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 50\r\nConnection: close\r\n\r\n{\"private\":\"synthetic-partial-secret\"}");
    });
    let error = truncated
        .transport(Duration::from_secs(1))
        .send(request())
        .await
        .err()
        .unwrap();
    assert_eq!(error, Error::Unavailable);
    assert!(!format!("{error:?} {error}").contains("synthetic-partial-secret"));
}

#[tokio::test]
async fn response_cookies_are_never_carried_to_later_bootstrap_requests() {
    let server = Server::new("127.0.0.1", |tls, _| {
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: fixture_session=synthetic-cookie-secret; Secure; Path=/\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    });
    let transport = server.transport(Duration::from_secs(1));
    assert!(transport.send(request()).await.is_ok());
    assert!(transport.send(request()).await.is_ok());
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for received in requests.iter() {
        assert!(!received.head.to_ascii_lowercase().contains("cookie:"));
    }
}

#[tokio::test]
async fn a_rejected_status_releases_capacity_for_the_next_get() {
    let responses = Arc::new(AtomicUsize::new(0));
    let server_responses = responses.clone();
    let server = Server::new("127.0.0.1", move |tls, _| {
        let status = if server_responses.fetch_add(1, Ordering::AcqRel) == 0 {
            503
        } else {
            200
        };
        let _ = write!(
            tls,
            "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
    });
    let transport = server.transport(Duration::from_secs(1));
    assert_eq!(
        transport.send(request()).await.err(),
        Some(Error::HttpStatus(503))
    );
    assert_eq!(
        transport.send(request()).await.unwrap().body.expose(),
        b"{}"
    );
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn an_empty_success_body_is_invalid_including_no_content_status() {
    for status in [200, 204] {
        let server = Server::new("127.0.0.1", move |tls, _| {
            let _ = write!(
                tls,
                "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        });
        assert_eq!(
            server
                .transport(Duration::from_secs(1))
                .send(request())
                .await
                .err(),
            Some(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn total_deadline_covers_headers_and_a_trickling_response_body() {
    let stalled = Server::new("127.0.0.1", |_tls, stop| {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    assert_eq!(
        stalled
            .transport(Duration::from_millis(180))
            .send(request())
            .await
            .err(),
        Some(Error::Deadline)
    );
    let trickling = Server::new("127.0.0.1", |tls, stop| {
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 500\r\nConnection: close\r\n\r\n");
        for _ in 0..20 {
            if stop.load(Ordering::Acquire) || tls.write_all(b" ").is_err() {
                break;
            }
            let _ = tls.flush();
            std::thread::sleep(Duration::from_millis(40));
        }
    });
    assert_eq!(
        trickling
            .transport(Duration::from_millis(180))
            .send(request())
            .await
            .err(),
        Some(Error::Deadline)
    );
}

#[tokio::test]
async fn one_active_get_returns_busy_and_dropping_it_releases_capacity() {
    let release = Arc::new(AtomicBool::new(false));
    let server_release = release.clone();
    let server = Server::new("127.0.0.1", move |tls, stop| {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !server_release.load(Ordering::Acquire)
            && !stop.load(Ordering::Acquire)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    });
    let transport = Arc::new(server.transport(Duration::from_secs(3)));
    let first_transport = transport.clone();
    let first = tokio::spawn(async move { first_transport.send(request()).await });
    wait_requests(&server, 1).await;
    assert_eq!(transport.send(request()).await.err(), Some(Error::Busy));
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let next_transport = transport.clone();
    let next = tokio::spawn(async move { next_transport.send(request()).await });
    wait_requests(&server, 2).await;
    release.store(true, Ordering::Release);
    assert!(next.await.unwrap().is_ok());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

async fn wait_requests(server: &Server, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while server.requests.lock().unwrap().len() < expected && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(server.requests.lock().unwrap().len(), expected);
}

fn private_credentials(bootstrap: &[u8], subscriber: &[u8]) -> Credentials {
    let mut bootstrap = HeaderValue::from_bytes(bootstrap).unwrap();
    let mut subscriber = HeaderValue::from_bytes(subscriber).unwrap();
    bootstrap.set_sensitive(true);
    subscriber.set_sensitive(true);
    Credentials {
        bootstrap,
        subscriber,
    }
}

fn account_request(target: Target) -> Request {
    Request {
        target,
        credentials: Some(private_credentials(
            b"Bearer synthetic-bootstrap-capability",
            b"synthetic-subscriber-capability",
        )),
    }
}

fn header_values<'a>(head: &'a str, name: &str) -> Vec<&'a str> {
    head.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then_some(value.trim())
        })
        .collect()
}

#[tokio::test]
async fn verified_account_gets_use_exact_regional_routes_and_distinct_private_headers() {
    for (target, path) in [
        (
            Target::MyListIds(Region::Us),
            "/api/us/content/my-stuff-ids",
        ),
        (
            Target::MyListIds(Region::Ca),
            "/api/ca/content/my-stuff-ids",
        ),
        (
            Target::ContinueWatching(Region::Us),
            "/api/us/content/continue-watching",
        ),
        (
            Target::ContinueWatching(Region::Ca),
            "/api/ca/content/continue-watching",
        ),
    ] {
        let server = Server::json(200, br#"{"private":"synthetic-account-result"}"#);
        let private_request = account_request(target);
        assert!(!format!("{private_request:?}").contains("synthetic-bootstrap-capability"));
        assert!(!format!("{private_request:?}").contains("synthetic-subscriber-capability"));
        let response = server
            .transport(Duration::from_secs(1))
            .send(private_request)
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(
            response.body.expose(),
            br#"{"private":"synthetic-account-result"}"#
        );
        assert!(!format!("{response:?}").contains("synthetic-account-result"));
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]
                .head
                .starts_with(&format!("GET {path} HTTP/1.1\r\n"))
        );
        assert_eq!(
            header_values(&requests[0].head, "Authorization"),
            ["Bearer synthetic-bootstrap-capability"]
        );
        assert_eq!(
            header_values(&requests[0].head, "x-auth-token"),
            ["synthetic-subscriber-capability"]
        );
        assert_eq!(
            header_values(&requests[0].head, "Accept"),
            ["application/json"]
        );
        assert!(header_values(&requests[0].head, "Cookie").is_empty());
        assert!(requests[0].body.is_empty());
    }
}

#[tokio::test]
async fn maximal_valid_private_headers_survive_the_bounded_request_path() {
    let bootstrap = format!("Bearer {}", "b".repeat(16_384));
    let subscriber = "s".repeat(16_384);
    let server = Server::json(200, b"{}");
    let response = server
        .transport(Duration::from_secs(1))
        .send(Request {
            target: Target::MyListIds(Region::Us),
            credentials: Some(private_credentials(
                bootstrap.as_bytes(),
                subscriber.as_bytes(),
            )),
        })
        .await
        .unwrap();
    assert_eq!(response.body.expose(), b"{}");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        header_values(&requests[0].head, "Authorization"),
        [bootstrap.as_str()]
    );
    assert_eq!(
        header_values(&requests[0].head, "x-auth-token"),
        [subscriber.as_str()]
    );
}

#[tokio::test]
async fn account_targets_require_both_sensitive_correctly_framed_bounded_headers_before_contact() {
    let oversized_bootstrap = format!("Bearer {}", "b".repeat(16_385));
    let oversized_subscriber = "s".repeat(16_385);
    for target in [
        Target::MyListIds(Region::Us),
        Target::ContinueWatching(Region::Ca),
        Target::WatchList(Region::Us),
        Target::WatchList(Region::Ca),
        Target::AddWatchList {
            region: Region::Us,
            media_id: criterion_provider::MediaId::new("W1rA2bC3").unwrap(),
            content_type: WatchListContentType::Film,
        },
        Target::RemoveWatchList {
            region: Region::Ca,
            media_id: criterion_provider::MediaId::new("W1rA2bC3").unwrap(),
        },
    ] {
        let server = Server::json(200, b"{}");
        let transport = server.transport(Duration::from_secs(1));
        assert_eq!(
            transport
                .send(Request {
                    target: target.clone(),
                    credentials: None
                })
                .await
                .err(),
            Some(Error::InvalidRequest)
        );
        for (bootstrap, subscriber) in [
            (&b""[..], &b"subscriber"[..]),
            (&b"Bearer "[..], &b"subscriber"[..]),
            (&b"Basic bootstrap"[..], &b"subscriber"[..]),
            (&b"bearer bootstrap"[..], &b"subscriber"[..]),
            (&b"Bearer bootstrap with-space"[..], &b"subscriber"[..]),
            (&b"Bearer bootstrap\t"[..], &b"subscriber"[..]),
            (&b"Bearer \x80"[..], &b"subscriber"[..]),
            (&b"Bearer bootstrap"[..], &b""[..]),
            (&b"Bearer bootstrap"[..], &b"Bearer subscriber"[..]),
            (&b"Bearer bootstrap"[..], &b"subscriber with-space"[..]),
            (&b"Bearer bootstrap"[..], &b"subscriber\t"[..]),
            (&b"Bearer bootstrap"[..], &b"\x80"[..]),
            (oversized_bootstrap.as_bytes(), &b"subscriber"[..]),
            (&b"Bearer bootstrap"[..], oversized_subscriber.as_bytes()),
        ] {
            let request = Request {
                target: target.clone(),
                credentials: Some(private_credentials(bootstrap, subscriber)),
            };
            assert_eq!(
                transport.send(request).await.err(),
                Some(Error::InvalidRequest)
            );
        }
        for clear_bootstrap in [true, false] {
            let mut credentials = private_credentials(b"Bearer bootstrap", b"subscriber");
            if clear_bootstrap {
                credentials.bootstrap.set_sensitive(false);
            } else {
                credentials.subscriber.set_sensitive(false);
            }
            assert_eq!(
                transport
                    .send(Request {
                        target: target.clone(),
                        credentials: Some(credentials)
                    })
                    .await
                    .err(),
                Some(Error::InvalidRequest)
            );
        }
        assert_eq!(server.connections.load(Ordering::Acquire), 0);
        assert!(server.requests.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn account_redirects_never_reissue_private_headers_to_bootstrap_or_another_origin() {
    for status in [302, 307, 308] {
        for (target, path) in [
            (
                Target::MyListIds(Region::Us),
                "/api/us/content/my-stuff-ids",
            ),
            (
                Target::ContinueWatching(Region::Ca),
                "/api/ca/content/continue-watching",
            ),
            (Target::WatchList(Region::Us), "/api/us/content/watch-list"),
            (Target::WatchList(Region::Ca), "/api/ca/content/watch-list"),
        ] {
            let outside = Server::json(200, b"{}");
            let location = format!("{}api/init", outside.origin);
            let redirect = Server::new("127.0.0.1", move |tls, _| {
                let _ = write!(
                    tls,
                    "HTTP/1.1 {status} Fixture\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            });
            assert_eq!(
                redirect
                    .transport(Duration::from_secs(1))
                    .send(account_request(target.clone()))
                    .await
                    .err(),
                Some(Error::HttpStatus(status))
            );
            assert_eq!(outside.connections.load(Ordering::Acquire), 0);
            assert_eq!(redirect.requests.lock().unwrap().len(), 1);
            let same_origin = Server::new("127.0.0.1", move |tls, _| {
                let _ = write!(
                    tls,
                    "HTTP/1.1 {status} Fixture\r\nLocation: /api/init\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            });
            assert_eq!(
                same_origin
                    .transport(Duration::from_secs(1))
                    .send(account_request(target.clone()))
                    .await
                    .err(),
                Some(Error::HttpStatus(status))
            );
            let requests = same_origin.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(
                requests[0]
                    .head
                    .starts_with(&format!("GET {path} HTTP/1.1\r\n"))
            );
        }
    }
}

#[tokio::test]
async fn subscriber_errors_do_not_retry_and_later_bootstrap_has_no_stale_headers() {
    for status in [401, 429, 503] {
        let responses = Arc::new(AtomicUsize::new(0));
        let server_responses = responses.clone();
        let server = Server::new("127.0.0.1", move |tls, _| {
            let status = if server_responses.fetch_add(1, Ordering::AcqRel) == 0 {
                status
            } else {
                200
            };
            let _ = write!(
                tls,
                "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nSet-Cookie: account=synthetic-cookie; Secure; Path=/\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            );
        });
        let transport = server.transport(Duration::from_secs(1));
        assert_eq!(
            transport
                .send(account_request(Target::ContinueWatching(Region::Us)))
                .await
                .err(),
            Some(Error::HttpStatus(status))
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert!(transport.send(request()).await.is_ok());
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].head.starts_with("GET /api/init HTTP/1.1\r\n"));
        assert!(header_values(&requests[1].head, "Authorization").is_empty());
        assert!(header_values(&requests[1].head, "x-auth-token").is_empty());
        assert!(header_values(&requests[1].head, "Cookie").is_empty());
    }
}

#[tokio::test]
async fn default_watch_list_gets_use_fixed_regional_routes_without_query_or_body() {
    for (target, path) in [
        (Target::WatchList(Region::Us), "/api/us/content/watch-list"),
        (Target::WatchList(Region::Ca), "/api/ca/content/watch-list"),
    ] {
        let server = Server::json(200, br#"{"playlist":[],"paging":{}}"#);
        let response = server
            .transport(Duration::from_secs(1))
            .send(account_request(target.clone()))
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body.expose(), br#"{"playlist":[],"paging":{}}"#);
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]
                .head
                .starts_with(&format!("GET {path} HTTP/1.1\r\n"))
        );
        assert_eq!(
            header_values(&requests[0].head, "Authorization"),
            ["Bearer synthetic-bootstrap-capability"]
        );
        assert_eq!(
            header_values(&requests[0].head, "x-auth-token"),
            ["synthetic-subscriber-capability"]
        );
        assert!(header_values(&requests[0].head, "Cookie").is_empty());
        assert!(requests[0].body.is_empty());
    }
}

fn write_targets(region: Region) -> [Target; 2] {
    [
        Target::AddWatchList {
            region,
            media_id: criterion_provider::MediaId::new("W1rA2bC3").unwrap(),
            content_type: WatchListContentType::Film,
        },
        Target::RemoveWatchList {
            region,
            media_id: criterion_provider::MediaId::new("W1rA2bC3").unwrap(),
        },
    ]
}

#[tokio::test]
async fn add_watch_list_posts_only_the_verified_id_and_nine_exact_content_types() {
    for (region, path) in [
        (Region::Us, "/api/us/content/watch-list"),
        (Region::Ca, "/api/ca/content/watch-list"),
    ] {
        for (content_type, value) in [
            (WatchListContentType::Film, "film"),
            (WatchListContentType::Series, "series"),
            (WatchListContentType::Collection, "collection"),
            (WatchListContentType::Episode, "episode"),
            (WatchListContentType::Supplement, "supplement"),
            (WatchListContentType::Category, "category"),
            (WatchListContentType::Franchise, "franchise"),
            (WatchListContentType::Live, "live"),
            (WatchListContentType::Original, "original"),
        ] {
            let target = Target::AddWatchList {
                region,
                media_id: criterion_provider::MediaId::new("W1rA2bC3").unwrap(),
                content_type,
            };
            let private_request = account_request(target);
            assert!(!format!("{private_request:?}").contains("W1rA2bC3"));
            assert!(!format!("{private_request:?}").contains("synthetic-bootstrap-capability"));
            assert!(!format!("{private_request:?}").contains("synthetic-subscriber-capability"));
            let server = Server::json(200, br#"{"sync":true}"#);
            let response = server
                .transport(Duration::from_secs(1))
                .send(private_request)
                .await
                .unwrap();
            assert_eq!(response.status, 200);
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(
                requests[0]
                    .head
                    .starts_with(&format!("POST {path} HTTP/1.1\r\n"))
            );
            assert_eq!(
                header_values(&requests[0].head, "Content-Type"),
                ["application/json"]
            );
            assert_eq!(
                header_values(&requests[0].head, "Authorization"),
                ["Bearer synthetic-bootstrap-capability"]
            );
            assert_eq!(
                header_values(&requests[0].head, "x-auth-token"),
                ["synthetic-subscriber-capability"]
            );
            assert!(header_values(&requests[0].head, "Cookie").is_empty());
            assert!(requests[0].body.len() <= 256);
            let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
            assert_eq!(body.as_object().unwrap().len(), 2);
            assert_eq!(body["media_id"], "W1rA2bC3");
            assert_eq!(body["content_type"], value);
        }
    }
}

#[tokio::test]
async fn remove_watch_list_deletes_the_verified_id_with_no_query_or_body() {
    for (region, path) in [
        (Region::Us, "/api/us/content/watch-list/W1rA2bC3"),
        (Region::Ca, "/api/ca/content/watch-list/W1rA2bC3"),
    ] {
        let target = Target::RemoveWatchList {
            region,
            media_id: criterion_provider::MediaId::new("W1rA2bC3").unwrap(),
        };
        assert!(!format!("{target:?}").contains("W1rA2bC3"));
        let server = Server::json(200, br#"{"sync":false}"#);
        assert_eq!(
            server
                .transport(Duration::from_secs(1))
                .send(account_request(target))
                .await
                .unwrap()
                .status,
            200
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]
                .head
                .starts_with(&format!("DELETE {path} HTTP/1.1\r\n"))
        );
        assert_eq!(
            header_values(&requests[0].head, "Authorization"),
            ["Bearer synthetic-bootstrap-capability"]
        );
        assert_eq!(
            header_values(&requests[0].head, "x-auth-token"),
            ["synthetic-subscriber-capability"]
        );
        assert!(header_values(&requests[0].head, "Content-Type").is_empty());
        assert!(header_values(&requests[0].head, "Transfer-Encoding").is_empty());
        assert!(header_values(&requests[0].head, "Cookie").is_empty());
        assert!(requests[0].body.is_empty());
    }
}

#[tokio::test]
async fn list_write_redirects_never_reissue_mutations_or_private_headers() {
    for status in [302, 307, 308] {
        for target in write_targets(Region::Us) {
            let destination = Server::json(200, b"{}");
            let location = format!("{}api/init", destination.origin);
            let redirect = Server::new("127.0.0.1", move |tls, _| {
                let _ = write!(
                    tls,
                    "HTTP/1.1 {status} Fixture\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            });
            let error = redirect
                .transport(Duration::from_secs(1))
                .send(account_request(target.clone()))
                .await
                .err()
                .unwrap();
            assert_eq!(error, Error::HttpStatus(status));
            assert!(!format!("{error:?} {error}").contains("W1rA2bC3"));
            assert!(!format!("{error:?} {error}").contains("synthetic-bootstrap-capability"));
            assert!(!format!("{error:?} {error}").contains("synthetic-subscriber-capability"));
            assert_eq!(redirect.requests.lock().unwrap().len(), 1);
            assert_eq!(destination.connections.load(Ordering::Acquire), 0);

            let same_origin = Server::new("127.0.0.1", move |tls, _| {
                let _ = write!(
                    tls,
                    "HTTP/1.1 {status} Fixture\r\nLocation: /api/init\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            });
            assert_eq!(
                same_origin
                    .transport(Duration::from_secs(1))
                    .send(account_request(target))
                    .await
                    .err(),
                Some(Error::HttpStatus(status))
            );
            assert_eq!(same_origin.requests.lock().unwrap().len(), 1);
        }
    }
}

#[tokio::test]
async fn list_write_errors_never_retry_and_following_bootstrap_has_no_private_defaults() {
    for status in [400, 401, 429, 503] {
        for target in write_targets(Region::Ca) {
            let responses = Arc::new(AtomicUsize::new(0));
            let server_responses = responses.clone();
            let server = Server::new("127.0.0.1", move |tls, _| {
                let status = if server_responses.fetch_add(1, Ordering::AcqRel) == 0 {
                    status
                } else {
                    200
                };
                let _ = write!(
                    tls,
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nSet-Cookie: write=synthetic-cookie; Secure; Path=/\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                );
            });
            let transport = server.transport(Duration::from_secs(1));
            assert_eq!(
                transport.send(account_request(target)).await.err(),
                Some(Error::HttpStatus(status))
            );
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            assert!(transport.send(request()).await.is_ok());
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests[1].head.starts_with("GET /api/init HTTP/1.1\r\n"));
            assert!(requests[1].body.is_empty());
            assert!(header_values(&requests[1].head, "Authorization").is_empty());
            assert!(header_values(&requests[1].head, "x-auth-token").is_empty());
            assert!(header_values(&requests[1].head, "Cookie").is_empty());
            assert!(header_values(&requests[1].head, "Content-Type").is_empty());
        }
    }
}

#[tokio::test]
async fn a_list_write_disconnected_after_delivery_is_never_replayed() {
    for target in write_targets(Region::Us) {
        let server = Server::new("127.0.0.1", |_tls, _stop| {});
        let error = server
            .transport(Duration::from_secs(1))
            .send(account_request(target))
            .await
            .err()
            .unwrap();
        assert_eq!(error, Error::Unavailable);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert!(!format!("{error:?} {error}").contains("W1rA2bC3"));
        assert!(!format!("{error:?} {error}").contains("synthetic-bootstrap-capability"));
        assert!(!format!("{error:?} {error}").contains("synthetic-subscriber-capability"));
    }
}

#[tokio::test]
async fn one_active_list_write_refuses_another_before_contact_and_releases_after_completion() {
    for target in write_targets(Region::Ca) {
        let release = Arc::new(AtomicBool::new(false));
        let server_release = release.clone();
        let server = Server::new("127.0.0.1", move |tls, stop| {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !server_release.load(Ordering::Acquire)
                && !stop.load(Ordering::Acquire)
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
        });
        let transport = Arc::new(server.transport(Duration::from_secs(3)));
        let first_transport = transport.clone();
        let other_target = target.clone();
        let first =
            tokio::spawn(async move { first_transport.send(account_request(target)).await });
        wait_requests(&server, 1).await;
        assert_eq!(
            transport.send(account_request(other_target)).await.err(),
            Some(Error::Busy)
        );
        assert_eq!(server.connections.load(Ordering::Acquire), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        release.store(true, Ordering::Release);
        assert_eq!(first.await.unwrap().unwrap().status, 200);
        assert!(transport.send(request()).await.is_ok());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    }
}
