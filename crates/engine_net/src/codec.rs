//! MessagePack codec helpers.
//!
//! Thin wrappers around `rmp-serde` for encoding and decoding messages. All
//! network payloads use MessagePack for compact binary serialisation.

use serde::{Deserialize, Serialize};

use crate::error::NetError;

/// Encode a value to MessagePack bytes.
///
/// # Errors
///
/// Returns [`NetError::Encode`] if serialisation fails.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, NetError> {
    rmp_serde::to_vec(value).map_err(NetError::Encode)
}

/// Decode a value from MessagePack bytes.
///
/// # Errors
///
/// Returns [`NetError::Decode`] if deserialisation fails.
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, NetError> {
    rmp_serde::from_slice(bytes).map_err(NetError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct TestMsg {
        value: u32,
        name: String,
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let msg = TestMsg {
            value: 42,
            name: "hello".to_string(),
        };
        let bytes = encode(&msg).unwrap();
        let restored: TestMsg = decode(&bytes).unwrap();
        assert_eq!(msg, restored);
    }

    #[test]
    fn test_decode_invalid_bytes() {
        let result: Result<TestMsg, _> = decode(&[0xFF, 0xFF]);
        assert!(result.is_err());
    }
}
