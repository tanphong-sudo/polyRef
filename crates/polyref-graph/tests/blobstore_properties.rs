//! Property-based tests for `FsBlobStore`.
//!
//! Per `docs/verification.md` Layer 1, properties pin the invariants
//! that one-line "improvements" could silently break.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_graph::{BlobKey, BlobStore, FsBlobStore};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        ..ProptestConfig::default()
    })]

    /// Round-trip identity: any byte sequence can be stored and
    /// retrieved unchanged.
    #[test]
    fn prop_put_get_round_trip(content in prop::collection::vec(any::<u8>(), 0..=4096)) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let key = store.put(&content).unwrap();
        let got = store.get(&key).unwrap().unwrap();
        prop_assert_eq!(got, content);
    }

    /// Idempotent put: same content always yields the same key, and
    /// repeated `put` calls do not bump `blobs_written` past 1.
    #[test]
    fn prop_dedup_same_content_idempotent(
        content in prop::collection::vec(any::<u8>(), 0..=2048),
        n in 2usize..=8,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();

        let mut keys = Vec::new();
        for _ in 0..n {
            keys.push(store.put(&content).unwrap());
        }
        let first = keys[0];
        for k in &keys {
            prop_assert_eq!(*k, first);
        }
        let s = store.stats();
        prop_assert_eq!(s.blobs_written, 1);
    }

    /// Distinct content always yields distinct keys (collision
    /// resistance is a SHA-256 property, but this checks our wiring
    /// doesn't accidentally truncate or normalize).
    #[test]
    fn prop_distinct_content_distinct_keys(
        a in prop::collection::vec(any::<u8>(), 1..=1024),
        b in prop::collection::vec(any::<u8>(), 1..=1024),
    ) {
        prop_assume!(a != b);
        let ka = BlobKey::from_bytes(&a);
        let kb = BlobKey::from_bytes(&b);
        prop_assert_ne!(ka, kb);
    }

    /// `has` agrees with `get` for any sequence of operations: after
    /// a `put(content)`, both `has(key)` is true and `get(key)`
    /// yields `Some(content)`.
    #[test]
    fn prop_has_agrees_with_get(content in prop::collection::vec(any::<u8>(), 0..=2048)) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let key = store.put(&content).unwrap();
        prop_assert!(store.has(&key).unwrap());
        prop_assert!(store.get(&key).unwrap().is_some());
    }
}
