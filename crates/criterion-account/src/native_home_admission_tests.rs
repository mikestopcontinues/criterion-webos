//! Root-only anonymous native Home index observation; no production Lander default.
use super::*;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::time::Instant;

#[derive(serde::Serialize)]
struct Report {
    experiment: &'static str,
    phase: &'static str,
    outcome: &'static str,
    bootstrap_attempts: usize,
    home_attempts: usize,
    bootstrap_status: Option<u16>,
    home_status: Option<u16>,
    body_sha256: Option<String>,
    schema: Option<Schema>,
}
impl Default for Report {
    fn default() -> Self {
        Self {
            experiment: "anonymous_native_home_index",
            phase: "not_started",
            outcome: "not_started",
            bootstrap_attempts: 0,
            home_attempts: 0,
            bootstrap_status: None,
            home_status: None,
            body_sha256: None,
            schema: None,
        }
    }
}

const MAX_BODY: usize = 512 * 1024;
const MAX_DEPTH: usize = 32;
const MAX_FIELDS: usize = 4096;
const MAX_NODES: usize = 16384;
const PUBLIC_FIELDS: [&str; 4] = ["page", "name", "longName", "blocks"];

#[derive(Clone, Copy, Default, serde::Serialize)]
struct Types {
    object: usize,
    array: usize,
    string: usize,
    number: usize,
    boolean: usize,
    null: usize,
}
#[derive(Clone, Copy)]
enum Kind {
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
}
impl Types {
    fn add(&mut self, kind: Kind) {
        let count = match kind {
            Kind::Object => &mut self.object,
            Kind::Array => &mut self.array,
            Kind::String => &mut self.string,
            Kind::Number => &mut self.number,
            Kind::Boolean => &mut self.boolean,
            Kind::Null => &mut self.null,
        };
        *count += 1; // Every node was already charged against MAX_NODES.
    }
}
#[derive(Default, serde::Serialize)]
struct Field {
    name: &'static str,
    occurrences: usize,
    types: Types,
    array_items: usize,
}
#[derive(serde::Serialize)]
struct Schema {
    nodes: usize,
    object_fields: usize,
    unallowlisted_fields: usize,
    array_items: usize,
    types: Types,
    fields: Vec<Field>, // At most the four fixed public names, never input names.
}
#[derive(Default)]
struct Scan {
    nodes: usize,
    object_fields: usize,
    unallowlisted_fields: usize,
    array_items: usize,
    types: Types,
    fields: [Field; 4],
}
struct Shape {
    kind: Kind,
    array_items: usize,
}
impl Shape {
    fn scalar(kind: Kind) -> Self {
        Self {
            kind,
            array_items: 0,
        }
    }
}
fn charge<E: de::Error>(count: &mut usize, maximum: usize) -> Result<(), E> {
    *count = count
        .checked_add(1)
        .ok_or_else(|| E::custom("schema bounds"))?;
    if *count > maximum {
        return Err(E::custom("schema bounds"));
    }
    Ok(())
}
struct ValueSeed<'a> {
    scan: &'a mut Scan,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = Shape;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Shape, D::Error> {
        if self.depth > MAX_DEPTH {
            return Err(de::Error::custom("schema bounds"));
        }
        charge(&mut self.scan.nodes, MAX_NODES)?;
        let shape = deserializer.deserialize_any(ValueVisitor {
            scan: self.scan,
            depth: self.depth,
        })?;
        self.scan.types.add(shape.kind);
        Ok(shape)
    }
}
struct PublicKey;
impl<'de> DeserializeSeed<'de> for PublicKey {
    type Value = Option<usize>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        struct KeyVisitor;
        impl Visitor<'_> for KeyVisitor {
            type Value = Option<usize>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON property name")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(PUBLIC_FIELDS.iter().position(|name| *name == value))
            }
        }
        deserializer.deserialize_str(KeyVisitor)
    }
}
struct ValueVisitor<'a> {
    scan: &'a mut Scan,
    depth: usize,
}
impl<'de> Visitor<'de> for ValueVisitor<'_> {
    type Value = Shape;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bounded JSON structure")
    }
    fn visit_str<E: de::Error>(self, _value: &str) -> Result<Shape, E> {
        Ok(Shape::scalar(Kind::String))
    }
    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<Shape, E> {
        Ok(Shape::scalar(Kind::Boolean))
    }
    fn visit_i64<E: de::Error>(self, _value: i64) -> Result<Shape, E> {
        Ok(Shape::scalar(Kind::Number))
    }
    fn visit_u64<E: de::Error>(self, _value: u64) -> Result<Shape, E> {
        Ok(Shape::scalar(Kind::Number))
    }
    fn visit_f64<E: de::Error>(self, _value: f64) -> Result<Shape, E> {
        Ok(Shape::scalar(Kind::Number))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Shape, E> {
        Ok(Shape::scalar(Kind::Null))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Shape, A::Error> {
        let mut items = 0;
        while sequence
            .next_element_seed(ValueSeed {
                scan: self.scan,
                depth: self.depth + 1,
            })?
            .is_some()
        {
            charge(&mut items, MAX_NODES)?;
            charge(&mut self.scan.array_items, MAX_NODES)?;
        }
        Ok(Shape {
            kind: Kind::Array,
            array_items: items,
        })
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Shape, A::Error> {
        while let Some(public) = map.next_key_seed(PublicKey)? {
            charge(&mut self.scan.object_fields, MAX_FIELDS)?;
            if public.is_none() {
                charge(&mut self.scan.unallowlisted_fields, MAX_FIELDS)?;
            }
            let value = map.next_value_seed(ValueSeed {
                scan: self.scan,
                depth: self.depth + 1,
            })?;
            if let Some(index) = public {
                let field = &mut self.scan.fields[index];
                field.name = PUBLIC_FIELDS[index];
                field.occurrences += 1;
                field.types.add(value.kind);
                field.array_items += value.array_items;
            }
        }
        Ok(Shape::scalar(Kind::Object))
    }
}

fn observe(body: &[u8]) -> Result<Schema, Error> {
    if body.len() > MAX_BODY {
        return Err(Error::ResponseTooLarge);
    }
    let mut scan = Scan::default();
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let root = ValueSeed {
        scan: &mut scan,
        depth: 1,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| Error::InvalidResponse)?;
    deserializer.end().map_err(|_| Error::InvalidResponse)?;
    if !matches!(root.kind, Kind::Object) {
        return Err(Error::InvalidResponse);
    }
    Ok(Schema {
        nodes: scan.nodes,
        object_fields: scan.object_fields,
        unallowlisted_fields: scan.unallowlisted_fields,
        array_items: scan.array_items,
        types: scan.types,
        fields: scan
            .fields
            .into_iter()
            .filter(|field| field.occurrences != 0)
            .collect(),
    })
}

async fn run_experiment(transport: &HttpTransport, deadline: Instant) -> Report {
    let mut report = Report::default();
    let result = tokio::time::timeout_at(deadline, async {
        check_deadline(deadline)?;
        report.phase = "bootstrap";
        report.bootstrap_attempts = 1;
        let response = transport.send(Request::Bootstrap).await?;
        report.bootstrap_status = Some(response.status);
        let bootstrap = crate::wire::bootstrap(&response)?;
        let authorization = crate::wire::token_header(b"Bearer ", bootstrap.token.expose())?;
        drop(response);
        check_deadline(deadline)?;
        let _permit = transport.permit.try_acquire().map_err(|_| Error::Busy)?;
        let target = home_index_target(transport, bootstrap.region)?;
        report.phase = "native_home_index";
        report.home_attempts = 1;
        let response = transport
            .client
            .get(target)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .header(reqwest::header::AUTHORIZATION, authorization)
            .send()
            .await
            .map_err(request_error)?;
        report.home_status = Some(response.status().as_u16());
        let body = home_index_body(response).await?;
        report.phase = "schema";
        let hash = sha256(body.expose())?;
        check_deadline(deadline)?;
        report.body_sha256 = Some(hash);
        let schema = observe(body.expose())?;
        check_deadline(deadline)?;
        report.schema = Some(schema);
        Ok(())
    })
    .await
    .unwrap_or(Err(Error::Deadline))
    .and_then(|()| check_deadline(deadline));
    report.outcome = match result {
        Ok(()) => "admitted_index_experiment",
        Err(Error::HttpStatus(status)) => {
            if report.home_attempts == 0 {
                report.bootstrap_status = Some(status);
            }
            "http_status_refused"
        }
        Err(Error::Deadline) => "deadline",
        Err(Error::ResponseTooLarge) => "response_too_large",
        Err(Error::InvalidResponse) => "invalid_response",
        Err(Error::UnsupportedRegion) => "unsupported_region",
        Err(_) => "unavailable",
    };
    if result.is_err() {
        report.schema = None;
        if matches!(result, Err(Error::Deadline)) {
            report.body_sha256 = None;
        }
    }
    report
}

fn sha256(body: &[u8]) -> Result<String, Error> {
    use std::fmt::Write as _;
    let suite = rustls::crypto::aws_lc_rs::cipher_suite::TLS13_AES_128_GCM_SHA256
        .tls13()
        .ok_or(Error::Unavailable)?;
    let digest = suite.common.hash_provider.hash(body);
    let mut encoded = String::with_capacity(64);
    for byte in digest.as_ref() {
        write!(&mut encoded, "{byte:02x}").map_err(|_| Error::Unavailable)?;
    }
    Ok(encoded)
}

const MAX_REPORT: usize = 32 * 1024;
const PRIVATE_DIRECTORY: &str = "/criterion-lander-admission";

fn encoded_report(report: &Report) -> Result<Vec<u8>, &'static str> {
    let mut storage = [0_u8; MAX_REPORT];
    let mut writer = std::io::Cursor::new(&mut storage[..]);
    serde_json::to_writer(&mut writer, report).map_err(|_| "report_bound")?;
    let length = usize::try_from(writer.position()).map_err(|_| "report_bound")?;
    Ok(storage[..length].to_vec())
}

fn reserve_private_report(path: &std::path::Path) -> Result<std::fs::File, &'static str> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let directory = std::fs::symlink_metadata(path).map_err(|_| "private_directory")?;
    if !directory.is_dir() || directory.permissions().mode() & 0o777 != 0o700 {
        return Err("private_directory");
    }
    let mut attempt = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path.join("attempt.json"))
        .map_err(|_| "attempt_already_reserved")?;
    attempt.write_all(br#"{"experiment":"anonymous_native_home_index","max_bootstrap":1,"max_home":1,"state":"reserved_before_contact"}"#)
        .map_err(|_| "attempt_ledger")?;
    attempt.sync_all().map_err(|_| "attempt_ledger")?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path.join("report.json"))
        .map_err(|_| "report_already_reserved")
}

/// Root must compile offline, review the complete source, then invoke this exact
/// ignored test once with a new private0700 mount and a separate process watchdog.
/// Native Home supplies the observed identifier index. Provider delivery and a
/// production Home default remain unadmitted until their separate checks.
#[tokio::test(flavor = "current_thread")]
#[ignore = "Root-only anonymous provider experiment; fixed private mount and explicit separate execution grant"]
async fn live_anonymous_native_home_index_once() -> Result<(), &'static str> {
    let mut output = reserve_private_report(std::path::Path::new(PRIVATE_DIRECTORY))?;
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut report = match HttpTransport::new() {
        Ok(transport) => run_experiment(&transport, deadline).await,
        Err(_) => Report {
            outcome: "unavailable",
            ..Report::default()
        },
    };
    if check_deadline(deadline).is_err() {
        report.outcome = "deadline";
        report.body_sha256 = None;
        report.schema = None;
    }
    let encoded = encoded_report(&report)?;
    output.write_all(&encoded).map_err(|_| "report_write")?;
    output.sync_all().map_err(|_| "report_write")?;
    if report.outcome == "admitted_index_experiment" {
        Ok(())
    } else {
        Err("native_home_index_experiment_refused")
    }
}

fn check_deadline(deadline: Instant) -> Result<(), Error> {
    if Instant::now() >= deadline {
        Err(Error::Deadline)
    } else {
        Ok(())
    }
}

fn home_index_target(transport: &HttpTransport, region: Region) -> Result<url::Url, Error> {
    let target = account_target(region, "/content/lander/index")?;
    if let Some(origin) = &transport.test_origin {
        let mut target_fixture = origin.clone();
        target_fixture.set_path(target.path());
        return Ok(target_fixture);
    }
    Ok(target)
}

async fn home_index_body(mut response: reqwest::Response) -> Result<SecretBody, Error> {
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(Error::HttpStatus(status));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Err(Error::ResponseTooLarge);
    }
    if !json_media_type(response.headers())
        || !response
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
    let mut body = Zeroizing::new(Vec::with_capacity(MAX_BODY));
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        if chunk.len() > MAX_BODY - body.len() {
            return Err(Error::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return Err(Error::InvalidResponse);
    }
    Ok(SecretBody::new(std::mem::take(&mut *body)))
}

const BOOTSTRAP: &[u8] = br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
const PAGE: &[u8] = br#"{"page":{"name":"Synthetic home","longName":"Synthetic long title","blocks":[{"title":"PRIVATE-SCALAR","private-key":"hidden"}]}}"#;

struct Reply {
    status: u16,
    headers: &'static str,
    body: Vec<u8>,
    delay: Duration,
    body_delay: Duration,
    chunked: bool,
}
impl Reply {
    fn json(body: &[u8]) -> Self {
        Self {
            status: 200,
            headers: "",
            body: body.into(),
            delay: Duration::ZERO,
            body_delay: Duration::ZERO,
            chunked: false,
        }
    }
}

struct Server {
    origin: url::Url,
    roots: rustls::RootCertStore,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(reply: impl Fn(usize) -> Reply + Send + Sync + 'static) -> Self {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let certificate = cert.der().clone();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate.clone()).unwrap();
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::aws_lc_rs::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate],
                rustls::pki_types::PrivateKeyDer::Pkcs8(signing_key.serialize_der().into()),
            )
            .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin =
            url::Url::parse(&format!("https://{}", listener.local_addr().unwrap())).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_requests = requests.clone();
        let thread_stop = stop.clone();
        let reply = Arc::new(reply);
        let thread = std::thread::spawn(move || {
            let end = std::time::Instant::now() + Duration::from_secs(5);
            let mut workers = Vec::new();
            while !thread_stop.load(Ordering::Acquire) && std::time::Instant::now() < end {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let config = config.clone();
                        let requests = thread_requests.clone();
                        let reply = reply.clone();
                        let stop = thread_stop.clone();
                        workers.push(std::thread::spawn(move || {
                            let mut tls = rustls::StreamOwned::new(
                                rustls::ServerConnection::new(config).unwrap(), stream,
                            );
                            let mut head = Vec::new();
                            let mut byte = [0_u8; 1];
                            while head.len() < 32 * 1024 && !head.ends_with(b"\r\n\r\n") {
                                if tls.read_exact(&mut byte).is_err() { return; }
                                head.push(byte[0]);
                            }
                            let index = {
                                let mut requests = requests.lock().unwrap();
                                let index = requests.len();
                                requests.push(String::from_utf8(head).unwrap());
                                index
                            };
                            let reply = reply(index);
                            let wait = |delay| {
                                let end = std::time::Instant::now() + delay;
                                while std::time::Instant::now() < end {
                                    if stop.load(Ordering::Acquire) { return false; }
                                    std::thread::sleep(Duration::from_millis(2));
                                }
                                !stop.load(Ordering::Acquire)
                            };
                            if !wait(reply.delay) { return; }
                            let framing = if reply.chunked { "Transfer-Encoding: chunked\r\n".to_owned() }
                                else { format!("Content-Length: {}\r\n", reply.body.len()) };
                            let _ = write!(tls,
                                "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\n{}Connection: close\r\n{}\r\n",
                                reply.status, framing, reply.headers,
                            );
                            let _ = tls.flush();
                            if !wait(reply.body_delay) { return; }
                            if reply.chunked {
                                let _ = write!(tls, "{:x}\r\n", reply.body.len());
                                let _ = tls.write_all(&reply.body);
                                let _ = tls.write_all(b"\r\n0\r\n\r\n");
                            } else { let _ = tls.write_all(&reply.body); }
                            tls.conn.send_close_notify();
                            let _ = tls.flush();
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => panic!("local TLS accept"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            origin,
            roots,
            requests,
            stop,
            thread: Some(thread),
        }
    }
    fn transport(&self) -> HttpTransport {
        HttpTransport::for_test_roots(
            self.origin.clone(),
            self.roots.clone(),
            Duration::from_secs(2),
        )
        .unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn anonymous_native_home_index_uses_one_bootstrap_then_one_regional_get() {
    let server = Server::new(|index| Reply::json(if index == 0 { BOOTSTRAP } else { PAGE }));
    let result = run_experiment(&server.transport(), Instant::now() + Duration::from_secs(2)).await;
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "native Home index observation must issue exactly two GETs"
    );
    assert!(requests[0].starts_with("GET /api/init HTTP/1.1\r\n"));
    assert!(requests[1].starts_with("GET /api/ca/content/lander/index HTTP/1.1\r\n"));
    assert_eq!(result.experiment, "anonymous_native_home_index");
    assert_eq!(result.phase, "schema");
    assert_eq!(result.outcome, "admitted_index_experiment");
    assert_eq!(result.bootstrap_attempts, 1);
    assert_eq!(result.home_attempts, 1);
    assert_eq!(result.bootstrap_status, Some(200));
    assert_eq!(result.home_status, Some(200));
    for forbidden in [
        "authorization:",
        "x-auth-token:",
        "cookie:",
        "content-type:",
        "content-length:",
        "transfer-encoding:",
    ] {
        assert!(!requests[0].to_ascii_lowercase().contains(forbidden));
    }
    let home = requests[1].to_ascii_lowercase();
    assert!(home.contains("authorization: bearer synthetic-bootstrap\r\n"));
    assert_eq!(
        home.lines()
            .filter(|line| line.starts_with("authorization:"))
            .count(),
        1
    );
    assert!(home.contains("accept: application/json\r\n"));
    assert!(home.contains("accept-encoding: identity\r\n"));
    for forbidden in [
        "x-auth-token:",
        "cookie:",
        "content-type:",
        "content-length:",
        "transfer-encoding:",
    ] {
        assert!(!home.contains(forbidden));
    }
}

#[test]
fn schema_observer_counts_structure_without_values_or_unallowlisted_names() {
    let report = serde_json::to_value(observe(PAGE).unwrap()).unwrap();
    assert_eq!(report["object_fields"], 6);
    assert_eq!(report["unallowlisted_fields"], 2);
    assert_eq!(report["array_items"], 1);
    assert_eq!(report["fields"][0]["name"], "page");
    assert_eq!(report["fields"][1]["name"], "name");
    assert_eq!(report["fields"][2]["name"], "longName");
    assert_eq!(report["fields"][3]["name"], "blocks");
    let encoded = serde_json::to_string(&report).unwrap();
    for discarded in [
        "Synthetic home",
        "Synthetic long title",
        "PRIVATE-SCALAR",
        "private-key",
        "hidden",
        "title",
    ] {
        assert!(!encoded.contains(discarded));
    }
}

#[test]
fn pinned_existing_crypto_hashes_the_literal_sha256_vector() {
    assert_eq!(
        sha256(b"abc").unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn observer_bounds_all_unknown_structure_and_requires_one_complete_object() {
    for invalid in [b"[]".as_slice(), b"null", b"{} {}", b"{", b"{\"page\":NaN}"] {
        assert!(matches!(observe(invalid), Err(Error::InvalidResponse)));
    }
    let fields = format!("{{{}}}", vec!["\"unknown\":null"; MAX_FIELDS].join(","));
    assert_eq!(
        observe(fields.as_bytes()).unwrap().object_fields,
        MAX_FIELDS
    );
    let excess = format!("{{{}}}", vec!["\"unknown\":null"; MAX_FIELDS + 1].join(","));
    assert!(matches!(
        observe(excess.as_bytes()),
        Err(Error::InvalidResponse)
    ));
    let nodes = format!("{{\"blocks\":[{}]}}", vec!["null"; MAX_NODES - 2].join(","));
    assert_eq!(observe(nodes.as_bytes()).unwrap().nodes, MAX_NODES);
    let excess = format!("{{\"blocks\":[{}]}}", vec!["null"; MAX_NODES - 1].join(","));
    assert!(matches!(
        observe(excess.as_bytes()),
        Err(Error::InvalidResponse)
    ));
    let nested = |arrays| format!("{{\"page\":{}0{}}}", "[".repeat(arrays), "]".repeat(arrays));
    assert!(observe(nested(MAX_DEPTH - 2).as_bytes()).is_ok());
    assert!(matches!(
        observe(nested(MAX_DEPTH - 1).as_bytes()),
        Err(Error::InvalidResponse)
    ));
    assert!(matches!(
        observe(&vec![b' '; MAX_BODY + 1]),
        Err(Error::ResponseTooLarge)
    ));
}

#[test]
fn observer_never_retains_unknown_property_or_scalar_text_at_any_depth() {
    let body = br#"{"page":{"name":"SECRET-TOKEN","longName":null,"blocks":[{"SECRET-PROPERTY":{"nested":[true,42,"https://private.invalid/license?token=hidden"]}},{"name":false}]}}"#;
    let schema = observe(body).unwrap();
    assert_eq!(schema.nodes, 13);
    assert_eq!(schema.object_fields, 7);
    assert_eq!(schema.unallowlisted_fields, 2);
    assert_eq!(schema.fields[1].occurrences, 2);
    assert_eq!(schema.fields[1].types.string, 1);
    assert_eq!(schema.fields[1].types.boolean, 1);
    let encoded = serde_json::to_string(&schema).unwrap();
    for forbidden in [
        "SECRET",
        "private.invalid",
        "license",
        "token",
        "hidden",
        "nested",
        "42",
        "true",
        "false",
    ] {
        assert!(!encoded.contains(forbidden));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn expired_original_deadline_refuses_all_contact_and_schema_publication() {
    let server = Server::new(|_| Reply::json(BOOTSTRAP));
    let report = run_experiment(&server.transport(), Instant::now()).await;
    assert_eq!(report.outcome, "deadline");
    assert_eq!(report.bootstrap_attempts, 0);
    assert_eq!(report.home_attempts, 0);
    assert!(report.schema.is_none());
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn original_deadline_covers_bootstrap_then_home_body_without_reset() {
    let server = Server::new(|index| {
        let mut reply = Reply::json(if index == 0 { BOOTSTRAP } else { PAGE });
        if index == 0 {
            reply.delay = Duration::from_millis(60);
        } else {
            reply.body_delay = Duration::from_secs(1);
        }
        reply
    });
    let report = run_experiment(
        &server.transport(),
        Instant::now() + Duration::from_millis(350),
    )
    .await;
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert_eq!(report.home_status, Some(200));
    assert_eq!(report.outcome, "deadline");
    assert!(report.schema.is_none());
    assert!(report.body_sha256.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_or_failed_bootstrap_stops_without_home_or_retry() {
    for body in [b"{}".as_slice(), br#"{"country":"CA","token":"bad token","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#,
        br#"{"country":"CA","token":"synthetic","baseUrl":{"us":"https://attacker.invalid/api/us","ca":"https://mw.criterion.com/api/ca"}}"#] {
        let body = body.to_vec();
        let server = Server::new(move |_| Reply::json(&body));
        let report = run_experiment(&server.transport(), Instant::now() + Duration::from_secs(2)).await;
        assert_eq!(report.outcome, "invalid_response");
        assert_eq!(report.home_attempts, 0);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
    let server = Server::new(|_| {
        let mut reply = Reply::json(b"{}");
        reply.status = 302;
        reply.headers = "Location: https://127.0.0.1:9/never\r\n";
        reply
    });
    let report = run_experiment(&server.transport(), Instant::now() + Duration::from_secs(2)).await;
    assert_eq!(report.outcome, "http_status_refused");
    assert_eq!(report.bootstrap_status, Some(302));
    assert_eq!(report.home_attempts, 0);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_home_stops_without_redirect_new_refresh_or_retry() {
    for status in [302, 401, 403, 500] {
        let server = Server::new(move |index| {
            let mut reply = Reply::json(if index == 0 { BOOTSTRAP } else { b"{}" });
            if index == 1 {
                reply.status = status;
                reply.headers = "Location: https://127.0.0.1:9/never\r\n";
            }
            reply
        });
        let report =
            run_experiment(&server.transport(), Instant::now() + Duration::from_secs(2)).await;
        assert_eq!(report.outcome, "http_status_refused");
        assert_eq!(report.home_status, Some(status));
        assert!(report.schema.is_none());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn home_index_body_has_inclusive_512k_limit_for_known_and_chunked_lengths() {
    for chunked in [false, true] {
        for length in [MAX_BODY, MAX_BODY + 1] {
            let server = Server::new(move |index| {
                if index == 0 {
                    return Reply::json(BOOTSTRAP);
                }
                let mut body = PAGE.to_vec();
                body.resize(length, b' ');
                let mut reply = Reply::json(&body);
                reply.chunked = chunked;
                reply
            });
            let report =
                run_experiment(&server.transport(), Instant::now() + Duration::from_secs(2)).await;
            assert_eq!(
                report.outcome,
                if length == MAX_BODY {
                    "admitted_index_experiment"
                } else {
                    "response_too_large"
                }
            );
            assert_eq!(server.requests.lock().unwrap().len(), 2);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn home_index_admits_only_json_identity_and_never_sends_received_cookies() {
    for headers in [
        "Content-Encoding: gzip\r\n",
        "Content-Type: text/html\r\n",
        "",
    ] {
        let server = Server::new(move |index| {
            let mut reply = Reply::json(if index == 0 { BOOTSTRAP } else { PAGE });
            reply.headers = if index == 0 {
                "Set-Cookie: synthetic=private; Secure; Path=/\r\n"
            } else {
                headers
            };
            reply
        });
        let report =
            run_experiment(&server.transport(), Instant::now() + Duration::from_secs(2)).await;
        assert_eq!(
            report.outcome,
            if headers.is_empty() {
                "admitted_index_experiment"
            } else {
                "invalid_response"
            }
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(!requests[1].to_ascii_lowercase().contains("cookie:"));
    }
}

struct PrivateDirectory(std::path::PathBuf);
impl PrivateDirectory {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "criterion-lander-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn private_attempt_is_reserved_once_with_private_files_before_any_request() {
    use std::os::unix::fs::PermissionsExt;
    let directory = PrivateDirectory::new();
    let output = reserve_private_report(&directory.0).unwrap();
    drop(output);
    for name in ["attempt.json", "report.json"] {
        assert_eq!(
            std::fs::metadata(directory.0.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert!(matches!(
        reserve_private_report(&directory.0),
        Err("attempt_already_reserved")
    ));
    let ledger = std::fs::read_to_string(directory.0.join("attempt.json")).unwrap();
    assert!(ledger.contains("reserved_before_contact"));
    assert_eq!(
        std::fs::metadata(directory.0.join("report.json"))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn unsafe_directory_or_existing_report_refuses_before_provider_contact() {
    use std::os::unix::fs::PermissionsExt;
    let directory = PrivateDirectory::new();
    std::fs::set_permissions(&directory.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        reserve_private_report(&directory.0),
        Err("private_directory")
    ));
    assert!(!directory.0.join("attempt.json").exists());
    std::fs::set_permissions(&directory.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(directory.0.join("report.json"), "preserve fixture original").unwrap();
    assert!(matches!(
        reserve_private_report(&directory.0),
        Err("report_already_reserved")
    ));
    assert_eq!(
        std::fs::read_to_string(directory.0.join("report.json")).unwrap(),
        "preserve fixture original"
    );
    assert!(directory.0.join("attempt.json").exists());
}

#[test]
fn serialized_report_is_bounded_and_contains_only_coarse_observation() {
    let report = Report {
        outcome: "admitted_index_experiment",
        body_sha256: Some(sha256(PAGE).unwrap()),
        schema: Some(observe(PAGE).unwrap()),
        ..Report::default()
    };
    let encoded = encoded_report(&report).unwrap();
    assert!(encoded.len() <= MAX_REPORT);
    let text = std::str::from_utf8(&encoded).unwrap();
    for forbidden in [
        "PRIVATE-SCALAR",
        "private-key",
        "synthetic-bootstrap",
        "Synthetic home",
        "Synthetic long title",
    ] {
        assert!(!text.contains(forbidden));
    }
    let excessive = Report {
        body_sha256: Some("x".repeat(MAX_REPORT)),
        ..Report::default()
    };
    assert!(matches!(encoded_report(&excessive), Err("report_bound")));
}

#[tokio::test(flavor = "current_thread")]
async fn source_owned_us_base_is_selected_only_from_validated_bootstrap() {
    let server = Server::new(|index| {
        if index == 0 {
            Reply::json(br#"{"country":"US","token":"synthetic-us","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#)
        } else {
            Reply::json(PAGE)
        }
    });
    let report = run_experiment(&server.transport(), Instant::now() + Duration::from_secs(2)).await;
    assert_eq!(report.outcome, "admitted_index_experiment");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("GET /api/us/content/lander/index HTTP/1.1\r\n"));
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-us\r\n")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn normal_tls_roots_reject_fixture_certificate_before_http_delivery() {
    let server = Server::new(|_| Reply::json(BOOTSTRAP));
    let transport = HttpTransport::for_test(server.origin.clone(), Duration::from_secs(2)).unwrap();
    let report = run_experiment(&transport, Instant::now() + Duration::from_secs(2)).await;
    assert_eq!(report.outcome, "unavailable");
    assert!(server.requests.lock().unwrap().is_empty());
    assert_eq!(report.home_attempts, 0);
    assert!(report.schema.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn bounded_malformed_home_keeps_only_hash_status_and_refuses_schema() {
    let server = Server::new(|index| {
        Reply::json(if index == 0 {
            BOOTSTRAP
        } else {
            b"{PRIVATE-UNPARSED"
        })
    });
    let report = run_experiment(&server.transport(), Instant::now() + Duration::from_secs(2)).await;
    assert_eq!(report.outcome, "invalid_response");
    assert_eq!(report.home_status, Some(200));
    assert_eq!(
        report.body_sha256.as_deref(),
        Some("830f9340622765925331c78459bf64dcc8866d69c5639df970eacc9057ae2520")
    );
    assert!(report.schema.is_none());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert!(
        !String::from_utf8(encoded_report(&report).unwrap())
            .unwrap()
            .contains("PRIVATE-UNPARSED")
    );
}
