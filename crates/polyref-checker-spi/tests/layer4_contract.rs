#![allow(clippy::unwrap_used, clippy::expect_used)]

use polyref_checker_spi::checker::{CheckRequest, EndpointArg};
use polyref_checker_spi::extractor::ExtractRequest;
use polyref_checker_spi::host::{encode_request_line, PluginMethod, PluginRequestId};
use polyref_checker_spi::limits::{Limits, SafePath};
use polyref_core::correspondence_kind::CorrespondenceKind;
use polyref_core::ids::{ArtifactId, EntityId};
use polyref_core::language::Language;
use polyref_core::migration_map::MigrationMap;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_RELATIVE_ROOT: &str = "../../fixtures/layer4/users-route-migration";
const DEADLINE_MS: u32 = 60_000;

#[derive(Debug, Deserialize)]
struct ExpectedFixture {
    artifacts: Vec<ExpectedArtifact>,
    entities: ExpectedEntities,
    migration_map_excerpt: Value,
    observation_excerpt: Value,
}

#[derive(Debug, Deserialize)]
struct ExpectedArtifact {
    side: String,
    path: String,
    language: String,
    sha256: String,
    artifact_id: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntities {
    old_route: ExpectedEntity,
    new_route: ExpectedEntity,
    old_handler: ExpectedEntity,
    new_handler: ExpectedEntity,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntity {
    id: String,
    method: Option<String>,
    path: Option<String>,
    operation_id: Option<String>,
    export: Option<String>,
}

#[test]
fn users_route_migration_fixture_files_are_utf8_and_hash_locked() {
    let expected = read_expected();

    for artifact in &expected.artifacts {
        let bytes = fs::read(fixture_root().join(&artifact.side).join(&artifact.path))
            .expect("fixture artifact should exist");
        std::str::from_utf8(&bytes).expect("fixture artifact should be utf-8");
        assert_eq!(sha256_hex(&bytes), artifact.sha256);
        assert_eq!(artifact.artifact_id, expected_artifact_id(artifact));
        ArtifactId::parse(&artifact.artifact_id).expect("artifact id should match grammar");
        SafePath::parse(&format!("{}/{}", artifact.side, artifact.path))
            .expect("fixture path should be safe and relative");
    }
}

#[test]
fn users_route_migration_entity_ids_are_current_grammar() {
    let expected = read_expected();
    let ids = [
        &expected.entities.old_route.id,
        &expected.entities.new_route.id,
        &expected.entities.old_handler.id,
        &expected.entities.new_handler.id,
    ];

    for id in ids {
        EntityId::parse(id).expect("entity id should match current grammar");
    }

    assert_eq!(expected.entities.old_route.method.as_deref(), Some("POST"));
    assert_eq!(expected.entities.old_route.path.as_deref(), Some("/users"));
    assert_eq!(
        expected.entities.old_route.operation_id.as_deref(),
        Some("createUser")
    );
    assert_eq!(expected.entities.new_route.method.as_deref(), Some("POST"));
    assert_eq!(
        expected.entities.new_route.path.as_deref(),
        Some("/v2/users")
    );
    assert_eq!(
        expected.entities.new_route.operation_id.as_deref(),
        Some("createUserV2")
    );
    assert_eq!(
        expected.entities.old_handler.export.as_deref(),
        Some("createUser")
    );
    assert_eq!(
        expected.entities.new_handler.export.as_deref(),
        Some("createUserV2")
    );
}

#[test]
fn users_route_migration_builds_extract_requests() {
    let expected = read_expected();

    for artifact in &expected.artifacts {
        let request = extract_request(artifact);
        assert_eq!(
            request.artifact_path.as_str(),
            format!("{}/{}", artifact.side, artifact.path)
        );
        assert_eq!(request.content_hash, artifact.sha256);
        assert_eq!(request.language.as_tag(), artifact.language);
        assert_eq!(request.deadline_ms, DEADLINE_MS);
        assert_eq!(
            request.log_dir.as_str(),
            format!("logs/extract/{}/{}", artifact.side, artifact.language)
        );
    }
}

#[test]
fn users_route_migration_map_is_type_respecting() {
    let expected = read_expected();
    let migration_map = migration_map(&expected);

    assert!(migration_map.is_type_respecting());
    assert_eq!(migration_map.iter().count(), 2);

    let old_route = EntityId::parse(&expected.entities.old_route.id).unwrap();
    let new_route = EntityId::parse(&expected.entities.new_route.id).unwrap();
    assert_eq!(migration_map.get(&old_route), Some(&new_route));
}

#[test]
fn users_route_migration_check_request_matches_host_protocol() {
    let expected = read_expected();
    let check_request = check_request(&expected);

    assert_eq!(check_request.contract_id, "polyref.route.migration.v1");
    assert_eq!(check_request.kind, CorrespondenceKind::Route);
    assert_eq!(check_request.old_repo_root.as_str(), "old");
    assert_eq!(check_request.new_repo_root.as_str(), "new");
    assert_eq!(check_request.endpoints.len(), 2);
    assert_eq!(
        check_request.migration_map_excerpt,
        expected.migration_map_excerpt
    );
    assert_eq!(
        check_request.observation_excerpt,
        expected.observation_excerpt
    );

    let params = serde_json::to_value(&check_request).unwrap();
    let id = PluginRequestId::new("layer4-fixture-route-check").unwrap();
    let line = encode_request_line(PluginMethod::Check, &id, params, Limits::default()).unwrap();

    assert!(line.ends_with(b"\n"));
    assert_eq!(line.iter().filter(|byte| **byte == b'\n').count(), 1);

    let payload: Value = serde_json::from_slice(&line[..line.len() - 1]).unwrap();
    assert_eq!(payload["jsonrpc"], "2.0");
    assert_eq!(payload["method"], "check");
    assert_eq!(payload["id"], "layer4-fixture-route-check");
    assert_eq!(payload["params"]["kind"], "route");
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_RELATIVE_ROOT)
}

fn read_expected() -> ExpectedFixture {
    let path = fixture_root().join("expected.json");
    let contents = fs::read_to_string(path).expect("expected.json should exist and be utf-8");
    serde_json::from_str(&contents).expect("expected.json should match test metadata shape")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn expected_artifact_id(artifact: &ExpectedArtifact) -> String {
    format!(
        "artifact:{}:{}:{}",
        artifact.side,
        artifact.path,
        &artifact.sha256[..12]
    )
}

fn extract_request(artifact: &ExpectedArtifact) -> ExtractRequest {
    ExtractRequest {
        artifact_path: SafePath::parse(&format!("{}/{}", artifact.side, artifact.path)).unwrap(),
        content_hash: artifact.sha256.clone(),
        language: Language::parse(&artifact.language).unwrap(),
        options: json!({}),
        deadline_ms: DEADLINE_MS,
        log_dir: SafePath::parse(&format!(
            "logs/extract/{}/{}",
            artifact.side, artifact.language
        ))
        .unwrap(),
    }
}

fn migration_map(expected: &ExpectedFixture) -> MigrationMap {
    let rewrites = BTreeMap::from([
        (
            EntityId::parse(&expected.entities.old_route.id).unwrap(),
            EntityId::parse(&expected.entities.new_route.id).unwrap(),
        ),
        (
            EntityId::parse(&expected.entities.old_handler.id).unwrap(),
            EntityId::parse(&expected.entities.new_handler.id).unwrap(),
        ),
    ]);
    MigrationMap::try_new(rewrites, Vec::new(), Vec::new()).unwrap()
}

fn check_request(expected: &ExpectedFixture) -> CheckRequest {
    CheckRequest {
        contract_id: "polyref.route.migration.v1".to_owned(),
        kind: CorrespondenceKind::Route,
        endpoints: vec![
            EndpointArg {
                entity_id: EntityId::parse(&expected.entities.old_route.id).unwrap(),
                r#type: "route".to_owned(),
            },
            EndpointArg {
                entity_id: EntityId::parse(&expected.entities.new_route.id).unwrap(),
                r#type: "route".to_owned(),
            },
        ],
        old_repo_root: SafePath::parse("old").unwrap(),
        new_repo_root: SafePath::parse("new").unwrap(),
        migration_map_excerpt: expected.migration_map_excerpt.clone(),
        observation_excerpt: expected.observation_excerpt.clone(),
        deadline_ms: DEADLINE_MS,
        log_dir: SafePath::parse("logs/check/route").unwrap(),
    }
}
