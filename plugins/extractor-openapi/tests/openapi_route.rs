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
    old_route: ExpectedEntity,
    new_route: ExpectedEntity,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntity {
    id: String,
    method: String,
    path: String,
    operation_id: String,
}

#[test]
fn fixture_openapi_specs_emit_expected_route_entities() {
    let expected = expected_fixture();

    let old = extract_fixture(&expected, "old");
    assert_single_route(&old, &expected.entities.old_route);
    assert_ref_fact(
        &old,
        "request_schema_refs",
        "#/components/schemas/UserCreateV1",
    );
    assert_ref_fact(&old, "response_schema_refs", "#/components/schemas/UserV1");

    let new = extract_fixture(&expected, "new");
    assert_single_route(&new, &expected.entities.new_route);
    assert_ref_fact(
        &new,
        "request_schema_refs",
        "#/components/schemas/UserCreateV2",
    );
    assert_ref_fact(&new, "response_schema_refs", "#/components/schemas/UserV2");
}

#[test]
fn content_hash_mismatch_fails_closed() {
    let expected = expected_fixture();
    let artifact = openapi_artifact(&expected, "old");
    let request = ExtractRequest {
        artifact_path: SafePath::parse("old/openapi.yaml").unwrap(),
        content_hash: "0".repeat(64),
        language: Language::Openapi,
        options: json!({}),
        deadline_ms: 60_000,
        log_dir: SafePath::parse("logs/openapi").unwrap(),
    };

    let err = polyref_extractor_openapi::extract_openapi(&fixture_root(), &request).unwrap_err();

    assert!(format!("{err}").contains(&artifact.sha256));
}

#[test]
fn remote_refs_are_reported_without_network_fetch() {
    let dir = fixture_with_openapi(
        r#"openapi: 3.0.3
info:
  title: Remote Ref Fixture
  version: 1.0.0
paths:
  /remote:
    post:
      operationId: createRemote
      requestBody:
        content:
          application/json:
            schema:
              $ref: 'https://example.com/schema.json#/Remote'
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/RemoteResponse'
components:
  schemas:
    RemoteResponse:
      type: object
"#,
    );
    let request = request_for_temp(&dir, "openapi.yaml");

    let result = polyref_extractor_openapi::extract_openapi(dir.path(), &request).unwrap();

    assert_eq!(result.entities.len(), 1);
    assert_eq!(result.unsupported_features.len(), 1);
    assert_eq!(result.unsupported_features[0].feature, "remote_ref");
    assert!(result.unsupported_features[0]
        .note
        .as_deref()
        .unwrap()
        .contains("https://example.com/schema.json#/Remote"));
}

#[test]
fn malformed_openapi_fails_closed() {
    let dir = fixture_with_openapi("openapi: [not valid yaml");
    let request = request_for_temp(&dir, "openapi.yaml");

    let err = polyref_extractor_openapi::extract_openapi(dir.path(), &request).unwrap_err();

    assert!(format!("{err}").contains("parse"));
}

#[test]
fn duplicate_operation_ids_fail_closed() {
    let dir = fixture_with_openapi(
        r#"openapi: 3.0.3
info:
  title: Duplicate Fixture
  version: 1.0.0
paths:
  /one:
    post:
      operationId: reused
      responses:
        '200':
          description: ok
  /two:
    post:
      operationId: reused
      responses:
        '200':
          description: ok
"#,
    );
    let request = request_for_temp(&dir, "openapi.yaml");

    let err = polyref_extractor_openapi::extract_openapi(dir.path(), &request).unwrap_err();

    assert!(format!("{err}").contains("duplicate operationId"));
}

#[test]
fn json_rpc_adapter_returns_extract_result_line() {
    let expected = expected_fixture();
    let artifact = openapi_artifact(&expected, "old");
    let request = extract_request(artifact);
    let id = PluginRequestId::new("extract-old-openapi").unwrap();
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
    assert_single_route(&result, &expected.entities.old_route);
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

fn openapi_artifact<'a>(expected: &'a ExpectedFixture, side: &str) -> &'a ExpectedArtifact {
    expected
        .artifacts
        .iter()
        .find(|artifact| artifact.side == side && artifact.language == "openapi")
        .unwrap()
}

fn extract_fixture(expected: &ExpectedFixture, side: &str) -> ExtractResult {
    let artifact = openapi_artifact(expected, side);
    let request = extract_request(artifact);
    polyref_extractor_openapi::extract_openapi(&fixture_root(), &request).unwrap()
}

fn extract_request(artifact: &ExpectedArtifact) -> ExtractRequest {
    ExtractRequest {
        artifact_path: SafePath::parse(&format!("{}/{}", artifact.side, artifact.path)).unwrap(),
        content_hash: artifact.sha256.clone(),
        language: Language::Openapi,
        options: json!({}),
        deadline_ms: 60_000,
        log_dir: SafePath::parse("logs/openapi").unwrap(),
    }
}

fn assert_single_route(result: &ExtractResult, expected: &ExpectedEntity) {
    assert_eq!(
        result.extractor_version,
        polyref_extractor_openapi::EXTRACTOR_VERSION
    );
    assert_eq!(result.entities.len(), 1);
    assert_eq!(result.local_facts.len(), 1);
    assert_eq!(result.unsupported_features.len(), 0);

    let entity = &result.entities[0];
    assert_eq!(entity.entity_id, EntityId::parse(&expected.id).unwrap());
    assert_eq!(entity.kind, "route");
    assert_eq!(entity.local_name, expected.operation_id);
    assert_eq!(entity.r#type.as_deref(), Some("route"));
    assert_eq!(
        entity.source_span.artifact().as_str(),
        expected_artifact_id(&expected.id)
    );

    let fact = &result.local_facts[0];
    assert_eq!(fact["kind"], "route");
    assert_eq!(fact["entity_id"], expected.id);
    assert_eq!(fact["method"], expected.method);
    assert_eq!(fact["path"], expected.path);
    assert_eq!(fact["operation_id"], expected.operation_id);
}

fn assert_ref_fact(result: &ExtractResult, key: &str, expected_ref: &str) {
    let refs = result.local_facts[0][key].as_array().unwrap();
    assert!(refs.iter().any(|value| value == expected_ref));
}

fn expected_artifact_id(entity_id: &str) -> &str {
    if entity_id.starts_with("old:") {
        "artifact:old:openapi.yaml:59b85435ba5c"
    } else {
        "artifact:new:openapi.yaml:67227c11e6b3"
    }
}

fn fixture_with_openapi(contents: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("openapi.yaml"), contents).unwrap();
    dir
}

fn request_for_temp(dir: &TempDir, path: &str) -> ExtractRequest {
    let bytes = fs::read(dir.path().join(path)).unwrap();
    ExtractRequest {
        artifact_path: SafePath::parse(path).unwrap(),
        content_hash: format!("{:x}", Sha256::digest(bytes)),
        language: Language::Openapi,
        options: json!({}),
        deadline_ms: 60_000,
        log_dir: SafePath::parse("logs/openapi").unwrap(),
    }
}

fn run_plugin_with_input(cwd: &Path, input: &[u8]) -> String {
    let binary = env!("CARGO_BIN_EXE_polyref-extractor-openapi");
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
