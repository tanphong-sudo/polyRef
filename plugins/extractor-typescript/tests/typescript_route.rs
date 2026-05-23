#![allow(clippy::unwrap_used, clippy::expect_used)]

use polyref_checker_spi::envelope::{JsonRpcRequest, JsonRpcResponse};
use polyref_checker_spi::extractor::{ExtractRequest, ExtractResult};
use polyref_checker_spi::host::{encode_request_line, PluginMethod, PluginRequestId};
use polyref_checker_spi::limits::{Limits, SafePath};
use polyref_core::ids::EntityId;
use polyref_core::language::Language;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const FIXTURE_ROOT: &str = "../../fixtures/layer4/users-route-migration";

#[derive(Debug, Deserialize)]
struct ExpectedFixture {
    artifacts: Vec<ExpectedArtifact>,
    entities: ExpectedEntities,
}

#[derive(Debug, Deserialize)]
struct ExpectedArtifact {
    side: String,
    path: String,
    language: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntities {
    old_handler: ExpectedHandler,
    new_handler: ExpectedHandler,
}

#[derive(Debug, Deserialize)]
struct ExpectedHandler {
    id: String,
    export: String,
}

#[test]
fn fixture_typescript_files_emit_expected_handler_entities_and_route_facts() {
    let expected = expected_fixture();

    let old = extract_fixture(&expected, "old");
    assert_single_handler(
        &old,
        &expected.entities.old_handler,
        "POST",
        "/users",
        "createUser",
    );

    let new = extract_fixture(&expected, "new");
    assert_single_handler(
        &new,
        &expected.entities.new_handler,
        "POST",
        "/v2/users",
        "createUserV2",
    );
}

#[test]
fn content_hash_mismatch_fails_closed_before_parse() {
    let request = ExtractRequest {
        artifact_path: SafePath::parse("old/src/users.ts").unwrap(),
        content_hash: "0".repeat(64),
        language: Language::Ts,
        options: json!({}),
        deadline_ms: 60_000,
        log_dir: SafePath::parse("logs/typescript").unwrap(),
    };

    let err =
        polyref_extractor_typescript::extract_typescript(&fixture_root(), &request).unwrap_err();

    assert!(format!("{err}").contains("content hash mismatch"));
}

#[test]
fn json_rpc_adapter_returns_extract_result_line() {
    let expected = expected_fixture();
    let artifact = ts_artifact(&expected, "old");
    let request = extract_request(artifact);
    let id = PluginRequestId::new("extract-old-ts").unwrap();
    let line = encode_request_line(
        PluginMethod::Extract,
        &id,
        serde_json::to_value(request).unwrap(),
        Limits::default(),
    )
    .unwrap();

    let response = run_plugin_with_input(&fixture_root(), &line);

    assert!(response.ends_with('\n'));
    assert_eq!(response.lines().count(), 1);
    let envelope: JsonRpcResponse = serde_json::from_str(response.trim_end()).unwrap();
    assert!(envelope.error.is_none());
    let result: ExtractResult = serde_json::from_value(envelope.result.unwrap()).unwrap();
    assert_single_handler(
        &result,
        &expected.entities.old_handler,
        "POST",
        "/users",
        "createUser",
    );
}

#[test]
fn json_rpc_adapter_rejects_unknown_method() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        method: "check".to_owned(),
        id: json!("bad-method"),
        params: json!({}),
    };
    let input = format!("{}\n", serde_json::to_string(&request).unwrap());

    let response = run_plugin_with_input(&fixture_root(), input.as_bytes());

    let envelope: JsonRpcResponse = serde_json::from_str(response.trim_end()).unwrap();
    assert!(envelope.result.is_none());
    assert_eq!(envelope.error.unwrap().code, -32601);
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_ROOT)
}

fn expected_fixture() -> ExpectedFixture {
    let contents = fs::read_to_string(fixture_root().join("expected.json")).unwrap();
    serde_json::from_str(&contents).unwrap()
}

fn ts_artifact<'a>(expected: &'a ExpectedFixture, side: &str) -> &'a ExpectedArtifact {
    expected
        .artifacts
        .iter()
        .find(|artifact| artifact.side == side && artifact.language == "ts")
        .unwrap()
}

fn extract_fixture(expected: &ExpectedFixture, side: &str) -> ExtractResult {
    let artifact = ts_artifact(expected, side);
    let request = extract_request(artifact);
    polyref_extractor_typescript::extract_typescript(&fixture_root(), &request).unwrap()
}

fn extract_request(artifact: &ExpectedArtifact) -> ExtractRequest {
    ExtractRequest {
        artifact_path: SafePath::parse(&format!("{}/{}", artifact.side, artifact.path)).unwrap(),
        content_hash: artifact.sha256.clone(),
        language: Language::Ts,
        options: json!({}),
        deadline_ms: 60_000,
        log_dir: SafePath::parse("logs/typescript").unwrap(),
    }
}

fn assert_single_handler(
    result: &ExtractResult,
    expected: &ExpectedHandler,
    method: &str,
    path: &str,
    operation_id: &str,
) {
    assert_eq!(
        result.extractor_version,
        polyref_extractor_typescript::EXTRACTOR_VERSION
    );
    assert_eq!(result.entities.len(), 1);
    assert_eq!(result.local_facts.len(), 1);
    assert_eq!(result.unsupported_features.len(), 0);

    let entity = &result.entities[0];
    assert_eq!(entity.entity_id, EntityId::parse(&expected.id).unwrap());
    assert_eq!(entity.kind, "handler");
    assert_eq!(entity.local_name, expected.export);
    assert_eq!(entity.r#type.as_deref(), Some("handler"));

    let fact = &result.local_facts[0];
    assert_eq!(fact["kind"], "route_metadata");
    assert_eq!(fact["handler_entity_id"], expected.id);
    assert_eq!(fact["method"], method);
    assert_eq!(fact["path"], path);
    assert_eq!(fact["operation_id"], operation_id);
    assert_eq!(fact["handler_export"], expected.export);
}

#[allow(dead_code)]
fn fixture_with_ts(contents: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/users.ts"), contents).unwrap();
    dir
}

#[allow(dead_code)]
fn request_for_temp(dir: &TempDir, path: &str) -> ExtractRequest {
    let bytes = fs::read(dir.path().join(path)).unwrap();
    ExtractRequest {
        artifact_path: SafePath::parse(path).unwrap(),
        content_hash: format!("{:x}", Sha256::digest(bytes)),
        language: Language::Ts,
        options: json!({}),
        deadline_ms: 60_000,
        log_dir: SafePath::parse("logs/typescript").unwrap(),
    }
}

fn run_plugin_with_input(cwd: &Path, input: &[u8]) -> String {
    let binary = env!("CARGO_BIN_EXE_polyref-extractor-typescript");
    let mut child = Command::new(binary)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), input).unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
