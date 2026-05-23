//! Layer 3 host memoization contract tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use polyref_checker_spi::host::{
    encode_request_payload, PluginBinary, PluginMemoKey, PluginMemoStore, PluginMethod,
    PluginRequestId,
};
use polyref_checker_spi::limits::Limits;
use serde_json::json;

#[test]
fn memo_key_is_deterministic_for_same_request_and_digest() {
    let id = PluginRequestId::new("req-1").unwrap();
    let bytes = encode_request_payload(
        PluginMethod::Check,
        &id,
        json!({"z": 1, "a": 2}),
        Limits::default(),
    )
    .unwrap();
    let binary = PluginBinary::new("/tmp/plugin", "digest-a").unwrap();

    let first = PluginMemoKey::new(PluginMethod::Check, &bytes, &binary, "0.1.0");
    let second = PluginMemoKey::new(PluginMethod::Check, &bytes, &binary, "0.1.0");

    assert_eq!(first, second);
    assert_eq!(first.as_hex().len(), 64);
}

#[test]
fn memo_key_changes_when_plugin_digest_changes() {
    let id = PluginRequestId::new("req-1").unwrap();
    let bytes = encode_request_payload(PluginMethod::Check, &id, json!({}), Limits::default())
        .unwrap();

    let first = PluginMemoKey::new(
        PluginMethod::Check,
        &bytes,
        &PluginBinary::new("/tmp/plugin", "digest-a").unwrap(),
        "0.1.0",
    );
    let second = PluginMemoKey::new(
        PluginMethod::Check,
        &bytes,
        &PluginBinary::new("/tmp/plugin", "digest-b").unwrap(),
        "0.1.0",
    );

    assert_ne!(first, second);
}

#[test]
fn memo_key_changes_when_protocol_version_changes() {
    let id = PluginRequestId::new("req-1").unwrap();
    let bytes = encode_request_payload(PluginMethod::Describe, &id, json!({}), Limits::default())
        .unwrap();
    let binary = PluginBinary::new("/tmp/plugin", "digest-a").unwrap();

    let first = PluginMemoKey::new(PluginMethod::Describe, &bytes, &binary, "0.1.0");
    let second = PluginMemoKey::new(PluginMethod::Describe, &bytes, &binary, "0.2.0");

    assert_ne!(first, second);
}

#[test]
fn memo_store_replays_exact_response_bytes() {
    let id = PluginRequestId::new("req-1").unwrap();
    let bytes = encode_request_payload(PluginMethod::Extract, &id, json!({}), Limits::default())
        .unwrap();
    let binary = PluginBinary::new("/tmp/plugin", "digest-a").unwrap();
    let key = PluginMemoKey::new(PluginMethod::Extract, &bytes, &binary, "0.1.0");
    let response = b"{\"jsonrpc\":\"2.0\",\"id\":\"req-1\",\"result\":{}}".to_vec();
    let mut store = PluginMemoStore::default();

    assert!(store.get(&key).is_none());
    store.insert(key.clone(), response.clone()).unwrap();

    assert_eq!(store.get(&key).unwrap(), response.as_slice());
}

#[test]
fn memo_store_rejects_oversized_response() {
    let key = PluginMemoKey::from_hex("00".repeat(32)).unwrap();
    let mut store = PluginMemoStore::with_response_limit(4);

    let err = store.insert(key, b"12345".to_vec()).unwrap_err();

    assert!(err.to_string().contains("exceeds"));
}
