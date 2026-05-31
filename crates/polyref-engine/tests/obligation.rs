//! Layer 6 obligation-generation tests (L6-01).
//!
//! Deterministic, bounded tests over the canonical §2 Layer 5 fixture. They lock
//! the obligation IR contract that A2 (L6-02) will consume: every frontier item
//! yields a base obligation, supp(o) items get an observation-support obligation,
//! dom(μ) endpoints get a migration obligation, and generation is byte-stable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_core::{
    artifact_kind::ArtifactKind,
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    language::Language,
    migration_map::MigrationMap,
    observation::SupportRef,
};
use polyref_engine::obligation::{generate_obligations, ObligationKind};
use polyref_frontier::{compute_frontier, FrontierInput, FrontierItem};
use polyref_graph::{
    Artifact, BuildEdge, Correspondence, Entity, GraphStore, RepoSide, SqliteGraphStore,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

const FIXTURE_ROOT: &str = "../../fixtures/layer5/users-route-frontier";
const VISIBLE_OBSERVATION_ID: &str = "obs:api:create-user-visible";

#[test]
fn fixture_generates_one_base_obligation_per_frontier_item() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let input = fixture_frontier_input(&fixture);
    let frontier = compute_frontier(&store, &input).unwrap();

    let set = generate_obligations(&store, &input, &frontier).unwrap();

    // 7 correspondences + 3 build edges in the §2 frontier ⇒ 10 base obligations.
    let base = set
        .obligations
        .iter()
        .filter(|o| {
            matches!(
                o.kind,
                ObligationKind::Correspondence | ObligationKind::Build
            )
        })
        .count();
    assert_eq!(base, 10, "one base obligation per frontier item");

    // Exactly one base obligation per distinct frontier item.
    let items_with_base: BTreeSet<_> = set
        .obligations
        .iter()
        .filter(|o| {
            matches!(
                o.kind,
                ObligationKind::Correspondence | ObligationKind::Build
            )
        })
        .map(|o| o.item.clone())
        .collect();
    assert_eq!(items_with_base.len(), 10);
}

#[test]
fn fixture_supp_items_get_observation_support_obligation() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let input = fixture_frontier_input(&fixture);
    let frontier = compute_frontier(&store, &input).unwrap();

    let set = generate_obligations(&store, &input, &frontier).unwrap();

    // Every §2 frontier item is in supp(o), so each gets an observation-support obligation.
    let support_items: BTreeSet<FrontierItem> = input
        .support
        .iter()
        .map(|s| match s {
            SupportRef::Corr(id) => FrontierItem::Correspondence(id.clone()),
            SupportRef::Edge(id) => FrontierItem::BuildEdge(id.clone()),
            _ => panic!("unexpected support ref"),
        })
        .collect();
    let obs_support_items: BTreeSet<FrontierItem> = set
        .obligations
        .iter()
        .filter(|o| o.kind == ObligationKind::ObservationSupport)
        .map(|o| o.item.clone())
        .collect();
    assert_eq!(obs_support_items, support_items);
}

#[test]
fn fixture_dom_mu_correspondence_gets_migration_obligation() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let input = fixture_frontier_input(&fixture);
    let frontier = compute_frontier(&store, &input).unwrap();

    let set = generate_obligations(&store, &input, &frontier).unwrap();

    // The route correspondence's endpoints are rewritten by μ ⇒ migration obligation.
    let route_item = FrontierItem::Correspondence(corr("corr:route:0000000000000001"));
    assert!(
        set.obligations
            .iter()
            .any(|o| o.item == route_item && o.kind == ObligationKind::Migration),
        "route correspondence with dom(μ) endpoints must get a migration obligation"
    );
}

#[test]
fn obligation_generation_is_byte_stable_across_repeats_and_shuffle() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let input = fixture_frontier_input(&fixture);
    let frontier = compute_frontier(&store, &input).unwrap();

    let first = generate_obligations(&store, &input, &frontier).unwrap();
    let second = generate_obligations(&store, &input, &frontier).unwrap();
    assert_eq!(first, second);

    // Shuffle the frontier entry order; obligations must come out identical.
    let mut shuffled = frontier.clone();
    shuffled.entries.reverse();
    let third = generate_obligations(&store, &input, &shuffled).unwrap();
    assert_eq!(first, third);
}

#[test]
fn clean_fixture_has_no_precheck_unknowns() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let input = fixture_frontier_input(&fixture);
    let frontier = compute_frontier(&store, &input).unwrap();

    let set = generate_obligations(&store, &input, &frontier).unwrap();
    assert!(set.precheck_unknowns.is_empty());
}

#[test]
fn missing_support_becomes_precheck_unknown_not_dropped() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let mut input = fixture_frontier_input(&fixture);
    // Reference a correspondence absent from the graph ⇒ frontier emits a diagnostic.
    input
        .support
        .push(SupportRef::Corr(corr("corr:route:9999999999999999")));
    let frontier = compute_frontier(&store, &input).unwrap();
    assert!(!frontier.diagnostics.is_empty());

    let set = generate_obligations(&store, &input, &frontier).unwrap();
    assert!(
        !set.precheck_unknowns.is_empty(),
        "missing support must surface as a pre-check Unknown, never dropped"
    );
}

#[test]
fn fixture_edited_artifact_endpoint_gets_local_obligation() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let input = fixture_frontier_input(&fixture);
    let frontier = compute_frontier(&store, &input).unwrap();

    let set = generate_obligations(&store, &input, &frontier).unwrap();

    // The route correspondence has the `createUser` handler endpoint, owned by the
    // edited artifact handler.py ⇒ it must get a Local obligation.
    let route_item = FrontierItem::Correspondence(corr("corr:route:0000000000000001"));
    assert!(
        set.obligations
            .iter()
            .any(|o| o.item == route_item && o.kind == ObligationKind::Local),
        "a correspondence with an endpoint owned by an edited artifact must get a Local obligation"
    );

    // The event correspondence's only endpoint also lives in handler.py (edited) ⇒ Local.
    let event_item = FrontierItem::Correspondence(corr("corr:event:0000000000000006"));
    assert!(
        set.obligations
            .iter()
            .any(|o| o.item == event_item && o.kind == ObligationKind::Local),
        "the event correspondence in the edited handler.py must get a Local obligation"
    );

    // A build edge is artifact→artifact with no endpoint entities ⇒ never Local.
    assert!(
        !set.obligations.iter().any(
            |o| matches!(o.item, FrontierItem::BuildEdge(_)) && o.kind == ObligationKind::Local
        ),
        "build edges have no endpoint entities and must not get a Local obligation"
    );
}

#[test]
fn no_edits_means_no_local_obligation() {
    let fixture = load_fixture();
    let store = seeded_store(&fixture);
    let mut input = fixture_frontier_input(&fixture);
    // Drop the edits: with Δ empty, no endpoint is owned by an edited artifact.
    input.edited_artifacts = BTreeSet::new();
    let frontier = compute_frontier(&store, &input).unwrap();

    let set = generate_obligations(&store, &input, &frontier).unwrap();
    assert!(
        !set.obligations
            .iter()
            .any(|o| o.kind == ObligationKind::Local),
        "with no edited artifacts there can be no Local obligation"
    );
}

// ---- fixture plumbing (mirrors crates/polyref-frontier/tests/closure.rs) ----

#[derive(Debug, Deserialize)]
struct Layer5Fixture {
    artifacts: Vec<FixtureArtifact>,
    build_edges: Vec<FixtureBuildEdge>,
    correspondences: Vec<FixtureCorrespondence>,
    entities: Vec<FixtureEntity>,
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
struct FixtureMigrationCandidate {
    old: String,
    new: String,
}

#[derive(Debug, Deserialize)]
struct FixtureObservation {
    observation_id: String,
    support: Vec<String>,
}

fn fixture_frontier_input(fixture: &Layer5Fixture) -> FrontierInput {
    let observation = fixture
        .observations
        .iter()
        .find(|obs| obs.observation_id == VISIBLE_OBSERVATION_ID)
        .unwrap();
    FrontierInput {
        edited_artifacts: BTreeSet::from([
            artifact("artifact:old:Dockerfile:111100000001"),
            artifact("artifact:old:handler.py:111100000004"),
            artifact("artifact:old:openapi.yaml:111100000005"),
        ]),
        migration_map: fixture_migration_map(fixture),
        observation_id: observation.observation_id.clone(),
        support: observation.support.iter().map(|s| support_ref(s)).collect(),
    }
}

fn fixture_migration_map(fixture: &Layer5Fixture) -> MigrationMap {
    let rewrites = fixture
        .migration_map_candidates
        .iter()
        .map(|c| (entity(&c.old), entity(&c.new)))
        .collect();
    MigrationMap::try_new(rewrites, Vec::new(), Vec::new()).unwrap()
}

fn seeded_store(fixture: &Layer5Fixture) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().unwrap();
    store.migrate().unwrap();
    for a in &fixture.artifacts {
        store.save_artifact(&fixture_artifact_row(a)).unwrap();
    }
    for e in &fixture.entities {
        store.save_entity(&fixture_entity_row(e)).unwrap();
    }
    for c in &fixture.correspondences {
        store.save_correspondence(&fixture_corr_row(c)).unwrap();
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

fn fixture_artifact_row(a: &FixtureArtifact) -> Artifact {
    Artifact {
        artifact_id: ArtifactId::parse(&a.artifact_id).unwrap(),
        repo_side: repo_side(&a.side),
        kind: ArtifactKind::parse(&a.kind).unwrap(),
        language: Language::parse(&a.language).unwrap(),
        local_path: a.path.clone(),
        content_hash: a.content_hash.clone(),
    }
}

fn fixture_entity_row(e: &FixtureEntity) -> Entity {
    Entity {
        entity_id: EntityId::parse(&e.entity_id).unwrap(),
        artifact_id: ArtifactId::parse(&e.artifact_id).unwrap(),
        repo_side: repo_side(&e.side),
        language: Language::parse(&e.language).unwrap(),
        kind: e.kind.clone(),
        local_path: e.local_path.clone(),
        stable_hash: e.stable_hash.clone(),
    }
}

fn fixture_corr_row(c: &FixtureCorrespondence) -> Correspondence {
    Correspondence {
        corr_id: CorrId::parse(&c.corr_id).unwrap(),
        kind: CorrespondenceKind::parse(&c.kind).unwrap(),
        rule_version: Some("layer5-fixture-v1".to_owned()),
        endpoints: c.endpoints.iter().map(|id| entity(id)).collect(),
    }
}

fn fixture_edge_row(edge: &FixtureBuildEdge) -> BuildEdge {
    BuildEdge {
        edge_id: EdgeId::parse(&edge.edge_id).unwrap(),
        src_artifact: artifact(&edge.src_artifact),
        dst_artifact: artifact(&edge.dst_artifact),
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
