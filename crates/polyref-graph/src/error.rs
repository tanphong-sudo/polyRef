//! Errors emitted by the GraphStore.

use thiserror::Error;

/// Errors a [`crate::GraphStore`] implementation may return.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GraphStoreError {
    /// Backing SQLite returned an error.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// JSON (de)serialization for `observation.payload` failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// Canonical JSON hashing failed for an audit-safe payload.
    #[error("canonical json: {message}")]
    Canonical {
        /// Human-readable canonicalization failure.
        message: String,
    },

    /// A graph-layer validation invariant failed.
    #[error("graph validation: {message}")]
    Validation {
        /// Human-readable validation failure.
        message: String,
    },

    /// The schema version stored on disk is newer than this binary
    /// supports, or older than the minimum supported version.
    #[error("schema version {found} is unsupported (this binary supports {supported})")]
    UnsupportedSchemaVersion {
        /// Version recorded in the database.
        found: i64,
        /// Latest version this binary knows how to read.
        supported: i64,
    },

    /// A migration failed to apply.
    #[error("migration {version} failed: {source}")]
    Migration {
        /// Migration version that failed.
        version: i64,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },

    /// A `#[non_exhaustive]` enum from `polyref-core` carried a variant
    /// the GraphStore tag table does not know how to encode or decode.
    /// Adding a new variant in `polyref-core` therefore requires a paired
    /// update here.
    #[error("unsupported {enum_name} variant: {value}")]
    UnsupportedEnum {
        /// Name of the enum (e.g. `"ArtifactKind"`, `"Language"`).
        enum_name: &'static str,
        /// String form of the offending value.
        value: String,
    },

    /// A collection index could not be represented in the SQLite
    /// integer domain without loss.
    #[error("graph store position {position} overflows sqlite integer range")]
    PositionOverflow {
        /// Collection position that overflowed.
        position: usize,
    },

    /// The connection mutex was poisoned by a panic in another thread
    /// while it held the lock. Per `std::sync::Mutex` semantics the
    /// connection is in an unknown state; recovery requires a fresh
    /// `SqliteGraphStore`. We surface this as a typed variant rather
    /// than reusing a generic SQLite error.
    #[error("graph store mutex poisoned (a previous handler panicked while holding the lock)")]
    PoisonedLock,
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, GraphStoreError>;
