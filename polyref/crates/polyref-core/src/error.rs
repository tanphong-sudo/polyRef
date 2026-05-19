//! Crate-wide error type.
//!
//! Slice 1 stub. Variant set will grow as parsers and constructors are
//! implemented; existing variants are stable.

use thiserror::Error;

/// Top-level error for `polyref-core` operations that can fail at the
/// type-construction boundary.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CoreError {
    /// An id (`EntityId`, `ArtifactId`, `CorrId`, `EdgeId`) failed to
    /// parse against its ADR-003 grammar.
    #[error("invalid id: {0}")]
    InvalidId(String),

    /// A `SourceSpan` was constructed with an inverted or otherwise
    /// invalid range.
    #[error("invalid source span: {0}")]
    InvalidSpan(&'static str),

    /// A canonical-JSON serialization failed (size cap, NaN, depth cap).
    #[error("canonical JSON error: {0}")]
    Canonical(&'static str),

    /// A report-assembly invariant was violated. See
    /// [`crate::report::ReportInvariantError`].
    #[error("report invariant violated: {0}")]
    ReportInvariant(String),
}
