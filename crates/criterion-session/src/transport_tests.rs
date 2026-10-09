use crate::{Endpoint, Error, HttpTransport, Request, SecretBody, Transport};
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
        if bytes.len() > 8192 {
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

fn request(endpoint: Endpoint) -> Request {
    Request {
        endpoint,
        body: SecretBody::new(b"client_id=fixture&device_code=synthetic-secret".to_vec()),
    }
}

#[tokio::test]
async fn verified_tls_sends_one_form_post_to_each_fixed_endpoint_and_returns_private_json() {
    for (endpoint, path) in [
        (Endpoint::DeviceCode, "/oauth/device/code"),
        (Endpoint::Token, "/oauth/token"),
        (Endpoint::Revoke, "/oauth/revoke"),
    ] {
        let server = Server::json(200, br#"{"access_token":"synthetic-result"}"#);
        let response = server
            .transport(Duration::from_secs(1))
            .post(request(endpoint))
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(
            response.body.expose(),
            br#"{"access_token":"synthetic-result"}"#
        );
        assert!(!format!("{response:?}").contains("synthetic-result"));
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]
                .head
                .starts_with(&format!("POST {path} HTTP/1.1\r\n"))
        );
        let headers = requests[0].head.to_ascii_lowercase();
        assert!(headers.contains("content-type: application/x-www-form-urlencoded\r\n"));
        assert!(headers.contains("accept: application/json\r\n"));
        assert!(headers.contains("accept-encoding: identity\r\n"));
        assert_eq!(
            requests[0].body,
            b"client_id=fixture&device_code=synthetic-secret"
        );
    }
}

#[tokio::test]
async fn oauth_error_json_is_returned_to_the_session_parser() {
    let server = Server::json(400, br#"{"error":"authorization_pending"}"#);
    let response = server
        .transport(Duration::from_secs(1))
        .post(request(Endpoint::Token))
        .await
        .unwrap();
    assert_eq!(response.status, 400);
    assert_eq!(
        response.body.expose(),
        br#"{"error":"authorization_pending"}"#
    );
}

#[tokio::test]
async fn untrusted_and_wrong_hostname_certificates_are_rejected_without_http_delivery() {
    let untrusted = Server::json(200, b"{}");
    let transport =
        HttpTransport::for_test(untrusted.origin.clone(), Duration::from_secs(1)).unwrap();
    assert_eq!(
        transport.post(request(Endpoint::Token)).await.err(),
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
            .post(request(Endpoint::Token))
            .await
            .err(),
        Some(Error::Unavailable)
    );
    assert!(wrong_host.requests.lock().unwrap().is_empty());
}

#[test]
fn test_origins_cannot_disable_https_or_attach_credentials_or_paths() {
    for origin in [
        "http://127.0.0.1:443/",
        "https://name@127.0.0.1:443/",
        "https://127.0.0.1:443/private",
        "https://127.0.0.1:443/?q=value",
        "https://127.0.0.1:443/#fragment",
        "https://remote.example/",
    ] {
        assert!(matches!(
            HttpTransport::for_test(url::Url::parse(origin).unwrap(), Duration::from_secs(1)),
            Err(Error::InvalidRequest)
        ));
    }
}

#[tokio::test]
async fn redirects_are_denied_and_the_target_is_never_contacted() {
    let target = Server::json(200, b"{}");
    let location = target.origin.to_string();
    let server = Server::new("127.0.0.1", move |tls, _| {
        let _ = write!(
            tls,
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    });
    assert_eq!(
        server
            .transport(Duration::from_secs(1))
            .post(request(Endpoint::Token))
            .await
            .err(),
        Some(Error::HttpStatus(307))
    );
    assert_eq!(target.connections.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn oversized_requests_are_rejected_before_network_access() {
    let server = Server::json(200, b"{}");
    let transport = server.transport(Duration::from_secs(1));
    let request = Request {
        endpoint: Endpoint::Token,
        body: SecretBody::new(vec![b'x'; 64 * 1024 + 1]),
    };
    assert_eq!(
        transport.post(request).await.err(),
        Some(Error::InvalidRequest)
    );
    assert_eq!(server.connections.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn maximal_admitted_secret_survives_form_expansion_within_request_bound() {
    let server = Server::json(200, b"{}");
    let secret: crate::Secret =
        serde_json::from_str(&serde_json::to_string(&"\"".repeat(crate::wire::MAX_TOKEN)).unwrap())
            .unwrap();
    let body = crate::wire::refresh_form(&secret);
    assert!(body.expose().len() > 32 * 1024);
    assert!(body.expose().len() < 64 * 1024);
    assert_eq!(
        server
            .transport(Duration::from_secs(1))
            .post(Request {
                endpoint: Endpoint::Token,
                body
            })
            .await
            .unwrap()
            .status,
        200
    );
    let requests = server.requests.lock().unwrap();
    let fields: Vec<_> = url::form_urlencoded::parse(&requests[0].body).collect();
    assert!(
        fields
            .iter()
            .any(|(name, value)| name == "refresh_token" && value == secret.expose())
    );
}

#[tokio::test]
async fn advertised_oversized_responses_are_rejected() {
    let server = Server::new("127.0.0.1", |tls, _| {
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 65537\r\nConnection: close\r\n\r\n");
    });
    assert_eq!(
        server
            .transport(Duration::from_secs(1))
            .post(request(Endpoint::Token))
            .await
            .err(),
        Some(Error::ResponseTooLarge)
    );
}

#[tokio::test]
async fn chunked_responses_are_bounded_without_content_length() {
    let server = Server::new("127.0.0.1", |tls, _| {
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
        server
            .transport(Duration::from_secs(1))
            .post(request(Endpoint::Token))
            .await
            .err(),
        Some(Error::ResponseTooLarge)
    );
}

#[tokio::test]
async fn non_json_and_encoded_responses_are_rejected() {
    for header in [
        "Content-Type: text/html",
        "Content-Type: application/json\r\nContent-Encoding: gzip",
        "Content-Type: application/json\r\nContent-Encoding: br",
    ] {
        let server = Server::new("127.0.0.1", move |tls, _| {
            let _ = write!(
                tls,
                "HTTP/1.1 200 OK\r\n{header}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            );
        });
        assert_eq!(
            server
                .transport(Duration::from_secs(1))
                .post(request(Endpoint::Token))
                .await
                .err(),
            Some(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn empty_success_is_allowed_only_for_revoke() {
    for (endpoint, status, expected) in [
        (Endpoint::Revoke, 200, None),
        (Endpoint::Token, 200, Some(Error::InvalidResponse)),
        (Endpoint::Revoke, 400, Some(Error::InvalidResponse)),
    ] {
        let server = Server::new("127.0.0.1", move |tls, _| {
            let _ = write!(
                tls,
                "HTTP/1.1 {status} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        });
        let result = server
            .transport(Duration::from_secs(1))
            .post(request(endpoint))
            .await;
        assert_eq!(result.as_ref().err().copied(), expected);
        if let Ok(response) = result {
            assert!(response.body.expose().is_empty());
        }
    }
}

#[tokio::test]
async fn total_deadline_covers_a_trickling_response_body() {
    let server = Server::new("127.0.0.1", |tls, stop| {
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
        server
            .transport(Duration::from_millis(180))
            .post(request(Endpoint::Token))
            .await
            .err(),
        Some(Error::Deadline)
    );
}

#[tokio::test]
async fn only_one_request_is_active_and_cancellation_releases_capacity() {
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
    let first = tokio::spawn(async move { first_transport.post(request(Endpoint::Token)).await });
    wait_requests(&server, 1).await;
    assert_eq!(
        transport.post(request(Endpoint::Token)).await.err(),
        Some(Error::Busy)
    );
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let next_transport = transport.clone();
    let next = tokio::spawn(async move { next_transport.post(request(Endpoint::Token)).await });
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
