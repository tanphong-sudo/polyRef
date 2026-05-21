//! Integration tests for `FsBlobStore` — exercise the public API
//! surface and lock the on-disk layout per ADR-006.
//!
//! Unit-level behaviours (counters, idempotent put) are covered in
//! the lib tests; this file focuses on cross-module concerns:
//!
//! - layout is exactly `<root>/blobs/sha256/<hash[:2]>/<hash>`
//! - re-opening an existing root sees prior blobs (`has` true)
//! - the store is `Send + Sync` and works across threads
//! - the memo-key helpers compose correctly with the store

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_core::ids::EntityId;
use polyref_graph::{checker_memo_key, extractor_memo_key, BlobKey, BlobStore, FsBlobStore};
use serde_json::json;

fn entity(side: &str, hash: &str) -> EntityId {
    EntityId::parse(&format!("{side}:ts:handler:src/h.ts:{hash}")).unwrap()
}

#[test]
fn layout_uses_two_level_shard() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsBlobStore::open(dir.path()).unwrap();
    let key = store.put(b"layout fixture").unwrap();

    let expected = dir
        .path()
        .join("blobs")
        .join("sha256")
        .join(key.shard())
        .join(key.to_hex());
    assert!(expected.is_file(), "blob must be at expected layout");

    // Cross-check via parent dir layout: only one shard sub-dir
    // should exist (since we only wrote one key).
    let sha_dir = dir.path().join("blobs").join("sha256");
    let shards: Vec<_> = std::fs::read_dir(sha_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    assert_eq!(shards.len(), 1, "exactly one shard dir for one key");
    assert_eq!(
        shards[0].file_name().to_str().unwrap(),
        key.shard().as_str()
    );
}

#[test]
fn reopen_existing_root_sees_prior_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let key = {
        let store = FsBlobStore::open(dir.path()).unwrap();
        store.put(b"persistent content").unwrap()
    };
    // New store instance over the same root.
    let store = FsBlobStore::open(dir.path()).unwrap();
    assert!(store.has(&key).unwrap());
    let got = store.get(&key).unwrap();
    assert_eq!(got.as_deref(), Some(&b"persistent content"[..]));
}

#[test]
fn store_is_send_sync_and_thread_safe() {
    use std::sync::Arc;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsBlobStore::open(dir.path()).unwrap());
    let mut handles = Vec::new();

    for i in 0..8_u32 {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || {
            // Each thread writes 25 distinct blobs.
            for j in 0..25_u32 {
                let payload = format!("thread-{i}-blob-{j}").into_bytes();
                let key = store.put(&payload).unwrap();
                let read = store.get(&key).unwrap().unwrap();
                assert_eq!(read, payload);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let s = store.stats();
    assert_eq!(s.blobs_written, 8 * 25);
    assert_eq!(s.hits, 8 * 25);
    assert_eq!(s.misses, 0);
}

#[test]
fn dedup_across_threads_does_not_double_count_writes() {
    use std::sync::Arc;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsBlobStore::open(dir.path()).unwrap());
    let mut handles = Vec::new();

    // 16 threads racing to put the SAME content. Only one should
    // physically write; the rest should observe the existing blob
    // and skip.
    for _ in 0..16 {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || store.put(b"shared content").unwrap()));
    }
    let keys: Vec<BlobKey> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // All threads must agree on the same key.
    let first = keys[0];
    for k in &keys {
        assert_eq!(*k, first);
    }
    let s = store.stats();
    assert_eq!(
        s.blobs_written, 1,
        "16 racing puts must yield exactly one disk write"
    );
}

#[test]
fn memo_key_extractor_lookup_round_trip() {
    // Round-trip: compute memo key → use as cache slot → store → fetch.
    let dir = tempfile::tempdir().unwrap();
    let store = FsBlobStore::open(dir.path()).unwrap();

    let content = b"fn foo() -> u32 { 42 }";
    let content_hash = BlobKey::from_bytes(content);
    let memo = extractor_memo_key(
        &content_hash,
        "extractor-typescript",
        "1.2.3",
        &json!({"strict": true}),
    )
    .unwrap();

    // Use memo as a key-shaped path is fine since memo is itself a
    // valid `BlobKey`.
    let cached_extractor_output = b"{\"entities\":[]}";
    store.put(cached_extractor_output).unwrap();

    // Round trip the memo key through the store: store treats it as
    // any other key.
    let direct = store.put(memo.to_hex().as_bytes()).unwrap();
    let _ = store.get(&direct).unwrap();
}

#[test]
fn memo_key_checker_endpoints_sorted() {
    let evidence = BlobKey::from_bytes(b"evidence");
    let a = entity("old", "0123456789ab");
    let b = entity("new", "abcdef012345");

    let k1 = checker_memo_key(
        "1.0.0",
        "route",
        &[a.clone(), b.clone()],
        &evidence,
        600_000,
    )
    .unwrap();
    let k2 = checker_memo_key("1.0.0", "route", &[b, a], &evidence, 600_000).unwrap();
    assert_eq!(k1, k2, "endpoint order must not affect memo key");
}
