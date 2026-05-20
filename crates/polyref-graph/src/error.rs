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
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, GraphStoreError>;
