#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_checker_spi::extractor::{ExtractRequest, UnsupportedFeatureNote};
use polyref_checker_spi::limits::SafePath;
use polyref_core::artifact_kind::ArtifactKind;
use polyref_core::ids::{ArtifactId, EntityId};
use polyref_core::language::Language;
use polyref_graph::builder::{
    build_extractor_graph, ExtractorArtifactInput, ExtractorOutputBundle,
};
use polyref_graph::model::RepoSide;
use polyref_graph::{GraphStore, SqliteGraphStore};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

const FIXTURE_ROOT: &str = "../../fixtures/layer4/users-route-migration";

#[derive(Debug, Deserialize)]
struct ExpectedFixture {
    artifacts: Vec<ExpectedArtifact>,
    entities: ExpectedEntities,
}

#[derive(Debug, Deserialize)]
struct ExpectedArtifact {
    artifact_id: String,
    language: String,
    path: String,
    sha256: String,
    side: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntities {
    old_handler: ExpectedEntity,
    old_route: ExpectedEntity,
    new_handler: ExpectedEntity,
    new_route: ExpectedEntity,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntity {
    id: String,
}

#[test]
fn layer4_fixture_outputs_build_graph_rows_and_route_correspondences() {
    let expected = expected_fixture();
    let store = migrated_store();
    let bundles = fixture_bundles(&expected);

    let result = build_extractor_graph(&store, &bundles).unwrap();

    assert_eq!(result.artifacts.len(), 4);
    assert_eq!(result.entities.len(), 4);
    assert_eq!(result.correspondences.len(), 2);
    assert!(result.unsupported_features.is_empty());
    assert_eq!(store.count_entities(RepoSide::Old).unwrap(), 2);
    assert_eq!(store.count_entities(RepoSide::New).unwrap(), 2);
    assert_eq!(store.count_correspondences().unwrap(), 2);

    for entity in [
        &expected.entities.old_route.id,
        &expected.entities.old_handler.id,
        &expected.entities.new_route.id,
        &expected.entities.new_handler.id,
    ] {
        assert!(store
            .find_entity(&EntityId::parse(entity).unwrap())
            .unwrap()
            .is_some());
    }

    assert_has_correspondence(
        &result,
        &expected.entities.old_route.id,
        &expected.entities.old_handler.id,
    );
    assert_has_correspondence(
        &result,
        &expected.entities.new_route.id,
        &expected.entities.new_handler.id,
    );
}

#[test]
fn duplicate_identical_facts_are_deduplicated_deterministically() {
    let expected = expected_fixture();
    let store = migrated_store();
    let mut bundles = fixture_bundles(&expected);
    for bundle in &mut bundles {
        if let Some(fact) = bundle.result.local_facts.first().cloned() {
            bundle.result.local_facts.push(fact);
        }
    }

    let result = build_extractor_graph(&store, &bundles).unwrap();

    assert_eq!(result.correspondences.len(), 2);
    assert_eq!(store.count_correspondences().unwrap(), 2);
}

#[test]
fn conflicting_duplicate_route_fact_fails_closed() {
    let expected = expected_fixture();
    let store = migrated_store();
    let mut bundles = fixture_bundles(&expected);
    let openapi = bundle_mut(&mut bundles, "old", "openapi");
    let mut conflict = openapi.result.local_facts[0].clone();
    conflict["entity_id"] =
        json!("old:openapi:route:openapi.yaml#/paths/~1users/post:aaaaaaaaaaaa");
    openapi.result.local_facts.push(conflict);

    let err = build_extractor_graph(&store, &bundles).unwrap_err();

    assert!(format!("{err}").contains("conflicting duplicate local fact"));
}

#[test]
fn mismatched_endpoint_kind_fails_closed() {
    let expected = expected_fixture();
    let store = migrated_store();
    let mut bundles = fixture_bundles(&expected);
    let ts = bundle_mut(&mut bundles, "old", "ts");
    ts.result.local_facts[0]["handler_entity_id"] = json!(expected.entities.old_route.id);

    let err = build_extractor_graph(&store, &bundles).unwrap_err();

    assert!(format!("{err}").contains("is not kind handler"));
}

#[test]
fn malformed_local_fact_fails_closed() {
    let expected = expected_fixture();
    let store = migrated_store();
    let mut bundles = fixture_bundles(&expected);
    bundles[0]
        .result
        .local_facts
        .push(json!({ "kind": "route" }));

    let err = build_extractor_graph(&store, &bundles).unwrap_err();

    assert!(format!("{err}").contains("invalid local fact"));
}

#[test]
fn unsupported_features_are_preserved_without_creating_correspondence() {
    let expected = expected_fixture();
    let store = migrated_store();
    let mut bundles = fixture_bundles(&expected);
    let ts = bundle_mut(&mut bundles, "old", "ts");
    let span = ts.result.entities[0].source_span.clone();
    ts.result.local_facts.clear();
    ts.result.unsupported_features.push(UnsupportedFeatureNote {
        feature: "dynamic_route".to_owned(),
        span,
        note: Some("test unsupported route".to_owned()),
    });

    let result = build_extractor_graph(&store, &bundles).unwrap();

    assert_eq!(result.unsupported_features.len(), 1);
    assert_eq!(result.unsupported_features[0].feature, "dynamic_route");
    assert_eq!(result.correspondences.len(), 1);
}

fn migrated_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().unwrap();
    store.migrate().unwrap();
    store
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_ROOT)
}

fn expected_fixture() -> ExpectedFixture {
    let contents = std::fs::read_to_string(fixture_root().join("expected.json")).unwrap();
    serde_json::from_str(&contents).unwrap()
}

fn fixture_bundles(expected: &ExpectedFixture) -> Vec<ExtractorOutputBundle> {
    expected
        .artifacts
        .iter()
        .map(|artifact| {
            let metadata = artifact_input(artifact);
            let request = extract_request(artifact);
            let result = match artifact.language.as_str() {
                "openapi" => {
                    polyref_extractor_openapi::extract_openapi(&fixture_root(), &request).unwrap()
                }
                "ts" => polyref_extractor_typescript::extract_typescript(&fixture_root(), &request)
                    .unwrap(),
                other => panic!("unexpected fixture language: {other}"),
            };
            ExtractorOutputBundle { metadata, result }
        })
        .collect()
}

fn bundle_mut<'a>(
    bundles: &'a mut [ExtractorOutputBundle],
    side: &str,
    language: &str,
) -> &'a mut ExtractorOutputBundle {
    bundles
        .iter_mut()
        .find(|bundle| {
            bundle.metadata.repo_side.as_str() == side
                && bundle.metadata.language.as_tag() == language
        })
        .unwrap()
}

fn artifact_input(artifact: &ExpectedArtifact) -> ExtractorArtifactInput {
    ExtractorArtifactInput {
        artifact_id: ArtifactId::parse(&artifact.artifact_id).unwrap(),
        repo_side: match artifact.side.as_str() {
            "old" => RepoSide::Old,
            "new" => RepoSide::New,
            other => panic!("unexpected side: {other}"),
        },
        kind: match artifact.language.as_str() {
            "openapi" => ArtifactKind::Schema,
            "ts" => ArtifactKind::SourceFile,
            other => panic!("unexpected language: {other}"),
        },
        language: match artifact.language.as_str() {
            "openapi" => Language::Openapi,
            "ts" => Language::Ts,
            other => panic!("unexpected language: {other}"),
        },
        local_path: artifact.path.clone(),
        content_hash: artifact.sha256[..12].to_owned(),
    }
}

fn extract_request(artifact: &ExpectedArtifact) -> ExtractRequest {
    ExtractRequest {
        artifact_path: SafePath::parse(&format!("{}/{}", artifact.side, artifact.path)).unwrap(),
        content_hash: artifact.sha256.clone(),
        language: match artifact.language.as_str() {
            "openapi" => Language::Openapi,
            "ts" => Language::Ts,
            other => panic!("unexpected language: {other}"),
        },
        options: json!({}),
        deadline_ms: 60_000,
        log_dir: SafePath::parse("logs/graph-builder").unwrap(),
    }
}

fn assert_has_correspondence(
    result: &polyref_graph::builder::GraphBuildResult,
    route: &str,
    handler: &str,
) {
    let route = EntityId::parse(route).unwrap();
    let handler = EntityId::parse(handler).unwrap();
    assert!(result
        .correspondences
        .iter()
        .any(|corr| corr.endpoints == vec![route.clone(), handler.clone()]));
}
