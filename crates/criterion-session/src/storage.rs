use crate::{CLIENT_ID, Error, ISSUER, Secret, SecretBody};
use serde::{Deserialize, Serialize};
use std::future::Future;

/// Opaque, versioned refresh-token checkpoint bound to the public registration.
/// Access tokens, device codes and activation instructions are never persisted.
#[derive(Debug)]
pub struct StoredSession {
    pub(crate) refresh_token: Secret,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    issuer: Secret,
    client_id: Secret,
    refresh_token: Secret,
}
#[derive(Serialize)]
struct BorrowedRecord<'a> {
    version: u8,
    issuer: &'a str,
    client_id: &'a str,
    refresh_token: &'a str,
}

impl StoredSession {
    /// Decode only after the platform store authenticates/decrypts its payload.
    pub fn decode(payload: SecretBody) -> Result<Self, Error> {
        if payload.expose().len() > crate::wire::MAX_BODY {
            return Err(Error::InvalidResponse);
        }
        let record: Record = crate::wire::deserialize_object(payload.expose())?;
        if record.version != 1
            || record.issuer.expose() != ISSUER
            || record.client_id.expose() != CLIENT_ID
            || !record.refresh_token.valid(crate::wire::MAX_TOKEN)
        {
            return Err(Error::InvalidResponse);
        }
        Ok(Self {
            refresh_token: record.refresh_token,
        })
    }

    /// Borrowed plaintext exists only for the admitted platform encryption seam.
    /// The caller must keep the resulting owned buffer private and zeroizing.
    pub fn encode(&self) -> SecretBody {
        // The serializer writes into the final zeroizing allocation, so owned
        // plaintext copies created here are erased when their owner is dropped.
        let mut output = zeroize::Zeroizing::new(Vec::with_capacity(crate::wire::MAX_BODY));
        serde_json::to_writer(
            &mut *output,
            &BorrowedRecord {
                version: 1,
                issuer: ISSUER,
                client_id: CLIENT_ID,
                refresh_token: self.refresh_token.expose(),
            },
        )
        .expect("writing JSON to a Vec cannot fail");
        SecretBody::new(std::mem::take(&mut *output))
    }
}

/// The platform owner admits and implements this contract; no backend is
/// provided. Payloads require authenticated encryption and restricted access.
/// Operations must be serialized, bounded, atomic and durably complete before
/// returning success. `take` removes the saved record before returning it,
/// preventing replay after a crash or uncertain refresh completion. On any
/// ambiguous storage result, the owner must block restore and require sign-in.
pub trait SecureSessionStore: Send {
    fn take(&mut self) -> impl Future<Output = Result<Option<StoredSession>, Error>> + Send;
    fn replace(&mut self, session: StoredSession)
    -> impl Future<Output = Result<(), Error>> + Send;
    fn clear(&mut self) -> impl Future<Output = Result<(), Error>> + Send;
}
