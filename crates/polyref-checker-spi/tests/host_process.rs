//! Layer 3 plugin process supervision contract tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use polyref_checker_spi::host::{
    run_plugin_call, PluginBinary, PluginHostError, PluginLaunchConfig, PluginMethod,
    PluginRequestId,
};
use polyref_checker_spi::limits::Limits;
use polyref_core::status::UnknownReason;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn echo_plugin_returns_typed_response() {
    let fixture = PluginFixture::new("echo", ECHO_PLUGIN);
    let config = launch_config(fixture.path());
    let id = PluginRequestId::new("req-1").unwrap();

    let response = run_plugin_call(
        &config,
        PluginMethod::Check,
        &id,
        json!({"value": 7}),
        Duration::from_secs(2),
    )
    .unwrap();

    assert_eq!(response.result.unwrap(), json!({"echo":"check"}));
}

#[test]
fn crashing_plugin_maps_to_plugin_failure() {
    let fixture = PluginFixture::new("crash", CRASH_PLUGIN);
    let config = launch_config(fixture.path());
    let id = PluginRequestId::new("req-1").unwrap();

    let err = run_plugin_call(
        &config,
        PluginMethod::Check,
        &id,
        json!({}),
        Duration::from_secs(2),
    )
    .unwrap_err();

    assert!(matches!(err, PluginHostError::NonZeroExit { .. }));
    assert_eq!(err.unknown_reason(), Some(UnknownReason::PluginFailure));
}

#[test]
fn sleeping_plugin_times_out_and_maps_to_checker_timeout() {
    let fixture = PluginFixture::new("sleep", SLEEP_PLUGIN);
    let config = launch_config(fixture.path());
    let id = PluginRequestId::new("req-1").unwrap();

    let err = run_plugin_call(
        &config,
        PluginMethod::Check,
        &id,
        json!({}),
        Duration::from_millis(100),
    )
    .unwrap_err();

    assert!(matches!(err, PluginHostError::Timeout { .. }));
    assert_eq!(err.unknown_reason(), Some(UnknownReason::CheckerTimeout));
}

#[test]
fn malformed_stdout_maps_to_plugin_failure() {
    let fixture = PluginFixture::new("malformed", MALFORMED_PLUGIN);
    let config = launch_config(fixture.path());
    let id = PluginRequestId::new("req-1").unwrap();

    let err = run_plugin_call(
        &config,
        PluginMethod::Describe,
        &id,
        json!({}),
        Duration::from_secs(2),
    )
    .unwrap_err();

    assert_eq!(err.unknown_reason(), Some(UnknownReason::PluginFailure));
}

#[test]
fn launch_config_debug_redacts_env_values() {
    let fixture = PluginFixture::new("echo", ECHO_PLUGIN);
    let config = PluginLaunchConfig::new(PluginBinary::new(fixture.path(), "digest-1").unwrap())
        .with_env("OPENAI_API_KEY", "sk-secret")
        .with_env("GH_TOKEN", "gh-secret");

    let debug = format!("{config:?}");

    assert!(debug.contains("OPENAI_API_KEY"));
    assert!(debug.contains("GH_TOKEN"));
    assert!(!debug.contains("sk-secret"));
    assert!(!debug.contains("gh-secret"));
}

fn launch_config(path: &Path) -> PluginLaunchConfig {
    PluginLaunchConfig::new(PluginBinary::new(path, "digest-1").unwrap())
        .with_limits(Limits::default())
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
IFS= read -r line
id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
printf '{"jsonrpc":"2.0","id":"%s","result":{"echo":"%s"}}\n' "$id" "$method"
"#;

const CRASH_PLUGIN: &str = r#"#!/bin/sh
echo 'boom' >&2
exit 42
"#;

const SLEEP_PLUGIN: &str = r#"#!/bin/sh
sleep 5
"#;

const MALFORMED_PLUGIN: &str = r#"#!/bin/sh
printf '{not-json}\n'
"#;
