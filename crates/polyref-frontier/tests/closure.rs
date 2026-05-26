#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_core::{
    artifact_kind::ArtifactKind,
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    language::Language,
    migration_map::MigrationMap,
    observation::SupportRef,
};
use polyref_frontier::{compute_frontier, FrontierInput, FrontierItem, FrontierReason};
use polyref_graph::{
    Artifact, BuildEdge, Correspondence, Entity, GraphStore, RepoSide, SqliteGraphStore,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const FIXTURE_ROOT: &str = "../../fixtures/layer5/users-route-frontier";

#[derive(Debug, Deserialize)]
struct Layer5Fixture {
    artifacts: Vec<FixtureArtifact>,
    build_edges: Vec<FixtureBuildEdge>,
    correspondences: Vec<FixtureCorrespondence>,
    entities: Vec<FixtureEntity>,
    expected_frontier: FixtureExpectedFrontier,
    migration_map_candidates: Vec<FixtureMigrationCandidate>,
    observations: Vec<FixtureObservation>,
}

#[derive(Debug, Deserialize)]
struct FixtureArtifact {
    artifact_id: String,
    content_hash: String,
    kind: String,
    language: String,
    path: String,
    side: String,
}

#[derive(Debug, Deserialize)]
struct FixtureBuildEdge {
    dst_artifact: String,
    edge_id: String,
    src_artifact: String,
}

#[derive(Debug, Deserialize)]
struct FixtureCorrespondence {
    corr_id: String,
    endpoints: Vec<String>,
    kind: String,
}

#[derive(Debug, Deserialize)]
struct FixtureEntity {
    artifact_id: String,
    entity_id: String,
    kind: String,
    language: String,
    local_path: String,
    side: String,
    stable_hash: String,
}

#[derive(Debug, Deserialize)]
struct FixtureExpectedFrontier {
    build_edge_ids: Vec<String>,
    correspondence_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureMigrationCandidate {
    old: String,
    new: String,
}

#[derive(Debug, Deserialize)]
struct FixtureObservation {
    observation_id: String,
    support: Vec<String>,
}

#[test]
fn fixture_visible_api_observation_computes_exact_golden_frontier() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let input = fixture_frontier_input(&fixture, "obs:api:create-user-visible");

    let result = compute_frontier(&store, &input).unwrap();

    assert!(result.diagnostics.is_empty());
    assert_eq!(
        corr_items(&result.entries),
        fixture.expected_frontier.correspondence_ids
    );
    assert_eq!(
        edge_items(&result.entries),
        fixture.expected_frontier.build_edge_ids
    );
    assert_entries_sorted(&result.entries);
    assert!(result.entries.iter().all(|entry| !entry.reasons.is_empty()));
}

#[test]
fn frontier_output_is_byte_stable_across_repeated_runs() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let input = fixture_frontier_input(&fixture, "obs:api:create-user-visible");

    let first = compute_frontier(&store, &input).unwrap();
    let second = compute_frontier(&store, &input).unwrap();

    assert_eq!(first, second);
}

#[test]
fn touched_correspondence_enters_frontier() {
    let graph = SmallGraph::new();
    let old_route = entity("old:openapi:route:openapi.yaml#/paths/~1users/post:aaaaaaaaaaaa");
    let input = FrontierInput {
        edited_artifacts: BTreeSet::new(),
        migration_map: MigrationMap::try_new(
            BTreeMap::from([(old_route, graph.new_route.clone())]),
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
        observation_id: "obs:test".to_owned(),
        support: vec![SupportRef::Corr(graph.route_corr.clone())],
    };

    let result = compute_frontier(&graph.store, &input).unwrap();

    let entry = result
        .entries
        .iter()
        .find(|entry| entry.item == FrontierItem::Correspondence(graph.route_corr.clone()))
        .unwrap();
    assert!(entry.reasons.contains(&FrontierReason::TouchedEndpoint));
}

#[test]
fn edited_artifact_build_chain_enters_frontier() {
    let graph = SmallGraph::new();
    let input = FrontierInput {
        edited_artifacts: BTreeSet::from([graph.source_artifact.clone()]),
        migration_map: MigrationMap::try_new(BTreeMap::new(), Vec::new(), Vec::new()).unwrap(),
        observation_id: "obs:test".to_owned(),
        support: vec![
            SupportRef::Edge(graph.first_edge.clone()),
            SupportRef::Edge(graph.second_edge.clone()),
        ],
    };

    let result = compute_frontier(&graph.store, &input).unwrap();

    assert_eq!(
        edge_items(&result.entries),
        vec![
            graph.first_edge.as_str().to_owned(),
            graph.second_edge.as_str().to_owned()
        ]
    );
    assert!(result.entries.iter().any(|entry| entry.item
        == FrontierItem::BuildEdge(graph.first_edge.clone())
        && entry.reasons.contains(&FrontierReason::EditedArtifactBuild)));
    assert!(result.entries.iter().any(|entry| entry.item
        == FrontierItem::BuildEdge(graph.second_edge.clone())
        && entry
            .reasons
            .contains(&FrontierReason::GeneratedArtifactBuild)));
}

#[test]
fn reachable_support_enters_and_unreachable_support_stays_out() {
    let graph = SmallGraph::new();
    let unrelated = corr("corr:event:0000000000000009");
    graph
        .store
        .save_correspondence(&Correspondence {
            corr_id: unrelated.clone(),
            kind: CorrespondenceKind::Event,
            rule_version: None,
            endpoints: vec![graph.unrelated_entity.clone()],
        })
        .unwrap();
    let input = FrontierInput {
        edited_artifacts: BTreeSet::from([graph.source_artifact.clone()]),
        migration_map: MigrationMap::try_new(BTreeMap::new(), Vec::new(), Vec::new()).unwrap(),
        observation_id: "obs:test".to_owned(),
        support: vec![
            SupportRef::Corr(graph.route_corr.clone()),
            SupportRef::Corr(unrelated.clone()),
        ],
    };

    let result = compute_frontier(&graph.store, &input).unwrap();

    assert!(result.entries.iter().any(|entry| entry.item
        == FrontierItem::Correspondence(graph.route_corr.clone())
        && entry.reasons.contains(&FrontierReason::ReachableSupport)));
    assert!(!result
        .entries
        .iter()
        .any(|entry| entry.item == FrontierItem::Correspondence(unrelated.clone())));
}

#[test]
fn missing_support_emits_diagnostic_and_is_not_accepted() {
    let graph = SmallGraph::new();
    let missing = corr("corr:route:9999999999999999");
    let input = FrontierInput {
        edited_artifacts: BTreeSet::from([graph.source_artifact.clone()]),
        migration_map: MigrationMap::try_new(BTreeMap::new(), Vec::new(), Vec::new()).unwrap(),
        observation_id: "obs:test".to_owned(),
        support: vec![SupportRef::Corr(missing.clone())],
    };

    let result = compute_frontier(&graph.store, &input).unwrap();

    assert_eq!(result.diagnostics.len(), 1);
    assert!(!result
        .entries
        .iter()
        .any(|entry| entry.item == FrontierItem::Correspondence(missing.clone())));
}

#[test]
fn closure_is_idempotent_for_same_graph_and_input() {
    let graph = SmallGraph::new();
    let input = FrontierInput {
        edited_artifacts: BTreeSet::from([graph.source_artifact.clone()]),
        migration_map: MigrationMap::try_new(BTreeMap::new(), Vec::new(), Vec::new()).unwrap(),
        observation_id: "obs:test".to_owned(),
        support: vec![
            SupportRef::Corr(graph.route_corr.clone()),
            SupportRef::Edge(graph.first_edge.clone()),
            SupportRef::Edge(graph.second_edge.clone()),
        ],
    };

    let first = compute_frontier(&graph.store, &input).unwrap();
    let second = compute_frontier(&graph.store, &input).unwrap();

    assert_eq!(first, second);
}

struct SmallGraph {
    store: SqliteGraphStore,
    source_artifact: ArtifactId,
    new_route: EntityId,
    unrelated_entity: EntityId,
    route_corr: CorrId,
    first_edge: EdgeId,
    second_edge: EdgeId,
}

impl SmallGraph {
    fn new() -> Self {
        let store = SqliteGraphStore::open_in_memory().unwrap();
        store.migrate().unwrap();
        let source_artifact = artifact("artifact:old:openapi.yaml:111111111111");
        let generated_artifact = artifact("artifact:old:client/sdk.ts:222222222222");
        let bundle_artifact = artifact("artifact:old:dist/client.js:333333333333");
        let unrelated_artifact = artifact("artifact:old:unrelated.py:444444444444");
        for id in [
            &source_artifact,
            &generated_artifact,
            &bundle_artifact,
            &unrelated_artifact,
        ] {
            store.save_artifact(&artifact_row_for_id(id)).unwrap();
        }
        let old_route = entity("old:openapi:route:openapi.yaml#/paths/~1users/post:aaaaaaaaaaaa");
        let new_route =
            entity("new:openapi:route:openapi.yaml#/paths/~1v2~1users/post:bbbbbbbbbbbb");
        let client = entity("old:ts:generated_client:client/sdk.ts#users:cccccccccccc");
        let unrelated_entity = entity("old:py:event:unrelated.py#event:dddddddddddd");
        for row in [
            entity_row_for_id(&old_route, &source_artifact),
            entity_row_for_id(&new_route, &source_artifact),
            entity_row_for_id(&client, &generated_artifact),
            entity_row_for_id(&unrelated_entity, &unrelated_artifact),
        ] {
            store.save_entity(&row).unwrap();
        }
        let route_corr = corr("corr:route:0000000000000001");
        store
            .save_correspondence(&Correspondence {
                corr_id: route_corr.clone(),
                kind: CorrespondenceKind::Route,
                rule_version: None,
                endpoints: vec![old_route, client],
            })
            .unwrap();
        let first_edge = edge("edge:build_codegen:0000000000000001");
        let second_edge = edge("edge:build_codegen:0000000000000002");
        store
            .save_build_edge(&BuildEdge {
                edge_id: first_edge.clone(),
                src_artifact: source_artifact.clone(),
                dst_artifact: generated_artifact.clone(),
            })
            .unwrap();
        store
            .save_build_edge(&BuildEdge {
                edge_id: second_edge.clone(),
                src_artifact: generated_artifact,
                dst_artifact: bundle_artifact,
            })
            .unwrap();
        Self {
            store,
            source_artifact,
            new_route,
            unrelated_entity,
            route_corr,
            first_edge,
            second_edge,
        }
    }
}

fn fixture_frontier_input(fixture: &Layer5Fixture, observation_id: &str) -> FrontierInput {
    let observation = fixture
        .observations
        .iter()
        .find(|obs| obs.observation_id == observation_id)
        .unwrap();
    FrontierInput {
        edited_artifacts: BTreeSet::from([
            artifact("artifact:old:Dockerfile:111100000001"),
            artifact("artifact:old:handler.py:111100000004"),
            artifact("artifact:old:openapi.yaml:111100000005"),
        ]),
        migration_map: fixture_migration_map(fixture),
        observation_id: observation.observation_id.clone(),
        support: observation
            .support
            .iter()
            .map(|item| support_ref(item))
            .collect(),
    }
}

fn fixture_migration_map(fixture: &Layer5Fixture) -> MigrationMap {
    let rewrites = fixture
        .migration_map_candidates
        .iter()
        .map(|candidate| (entity(&candidate.old), entity(&candidate.new)))
        .collect();
    MigrationMap::try_new(rewrites, Vec::new(), Vec::new()).unwrap()
}

fn seeded_store(fixture: &Layer5Fixture) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().unwrap();
    store.migrate().unwrap();
    for artifact in &fixture.artifacts {
        store
            .save_artifact(&fixture_artifact_row(artifact))
            .unwrap();
    }
    for entity in &fixture.entities {
        store.save_entity(&fixture_entity_row(entity)).unwrap();
    }
    for corr in &fixture.correspondences {
        store.save_correspondence(&fixture_corr_row(corr)).unwrap();
    }
    for edge in &fixture.build_edges {
        store.save_build_edge(&fixture_edge_row(edge)).unwrap();
    }
    store
}

fn load_fixture() -> Layer5Fixture {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join("expected.json");
    let contents = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&contents).unwrap()
}

fn fixture_artifact_row(artifact: &FixtureArtifact) -> Artifact {
    Artifact {
        artifact_id: ArtifactId::parse(&artifact.artifact_id).unwrap(),
        repo_side: repo_side(&artifact.side),
        kind: ArtifactKind::parse(&artifact.kind).unwrap(),
        language: Language::parse(&artifact.language).unwrap(),
        local_path: artifact.path.clone(),
        content_hash: artifact.content_hash.clone(),
    }
}

fn fixture_entity_row(entity: &FixtureEntity) -> Entity {
    Entity {
        entity_id: EntityId::parse(&entity.entity_id).unwrap(),
        artifact_id: ArtifactId::parse(&entity.artifact_id).unwrap(),
        repo_side: repo_side(&entity.side),
        language: Language::parse(&entity.language).unwrap(),
        kind: entity.kind.clone(),
        local_path: entity.local_path.clone(),
        stable_hash: entity.stable_hash.clone(),
    }
}

fn fixture_corr_row(corr: &FixtureCorrespondence) -> Correspondence {
    Correspondence {
        corr_id: CorrId::parse(&corr.corr_id).unwrap(),
        kind: CorrespondenceKind::parse(&corr.kind).unwrap(),
        rule_version: Some("layer5-fixture-v1".to_owned()),
        endpoints: corr.endpoints.iter().map(|id| entity(id)).collect(),
    }
}

fn fixture_edge_row(edge: &FixtureBuildEdge) -> BuildEdge {
    BuildEdge {
        edge_id: EdgeId::parse(&edge.edge_id).unwrap(),
        src_artifact: artifact(&edge.src_artifact),
        dst_artifact: artifact(&edge.dst_artifact),
    }
}

fn artifact_row_for_id(id: &ArtifactId) -> Artifact {
    Artifact {
        artifact_id: id.clone(),
        repo_side: RepoSide::Old,
        kind: ArtifactKind::Schema,
        language: Language::parse("openapi").unwrap(),
        local_path: id.as_str().split(':').nth(2).unwrap().to_owned(),
        content_hash: id.as_str().rsplit(':').next().unwrap().to_owned(),
    }
}

fn entity_row_for_id(id: &EntityId, artifact_id: &ArtifactId) -> Entity {
    Entity {
        entity_id: id.clone(),
        artifact_id: artifact_id.clone(),
        repo_side: if id.repo_side() == "old" {
            RepoSide::Old
        } else {
            RepoSide::New
        },
        language: Language::parse(id.language()).unwrap(),
        kind: id.kind().to_owned(),
        local_path: id.local_path().to_owned(),
        stable_hash: id.stable_hash().to_owned(),
    }
}

fn repo_side(side: &str) -> RepoSide {
    match side {
        "old" => RepoSide::Old,
        "new" => RepoSide::New,
        other => panic!("unexpected repo side: {other}"),
    }
}

fn support_ref(value: &str) -> SupportRef {
    if value.starts_with("corr:") {
        SupportRef::Corr(corr(value))
    } else if value.starts_with("edge:") {
        SupportRef::Edge(edge(value))
    } else {
        panic!("unexpected support: {value}")
    }
}

fn corr_items(entries: &[polyref_frontier::FrontierEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match &entry.item {
            FrontierItem::Correspondence(id) => Some(id.as_str().to_owned()),
            FrontierItem::BuildEdge(_) => None,
        })
        .collect()
}

fn edge_items(entries: &[polyref_frontier::FrontierEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match &entry.item {
            FrontierItem::BuildEdge(id) => Some(id.as_str().to_owned()),
            FrontierItem::Correspondence(_) => None,
        })
        .collect()
}

fn assert_entries_sorted(entries: &[polyref_frontier::FrontierEntry]) {
    for window in entries.windows(2) {
        assert!(window[0].item < window[1].item);
    }
}

fn artifact(value: &str) -> ArtifactId {
    ArtifactId::parse(value).unwrap()
}
fn entity(value: &str) -> EntityId {
    EntityId::parse(value).unwrap()
}
fn corr(value: &str) -> CorrId {
    CorrId::parse(value).unwrap()
}
fn edge(value: &str) -> EdgeId {
    EdgeId::parse(value).unwrap()
}
