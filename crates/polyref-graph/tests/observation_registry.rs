#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_core::{
    artifact_kind::ArtifactKind,
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    language::Language,
    observation::{HttpMethod, SupportRef, Visibility},
};
use polyref_graph::{
    register_observations, Artifact, BuildEdge, Correspondence, Entity, GraphReadModel, GraphStore,
    ObservationRegistrationSpec, ObservationRegistryDiagnosticKind, ObservationSpecKind, RepoSide,
    SqliteGraphStore,
};
use serde::Deserialize;
use std::path::Path;

const FIXTURE_ROOT: &str = "../../fixtures/layer5/users-route-frontier";

#[derive(Debug, Deserialize)]
struct Layer5Fixture {
    artifacts: Vec<FixtureArtifact>,
    entities: Vec<FixtureEntity>,
    correspondences: Vec<FixtureCorrespondence>,
    build_edges: Vec<FixtureBuildEdge>,
    observations: Vec<FixtureObservation>,
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
struct FixtureObservation {
    observation_id: String,
    visibility: String,
    kind: String,
    support: Vec<String>,
}

#[test]
fn registry_persists_visible_and_held_out_fixture_observations() {
    let fixture = load_fixture();
    let store = seeded_store_without_observations(&fixture);
    let specs = fixture_specs(&fixture);

    let first = register_observations(&store, specs.clone()).unwrap();
    let second = register_observations(&store, specs).unwrap();

    assert_eq!(first.registered_count, 2);
    assert!(first.diagnostics.is_empty());
    assert_eq!(first, second);

    let visible = store
        .find_observation("obs:api:create-user-visible")
        .unwrap()
        .unwrap();
    assert_eq!(visible.header().visibility, Visibility::Visible);
    assert!(visible.header().defined_semantics);
    assert_sorted_support(&visible.header().support);
    assert_eq!(visible.header().support.len(), 10);

    let held_out = store
        .find_observation("obs:test:create-user-held-out")
        .unwrap()
        .unwrap();
    assert_eq!(held_out.header().visibility, Visibility::HeldOut);
    assert!(held_out.header().defined_semantics);
    assert_sorted_support(&held_out.header().support);

    let rows = store.list_observations().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].observation_id, "obs:api:create-user-visible");
    assert_eq!(rows[1].observation_id, "obs:test:create-user-held-out");
}

#[test]
fn registry_dedupes_duplicate_support_and_reports_diagnostic() {
    let fixture = load_fixture();
    let store = seeded_store_without_observations(&fixture);
    let route = SupportRef::Corr(CorrId::parse("corr:route:0000000000000001").unwrap());

    let result = register_observations(
        &store,
        [ObservationRegistrationSpec {
            observation_id: "obs:api:duplicate-support".to_owned(),
            visibility: Visibility::Visible,
            kind: api_kind(&fixture),
            support: vec![route.clone(), route.clone()],
            unsupported_evidence: Vec::new(),
        }],
    )
    .unwrap();

    assert_eq!(
        diagnostic_kinds(&result),
        [ObservationRegistryDiagnosticKind::DuplicateSupport]
    );
    let stored = store
        .find_observation("obs:api:duplicate-support")
        .unwrap()
        .unwrap();
    assert!(stored.header().defined_semantics);
    assert_eq!(stored.header().support, vec![route]);
}

#[test]
fn registry_marks_missing_support_as_undefined_semantics() {
    let fixture = load_fixture();
    let store = seeded_store_without_observations(&fixture);
    let missing = SupportRef::Corr(CorrId::parse("corr:route:9999999999999999").unwrap());

    let result = register_observations(
        &store,
        [ObservationRegistrationSpec {
            observation_id: "obs:api:missing-support".to_owned(),
            visibility: Visibility::Visible,
            kind: api_kind(&fixture),
            support: vec![missing],
            unsupported_evidence: Vec::new(),
        }],
    )
    .unwrap();

    assert_eq!(
        diagnostic_kinds(&result),
        [ObservationRegistryDiagnosticKind::MissingSupport]
    );
    let stored = store
        .find_observation("obs:api:missing-support")
        .unwrap()
        .unwrap();
    assert!(!stored.header().defined_semantics);
    assert!(stored.header().support.is_empty());
}

#[test]
fn registry_marks_dynamic_evidence_as_undefined_semantics() {
    let fixture = load_fixture();
    let store = seeded_store_without_observations(&fixture);
    let route = SupportRef::Corr(CorrId::parse("corr:route:0000000000000001").unwrap());

    let result = register_observations(
        &store,
        [ObservationRegistrationSpec {
            observation_id: "obs:api:dynamic-route".to_owned(),
            visibility: Visibility::Visible,
            kind: api_kind(&fixture),
            support: vec![route],
            unsupported_evidence: vec!["dynamic_route_path".to_owned()],
        }],
    )
    .unwrap();

    assert_eq!(
        diagnostic_kinds(&result),
        [ObservationRegistryDiagnosticKind::UnsupportedEvidence]
    );
    let stored = store
        .find_observation("obs:api:dynamic-route")
        .unwrap()
        .unwrap();
    assert!(!stored.header().defined_semantics);
}

#[test]
fn registry_rejects_invalid_observation_ids_without_persisting() {
    let fixture = load_fixture();
    let store = seeded_store_without_observations(&fixture);
    let route = SupportRef::Corr(CorrId::parse("corr:route:0000000000000001").unwrap());

    let result = register_observations(
        &store,
        [ObservationRegistrationSpec {
            observation_id: "../secret".to_owned(),
            visibility: Visibility::Visible,
            kind: api_kind(&fixture),
            support: vec![route],
            unsupported_evidence: Vec::new(),
        }],
    )
    .unwrap();

    assert_eq!(
        diagnostic_kinds(&result),
        [ObservationRegistryDiagnosticKind::InvalidObservationId]
    );
    assert_eq!(result.registered_count, 0);
    assert!(store.list_observations().unwrap().is_empty());
}

fn diagnostic_kinds(
    result: &polyref_graph::ObservationRegistryResult,
) -> Vec<ObservationRegistryDiagnosticKind> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.kind)
        .collect()
}

fn fixture_specs(fixture: &Layer5Fixture) -> Vec<ObservationRegistrationSpec> {
    fixture
        .observations
        .iter()
        .map(|observation| ObservationRegistrationSpec {
            observation_id: observation.observation_id.clone(),
            visibility: visibility(&observation.visibility),
            kind: match observation.kind.as_str() {
                "api_call" => api_kind(fixture),
                "test_invocation" => test_kind(fixture),
                other => panic!("unexpected observation kind: {other}"),
            },
            support: observation
                .support
                .iter()
                .map(|support| support_ref(support))
                .collect(),
            unsupported_evidence: Vec::new(),
        })
        .collect()
}

fn api_kind(fixture: &Layer5Fixture) -> ObservationSpecKind {
    ObservationSpecKind::ApiCall {
        method: HttpMethod::Post,
        path: "/users".to_owned(),
        request_schema_id: Some(entity_id(
            fixture,
            "old:openapi:schema:openapi.yaml#/components/schemas/UserCreateV1:100000000003",
        )),
        response_schema_id: Some(entity_id(
            fixture,
            "old:openapi:schema:openapi.yaml#/components/schemas/UserV1:100000000004",
        )),
        client_id: Some(entity_id(
            fixture,
            "old:ts:generated_client:client/sdk.ts#users_client:100000000006",
        )),
    }
}

fn test_kind(fixture: &Layer5Fixture) -> ObservationSpecKind {
    ObservationSpecKind::TestInvocation {
        test_id: entity_id(
            fixture,
            "old:py:test:tests/test_users.py#test_create_user:100000000011",
        ),
        public_entrypoint: Some(entity_id(
            fixture,
            "old:openapi:route:openapi.yaml#/paths/~1users/post:100000000001",
        )),
    }
}

fn seeded_store_without_observations(fixture: &Layer5Fixture) -> SqliteGraphStore {
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
        artifact_id: ArtifactId::parse(&artifact.artifact_id).unwrap(),
        repo_side: repo_side(&artifact.side),
        kind: ArtifactKind::parse(&artifact.kind).unwrap(),
        language: Language::parse(&artifact.language).unwrap(),
        local_path: artifact.path.clone(),
        content_hash: artifact.content_hash.clone(),
    }
}

fn entity_row(entity: &FixtureEntity) -> Entity {
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

fn corr_row(corr: &FixtureCorrespondence) -> Correspondence {
    Correspondence {
        corr_id: CorrId::parse(&corr.corr_id).unwrap(),
        kind: CorrespondenceKind::parse(&corr.kind).unwrap(),
        rule_version: Some("layer5-fixture-v1".to_owned()),
        endpoints: corr
            .endpoints
            .iter()
            .map(|id| EntityId::parse(id).unwrap())
            .collect(),
    }
}

fn edge_row(edge: &FixtureBuildEdge) -> BuildEdge {
    BuildEdge {
        edge_id: EdgeId::parse(&edge.edge_id).unwrap(),
        src_artifact: ArtifactId::parse(&edge.src_artifact).unwrap(),
        dst_artifact: ArtifactId::parse(&edge.dst_artifact).unwrap(),
    }
}

fn support_ref(support: &str) -> SupportRef {
    if support.starts_with("corr:") {
        SupportRef::Corr(CorrId::parse(support).unwrap())
    } else if support.starts_with("edge:") {
        SupportRef::Edge(EdgeId::parse(support).unwrap())
    } else {
        panic!("unexpected support ref: {support}")
    }
}

fn entity_id(fixture: &Layer5Fixture, id: &str) -> EntityId {
    assert!(fixture.entities.iter().any(|entity| entity.entity_id == id));
    EntityId::parse(id).unwrap()
}

fn repo_side(side: &str) -> RepoSide {
    match side {
        "old" => RepoSide::Old,
        "new" => RepoSide::New,
        other => panic!("unexpected repo side: {other}"),
    }
}

fn visibility(value: &str) -> Visibility {
    match value {
        "visible" => Visibility::Visible,
        "held_out" => Visibility::HeldOut,
        "evaluation_only" => Visibility::EvaluationOnly,
        other => panic!("unexpected visibility: {other}"),
    }
}

fn assert_sorted_support(support: &[SupportRef]) {
    let keys: Vec<_> = support.iter().map(support_key).collect();
    for window in keys.windows(2) {
        assert!(window[0] < window[1]);
    }
}

fn support_key(support: &SupportRef) -> String {
    match support {
        SupportRef::Corr(id) => id.as_str().to_owned(),
        SupportRef::Edge(id) => id.as_str().to_owned(),
        _ => panic!("unexpected support ref variant"),
    }
}
