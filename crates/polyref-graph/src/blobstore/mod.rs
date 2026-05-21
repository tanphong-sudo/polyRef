//! Content-addressed blob store per ADR-006.
//!
//! Stores extractor outputs, raw checker logs, replay artifacts. The
//! key is a [`BlobKey`] (SHA-256 of content), so the same content
//! always lives at the same path:
//!
//! ```text
//! .polyref/cache/blobs/sha256/<hash[:2]>/<hash>
//! ```
//!
//! The two-level shard avoids a single directory with millions of
//! files (POSIX directory lookup degrades).
//!
//! # Trait split
//!
//! Layer 1 ships only the filesystem implementation. The trait is
//! kept narrow (`put` / `get` / `has` / `stats`) so an in-memory
//! variant can land later for property tests without changing the
//! call surface.
//!
//! # Counters
//!
//! [`CacheStats`] is the Layer 1 acceptance gate: every `get` bumps
//! either `hits` or `misses`; every successful `put` of a new blob
//! bumps `blobs_written`. Counters are atomic; reading [`CacheStats`]
//! is a snapshot.
//!
//! # Security
//!
//! - [`BlobKey::parse`] rejects anything but 64 lowercase hex chars,
//!   so a malicious key can never escape the cache root.
//! - Writes are atomic via `tempfile::NamedTempFile::persist` — a
//!   reader observing a path either sees the full content or no file.
//! - No `unsafe`. Workspace-wide `#![forbid(unsafe_code)]`.

mod fs;
mod key;
mod stats;

pub use fs::FsBlobStore;
pub use key::{BlobKey, BlobKeyError};
pub use stats::CacheStats;

use thiserror::Error;

/// Persistent content-addressed blob store.
///
/// Implementations must be `Send + Sync` so the bounded plugin pool
/// (Layer 3) can share a single store across worker threads.
pub trait BlobStore: Send + Sync {
    /// Hash `content` and store it. Returns the resulting [`BlobKey`].
    /// Idempotent: storing the same content twice returns the same
    /// key and does not re-write the file.
    ///
    /// # Errors
    ///
    /// Returns [`BlobStoreError::Io`] when the underlying filesystem
    /// or temp-file operation fails.
    fn put(&self, content: &[u8]) -> Result<BlobKey, BlobStoreError>;

    /// Read the blob with the given `key`. Returns `None` if no blob
    /// exists for this key.
    ///
    /// # Errors
    ///
    /// Returns [`BlobStoreError::Io`] for any filesystem error other
    /// than "not found".
    fn get(&self, key: &BlobKey) -> Result<Option<Vec<u8>>, BlobStoreError>;

    /// Cheap existence probe. Does not bump counters.
    ///
    /// # Errors
    ///
    /// Returns [`BlobStoreError::Io`] for any filesystem error other
    /// than "not found".
    fn has(&self, key: &BlobKey) -> Result<bool, BlobStoreError>;

    /// Snapshot of the cache counters at this instant. Counters are
    /// updated atomically; no two snapshots are guaranteed to satisfy
    /// `hits + misses = sum(get_calls)` if observed mid-flight.
    fn stats(&self) -> CacheStats;
}

/// Failure to read or write a blob.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BlobStoreError {
    /// Underlying I/O error from the host filesystem.
    #[error("blobstore io error: {0}")]
    Io(#[from] std::io::Error),

    /// A `BlobKey` failed validation (should never happen for keys we
    /// produce ourselves, but `parse()` errors bubble up here).
    #[error("blobstore key error: {0}")]
    Key(#[from] BlobKeyError),
}
