//! [`CacheStats`] — atomic hit/miss/write counters for a [`crate::BlobStore`].
//!
//! The Layer 1 acceptance gate (per `docs/verification.md`) calls for
//! "cache hit/miss counters wired". Counters are owned by the store
//! implementation; this module just provides the snapshot type and
//! the inner atomic helper used by `FsBlobStore`.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Snapshot of cache counters at a point in time.
///
/// Returned by [`crate::BlobStore::stats`]. Reads are atomic per
/// counter; the snapshot is not a consistent transaction across all
/// three (a concurrent `put` between the three loads would be
/// observable). For a strict snapshot wrap a single-thread reader at
/// a barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheStats {
    /// Number of `get` calls that returned a value.
    pub hits: u64,
    /// Number of `get` calls that returned `None`.
    pub misses: u64,
    /// Number of `put` calls that wrote a new blob to disk. `put`
    /// calls that hit an existing blob (idempotent put) do not bump
    /// this counter.
    pub blobs_written: u64,
}

impl CacheStats {
    /// All counters at zero.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            hits: 0,
            misses: 0,
            blobs_written: 0,
        }
    }
}

impl Default for CacheStats {
    fn default() -> Self {
        Self::zero()
    }
}

/// Internal atomic counters owned by the store. Not part of the
/// public API.
#[derive(Debug, Default)]
pub(crate) struct AtomicCacheStats {
    pub(crate) hits: AtomicU64,
    pub(crate) misses: AtomicU64,
    pub(crate) blobs_written: AtomicU64,
}

impl AtomicCacheStats {
    pub(crate) fn snapshot(&self) -> CacheStats {
        // `Relaxed` is sufficient: counters are advisory and not used
        // for synchronization. Layers above do their own locking.
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            blobs_written: self.blobs_written.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_write(&self) {
        self.blobs_written.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn cache_stats_zero_default() {
        let s = CacheStats::default();
        assert_eq!(s.hits, 0);
        assert_eq!(s.misses, 0);
        assert_eq!(s.blobs_written, 0);
    }

    #[test]
    fn atomic_increments_visible_in_snapshot() {
        let a = AtomicCacheStats::default();
        a.record_hit();
        a.record_hit();
        a.record_miss();
        a.record_write();
        let s = a.snapshot();
        assert_eq!(s.hits, 2);
        assert_eq!(s.misses, 1);
        assert_eq!(s.blobs_written, 1);
    }

    #[test]
    fn cache_stats_serde_round_trip() {
        let s = CacheStats {
            hits: 10,
            misses: 3,
            blobs_written: 7,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: CacheStats = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn atomic_increments_under_concurrent_threads() {
        use std::sync::Arc;
        use std::thread;

        let a = Arc::new(AtomicCacheStats::default());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let a = Arc::clone(&a);
            handles.push(thread::spawn(move || {
                for _ in 0..1_000 {
                    a.record_hit();
                    a.record_miss();
                    a.record_write();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let s = a.snapshot();
        assert_eq!(s.hits, 8_000);
        assert_eq!(s.misses, 8_000);
        assert_eq!(s.blobs_written, 8_000);
    }
}
