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
    let bytes =
        encode_request_payload(PluginMethod::Check, &id, json!({}), Limits::default()).unwrap();

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
    let bytes =
        encode_request_payload(PluginMethod::Describe, &id, json!({}), Limits::default()).unwrap();
    let binary = PluginBinary::new("/tmp/plugin", "digest-a").unwrap();

    let first = PluginMemoKey::new(PluginMethod::Describe, &bytes, &binary, "0.1.0");
    let second = PluginMemoKey::new(PluginMethod::Describe, &bytes, &binary, "0.2.0");

    assert_ne!(first, second);
}

#[test]
fn memo_store_replays_exact_response_bytes() {
    let id = PluginRequestId::new("req-1").unwrap();
    let bytes =
        encode_request_payload(PluginMethod::Extract, &id, json!({}), Limits::default()).unwrap();
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

#[test]
fn pool_memoized_call_replays_cached_bytes_without_live_plugin() {
    let fixture = PoolPluginFixture::new("echo", POOL_ECHO_PLUGIN);
    let launch = polyref_checker_spi::host::PluginLaunchConfig::new(
        PluginBinary::new(fixture.path(), "digest-a").unwrap(),
    );
    let config = polyref_checker_spi::host::PluginPoolConfig::new(
        polyref_checker_spi::host::PluginKind::Extractor,
    );
    let pool = polyref_checker_spi::host::PluginPool::new(config, launch).unwrap();
    let id = PluginRequestId::new("req-live").unwrap();
    let mut memo = PluginMemoStore::default();

    let first = pool
        .call_memoized(
            &mut memo,
            "0.1.0",
            PluginMethod::Extract,
            &id,
            json!({}),
            std::time::Duration::from_secs(2),
        )
        .unwrap();

    std::fs::remove_file(fixture.path()).unwrap();

    let second = pool
        .call_memoized(
            &mut memo,
            "0.1.0",
            PluginMethod::Extract,
            &id,
            json!({}),
            std::time::Duration::from_secs(2),
        )
        .unwrap();

    assert_eq!(first.result, second.result);
}

struct PoolPluginFixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl PoolPluginFixture {
    fn new(name: &str, body: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        Self { _dir: dir, path }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

const POOL_ECHO_PLUGIN: &str = r#"#!/bin/sh
IFS= read -r line
id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
printf '{"jsonrpc":"2.0","id":"%s","result":{"cached":true}}\n' "$id"
"#;
