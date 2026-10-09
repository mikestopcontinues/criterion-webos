use crate::{BrowseRequest, Catalog, HttpTransport};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

struct Server {
    origin: url::Url,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    connections: Arc<std::sync::atomic::AtomicUsize>,
}

impl Server {
    fn holding() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin =
            url::Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = connections.clone();
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut workers = Vec::new();
            while !thread_stop.load(Ordering::Acquire) && Instant::now() < deadline {
                if let Ok((mut stream, _)) = listener.accept() {
                    count.fetch_add(1, Ordering::Release);
                    let release = thread_stop.clone();
                    workers.push(std::thread::spawn(move || {
                        stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                        let mut request = [0_u8; 4096];
                        let _ = stream.read(&mut request);
                        while !release.load(Ordering::Acquire) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(2)); }
                        let body = include_bytes!("../../../tests/fixtures/provider/all-films.json");
                        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).and_then(|()| stream.write_all(body));
                    }));
                } else {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            origin,
            stop,
            thread: Some(thread),
            connections,
        }
    }

    fn json(body: &[u8]) -> Self {
        let response = [format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes(), body.to_vec()].concat();
        Self::respond(move |mut stream| {
            let _ = stream.write_all(&response);
        })
    }

    fn respond(handler: impl FnOnce(std::net::TcpStream) + Send + 'static) -> Self {
        Self::respond_raw(true, handler)
    }

    fn respond_raw(
        read_http: bool,
        handler: impl FnOnce(std::net::TcpStream) + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin =
            url::Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = connections.clone();
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !thread_stop.load(Ordering::Acquire) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        count.fetch_add(1, Ordering::Release);
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        if read_http {
                            let mut request = [0_u8; 4096];
                            let _ = stream.read(&mut request);
                        }
                        handler(stream);
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("local HTTP fixture server failed: {error}"),
                }
            }
        });
        Self {
            origin,
            stop,
            thread: Some(thread),
            connections,
        }
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

#[tokio::test]
async fn real_http_transport_returns_the_catalog_metadata() {
    let server = Server::json(include_bytes!(
        "../../../tests/fixtures/provider/all-films.json"
    ));
    let transport = HttpTransport::for_test(server.origin.clone(), Duration::from_secs(1));
    let page = Catalog::with_transport(transport)
        .browse(&BrowseRequest::default())
        .await
        .unwrap();
    assert_eq!(page.items[0].title, "2 or 3 Things I Know About Her");
}

#[tokio::test]
async fn real_http_transport_denies_redirects_without_contacting_the_target() {
    let target = Server::json(include_bytes!(
        "../../../tests/fixtures/provider/all-films.json"
    ));
    let location = target.origin.to_string();
    let server = Server::respond(move |mut stream| {
        write!(stream, "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let transport = HttpTransport::for_test(server.origin.clone(), Duration::from_secs(1));
    assert_eq!(
        Catalog::with_transport(transport)
            .browse(&BrowseRequest::default())
            .await,
        Err(crate::Error::HttpStatus(302))
    );
    assert_eq!(target.connections.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn real_http_transport_caps_chunked_bodies_without_content_length() {
    let server = Server::respond(|mut stream| {
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
        let bytes = vec![b' '; 16_384];
        for _ in 0..129 {
            if write!(stream, "4000\r\n")
                .and_then(|()| stream.write_all(&bytes))
                .and_then(|()| stream.write_all(b"\r\n"))
                .is_err()
            {
                return;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n");
    });
    let transport = HttpTransport::for_test(server.origin.clone(), Duration::from_secs(1));
    assert_eq!(
        Catalog::with_transport(transport)
            .browse(&BrowseRequest::default())
            .await,
        Err(crate::Error::ResponseTooLarge)
    );
}

#[tokio::test]
async fn real_http_transport_caps_decompressed_gzip_bytes() {
    // An admitted catalog fixture padded with spaces to 2 MiB + 1 and compressed with gzip, mtime=0.
    let encoded = include_bytes!("../tests/data/oversize.json.gz");
    let server = Server::respond(move |mut stream| {
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", encoded.len()).unwrap();
        let _ = stream.write_all(encoded);
    });
    let transport = HttpTransport::for_test(server.origin.clone(), Duration::from_secs(1));
    assert_eq!(
        Catalog::with_transport(transport)
            .browse(&BrowseRequest::default())
            .await,
        Err(crate::Error::ResponseTooLarge)
    );
}

#[tokio::test]
async fn real_http_transport_applies_a_total_deadline_to_a_trickling_body() {
    let server = Server::respond(|mut stream| {
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 500\r\nConnection: close\r\n\r\n").unwrap();
        for _ in 0..20 {
            if stream.write_all(b" ").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let transport = HttpTransport::for_test(server.origin.clone(), Duration::from_millis(90));
    assert_eq!(
        Catalog::with_transport(transport)
            .browse(&BrowseRequest::default())
            .await,
        Err(crate::Error::Deadline)
    );
}

#[tokio::test]
async fn real_http_transport_rejects_non_catalog_origins_and_paths_before_network_access() {
    use crate::RequestTransport;
    let transport = HttpTransport::new().unwrap();
    for url in [
        "http://www.criterionchannel.com/api/search?q=s",
        "https://criterionchannel.com/api/search?q=s",
        "https://www.criterionchannel.com:444/api/search?q=s",
        "https://token@www.criterionchannel.com/api/search?q=s",
        "https://www.criterionchannel.com/api/search?q=s#token",
        "https://www.criterionchannel.com/api/login",
        "https://www.criterionchannel.com/api/media/../token",
    ] {
        assert!(matches!(
            transport
                .get(crate::Request {
                    url: url::Url::parse(url).unwrap()
                })
                .await,
            Err(crate::Error::InvalidRequest)
        ));
    }
}

#[tokio::test]
async fn real_http_transport_bounds_active_requests_and_releases_cancelled_capacity() {
    let server = Server::holding();
    let catalog = Arc::new(Catalog::with_transport(HttpTransport::for_test(
        server.origin.clone(),
        Duration::from_secs(2),
    )));
    let first_catalog = catalog.clone();
    let first = tokio::spawn(async move { first_catalog.browse(&BrowseRequest::default()).await });
    let second_catalog = catalog.clone();
    let second =
        tokio::spawn(async move { second_catalog.browse(&BrowseRequest::default()).await });
    let ready = Instant::now() + Duration::from_secs(1);
    while server.connections.load(Ordering::Acquire) < 2 && Instant::now() < ready {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(server.connections.load(Ordering::Acquire), 2);
    assert_eq!(
        catalog.browse(&BrowseRequest::default()).await,
        Err(crate::Error::Busy)
    );
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let third_catalog = catalog.clone();
    let third = tokio::spawn(async move { third_catalog.browse(&BrowseRequest::default()).await });
    let ready = Instant::now() + Duration::from_secs(1);
    while server.connections.load(Ordering::Acquire) < 3 && Instant::now() < ready {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(server.connections.load(Ordering::Acquire), 3);
    server.stop.store(true, Ordering::Release);
    assert!(second.await.unwrap().is_ok());
    assert!(third.await.unwrap().is_ok());
}

fn tls_server(name: &str) -> (Server, rustls::RootCertStore) {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec![name.to_owned()]).unwrap();
    let certificate = cert.der().clone();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate.clone()).unwrap();
    let private_key = rustls::pki_types::PrivateKeyDer::Pkcs8(
        rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()),
    );
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![certificate], private_key)
    .unwrap();
    let mut server = Server::respond_raw(false, move |stream| {
        let mut tls = rustls::StreamOwned::new(
            rustls::ServerConnection::new(Arc::new(config)).unwrap(),
            stream,
        );
        let mut request = [0_u8; 4096];
        if tls.read(&mut request).is_ok() {
            let body = include_bytes!("../../../tests/fixtures/provider/all-films.json");
            let _ = write!(tls, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).and_then(|()| tls.write_all(body));
        }
    });
    server.origin.set_scheme("https").unwrap();
    (server, roots)
}

#[tokio::test]
async fn real_tls_verifies_the_certificate_chain_and_hostname() {
    let (trusted, trusted_roots) = tls_server("127.0.0.1");
    let transport = HttpTransport::for_test_roots(trusted.origin.clone(), trusted_roots);
    assert!(
        Catalog::with_transport(transport)
            .browse(&BrowseRequest::default())
            .await
            .is_ok()
    );
    let (untrusted, _) = tls_server("127.0.0.1");
    let transport = HttpTransport::for_test(untrusted.origin.clone(), Duration::from_secs(1));
    assert_eq!(
        Catalog::with_transport(transport)
            .browse(&BrowseRequest::default())
            .await,
        Err(crate::Error::Unavailable)
    );
    let (wrong_host, trusted_roots) = tls_server("wrong.example");
    let transport = HttpTransport::for_test_roots(wrong_host.origin.clone(), trusted_roots);
    assert_eq!(
        Catalog::with_transport(transport)
            .browse(&BrowseRequest::default())
            .await,
        Err(crate::Error::Unavailable)
    );
}
