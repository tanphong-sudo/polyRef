//! [`BlobKey`] — newtype wrapping a 64-char lowercase hex SHA-256 digest.
//!
//! Same security pattern as `polyref-core::ids::EntityId`: private
//! inner storage, parse-only ingress (no `From<String>`), serde routes
//! through `parse`. The 64-char form is the only canonical
//! representation; raw byte access is exposed for hashing chains.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;
use thiserror::Error;

/// Length of the canonical hex form, in characters.
pub const BLOB_KEY_HEX_LEN: usize = 64;

/// Length of the raw digest, in bytes.
pub const BLOB_KEY_DIGEST_LEN: usize = 32;

/// Failure to parse a [`BlobKey`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlobKeyError {
    /// Wrong length. SHA-256 hex is exactly 64 chars.
    #[error("blob key has wrong length: {0} (expected {BLOB_KEY_HEX_LEN})")]
    WrongLength(usize),

    /// A non-lowercase-hex byte appeared in the input.
    #[error("blob key contains non-lowercase-hex byte: {0:?}")]
    NonHex(char),
}

/// Content-addressed key for a blob in the [`crate::BlobStore`].
///
/// Constructed only via [`Self::from_bytes`] (compute the hash) or
/// [`Self::parse`] (validate an existing hex string). Inner state is
/// the 32-byte digest; the hex form is computed on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlobKey {
    digest: [u8; BLOB_KEY_DIGEST_LEN],
}

impl BlobKey {
    /// Compute the SHA-256 of `content` and wrap it.
    #[must_use]
    pub fn from_bytes(content: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let arr: [u8; BLOB_KEY_DIGEST_LEN] = hasher.finalize().into();
        Self { digest: arr }
    }

    /// Parse a 64-char lowercase hex string.
    ///
    /// # Errors
    ///
    /// - [`BlobKeyError::WrongLength`] if `s` is not exactly 64 chars.
    /// - [`BlobKeyError::NonHex`] for any byte that isn't `0-9` or
    ///   `a-f` (uppercase is rejected — the canonical form is
    ///   lowercase).
    pub fn parse(s: &str) -> Result<Self, BlobKeyError> {
        if s.len() != BLOB_KEY_HEX_LEN {
            return Err(BlobKeyError::WrongLength(s.len()));
        }
        let mut digest = [0_u8; BLOB_KEY_DIGEST_LEN];
        let bytes = s.as_bytes();
        for i in 0..BLOB_KEY_DIGEST_LEN {
            let hi = decode_nibble(bytes[2 * i])?;
            let lo = decode_nibble(bytes[2 * i + 1])?;
            digest[i] = (hi << 4) | lo;
        }
        Ok(Self { digest })
    }

    /// Borrow the canonical 64-char lowercase hex view.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(BLOB_KEY_HEX_LEN);
        for byte in self.digest {
            out.push(NIBBLE_TO_HEX[(byte >> 4) as usize] as char);
            out.push(NIBBLE_TO_HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }

    /// Borrow the raw 32-byte digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; BLOB_KEY_DIGEST_LEN] {
        &self.digest
    }

    /// First two hex characters of the digest. Used by
    /// [`crate::FsBlobStore`] as a directory shard so a single dir
    /// never holds more than ~1/256 of all blobs.
    #[must_use]
    pub fn shard(&self) -> String {
        let high = NIBBLE_TO_HEX[(self.digest[0] >> 4) as usize] as char;
        let low = NIBBLE_TO_HEX[(self.digest[0] & 0x0f) as usize] as char;
        format!("{high}{low}")
    }
}

const NIBBLE_TO_HEX: &[u8; 16] = b"0123456789abcdef";

fn decode_nibble(b: u8) -> Result<u8, BlobKeyError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        // Uppercase is intentionally rejected — canonical form is
        // lowercase, otherwise a single content has two valid keys.
        other => Err(BlobKeyError::NonHex(other as char)),
    }
}

impl fmt::Display for BlobKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for BlobKey {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for BlobKey {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        BlobKey::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn h(byte: u8) -> String {
        std::iter::repeat(byte as char)
            .take(BLOB_KEY_HEX_LEN)
            .collect()
    }

    #[test]
    fn from_bytes_is_deterministic() {
        let k1 = BlobKey::from_bytes(b"hello");
        let k2 = BlobKey::from_bytes(b"hello");
        assert_eq!(k1, k2);
    }

    #[test]
    fn from_bytes_distinguishes_content() {
        let k1 = BlobKey::from_bytes(b"hello");
        let k2 = BlobKey::from_bytes(b"world");
        assert_ne!(k1, k2);
    }

    #[test]
    fn from_bytes_matches_known_sha256_of_empty() {
        // RFC: SHA-256 of empty input.
        let k = BlobKey::from_bytes(b"");
        assert_eq!(
            k.to_hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn parse_accepts_64_lowercase_hex() {
        let s = h(b'a');
        let k = BlobKey::parse(&s).unwrap();
        assert_eq!(k.to_hex(), s);
    }

    #[test]
    fn parse_rejects_short() {
        assert!(matches!(
            BlobKey::parse("deadbeef"),
            Err(BlobKeyError::WrongLength(8))
        ));
    }

    #[test]
    fn parse_rejects_long() {
        let s = format!("{}{}", h(b'a'), 'a');
        assert!(matches!(
            BlobKey::parse(&s),
            Err(BlobKeyError::WrongLength(65))
        ));
    }

    #[test]
    fn parse_rejects_uppercase_hex() {
        let s = h(b'A');
        assert!(matches!(BlobKey::parse(&s), Err(BlobKeyError::NonHex(_))));
    }

    #[test]
    fn parse_rejects_non_hex_byte() {
        let mut s = h(b'a');
        // Replace last char with 'g'.
        s.pop();
        s.push('g');
        assert!(matches!(BlobKey::parse(&s), Err(BlobKeyError::NonHex('g'))));
    }

    #[test]
    fn parse_rejects_unicode() {
        // Multi-byte char makes the byte length != 64 even if the
        // char count is 64.
        let mut s = String::new();
        for _ in 0..64 {
            s.push('é');
        }
        assert!(BlobKey::parse(&s).is_err());
    }

    #[test]
    fn shard_is_first_two_hex_chars() {
        let k = BlobKey::parse(&h(b'a')).unwrap();
        assert_eq!(k.shard(), "aa");

        // Different first byte → different shard.
        let mut s = h(b'a');
        // Replace first 2 chars with "5f".
        s.replace_range(0..2, "5f");
        let k = BlobKey::parse(&s).unwrap();
        assert_eq!(k.shard(), "5f");
    }

    #[test]
    fn as_bytes_returns_32_byte_digest() {
        let k = BlobKey::from_bytes(b"hello");
        let bytes: &[u8; BLOB_KEY_DIGEST_LEN] = k.as_bytes();
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn serde_round_trip_through_hex() {
        let k = BlobKey::from_bytes(b"hello");
        let json = serde_json::to_string(&k).unwrap();
        // Stored as a JSON string, not a byte array.
        assert!(json.starts_with('"'));
        assert!(json.ends_with('"'));
        let back: BlobKey = serde_json::from_str(&json).unwrap();
        assert_eq!(k, back);
    }

    #[test]
    fn serde_does_not_bypass_parse() {
        let bad = "\"NOT A VALID HEX KEY\"";
        let result: Result<BlobKey, _> = serde_json::from_str(bad);
        assert!(
            result.is_err(),
            "deserialization must route through BlobKey::parse"
        );
    }

    #[test]
    fn display_emits_canonical_hex() {
        let k = BlobKey::from_bytes(b"hello");
        assert_eq!(format!("{k}"), k.to_hex());
    }
}
