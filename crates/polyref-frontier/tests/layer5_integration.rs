#![allow(clippy::unwrap_used, clippy::panic)]

use polyref_core::{
    artifact_kind::ArtifactKind,
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    language::Language,
    observation::{HttpMethod, SupportRef, Visibility},
    status::UnknownReason,
};
use polyref_frontier::{
    classify_coverage_risk, compute_frontier, CoverageRiskInput, FrontierInput, FrontierItem,
};
use polyref_graph::{
    build_migration_map, register_observations, Artifact, BuildEdge, CandidateProvenance,
    Correspondence, Entity, EntityRewriteCandidate, GraphReadModel, GraphStore,
    MigrationMapDiagnosticKind, ObservationRegistrationSpec, ObservationSpecKind, RepoSide,
    RewriteConfidence, SqliteGraphStore,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const FIXTURE_ROOT: &str = "../../fixtures/layer5/users-route-frontier";
const VISIBLE_OBSERVATION_ID: &str = "obs:api:create-user-visible";

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
    kind: String,
    observation_id: String,
    support: Vec<String>,
    visibility: String,
}

#[test]
fn full_layer5_fixture_path_is_clean_and_matches_golden_frontier() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let pipeline = run_pipeline(&store, &fixture, fixture_candidates(&fixture));

    assert!(pipeline.registry_diagnostics.is_empty());
    assert!(pipeline.migration_diagnostics.is_empty());
    assert!(pipeline.frontier.diagnostics.is_empty());
    assert_eq!(pipeline.coverage.observation_id, VISIBLE_OBSERVATION_ID);
    assert!(!pipeline.coverage.is_blocked);
    assert!(pipeline.coverage.risks.is_empty());
    assert_eq!(
        pipeline.frontier_corr_ids(),
        fixture.expected_frontier.correspondence_ids
    );
    assert_eq!(
        pipeline.frontier_edge_ids(),
        fixture.expected_frontier.build_edge_ids
    );
}

#[test]
fn full_layer5_pipeline_is_byte_stable_across_repeated_runs_and_shuffled_candidates() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);

    let first = run_pipeline(&store, &fixture, fixture_candidates(&fixture));
    let second = run_pipeline(&store, &fixture, fixture_candidates(&fixture));
    let shuffled = run_pipeline(&store, &fixture, reversed_fixture_candidates(&fixture));

    assert_eq!(first, second);
    assert_eq!(first, shuffled);
}

#[test]
fn every_frontier_item_is_graph_backed_and_fixture_reachable() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let pipeline = run_pipeline(&store, &fixture, fixture_candidates(&fixture));
    let graph_corrs = store
        .list_correspondences()
        .unwrap()
        .into_iter()
        .map(|corr| corr.corr_id)
        .collect::<BTreeSet<_>>();
    let graph_edges = store
        .list_build_edges()
        .unwrap()
        .into_iter()
        .map(|edge| edge.edge_id)
        .collect::<BTreeSet<_>>();
    let support = store
        .observation_support(VISIBLE_OBSERVATION_ID)
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|support_ref| support_key(&support_ref))
        .collect::<BTreeSet<_>>();

    for entry in &pipeline.frontier.entries {
        match &entry.item {
            FrontierItem::Correspondence(id) => {
                assert!(graph_corrs.contains(id));
                assert!(support.contains(id.as_str()));
            }
            FrontierItem::BuildEdge(id) => {
                assert!(graph_edges.contains(id));
                assert!(support.contains(id.as_str()));
            }
        }
    }
}

#[test]
fn frontier_closure_is_idempotent_when_frontier_items_become_support() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let pipeline = run_pipeline(&store, &fixture, fixture_candidates(&fixture));
    let migration = build_migration_map(&store, fixture_candidates(&fixture)).unwrap();
    let frontier_support = pipeline
        .frontier
        .entries
        .iter()
        .map(|entry| match &entry.item {
            FrontierItem::Correspondence(id) => SupportRef::Corr(id.clone()),
            FrontierItem::BuildEdge(id) => SupportRef::Edge(id.clone()),
        })
        .collect::<Vec<_>>();

    let recomputed = compute_frontier(
        &store,
        &FrontierInput {
            edited_artifacts: fixture_edited_artifacts(),
            migration_map: migration.migration_map,
            observation_id: VISIBLE_OBSERVATION_ID.to_owned(),
            support: frontier_support,
        },
    )
    .unwrap();

    assert_eq!(pipeline.frontier, recomputed);
}

#[test]
fn reachable_fixture_support_is_closed_into_frontier_without_extras() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let pipeline = run_pipeline(&store, &fixture, fixture_candidates(&fixture));
    let expected_support = fixture
        .expected_frontier
        .correspondence_ids
        .iter()
        .chain(fixture.expected_frontier.build_edge_ids.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let actual_frontier = pipeline
        .frontier
        .entries
        .iter()
        .map(|entry| match &entry.item {
            FrontierItem::Correspondence(id) => id.as_str().to_owned(),
            FrontierItem::BuildEdge(id) => id.as_str().to_owned(),
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(actual_frontier, expected_support);
}

#[test]
fn missing_support_is_not_coverage_clean() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let migration = build_migration_map(&store, fixture_candidates(&fixture)).unwrap();
    let missing = SupportRef::Corr(corr("corr:route:9999999999999999"));
    let input = FrontierInput {
        edited_artifacts: fixture_edited_artifacts(),
        migration_map: migration.migration_map,
        observation_id: VISIBLE_OBSERVATION_ID.to_owned(),
        support: vec![missing],
    };

    let frontier = compute_frontier(&store, &input).unwrap();
    let coverage = classify_coverage_risk(CoverageRiskInput {
        observation_id: VISIBLE_OBSERVATION_ID.to_owned(),
        frontier,
        support: input.support,
        registry_diagnostics: Vec::new(),
        migration_diagnostics: Vec::new(),
        unsupported_features: Vec::new(),
    });

    assert!(coverage.is_blocked);
    assert_eq!(coverage.risks.len(), 1);
    assert_eq!(coverage.risks[0].reason, UnknownReason::MissingEndpoint);
}

#[test]
fn accepted_migration_map_rewrites_are_type_respecting() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let result = build_migration_map(&store, fixture_candidates(&fixture)).unwrap();
    let entities = store
        .list_entities()
        .unwrap()
        .into_iter()
        .map(|entity| (entity.entity_id.clone(), entity))
        .collect::<BTreeMap<_, _>>();

    assert!(result.migration_map.is_type_respecting());
    for (old, new) in result.migration_map.iter() {
        let old_entity = entities.get(old).unwrap();
        let new_entity = entities.get(new).unwrap();
        assert_eq!(old.kind(), new.kind());
        assert_eq!(old_entity.kind, new_entity.kind);
        assert_eq!(old.repo_side(), "old");
        assert_eq!(new.repo_side(), "new");
    }
}

#[test]
fn ambiguous_migration_candidate_blocks_clean_coverage() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let old = entity("old:py:handler:handler.py#createUser:100000000002");
    register_observations(&store, fixture_specs(&fixture)).unwrap();
    let result = build_migration_map(
        &store,
        [EntityRewriteCandidate {
            old: old.clone(),
            new: None,
            confidence: RewriteConfidence::Ambiguous,
            provenance: CandidateProvenance {
                source: "layer5-integration-test".to_owned(),
                payload_hash: None,
            },
        }],
    )
    .unwrap();

    assert!(result.migration_map.iter().next().is_none());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(
        result.diagnostics[0].kind,
        MigrationMapDiagnosticKind::MigrationMapAmbiguous
    );

    let coverage = classify_coverage_risk(CoverageRiskInput {
        observation_id: VISIBLE_OBSERVATION_ID.to_owned(),
        frontier: compute_frontier(
            &store,
            &FrontierInput {
                edited_artifacts: fixture_edited_artifacts(),
                migration_map: result.migration_map,
                observation_id: VISIBLE_OBSERVATION_ID.to_owned(),
                support: store
                    .observation_support(VISIBLE_OBSERVATION_ID)
                    .unwrap()
                    .unwrap(),
            },
        )
        .unwrap(),
        support: Vec::new(),
        registry_diagnostics: Vec::new(),
        migration_diagnostics: result.diagnostics,
        unsupported_features: Vec::new(),
    });

    assert!(coverage.is_blocked);
    assert!(coverage
        .risks
        .iter()
        .any(|risk| risk.reason == UnknownReason::MigrationMapAmbiguous));
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Layer5PipelineOutput {
    registry_diagnostics: Vec<polyref_graph::ObservationRegistryDiagnostic>,
    migration_diagnostics: Vec<polyref_graph::MigrationMapDiagnostic>,
    frontier: polyref_frontier::FrontierResult,
    coverage: polyref_frontier::CoverageRiskReport,
}

impl Layer5PipelineOutput {
    fn frontier_corr_ids(&self) -> Vec<String> {
        self.frontier
            .entries
            .iter()
            .filter_map(|entry| match &entry.item {
                FrontierItem::Correspondence(id) => Some(id.as_str().to_owned()),
                FrontierItem::BuildEdge(_) => None,
            })
            .collect()
    }

    fn frontier_edge_ids(&self) -> Vec<String> {
        self.frontier
            .entries
            .iter()
            .filter_map(|entry| match &entry.item {
                FrontierItem::Correspondence(_) => None,
                FrontierItem::BuildEdge(id) => Some(id.as_str().to_owned()),
            })
            .collect()
    }
}

fn run_pipeline(
    store: &SqliteGraphStore,
    fixture: &Layer5Fixture,
    candidates: Vec<EntityRewriteCandidate>,
) -> Layer5PipelineOutput {
    let registry = register_observations(store, fixture_specs(fixture)).unwrap();
    let migration = build_migration_map(store, candidates).unwrap();
    let support = store
        .observation_support(VISIBLE_OBSERVATION_ID)
        .unwrap()
        .unwrap();
    let frontier = compute_frontier(
        store,
        &FrontierInput {
            edited_artifacts: fixture_edited_artifacts(),
            migration_map: migration.migration_map,
            observation_id: VISIBLE_OBSERVATION_ID.to_owned(),
            support: support.clone(),
        },
    )
    .unwrap();
    let coverage = classify_coverage_risk(CoverageRiskInput {
        observation_id: VISIBLE_OBSERVATION_ID.to_owned(),
        frontier: frontier.clone(),
        support,
        registry_diagnostics: registry.diagnostics.clone(),
        migration_diagnostics: migration.diagnostics.clone(),
        unsupported_features: Vec::new(),
    });

    Layer5PipelineOutput {
        registry_diagnostics: registry.diagnostics,
        migration_diagnostics: migration.diagnostics,
        frontier,
        coverage,
    }
}

fn fixture_specs(fixture: &Layer5Fixture) -> Vec<ObservationRegistrationSpec> {
    fixture
        .observations
        .iter()
        .map(|observation| ObservationRegistrationSpec {
            observation_id: observation.observation_id.clone(),
            visibility: Visibility::parse(&observation.visibility).unwrap(),
            kind: match observation.kind.as_str() {
                "api_call" => ObservationSpecKind::ApiCall {
                    method: HttpMethod::Post,
                    path: "/users".to_owned(),
                    request_schema_id: Some(entity(
                        "old:openapi:schema:openapi.yaml#/components/schemas/UserCreateV1:100000000003",
                    )),
                    response_schema_id: Some(entity(
                        "old:openapi:schema:openapi.yaml#/components/schemas/UserV1:100000000004",
                    )),
                    client_id: Some(entity(
                        "old:ts:generated_client:client/sdk.ts#users_client:100000000006",
                    )),
                },
                "test_invocation" => ObservationSpecKind::TestInvocation {
                    test_id: entity("old:py:test:tests/test_users.py#test_create_user:100000000011"),
                    public_entrypoint: Some(entity(
                        "old:openapi:route:openapi.yaml#/paths/~1users/post:100000000001",
                    )),
                },
                other => panic!("unexpected observation kind: {other}"),
            },
            support: observation.support.iter().map(|item| support_ref(item)).collect(),
            unsupported_evidence: Vec::new(),
        })
        .collect()
}

fn fixture_candidates(fixture: &Layer5Fixture) -> Vec<EntityRewriteCandidate> {
    fixture
        .migration_map_candidates
        .iter()
        .map(|candidate| EntityRewriteCandidate {
            old: entity(&candidate.old),
            new: Some(entity(&candidate.new)),
            confidence: RewriteConfidence::Concrete,
            provenance: CandidateProvenance {
                source: "layer5-fixture".to_owned(),
                payload_hash: None,
            },
        })
        .collect()
}

fn reversed_fixture_candidates(fixture: &Layer5Fixture) -> Vec<EntityRewriteCandidate> {
    let mut candidates = fixture_candidates(fixture);
    candidates.reverse();
    candidates
}

fn seeded_store(fixture: &Layer5Fixture) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().unwrap();
    store.migrate().unwrap();
    for artifact in &fixture.artifacts {
        store.save_artifact(&artifact_row(artifact)).unwrap();
    }
    for entity in &fixture.entities {
        store.save_entity(&entity_row(entity)).unwrap();
    }
    for corr in &fixture.correspondences {
        store.save_correspondence(&corr_row(corr)).unwrap();
    }
    for edge in &fixture.build_edges {
        store.save_build_edge(&edge_row(edge)).unwrap();
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

fn artifact_row(artifact: &FixtureArtifact) -> Artifact {
    Artifact {
        artifact_id: artifact_id(&artifact.artifact_id),
        repo_side: repo_side(&artifact.side),
        kind: ArtifactKind::parse(&artifact.kind).unwrap(),
        language: Language::parse(&artifact.language).unwrap(),
        local_path: artifact.path.clone(),
        content_hash: artifact.content_hash.clone(),
    }
}

fn entity_row(entity: &FixtureEntity) -> Entity {
    Entity {
        entity_id: self::entity(&entity.entity_id),
        artifact_id: artifact_id(&entity.artifact_id),
        repo_side: repo_side(&entity.side),
        language: Language::parse(&entity.language).unwrap(),
        kind: entity.kind.clone(),
        local_path: entity.local_path.clone(),
        stable_hash: entity.stable_hash.clone(),
    }
}

fn corr_row(corr: &FixtureCorrespondence) -> Correspondence {
    Correspondence {
        corr_id: self::corr(&corr.corr_id),
        kind: CorrespondenceKind::parse(&corr.kind).unwrap(),
        rule_version: Some("layer5-fixture-v1".to_owned()),
        endpoints: corr.endpoints.iter().map(|id| entity(id)).collect(),
    }
}

fn edge_row(edge: &FixtureBuildEdge) -> BuildEdge {
    BuildEdge {
        edge_id: self::edge(&edge.edge_id),
        src_artifact: artifact_id(&edge.src_artifact),
        dst_artifact: artifact_id(&edge.dst_artifact),
    }
}

fn fixture_edited_artifacts() -> BTreeSet<ArtifactId> {
    BTreeSet::from([
        artifact_id("artifact:old:Dockerfile:111100000001"),
        artifact_id("artifact:old:handler.py:111100000004"),
        artifact_id("artifact:old:openapi.yaml:111100000005"),
    ])
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
        panic!("unexpected support ref: {value}");
    }
}

fn artifact_id(value: &str) -> ArtifactId {
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

fn support_key(support_ref: &SupportRef) -> String {
    match support_ref {
        SupportRef::Corr(id) => id.as_str().to_owned(),
        SupportRef::Edge(id) => id.as_str().to_owned(),
        _ => panic!("unsupported support ref"),
    }
}
