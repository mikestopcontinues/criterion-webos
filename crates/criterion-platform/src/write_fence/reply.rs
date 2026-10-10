use super::{FenceError, store::MAX_REPLY};

pub(super) fn admit(
    expected: u64,
    token: u64,
    sender: Option<&[u8]>,
    hub_error: bool,
    payload: Option<&[u8]>,
) -> Result<Vec<u8>, FenceError> {
    if expected == 0 || token != expected || sender != Some(b"com.palm.db") || hub_error {
        return Err(FenceError::Unconfirmed);
    }
    payload
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX_REPLY)
        .map(|bytes| bytes.to_vec())
        .ok_or(FenceError::Unconfirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_live_db8_sender_and_token_can_supply_bounded_payload() {
        let bytes = br#"{"returnValue":true,"results":[]}"#;
        assert_eq!(
            admit(7, 7, Some(b"com.palm.db"), false, Some(bytes)),
            Ok(bytes.to_vec())
        );
        for (expected, token, sender, hub_error, payload) in [
            (
                0,
                0,
                Some(b"com.palm.db".as_slice()),
                false,
                Some(bytes.as_slice()),
            ),
            (
                7,
                6,
                Some(b"com.palm.db".as_slice()),
                false,
                Some(bytes.as_slice()),
            ),
            (
                7,
                7,
                Some(b"other.db".as_slice()),
                false,
                Some(bytes.as_slice()),
            ),
            (7, 7, None, false, Some(bytes.as_slice())),
            (
                7,
                7,
                Some(b"com.palm.db".as_slice()),
                true,
                Some(bytes.as_slice()),
            ),
            (7, 7, Some(b"com.palm.db".as_slice()), false, None),
            (
                7,
                7,
                Some(b"com.palm.db".as_slice()),
                false,
                Some(b"".as_slice()),
            ),
        ] {
            assert_eq!(
                admit(expected, token, sender, hub_error, payload),
                Err(FenceError::Unconfirmed)
            );
        }
        assert_eq!(
            admit(7, 7, Some(b"com.palm.db"), false, Some(&vec![b' '; 4097])),
            Err(FenceError::Unconfirmed)
        );
    }
}
