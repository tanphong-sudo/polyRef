#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_core::{
    artifact_kind::ArtifactKind,
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    language::Language,
};
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

const FIXTURE_ROOT: &str = "../../fixtures/layer5/users-route-frontier";

const GOLDEN_CORR_IDS: &[&str] = &[
    "corr:event:0000000000000006",
    "corr:generated_client:0000000000000004",
    "corr:query_table:0000000000000005",
    "corr:route:0000000000000001",
    "corr:schema:0000000000000002",
    "corr:schema:0000000000000003",
    "corr:workflow:0000000000000007",
];

const GOLDEN_EDGE_IDS: &[&str] = &[
    "edge:build_codegen:0000000000000001",
    "edge:build_codegen:0000000000000002",
    "edge:build_codegen:0000000000000003",
];

#[derive(Debug, Deserialize)]
struct Layer5Fixture {
    artifacts: Vec<FixtureArtifact>,
    entities: Vec<FixtureEntity>,
    correspondences: Vec<FixtureCorrespondence>,
    build_edges: Vec<FixtureBuildEdge>,
    migration_map_candidates: Vec<FixtureMigrationCandidate>,
    observations: Vec<FixtureObservation>,
    expected_frontier: FixtureExpectedFrontier,
}

#[derive(Debug, Deserialize)]
struct FixtureArtifact {
    artifact_id: String,
    side: String,
    kind: String,
    language: String,
    path: String,
    content_hash: String,
}

#[derive(Debug, Deserialize)]
struct FixtureEntity {
    entity_id: String,
    artifact_id: String,
    side: String,
    language: String,
    kind: String,
    local_path: String,
    stable_hash: String,
}

#[derive(Debug, Deserialize)]
struct FixtureCorrespondence {
    corr_id: String,
    kind: String,
    endpoints: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureBuildEdge {
    edge_id: String,
    src_artifact: String,
    dst_artifact: String,
}

#[derive(Debug, Deserialize)]
struct FixtureMigrationCandidate {
    old: String,
    new: String,
    evidence: String,
}

#[derive(Debug, Deserialize)]
struct FixtureObservation {
    observation_id: String,
    visibility: String,
    kind: String,
    defined_semantics: bool,
    support: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureExpectedFrontier {
    correspondence_ids: Vec<String>,
    build_edge_ids: Vec<String>,
}

#[test]
fn layer5_fixture_exists_and_is_utf8_json() {
    let fixture = load_fixture();
    assert!(!fixture.artifacts.is_empty());
    assert!(!fixture.entities.is_empty());
}

#[test]
fn fixture_ids_parse_and_rows_are_sorted() {
    let fixture = load_fixture();

    assert_sorted_by(&fixture.artifacts, |artifact| artifact.artifact_id.as_str());
    assert_sorted_by(&fixture.entities, |entity| entity.entity_id.as_str());
    assert_sorted_by(&fixture.correspondences, |corr| corr.corr_id.as_str());
    assert_sorted_by(&fixture.build_edges, |edge| edge.edge_id.as_str());
    assert_sorted_by(&fixture.migration_map_candidates, |candidate| {
        candidate.old.as_str()
    });
    assert_sorted_by(&fixture.observations, |obs| obs.observation_id.as_str());

    for artifact in &fixture.artifacts {
        let parsed = ArtifactId::parse(&artifact.artifact_id).unwrap();
        assert_eq!(parsed.as_str(), artifact.artifact_id);
        ArtifactKind::parse(&artifact.kind).unwrap();
        Language::parse(&artifact.language).unwrap();
        assert!(matches!(artifact.side.as_str(), "old" | "new"));
        assert_eq!(artifact.content_hash.len(), 12);
        assert_eq!(
            artifact.content_hash,
            parsed.as_str().rsplit(':').next().unwrap()
        );
        assert!(!artifact.path.starts_with('/'));
        assert!(!artifact.path.split('/').any(|segment| segment == ".."));
    }

    for entity in &fixture.entities {
        let parsed = EntityId::parse(&entity.entity_id).unwrap();
        assert_eq!(parsed.as_str(), entity.entity_id);
        assert_eq!(parsed.repo_side(), entity.side);
        assert_eq!(parsed.language(), entity.language);
        assert_eq!(parsed.kind(), entity.kind);
        assert_eq!(parsed.local_path(), entity.local_path);
        assert_eq!(parsed.stable_hash(), entity.stable_hash);
        ArtifactId::parse(&entity.artifact_id).unwrap();
        Language::parse(&entity.language).unwrap();
        assert!(matches!(entity.side.as_str(), "old" | "new"));
    }

    for corr in &fixture.correspondences {
        CorrId::parse(&corr.corr_id).unwrap();
        CorrespondenceKind::parse(&corr.kind).unwrap();
        assert!(!corr.endpoints.is_empty());
    }

    for edge in &fixture.build_edges {
        EdgeId::parse(&edge.edge_id).unwrap();
    }
}

#[test]
fn fixture_references_are_resolvable() {
    let fixture = load_fixture();
    let artifact_ids = artifact_ids(&fixture);
    let entity_ids = entity_ids(&fixture);
    let corr_ids = corr_ids(&fixture);
    let edge_ids = edge_ids(&fixture);

    for entity in &fixture.entities {
        assert!(artifact_ids.contains(entity.artifact_id.as_str()));
    }

    for corr in &fixture.correspondences {
        for endpoint in &corr.endpoints {
            assert!(
                entity_ids.contains(endpoint.as_str()),
                "missing endpoint {endpoint}"
            );
        }
    }

    for edge in &fixture.build_edges {
        assert!(artifact_ids.contains(edge.src_artifact.as_str()));
        assert!(artifact_ids.contains(edge.dst_artifact.as_str()));
    }

    for candidate in &fixture.migration_map_candidates {
        let old = EntityId::parse(&candidate.old).unwrap();
        let new = EntityId::parse(&candidate.new).unwrap();
        assert!(entity_ids.contains(candidate.old.as_str()));
        assert!(entity_ids.contains(candidate.new.as_str()));
        assert_eq!(old.repo_side(), "old");
        assert_eq!(new.repo_side(), "new");
        assert_eq!(old.kind(), new.kind());
        assert_eq!(candidate.evidence, "concrete");
    }

    for obs in &fixture.observations {
        assert!(matches!(obs.visibility.as_str(), "visible" | "held_out"));
        assert!(matches!(obs.kind.as_str(), "api_call" | "test_invocation"));
        assert!(obs.defined_semantics);
        assert!(!obs.support.is_empty());
        assert_sorted_strings(&obs.support);
        for support in &obs.support {
            if support.starts_with("corr:") {
                CorrId::parse(support).unwrap();
                assert!(corr_ids.contains(support.as_str()));
            } else if support.starts_with("edge:") {
                EdgeId::parse(support).unwrap();
                assert!(edge_ids.contains(support.as_str()));
            } else {
                panic!("support must be corr/edge ref, got {support}");
            }
        }
    }
}

#[test]
fn expected_frontier_is_exact_golden_set() {
    let fixture = load_fixture();
    let corr_ids = corr_ids(&fixture);
    let edge_ids = edge_ids(&fixture);

    assert_eq!(
        fixture.expected_frontier.correspondence_ids,
        GOLDEN_CORR_IDS
    );
    assert_eq!(fixture.expected_frontier.build_edge_ids, GOLDEN_EDGE_IDS);

    for id in &fixture.expected_frontier.correspondence_ids {
        assert!(corr_ids.contains(id.as_str()));
    }
    for id in &fixture.expected_frontier.build_edge_ids {
        assert!(edge_ids.contains(id.as_str()));
    }

    for corr in &fixture.correspondences {
        let is_golden = GOLDEN_CORR_IDS.contains(&corr.corr_id.as_str());
        assert_eq!(
            fixture
                .expected_frontier
                .correspondence_ids
                .contains(&corr.corr_id),
            is_golden
        );
    }
    for edge in &fixture.build_edges {
        let is_golden = GOLDEN_EDGE_IDS.contains(&edge.edge_id.as_str());
        assert_eq!(
            fixture
                .expected_frontier
                .build_edge_ids
                .contains(&edge.edge_id),
            is_golden
        );
    }
}

fn load_fixture() -> Layer5Fixture {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join("expected.json");
    let contents = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&contents).unwrap()
}

fn assert_sorted_by<T, F>(items: &[T], key: F)
where
    F: Fn(&T) -> &str,
{
    for window in items.windows(2) {
        assert!(key(&window[0]) < key(&window[1]));
    }
}

fn assert_sorted_strings(items: &[String]) {
    for window in items.windows(2) {
        assert!(window[0] < window[1]);
    }
}

fn artifact_ids(fixture: &Layer5Fixture) -> BTreeSet<&str> {
    fixture
        .artifacts
        .iter()
        .map(|artifact| artifact.artifact_id.as_str())
        .collect()
}

fn entity_ids(fixture: &Layer5Fixture) -> BTreeSet<&str> {
    fixture
        .entities
        .iter()
        .map(|entity| entity.entity_id.as_str())
        .collect()
}

fn corr_ids(fixture: &Layer5Fixture) -> BTreeSet<&str> {
    fixture
        .correspondences
        .iter()
        .map(|corr| corr.corr_id.as_str())
        .collect()
}

fn edge_ids(fixture: &Layer5Fixture) -> BTreeSet<&str> {
    fixture
        .build_edges
        .iter()
        .map(|edge| edge.edge_id.as_str())
        .collect()
}
