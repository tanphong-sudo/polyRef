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
    Artifact, BuildEdge, Correspondence, Entity, GraphReadModel, GraphStore, RepoSide,
    SqliteGraphStore,
};
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

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
    defined_semantics: bool,
    support: Vec<String>,
}

#[test]
fn read_model_lists_rows_in_deterministic_order() {
    let store = seeded_store();

    assert_strictly_sorted(
        store
            .list_artifacts()
            .unwrap()
            .iter()
            .map(|artifact| artifact.artifact_id.as_str()),
    );
    assert_strictly_sorted(
        store
            .list_entities()
            .unwrap()
            .iter()
            .map(|entity| entity.entity_id.as_str()),
    );
    assert_strictly_sorted(
        store
            .list_correspondences()
            .unwrap()
            .iter()
            .map(|corr| corr.corr_id.as_str()),
    );
    assert_strictly_sorted(
        store
            .list_build_edges()
            .unwrap()
            .iter()
            .map(|edge| edge.edge_id.as_str()),
    );
    assert_strictly_sorted(
        store
            .list_observations()
            .unwrap()
            .iter()
            .map(|record| record.observation_id.as_str()),
    );

    assert_eq!(
        store.list_artifacts().unwrap(),
        store.list_artifacts().unwrap()
    );
    assert_eq!(
        store.list_entities().unwrap(),
        store.list_entities().unwrap()
    );
    assert_eq!(
        store.list_correspondences().unwrap(),
        store.list_correspondences().unwrap()
    );
    assert_eq!(
        store.list_build_edges().unwrap(),
        store.list_build_edges().unwrap()
    );
}

#[test]
fn read_model_resolves_endpoint_and_build_edge_indexes() {
    let store = seeded_store();
    let old_route =
        EntityId::parse("old:openapi:route:openapi.yaml#/paths/~1users/post:100000000001").unwrap();
    let openapi_artifact = ArtifactId::parse("artifact:old:openapi.yaml:111100000005").unwrap();
    let generated_client = ArtifactId::parse("artifact:old:client/sdk.ts:111100000002").unwrap();

    let route_corrs = store.correspondences_for_entity(&old_route).unwrap();
    let route_corr_ids: BTreeSet<_> = route_corrs
        .iter()
        .map(|corr| corr.corr_id.as_str())
        .collect();
    assert!(route_corr_ids.contains("corr:route:0000000000000001"));
    assert!(route_corr_ids.contains("corr:generated_client:0000000000000004"));
    assert!(!route_corr_ids.contains("corr:event:0000000000000006"));

    let route_corr = route_corrs
        .iter()
        .find(|corr| corr.corr_id.as_str() == "corr:route:0000000000000001")
        .unwrap();
    assert_eq!(route_corr.endpoints[0], old_route);

    let outgoing = store.build_edges_from(&openapi_artifact).unwrap();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(
        outgoing[0].edge_id.as_str(),
        "edge:build_codegen:0000000000000003"
    );

    let incoming = store.build_edges_to(&generated_client).unwrap();
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].src_artifact, openapi_artifact);
}

#[test]
fn read_model_loads_observation_support_refs() {
    let store = seeded_store();
    let support = store
        .observation_support("obs:api:create-user-visible")
        .unwrap();

    assert_eq!(support.len(), 10);
    assert!(support.contains(&SupportRef::Corr(
        CorrId::parse("corr:route:0000000000000001").unwrap()
    )));
    assert!(support.contains(&SupportRef::Edge(
        EdgeId::parse("edge:build_codegen:0000000000000003").unwrap()
    )));

    let observations = store.list_observations().unwrap();
    assert_eq!(observations.len(), 2);
    assert_eq!(
        observations[0].observation_id,
        "obs:api:create-user-visible"
    );
    assert_eq!(observations[0].observation.header().support, support);
}

fn seeded_store() -> SqliteGraphStore {
    let fixture = load_fixture();
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
    for observation in &fixture.observations {
        store
            .save_observation(
                &observation.observation_id,
                &observation_row(observation, &fixture),
            )
            .unwrap();
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
            .map(|endpoint| EntityId::parse(endpoint).unwrap())
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

fn observation_row(observation: &FixtureObservation, fixture: &Layer5Fixture) -> Observation {
    let header = ObsHeader {
        visibility: visibility(&observation.visibility),
        support: observation
            .support
            .iter()
            .map(|support| support_ref(support))
            .collect(),
        defined_semantics: observation.defined_semantics,
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
        other => panic!("unexpected observation kind: {other}"),
    }
}

fn support_ref(support: &str) -> SupportRef {
    if support.starts_with("corr:") {
        SupportRef::Corr(CorrId::parse(support).unwrap())
    } else if support.starts_with("edge:") {
        SupportRef::Edge(EdgeId::parse(support).unwrap())
    } else {
        panic!("unexpected support ref: {support}");
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

fn assert_strictly_sorted<'a>(values: impl Iterator<Item = &'a str>) {
    let values: Vec<_> = values.collect();
    for window in values.windows(2) {
        assert!(window[0] < window[1]);
    }
}
