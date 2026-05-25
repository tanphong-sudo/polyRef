#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_core::{
    artifact_kind::ArtifactKind,
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    language::Language,
    observation::{
        ApiCallObs, HttpMethod, ObsHeader, Observation, SupportRef, TestObs, Visibility,
    },
};
use polyref_graph::{
    build_migration_map, Artifact, BuildEdge, CandidateProvenance, Correspondence, Entity,
    EntityRewriteCandidate, GraphStore, MigrationMapDiagnosticKind, RepoSide, RewriteConfidence,
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
    migration_map_candidates: Vec<FixtureMigrationCandidate>,
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
    defined_semantics: bool,
    support: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureMigrationCandidate {
    old: String,
    new: String,
}

#[test]
fn fixture_candidates_build_type_respecting_migration_map() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let result = build_migration_map(&store, fixture_candidates(&fixture)).unwrap();

    assert!(result.migration_map.is_type_respecting());
    assert!(result.migration_map.conflicts().is_empty());
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.audit.candidate_count, 10);
    assert_eq!(result.audit.rewrite_count, 10);
    assert_eq!(result.audit.diagnostic_count, 0);

    let old_route = entity("old:openapi:route:openapi.yaml#/paths/~1users/post:100000000001");
    let new_route = entity("new:openapi:route:openapi.yaml#/paths/~1v2~1users/post:200000000001");
    let old_handler = entity("old:py:handler:handler.py#createUser:100000000002");
    let new_handler = entity("new:py:handler:handler.py#createUserV2:200000000002");

    assert_eq!(result.migration_map.get(&old_route), Some(&new_route));
    assert_eq!(result.migration_map.get(&old_handler), Some(&new_handler));

    let second = build_migration_map(&store, fixture_candidates_reversed(&fixture)).unwrap();
    assert_eq!(result.migration_map, second.migration_map);
    assert_eq!(result.audit, second.audit);
}

#[test]
fn cross_language_same_kind_rewrite_is_accepted() {
    let store = store_with_entities([
        entity_row(
            "old:ts:handler:src/users.ts#createUser:aaaaaaaaaaaa",
            "handler",
        ),
        entity_row(
            "new:py:handler:src/users.py#createUser:bbbbbbbbbbbb",
            "handler",
        ),
    ]);
    let old = entity("old:ts:handler:src/users.ts#createUser:aaaaaaaaaaaa");
    let new = entity("new:py:handler:src/users.py#createUser:bbbbbbbbbbbb");

    let result = build_migration_map(&store, [concrete(&old, &new, "cross-lang")]).unwrap();

    assert!(result.diagnostics.is_empty());
    assert_eq!(result.migration_map.get(&old), Some(&new));
}

#[test]
fn different_kind_rewrite_is_rejected() {
    let store = store_with_entities([
        entity_row(
            "old:ts:handler:src/users.ts#createUser:aaaaaaaaaaaa",
            "handler",
        ),
        entity_row(
            "new:openapi:route:openapi.yaml#/paths/~1users/post:bbbbbbbbbbbb",
            "route",
        ),
    ]);
    let old = entity("old:ts:handler:src/users.ts#createUser:aaaaaaaaaaaa");
    let new = entity("new:openapi:route:openapi.yaml#/paths/~1users/post:bbbbbbbbbbbb");

    let result = build_migration_map(&store, [concrete(&old, &new, "bad-kind")]).unwrap();

    assert_eq!(
        diagnostic_kinds(&result.diagnostics),
        [MigrationMapDiagnosticKind::KindMismatch]
    );
    assert_eq!(result.migration_map.get(&old), None);
    assert!(result.migration_map.is_type_respecting());
}

#[test]
fn duplicate_identical_concrete_rewrites_are_deduped() {
    let store = store_with_entities([
        entity_row(
            "old:py:handler:handler.py#createUser:aaaaaaaaaaaa",
            "handler",
        ),
        entity_row(
            "new:py:handler:handler.py#createUserV2:bbbbbbbbbbbb",
            "handler",
        ),
    ]);
    let old = entity("old:py:handler:handler.py#createUser:aaaaaaaaaaaa");
    let new = entity("new:py:handler:handler.py#createUserV2:bbbbbbbbbbbb");

    let result = build_migration_map(
        &store,
        [concrete(&old, &new, "ide"), concrete(&old, &new, "llm")],
    )
    .unwrap();

    assert!(result.diagnostics.is_empty());
    assert_eq!(result.audit.candidate_count, 2);
    assert_eq!(result.audit.rewrite_count, 1);
    assert_eq!(result.migration_map.iter().count(), 1);
}

#[test]
fn conflicting_concrete_rewrites_fail_closed_deterministically() {
    let store = store_with_entities([
        entity_row(
            "old:py:handler:handler.py#createUser:aaaaaaaaaaaa",
            "handler",
        ),
        entity_row("new:py:handler:handler.py#createA:111111111111", "handler"),
        entity_row("new:py:handler:handler.py#createB:222222222222", "handler"),
    ]);
    let old = entity("old:py:handler:handler.py#createUser:aaaaaaaaaaaa");
    let new_b = entity("new:py:handler:handler.py#createB:222222222222");
    let new_a = entity("new:py:handler:handler.py#createA:111111111111");

    let result = build_migration_map(
        &store,
        [concrete(&old, &new_b, "llm"), concrete(&old, &new_a, "ide")],
    )
    .unwrap();

    assert_eq!(
        diagnostic_kinds(&result.diagnostics),
        [MigrationMapDiagnosticKind::MigrationMapConflict]
    );
    assert_eq!(result.migration_map.get(&old), None);
    assert_eq!(result.migration_map.conflicts().len(), 1);
    assert_eq!(result.migration_map.conflicts()[0].first, new_a);
    assert_eq!(result.migration_map.conflicts()[0].second, new_b);
    assert!(!result.migration_map.is_type_respecting());
}

#[test]
fn ambiguous_missing_and_mixed_candidates_do_not_choose_target() {
    let store = store_with_entities([
        entity_row(
            "old:py:handler:handler.py#createUser:aaaaaaaaaaaa",
            "handler",
        ),
        entity_row(
            "new:py:handler:handler.py#createUserV2:bbbbbbbbbbbb",
            "handler",
        ),
        entity_row(
            "old:openapi:route:openapi.yaml#/paths/~1users/post:cccccccccccc",
            "route",
        ),
    ]);
    let old_handler = entity("old:py:handler:handler.py#createUser:aaaaaaaaaaaa");
    let new_handler = entity("new:py:handler:handler.py#createUserV2:bbbbbbbbbbbb");
    let old_route = entity("old:openapi:route:openapi.yaml#/paths/~1users/post:cccccccccccc");

    let result = build_migration_map(
        &store,
        [
            concrete(&old_handler, &new_handler, "llm"),
            ambiguous(&old_handler, Some(&new_handler), "extractor"),
            missing(&old_route, "missing-target"),
        ],
    )
    .unwrap();

    assert_eq!(
        diagnostic_kinds(&result.diagnostics),
        [
            MigrationMapDiagnosticKind::MissingEndpoint,
            MigrationMapDiagnosticKind::MigrationMapAmbiguous,
        ]
    );
    assert_eq!(result.migration_map.get(&old_handler), None);
    assert_eq!(result.migration_map.get(&old_route), None);
}

#[test]
fn missing_graph_entities_are_reported_as_missing_endpoint() {
    let store = store_with_entities([entity_row(
        "old:py:handler:handler.py#createUser:aaaaaaaaaaaa",
        "handler",
    )]);
    let old = entity("old:py:handler:handler.py#createUser:aaaaaaaaaaaa");
    let missing_new = entity("new:py:handler:handler.py#createUserV2:bbbbbbbbbbbb");

    let result =
        build_migration_map(&store, [concrete(&old, &missing_new, "missing-new")]).unwrap();

    assert_eq!(
        diagnostic_kinds(&result.diagnostics),
        [MigrationMapDiagnosticKind::MissingEndpoint]
    );
    assert_eq!(result.migration_map.get(&old), None);
}

#[test]
fn non_old_to_new_candidates_are_rejected() {
    let store = store_with_entities([
        entity_row(
            "old:py:handler:handler.py#createUser:aaaaaaaaaaaa",
            "handler",
        ),
        entity_row(
            "old:py:handler:handler.py#createUserV2:bbbbbbbbbbbb",
            "handler",
        ),
        entity_row("new:py:handler:handler.py#createA:cccccccccccc", "handler"),
        entity_row("new:py:handler:handler.py#createB:dddddddddddd", "handler"),
    ]);
    let old_source = entity("old:py:handler:handler.py#createUser:aaaaaaaaaaaa");
    let old_target = entity("old:py:handler:handler.py#createUserV2:bbbbbbbbbbbb");
    let new_source = entity("new:py:handler:handler.py#createA:cccccccccccc");
    let new_target = entity("new:py:handler:handler.py#createB:dddddddddddd");

    let result = build_migration_map(
        &store,
        [
            concrete(&old_source, &old_target, "old-to-old"),
            concrete(&new_source, &new_target, "new-to-new"),
        ],
    )
    .unwrap();

    assert_eq!(
        diagnostic_kinds(&result.diagnostics),
        [
            MigrationMapDiagnosticKind::MissingEndpoint,
            MigrationMapDiagnosticKind::MissingEndpoint,
        ]
    );
    assert_eq!(result.migration_map.iter().count(), 0);
}

fn diagnostic_kinds(
    diagnostics: &[polyref_graph::MigrationMapDiagnostic],
) -> Vec<MigrationMapDiagnosticKind> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.kind)
        .collect()
}

fn concrete(old: &EntityId, new: &EntityId, source: &str) -> EntityRewriteCandidate {
    candidate(old, Some(new), RewriteConfidence::Concrete, source)
}

fn ambiguous(old: &EntityId, new: Option<&EntityId>, source: &str) -> EntityRewriteCandidate {
    candidate(old, new, RewriteConfidence::Ambiguous, source)
}

fn missing(old: &EntityId, source: &str) -> EntityRewriteCandidate {
    candidate(old, None, RewriteConfidence::Missing, source)
}

fn candidate(
    old: &EntityId,
    new: Option<&EntityId>,
    confidence: RewriteConfidence,
    source: &str,
) -> EntityRewriteCandidate {
    EntityRewriteCandidate {
        old: old.clone(),
        new: new.cloned(),
        confidence,
        provenance: CandidateProvenance {
            source: source.to_owned(),
            payload_hash: None,
        },
    }
}

fn fixture_candidates(fixture: &Layer5Fixture) -> Vec<EntityRewriteCandidate> {
    fixture
        .migration_map_candidates
        .iter()
        .map(|candidate| {
            concrete(
                &entity(&candidate.old),
                &entity(&candidate.new),
                "layer5-fixture",
            )
        })
        .collect()
}

fn fixture_candidates_reversed(fixture: &Layer5Fixture) -> Vec<EntityRewriteCandidate> {
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
        store.save_entity(&fixture_entity_row(entity)).unwrap();
    }
    for corr in &fixture.correspondences {
        store.save_correspondence(&corr_row(corr)).unwrap();
    }
    for edge in &fixture.build_edges {
        store.save_build_edge(&edge_row(edge)).unwrap();
    }
    for observation in &fixture.observations {
        store
            .save_observation(
                &observation.observation_id,
                &observation_row(observation, fixture),
            )
            .unwrap();
    }

    store
}

fn store_with_entities<const N: usize>(entities: [Entity; N]) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().unwrap();
    store.migrate().unwrap();
    let artifact = Artifact {
        artifact_id: ArtifactId::parse("artifact:old:test:111111111111").unwrap(),
        repo_side: RepoSide::Old,
        kind: ArtifactKind::parse("source_file").unwrap(),
        language: Language::parse("ts").unwrap(),
        local_path: "test".to_owned(),
        content_hash: "111111111111".to_owned(),
    };
    store.save_artifact(&artifact).unwrap();
    for entity in entities {
        store.save_entity(&entity).unwrap();
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

fn entity_row(id: &str, kind: &str) -> Entity {
    let entity_id = entity(id);
    Entity {
        entity_id: entity_id.clone(),
        artifact_id: ArtifactId::parse("artifact:old:test:111111111111").unwrap(),
        repo_side: repo_side(entity_id.repo_side()),
        language: Language::parse(entity_id.language()).unwrap(),
        kind: kind.to_owned(),
        local_path: entity_id.local_path().to_owned(),
        stable_hash: entity_id.stable_hash().to_owned(),
    }
}

fn corr_row(corr: &FixtureCorrespondence) -> Correspondence {
    Correspondence {
        corr_id: CorrId::parse(&corr.corr_id).unwrap(),
        kind: CorrespondenceKind::parse(&corr.kind).unwrap(),
        rule_version: None,
        endpoints: corr.endpoints.iter().map(|id| entity(id)).collect(),
    }
}

fn edge_row(edge: &FixtureBuildEdge) -> BuildEdge {
    BuildEdge {
        edge_id: EdgeId::parse(&edge.edge_id).unwrap(),
        src_artifact: ArtifactId::parse(&edge.src_artifact).unwrap(),
        dst_artifact: ArtifactId::parse(&edge.dst_artifact).unwrap(),
    }
}

fn observation_row(observation: &FixtureObservation, fixture: &Layer5Fixture) -> Observation {
    let header = ObsHeader {
        visibility: match observation.visibility.as_str() {
            "visible" => Visibility::Visible,
            "held_out" => Visibility::HeldOut,
            other => panic!("unknown visibility {other}"),
        },
        defined_semantics: observation.defined_semantics,
        support: observation
            .support
            .iter()
            .map(|item| support_ref(item))
            .collect(),
    };

    match observation.kind.as_str() {
        "api_call" => Observation::ApiCall(ApiCallObs {
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
            header,
        }),
        "test_invocation" => Observation::TestInvocation(TestObs {
            test_id: entity_id(
                fixture,
                "old:py:test:tests/test_users.py#test_create_user:100000000011",
            ),
            public_entrypoint: Some(entity_id(
                fixture,
                "old:openapi:route:openapi.yaml#/paths/~1users/post:100000000001",
            )),
            header,
        }),
        other => panic!("unknown observation kind {other}"),
    }
}

fn entity_id(fixture: &Layer5Fixture, id: &str) -> EntityId {
    assert!(fixture.entities.iter().any(|entity| entity.entity_id == id));
    EntityId::parse(id).unwrap()
}

fn support_ref(item: &str) -> SupportRef {
    if item.starts_with("corr:") {
        SupportRef::Corr(CorrId::parse(item).unwrap())
    } else if item.starts_with("edge:") {
        SupportRef::Edge(EdgeId::parse(item).unwrap())
    } else {
        panic!("unknown support ref {item}")
    }
}

fn repo_side(side: &str) -> RepoSide {
    match side {
        "old" => RepoSide::Old,
        "new" => RepoSide::New,
        other => panic!("unknown side {other}"),
    }
}

fn entity(id: &str) -> EntityId {
    EntityId::parse(id).unwrap()
}
