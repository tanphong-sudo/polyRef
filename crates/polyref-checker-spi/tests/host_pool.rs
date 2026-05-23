//! Layer 3 bounded plugin pool contract tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use polyref_checker_spi::host::{
    PluginBinary, PluginHostError, PluginKind, PluginLaunchConfig, PluginMethod, PluginPool,
    PluginPoolConfig, PluginRequestId,
};
use polyref_core::correspondence_kind::CorrespondenceKind;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn bounded_pool_runs_echo_call() {
    let fixture = PluginFixture::new("echo", ECHO_PLUGIN);
    let pool = pool_for(fixture.path(), PluginKind::Extractor, 1, 1);
    let id = PluginRequestId::new("req-1").unwrap();

    let response = pool
        .call(PluginMethod::Extract, &id, json!({}), Duration::from_secs(2))
        .unwrap();

    assert_eq!(response.result.unwrap(), json!({"ok": true}));
}

#[test]
fn bounded_pool_rejects_when_active_and_queue_full() {
    let fixture = PluginFixture::new("sleep", SLEEP_PLUGIN);
    let pool = std::sync::Arc::new(pool_for(fixture.path(), PluginKind::Extractor, 1, 0));
    let first_pool = std::sync::Arc::clone(&pool);
    let first = thread::spawn(move || {
        let id = PluginRequestId::new("req-1").unwrap();
        first_pool.call(PluginMethod::Extract, &id, json!({}), Duration::from_secs(2))
    });
    thread::sleep(Duration::from_millis(100));

    let id = PluginRequestId::new("req-2").unwrap();
    let err = pool
        .call(PluginMethod::Extract, &id, json!({}), Duration::from_millis(200))
        .unwrap_err();

    assert!(matches!(err, PluginHostError::Backpressure { .. }));
    let _ = first.join().unwrap();
}

#[test]
fn pools_are_isolated_by_kind() {
    let fixture = PluginFixture::new("echo", ECHO_PLUGIN);
    let extractor = pool_for(fixture.path(), PluginKind::Extractor, 1, 0);
    let checker = pool_for(
        fixture.path(),
        PluginKind::Checker(CorrespondenceKind::Route),
        1,
        0,
    );
    let id_a = PluginRequestId::new("req-a").unwrap();
    let id_b = PluginRequestId::new("req-b").unwrap();

    let a = extractor
        .call(PluginMethod::Extract, &id_a, json!({}), Duration::from_secs(2))
        .unwrap();
    let b = checker
        .call(PluginMethod::Check, &id_b, json!({}), Duration::from_secs(2))
        .unwrap();

    assert_eq!(a.result.unwrap(), json!({"ok": true}));
    assert_eq!(b.result.unwrap(), json!({"ok": true}));
}

fn pool_for(path: &Path, kind: PluginKind, max_processes: usize, queue_bound: usize) -> PluginPool {
    let launch = PluginLaunchConfig::new(PluginBinary::new(path, "digest-1").unwrap());
    let config = PluginPoolConfig::new(kind)
        .with_max_processes(max_processes)
        .with_queue_bound(queue_bound);
    PluginPool::new(config, launch).unwrap()
}

struct PluginFixture {
    _dir: TempDir,
    path: std::path::PathBuf,
}

impl PluginFixture {
    fn new(name: &str, body: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
        }
        Self { _dir: dir, path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

const ECHO_PLUGIN: &str = r#"#!/bin/sh
line=$(cat)
id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
printf '{"jsonrpc":"2.0","id":"%s","result":{"ok":true}}\n' "$id"
"#;

const SLEEP_PLUGIN: &str = r#"#!/bin/sh
line=$(cat)
id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
sleep 1
printf '{"jsonrpc":"2.0","id":"%s","result":{"ok":true}}\n' "$id"
"#;
