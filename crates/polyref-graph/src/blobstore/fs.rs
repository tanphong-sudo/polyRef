//! Filesystem-backed [`BlobStore`] implementation.
//!
//! Layout per ADR-006:
//!
//! ```text
//! <root>/blobs/sha256/<hash[:2]>/<hash>
//! ```
//!
//! - `<root>` is supplied by the caller (typically
//!   `.polyref/cache/`); the store creates the `blobs/sha256/` prefix
//!   on `open` if missing.
//! - `<hash[:2]>` is the first two hex chars of the SHA-256 digest;
//!   ~256 sub-directories keep any single dir small even at millions
//!   of blobs.
//! - `<hash>` is the full 64-char hex.
//!
//! # Atomic write
//!
//! Every `put` writes to a temp file in the same shard directory and
//! `persist()`s it to the final path. POSIX `rename(2)` is atomic on
//! the same filesystem, so a concurrent `get` either sees no file or
//! the full content — never a half-written one.
//!
//! # Counters
//!
//! `get` bumps `hits` or `misses`. `put` bumps `blobs_written` only
//! on the first write of a given key (idempotent put).
//!
//! # Security
//!
//! `path_for(key)` joins the validated 64-hex key under the cache
//! root. There is no other ingress to path construction.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use super::key::BlobKey;
use super::stats::{AtomicCacheStats, CacheStats};
use super::{BlobStore, BlobStoreError};

/// Subdirectory layout under the cache root.
const BLOBS_PREFIX: &str = "blobs/sha256";

/// Filesystem-backed [`BlobStore`].
///
/// Created via [`FsBlobStore::open`]; the directory is created if it
/// does not exist.
pub struct FsBlobStore {
    /// Absolute path to the cache root (e.g. `.polyref/cache/`).
    root: PathBuf,
    stats: AtomicCacheStats,
}

impl FsBlobStore {
    /// Open or create a blob store rooted at `root`. The
    /// `blobs/sha256/` subtree is created if missing.
    ///
    /// # Errors
    ///
    /// Returns [`BlobStoreError::Io`] when the directory cannot be
    /// created.
    pub fn open<P: Into<PathBuf>>(root: P) -> Result<Self, BlobStoreError> {
        let root = root.into();
        let prefix = root.join(BLOBS_PREFIX);
        fs::create_dir_all(prefix)?;
        Ok(Self {
            root,
            stats: AtomicCacheStats::default(),
        })
    }

    /// Path of the directory shard for a given key. Public-in-crate
    /// only; tests check that the layout matches the ADR-006 spec.
    pub(crate) fn shard_dir(&self, key: &BlobKey) -> PathBuf {
        self.root.join(BLOBS_PREFIX).join(key.shard())
    }

    /// Final path of a blob with the given key.
    pub(crate) fn path_for(&self, key: &BlobKey) -> PathBuf {
        self.shard_dir(key).join(key.to_hex())
    }
}

impl BlobStore for FsBlobStore {
    fn put(&self, content: &[u8]) -> Result<BlobKey, BlobStoreError> {
        let key = BlobKey::from_bytes(content);
        let target = self.path_for(&key);

        // Idempotent put: if the blob already exists, do nothing.
        // We don't bump `blobs_written` for a no-op write.
        if target.exists() {
            return Ok(key);
        }

        let shard = self.shard_dir(&key);
        fs::create_dir_all(&shard)?;

        // Write to a temp file in the SAME directory so the rename is
        // same-filesystem (POSIX atomic). `tempfile` cleans up the
        // file on drop if `persist` is not called.
        let mut tmp = NamedTempFile::new_in(&shard)?;
        tmp.write_all(content)?;
        tmp.flush()?;

        // `persist` returns an error wrapper; collapse to plain io.
        match tmp.persist(&target) {
            Ok(_) => {
                self.stats.record_write();
                Ok(key)
            }
            Err(persist_err) => {
                // If another writer raced us to the same path, the
                // file already exists — that's fine; we still return
                // the key, no double-count.
                if target.exists() {
                    Ok(key)
                } else {
                    Err(BlobStoreError::Io(persist_err.error))
                }
            }
        }
    }

    fn get(&self, key: &BlobKey) -> Result<Option<Vec<u8>>, BlobStoreError> {
        let target = self.path_for(key);
        match fs::read(target) {
            Ok(content) => {
                self.stats.record_hit();
                Ok(Some(content))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.stats.record_miss();
                Ok(None)
            }
            Err(e) => Err(BlobStoreError::Io(e)),
        }
    }

    fn has(&self, key: &BlobKey) -> Result<bool, BlobStoreError> {
        let target = self.path_for(key);
        match fs::metadata(target) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(BlobStoreError::Io(e)),
        }
    }

    fn stats(&self) -> CacheStats {
        self.stats.snapshot()
    }
}

// `path_for` and `shard_dir` are pub(crate) so layout-locking
// integration tests can assert the on-disk shape. They never accept
// a path from outside; only the validated 64-hex `BlobKey`.
//
// Send + Sync: PathBuf is Send + Sync; AtomicCacheStats is Sync; thus
// FsBlobStore is Send + Sync without any explicit impl.
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn _assert() {
        assert_send_sync::<FsBlobStore>();
    }
};

/// Manually expose the layout for `path_for`/`shard_dir` to the
/// integration test crate (a `pub` test would expose internals to
/// external consumers we don't want).
#[doc(hidden)]
pub fn _path_for_test<P: AsRef<Path>>(root: P, key: &BlobKey) -> PathBuf {
    root.as_ref()
        .join(BLOBS_PREFIX)
        .join(key.shard())
        .join(key.to_hex())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn open_creates_root_layout() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        assert!(dir.path().join(BLOBS_PREFIX).is_dir());
        // Counters start at zero.
        assert_eq!(store.stats(), CacheStats::zero());
    }

    #[test]
    fn put_get_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let key = store.put(b"hello").unwrap();
        let got = store.get(&key).unwrap();
        assert_eq!(got.as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn put_is_idempotent_does_not_double_write() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let k1 = store.put(b"hello").unwrap();
        let k2 = store.put(b"hello").unwrap();
        assert_eq!(k1, k2);
        let s = store.stats();
        assert_eq!(s.blobs_written, 1, "second put must not double-count");
    }

    #[test]
    fn distinct_content_distinct_keys_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let k1 = store.put(b"hello").unwrap();
        let k2 = store.put(b"world").unwrap();
        assert_ne!(k1, k2);
        let s = store.stats();
        assert_eq!(s.blobs_written, 2);
    }

    #[test]
    fn get_missing_returns_none_increments_misses() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let key = BlobKey::from_bytes(b"never written");
        assert!(store.get(&key).unwrap().is_none());
        let s = store.stats();
        assert_eq!(s.misses, 1);
        assert_eq!(s.hits, 0);
    }

    #[test]
    fn get_hit_increments_hits() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let key = store.put(b"hello").unwrap();
        assert!(store.get(&key).unwrap().is_some());
        assert!(store.get(&key).unwrap().is_some());
        let s = store.stats();
        assert_eq!(s.hits, 2);
        assert_eq!(s.misses, 0);
    }

    #[test]
    fn has_does_not_bump_counters() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let key = store.put(b"hello").unwrap();
        assert!(store.has(&key).unwrap());

        let missing = BlobKey::from_bytes(b"missing");
        assert!(!store.has(&missing).unwrap());

        let s = store.stats();
        assert_eq!(s.hits, 0, "has() must not increment hits");
        assert_eq!(s.misses, 0, "has() must not increment misses");
    }

    #[test]
    fn layout_matches_adr_006() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let key = store.put(b"layout fixture").unwrap();
        let expected = dir
            .path()
            .join(BLOBS_PREFIX)
            .join(key.shard())
            .join(key.to_hex());
        assert!(expected.is_file(), "blob must be at expected layout");
        assert_eq!(store.path_for(&key), expected);
    }

    #[test]
    fn empty_content_is_storable() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let key = store.put(b"").unwrap();
        let got = store.get(&key).unwrap().unwrap();
        assert_eq!(got, b"");
        // SHA-256 of empty input is a known value.
        assert_eq!(
            key.to_hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
