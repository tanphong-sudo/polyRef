//! Deterministic plugin response memoization for Layer 3.
//!
//! Memo keys are derived from canonical request bytes plus plugin binary digest,
//! method, and protocol version per ADR-002. Cached values are exact raw response
//! bytes so replay can validate byte identity before deserialization.

use crate::host::{PluginBinary, PluginHostError, PluginMethod};
use crate::limits::Limits;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Deterministic key for memoized plugin response bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginMemoKey(String);

/// In-memory plugin memo store for deterministic replay tests and adapters.
#[derive(Debug, Clone)]
pub struct PluginMemoStore {
    responses: BTreeMap<PluginMemoKey, Vec<u8>>,
    response_limit_bytes: usize,
}

impl PluginMemoKey {
    /// Build a memo key from canonical request bytes and plugin identity.
    #[must_use]
    pub fn new(
        method: PluginMethod,
        request_bytes: &[u8],
        binary: &PluginBinary,
        protocol_version: &str,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"polyref-plugin-memo-v1\0");
        hasher.update(protocol_version.as_bytes());
        hasher.update([0]);
        hasher.update(method.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(binary.digest().as_bytes());
        hasher.update([0]);
        hasher.update(request_bytes);
        Self(format!("{:x}", hasher.finalize()))
    }

    /// Parse a lowercase hex SHA-256 memo key.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::InvalidMemoKey`] when the input is not a
    /// 64-character lowercase hex digest.
    pub fn from_hex(input: impl Into<String>) -> Result<Self, PluginHostError> {
        let input = input.into();
        if input.len() != 64
            || !input
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(PluginHostError::InvalidMemoKey(
                "expected 64 hex characters".to_owned(),
            ));
        }
        Ok(Self(input))
    }

    /// Return the key as hex.
    #[must_use]
    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

impl Default for PluginMemoStore {
    fn default() -> Self {
        Self::with_response_limit(Limits::default().max_payload_bytes)
    }
}

impl PluginMemoStore {
    /// Create an empty memo store with a response byte cap.
    #[must_use]
    pub fn with_response_limit(response_limit_bytes: usize) -> Self {
        Self {
            responses: BTreeMap::new(),
            response_limit_bytes,
        }
    }

    /// Get exact cached response bytes.
    #[must_use]
    pub fn get(&self, key: &PluginMemoKey) -> Option<&[u8]> {
        self.responses.get(key).map(Vec::as_slice)
    }

    /// Insert exact response bytes.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::PayloadTooLarge`] when the response exceeds
    /// the configured cap.
    pub fn insert(&mut self, key: PluginMemoKey, response: Vec<u8>) -> Result<(), PluginHostError> {
        if response.len() > self.response_limit_bytes {
            return Err(PluginHostError::PayloadTooLarge {
                limit: self.response_limit_bytes,
                actual: response.len(),
            });
        }
        self.responses.insert(key, response);
        Ok(())
    }
}
