//! Canonical-JSON helper.
//!
//! Slice 1 stub — the implementation is RFC 8785 (JCS). The choice of
//! source (in-house vs `serde_jcs`) is hard blocker F-5; until that
//! decision lands the bodies are `todo!()` and the tests in
//! `tests/canonical.rs` stay `#[ignore]`.

use thiserror::Error;

/// Hard cap on canonical-JSON payload size (bytes). Per F-6.
pub const PAYLOAD_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Hard cap on JSON nesting depth. Per F-6.
pub const PAYLOAD_MAX_DEPTH: usize = 64;

/// Errors emitted by [`canonicalize`].
#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum CanonicalError {
    /// Input exceeded [`PAYLOAD_MAX_BYTES`].
    #[error("payload exceeds {} bytes", PAYLOAD_MAX_BYTES)]
    Oversize,
    /// Input nesting exceeds [`PAYLOAD_MAX_DEPTH`].
    #[error("payload exceeds depth {}", PAYLOAD_MAX_DEPTH)]
    TooDeep,
    /// Input contains a NaN or infinity.
    #[error("payload contains non-finite number")]
    NonFinite,
    /// Other RFC 8785 violation.
    #[error("canonical JSON error: {0}")]
    Other(&'static str),
}

/// Canonicalize a JSON value per RFC 8785.
///
/// Slice 1: not implemented. Returns `Err(CanonicalError::Other("F-5
/// open"))` once F-5 is closed; until then callers should not depend
/// on a successful return.
pub fn canonicalize(_value: &serde_json::Value) -> Result<Vec<u8>, CanonicalError> {
    todo!("hard blocker F-5: choose RFC 8785 implementation source")
}
