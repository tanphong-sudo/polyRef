//! SPI-level errors.

use thiserror::Error;

/// Errors at the SPI envelope or limit level.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SpiError {
    /// JSON parse failure.
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),

    /// Envelope failed structural validation.
    #[error("invalid envelope: {0}")]
    Envelope(#[from] crate::envelope::JsonRpcEnvelopeError),

    /// A limit was exceeded.
    #[error("limit exceeded: {0}")]
    Limit(#[from] crate::limits::LimitsError),

    /// A safe path was malformed.
    #[error("invalid safe path: {0}")]
    Path(#[from] crate::limits::SafePathError),
}
