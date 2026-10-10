use super::{
    Db8Transport, FenceError, FenceState, WRITE_FENCE_ID, WRITE_FENCE_KIND, WriteReservation,
};
use serde::Deserialize;
use std::{sync::Arc, time::Instant};

pub(super) const MAX_REPLY: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record<'a> {
    #[serde(rename = "_id", borrow)]
    id: &'a str,
    #[serde(rename = "_kind", borrow)]
    kind: &'a str,
    #[serde(rename = "_rev")]
    revision: u64,
    version: u8,
    #[serde(rename = "possiblyIssued")]
    issued: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Get<'a> {
    #[serde(rename = "returnValue")]
    accepted: bool,
    #[serde(borrow)]
    results: Vec<Record<'a>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Changed<'a> {
    #[serde(borrow)]
    id: &'a str,
    rev: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Put<'a> {
    #[serde(rename = "returnValue")]
    accepted: bool,
    #[serde(borrow)]
    results: Vec<Changed<'a>>,
}
fn get(bytes: &[u8]) -> Result<(u64, bool), FenceError> {
    if bytes.len() > MAX_REPLY {
        return Err(FenceError::Invalid);
    }
    let reply: Get<'_> = serde_json::from_slice(bytes).map_err(|_| FenceError::Invalid)?;
    if !reply.accepted {
        return Err(FenceError::Unavailable);
    }
    let [record] = reply.results.as_slice() else {
        return Err(if reply.results.is_empty() {
            FenceError::Held
        } else {
            FenceError::Invalid
        });
    };
    if record.id != WRITE_FENCE_ID
        || record.kind != WRITE_FENCE_KIND
        || record.version != 1
        || record.revision == 0
    {
        return Err(FenceError::Invalid);
    }
    Ok((record.revision, record.issued))
}
fn put(bytes: &[u8], prior: u64) -> Result<u64, FenceError> {
    if bytes.len() > MAX_REPLY {
        return Err(FenceError::Invalid);
    }
    let reply: Put<'_> = serde_json::from_slice(bytes).map_err(|_| FenceError::Invalid)?;
    if !reply.accepted {
        return Err(FenceError::Unavailable);
    }
    let [record] = reply.results.as_slice() else {
        return Err(FenceError::Invalid);
    };
    if record.id != WRITE_FENCE_ID || record.rev <= prior {
        return Err(FenceError::Invalid);
    }
    Ok(record.rev)
}

pub(super) struct Store<T> {
    transport: T,
    held: Option<(Arc<()>, u64)>,
    unconfirmed: bool,
}
impl<T: Db8Transport> Store<T> {
    pub(super) fn new(transport: T) -> Self {
        Self {
            transport,
            held: None,
            unconfirmed: false,
        }
    }
    fn admitted(&self, deadline: Instant) -> Result<(), FenceError> {
        if self.unconfirmed || Instant::now() >= deadline {
            Err(FenceError::Unconfirmed)
        } else {
            Ok(())
        }
    }
    fn read(&mut self, deadline: Instant) -> Result<(u64, bool), FenceError> {
        self.admitted(deadline)?;
        let bytes = self.transport.get(deadline)?;
        self.admitted(deadline)?;
        get(&bytes)
    }
    pub(super) fn state(&mut self, deadline: Instant) -> Result<FenceState, FenceError> {
        self.admitted(deadline)?;
        if self.held.is_some() {
            return Ok(FenceState::PossiblyIssued);
        }
        match self.read(deadline) {
            Ok((_, false)) => Ok(FenceState::Clean),
            Ok((_, true)) => Ok(FenceState::PossiblyIssued),
            Err(error) => {
                self.unconfirmed = true;
                Err(error)
            }
        }
    }
    fn change(
        &mut self,
        revision: u64,
        issued: bool,
        deadline: Instant,
    ) -> Result<u64, FenceError> {
        self.admitted(deadline)?;
        // After this call may have been issued, any lost/rejected/malformed/late reply stays closed.
        let result = (|| {
            let bytes = self.transport.put(revision, issued, deadline)?;
            self.admitted(deadline)?;
            let changed = put(&bytes, revision)?;
            if self.read(deadline)? != (changed, issued) {
                return Err(FenceError::Unconfirmed);
            }
            Ok(changed)
        })();
        if result.is_err() {
            self.unconfirmed = true;
        }
        result.map_err(|_| FenceError::Unconfirmed)
    }
    pub(super) fn reserve(&mut self, deadline: Instant) -> Result<WriteReservation, FenceError> {
        self.admitted(deadline)?;
        if self.held.is_some() {
            return Err(FenceError::Held);
        }
        let (revision, issued) = self
            .read(deadline)
            .inspect_err(|_| self.unconfirmed = true)?;
        if issued {
            return Err(FenceError::Held);
        }
        let revision = self.change(revision, true, deadline)?;
        let brand = Arc::new(());
        self.held = Some((brand.clone(), revision));
        Ok(WriteReservation { brand, revision })
    }
    pub(super) fn complete(
        &mut self,
        reservation: WriteReservation,
        deadline: Instant,
    ) -> Result<(), FenceError> {
        self.admitted(deadline)?;
        let Some((brand, revision)) = &self.held else {
            return Err(FenceError::Invalid);
        };
        if !Arc::ptr_eq(brand, &reservation.brand) || *revision != reservation.revision {
            return Err(FenceError::Invalid);
        }
        self.change(reservation.revision, false, deadline)?;
        self.held = None;
        Ok(())
    }
}
