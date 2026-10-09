//! Fixed disposable application-to-service admission. No account or storage implementation.
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::{fmt, marker::PhantomData};

pub const APP_ID: &str = "com.mikestopcontinues.criterion.probe.native";
pub const SERVICE_ID: &str = "com.mikestopcontinues.criterion.probe.player.bridge";
pub const KEYMANAGER_ID: &str = "com.webos.service.keymanager3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAdmission {
    MissingKey,
    ExistingPublicKey,
}

pub fn admit_keymanager(
    source: Option<&str>,
    token: u64,
    expected_token: u64,
    payload: &[u8],
) -> Result<KeyAdmission, Failure> {
    identity(source, KEYMANAGER_ID, token, expected_token)?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Missing<'a> {
        return_value: bool,
        error_code: i32,
        error_text: &'a str,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Public<'a> {
        return_value: bool,
        pubkey: &'a str,
    }
    if object::<Returned>(payload)?.return_value {
        let wire: Public<'_> = object(payload)?;
        if wire.return_value
            && !wire.pubkey.is_empty()
            && wire.pubkey.len() <= 3072
            && wire
                .pubkey
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        {
            // The public value is borrowed for validation and never retained or reported.
            Ok(KeyAdmission::ExistingPublicKey)
        } else {
            Err(Failure::InvalidResponse)
        }
    } else {
        let wire: Missing<'_> = object(payload)?;
        if !wire.return_value && wire.error_code == -10001 && wire.error_text == "Key not found" {
            Ok(KeyAdmission::MissingKey)
        } else {
            Err(Failure::ServiceRejected)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    InvalidResponse,
    WrongSource,
    WrongToken,
    HubRejected,
    ServiceRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub pid: u32,
    pub counter: u32,
    pub subscribers: u8,
    pub state: State,
    pub stop_acknowledged: bool,
    pub cleanup_confirmed: bool,
    pub exit_code: Option<i32>,
}

impl Snapshot {
    pub fn running(self) -> bool {
        self.state == State::Running
            && !self.cleanup_confirmed
            && !self.stop_acknowledged
            && self.exit_code.is_none()
            && self.subscribers > 0
    }
    pub fn pinged_after(self, initial: Self) -> bool {
        self.running()
            && initial.running()
            && self.pid == initial.pid
            && self.counter > initial.counter
    }
}

/// A close reply owns completion until confirmed retirement or its original deadline.
pub struct CloseWait {
    pid: Option<u32>,
    counter: u32,
    deadline_ms: u64,
    retired: bool,
}
impl CloseWait {
    pub fn new(baseline: Option<Snapshot>, deadline_ms: u64) -> Self {
        Self {
            pid: baseline.map(|reply| reply.pid),
            counter: baseline.map_or(0, |reply| reply.counter),
            deadline_ms,
            retired: false,
        }
    }
    pub fn observe(&mut self, now_ms: u64, reply: Result<Snapshot, Failure>) {
        if now_ms >= self.deadline_ms {
            return;
        }
        if let Ok(reply) = reply {
            self.retired |= self.pid.is_none_or(|pid| pid == reply.pid)
                && reply.counter >= self.counter
                && reply.state == State::Closed
                && reply.subscribers == 0
                && reply.stop_acknowledged
                && reply.cleanup_confirmed
                && reply.exit_code == Some(0);
        }
    }
    pub fn complete(&self, now_ms: u64) -> bool {
        self.retired || now_ms >= self.deadline_ms
    }
    pub fn retired(&self) -> bool {
        self.retired
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Running,
    Closed,
    Failed,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireSnapshot<'a> {
    return_value: bool,
    version: &'a str,
    state: State,
    counter: u32,
    pid: u32,
    subscribers: u8,
    stop_acknowledged: bool,
    cleanup_confirmed: bool,
    exit_code: Exit,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Returned {
    return_value: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rejected<'a> {
    return_value: bool,
    error_code: &'a str,
}
struct Exit(Option<i32>);
impl<'de> Deserialize<'de> for Exit {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Option::<i32>::deserialize(de).map(Self)
    }
}

struct Object<T>(T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct Map<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Map<T> {
            type Value = Object<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("object")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(Object)
            }
        }
        de.deserialize_map(Map(PhantomData))
    }
}

fn object<'a, T: Deserialize<'a>>(payload: &'a [u8]) -> Result<T, Failure> {
    if payload.is_empty() || payload.len() > 4096 {
        return Err(Failure::InvalidResponse);
    }
    let mut de = serde_json::Deserializer::from_slice(payload);
    let Object(value) = Object::<T>::deserialize(&mut de).map_err(|_| Failure::InvalidResponse)?;
    de.end().map_err(|_| Failure::InvalidResponse)?;
    Ok(value)
}

pub fn admit_bridge(
    source: Option<&str>,
    token: u64,
    expected_token: u64,
    payload: &[u8],
) -> Result<Snapshot, Failure> {
    identity(source, SERVICE_ID, token, expected_token)?;
    if !object::<Returned>(payload)?.return_value {
        let wire: Rejected<'_> = object(payload)?;
        return Err(
            if !wire.return_value
                && matches!(
                    wire.error_code,
                    "unauthorized"
                        | "invalidRequest"
                        | "versionMismatch"
                        | "busy"
                        | "unavailable"
                        | "closed"
                        | "cleanupUnconfirmed"
                )
            {
                Failure::ServiceRejected
            } else {
                Failure::InvalidResponse
            },
        );
    }
    let wire: WireSnapshot<'_> = object(payload)?;
    if !wire.return_value
        || wire.version != "0.1.0"
        || wire.pid == 0
        || wire.pid > i32::MAX as u32
        || wire.counter > 1_000_000
        || wire.subscribers > 3
    {
        return Err(Failure::InvalidResponse);
    }
    Ok(Snapshot {
        pid: wire.pid,
        counter: wire.counter,
        subscribers: wire.subscribers,
        state: wire.state,
        stop_acknowledged: wire.stop_acknowledged,
        cleanup_confirmed: wire.cleanup_confirmed,
        exit_code: wire.exit_code.0,
    })
}

fn identity(
    source: Option<&str>,
    expected_source: &str,
    token: u64,
    expected_token: u64,
) -> Result<(), Failure> {
    if token == 0 || expected_token == 0 || token != expected_token {
        return Err(Failure::WrongToken);
    }
    if source != Some(expected_source) {
        return Err(Failure::WrongSource);
    }
    Ok(())
}
