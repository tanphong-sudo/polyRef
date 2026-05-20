//! Round-trip tests for `SqliteGraphStore`.
//!
//! Layer 1 acceptance gate:
//! - migration is idempotent
//! - 10k entities + correspondences round-trip through SQLite
//! - reads return the inserted data byte-equal under serde
//!
//! These tests use `:memory:` SQLite databases so they run on any CI
//! runner without filesystem state.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_core::{
    artifact_kind::ArtifactKind,
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    language::Language,
    observation::{ApiCallObs, HttpMethod, ObsHeader, Visibility},
    Observation,
};
use polyref_graph::{
    model::RepoSide, Artifact, BuildEdge, Correspondence, Entity, GraphStore, SqliteGraphStore,
};

// ─── Test helpers ──────────────────────────────────────────────────────

fn make_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("open in-memory store");
    store.migrate().expect("first migration");
    store
}

fn entity_id(side: &str, kind: &str, path: &str, hash: &str) -> EntityId {
    let s = format!("{side}:ts:{kind}:{path}:{hash}");
    EntityId::parse(&s).unwrap_or_else(|e| panic!("bad entity_id fixture {s}: {e}"))
}

fn artifact_id(side: &str, path: &str, hash: &str) -> ArtifactId {
    // ArtifactId grammar (ADR-003): artifact:<repo_side>:<path>:<content_hash>
    let s = format!("artifact:{side}:{path}:{hash}");
    ArtifactId::parse(&s).unwrap_or_else(|e| panic!("bad artifact_id fixture {s}: {e}"))
}

fn corr_id(kind: &str, hash: &str) -> CorrId {
    let s = format!("corr:{kind}:{hash}");
    CorrId::parse(&s).unwrap_or_else(|e| panic!("bad corr_id fixture {s}: {e}"))
}

fn edge_id(kind: &str, hash: &str) -> EdgeId {
    // EdgeId grammar (ADR-003): edge:<kind>:<hash> (16–64 hex)
    let s = format!("edge:{kind}:{hash}");
    EdgeId::parse(&s).unwrap_or_else(|e| panic!("bad edge_id fixture {s}: {e}"))
}

fn sample_artifact(suffix: u32, side: RepoSide) -> Artifact {
    let side_str = side.as_str();
    let path = format!("src/foo_{suffix}.ts");
    let hash = format!("{:012x}", 0xabcd_1234_u64.wrapping_add(u64::from(suffix)));
    Artifact {
        artifact_id: artifact_id(side_str, &path, &hash),
        repo_side: side,
        kind: ArtifactKind::SourceFile,
        language: Language::Ts,
        local_path: path,
        content_hash: hash,
    }
}

fn sample_entity(suffix: u32, parent: &Artifact) -> Entity {
    let kind = "handler";
    let path = format!("src/foo_{suffix}.ts#handler_{suffix}");
    let hash = format!("{:012x}", 0xdead_beef_u64.wrapping_add(u64::from(suffix)));
    Entity {
        entity_id: entity_id(parent.repo_side.as_str(), kind, &path, &hash),
        artifact_id: parent.artifact_id.clone(),
        repo_side: parent.repo_side,
        language: parent.language,
        kind: kind.to_string(),
        local_path: path,
        stable_hash: hash,
    }
}

// ─── Migration ─────────────────────────────────────────────────────────

#[test]
fn migrate_is_idempotent() {
    let store = SqliteGraphStore::open_in_memory().expect("open store");
    store.migrate().expect("first migrate");
    store.migrate().expect("second migrate");
    store.migrate().expect("third migrate");
}

// ─── Artifact round-trip ───────────────────────────────────────────────

#[test]
fn artifact_round_trip() {
    let store = make_store();
    let artifact = sample_artifact(1, RepoSide::Old);
    store.save_artifact(&artifact).expect("save");
    let got = store
        .find_artifact(&artifact.artifact_id)
        .expect("find")
        .expect("present");
    assert_eq!(got, artifact);
}

#[test]
fn artifact_upsert_replaces_existing() {
    let store = make_store();
    let mut artifact = sample_artifact(1, RepoSide::Old);
    store.save_artifact(&artifact).expect("save 1");
    artifact.local_path = "src/renamed.ts".to_string();
    store.save_artifact(&artifact).expect("save 2");
    let got = store
        .find_artifact(&artifact.artifact_id)
        .expect("find")
        .expect("present");
    assert_eq!(got.local_path, "src/renamed.ts");
}

#[test]
fn artifact_missing_returns_none() {
    let store = make_store();
    let id = artifact_id("old", "src/missing.ts", "0123456789ab");
    assert!(store.find_artifact(&id).expect("find").is_none());
}

// ─── Entity round-trip ─────────────────────────────────────────────────

#[test]
fn entity_round_trip() {
    let store = make_store();
    let artifact = sample_artifact(1, RepoSide::Old);
    store.save_artifact(&artifact).expect("save artifact");
    let entity = sample_entity(1, &artifact);
    store.save_entity(&entity).expect("save entity");
    let got = store
        .find_entity(&entity.entity_id)
        .expect("find")
        .expect("present");
    assert_eq!(got, entity);
}

#[test]
fn entity_count_per_repo_side() {
    let store = make_store();
    let old_artifact = sample_artifact(1, RepoSide::Old);
    let new_artifact = sample_artifact(1, RepoSide::New);
    store
        .save_artifact(&old_artifact)
        .expect("save old artifact");
    store
        .save_artifact(&new_artifact)
        .expect("save new artifact");
    for i in 0..5 {
        let e = sample_entity(i, &old_artifact);
        store.save_entity(&e).expect("save old entity");
    }
    for i in 0..3 {
        let e = sample_entity(i, &new_artifact);
        store.save_entity(&e).expect("save new entity");
    }
    assert_eq!(store.count_entities(RepoSide::Old).expect("count old"), 5);
    assert_eq!(store.count_entities(RepoSide::New).expect("count new"), 3);
}

// ─── Correspondence round-trip ─────────────────────────────────────────

#[test]
fn correspondence_round_trip_preserves_endpoint_order() {
    let store = make_store();
    let artifact = sample_artifact(1, RepoSide::Old);
    store.save_artifact(&artifact).expect("save artifact");
    let endpoints: Vec<EntityId> = (0..4)
        .map(|i| {
            let e = sample_entity(i, &artifact);
            store.save_entity(&e).expect("save entity");
            e.entity_id
        })
        .collect();

    let corr = Correspondence {
        corr_id: corr_id("route", "1234567890abcdef"),
        kind: CorrespondenceKind::Route,
        rule_version: Some("route-v1".to_string()),
        endpoints: endpoints.clone(),
    };
    store.save_correspondence(&corr).expect("save corr");
    let got = store
        .find_correspondence(&corr.corr_id)
        .expect("find")
        .expect("present");
    assert_eq!(got.endpoints, endpoints, "endpoint order must be preserved");
    assert_eq!(got, corr);
}

#[test]
fn correspondence_upsert_replaces_endpoints() {
    let store = make_store();
    let artifact = sample_artifact(1, RepoSide::Old);
    store.save_artifact(&artifact).expect("save artifact");
    let e0 = sample_entity(0, &artifact);
    let e1 = sample_entity(1, &artifact);
    store.save_entity(&e0).expect("save e0");
    store.save_entity(&e1).expect("save e1");

    let id = corr_id("route", "fedcba9876543210");
    let corr_a = Correspondence {
        corr_id: id.clone(),
        kind: CorrespondenceKind::Route,
        rule_version: None,
        endpoints: vec![e0.entity_id.clone(), e1.entity_id.clone()],
    };
    store.save_correspondence(&corr_a).expect("save a");

    let corr_b = Correspondence {
        corr_id: id.clone(),
        kind: CorrespondenceKind::Route,
        rule_version: Some("route-v2".to_string()),
        endpoints: vec![e1.entity_id.clone()],
    };
    store.save_correspondence(&corr_b).expect("save b");

    let got = store
        .find_correspondence(&id)
        .expect("find")
        .expect("present");
    assert_eq!(got.endpoints, vec![e1.entity_id]);
    assert_eq!(got.rule_version.as_deref(), Some("route-v2"));
}

// ─── Build edge round-trip ─────────────────────────────────────────────

#[test]
fn build_edge_round_trip() {
    let store = make_store();
    let src = sample_artifact(1, RepoSide::Old);
    let dst = sample_artifact(2, RepoSide::Old);
    store.save_artifact(&src).expect("save src");
    store.save_artifact(&dst).expect("save dst");
    let edge = BuildEdge {
        edge_id: edge_id("build", "000011112222333344445555"),
        src_artifact: src.artifact_id.clone(),
        dst_artifact: dst.artifact_id.clone(),
    };
    store.save_build_edge(&edge).expect("save edge");
    let got = store
        .find_build_edge(&edge.edge_id)
        .expect("find")
        .expect("present");
    assert_eq!(got, edge);
}

// ─── Observation round-trip ────────────────────────────────────────────

#[test]
fn observation_round_trip() {
    let store = make_store();
    let observation = Observation::ApiCall(ApiCallObs {
        method: HttpMethod::Post,
        path: "/users".to_string(),
        request_schema_id: None,
        response_schema_id: None,
        client_id: None,
        header: ObsHeader {
            visibility: Visibility::Visible,
            support: vec![],
            defined_semantics: true,
        },
    });
    store
        .save_observation("obs-1", &observation)
        .expect("save observation");
    let got = store
        .find_observation("obs-1")
        .expect("find")
        .expect("present");
    assert_eq!(got, observation);
}

// ─── Layer 1 acceptance: 10k round-trip ───────────────────────────────
//
// The build-plan demands a 10k-entity round-trip stress test.

#[test]
fn round_trip_10k_entities_and_correspondences() {
    let store = make_store();
    let artifact = sample_artifact(0, RepoSide::Old);
    store.save_artifact(&artifact).expect("save artifact");

    const N: u32 = 10_000;

    // Insert entities. Use one big transaction-equivalent via WAL —
    // the save loop is fine for N=10k on a hot in-memory DB.
    for i in 0..N {
        let e = sample_entity(i, &artifact);
        store.save_entity(&e).expect("save entity");
    }

    // Insert correspondences pairwise (~N/2 corrs).
    const M: u32 = 5_000;
    for i in 0..M {
        let e0 = sample_entity(2 * i, &artifact);
        let e1 = sample_entity(2 * i + 1, &artifact);
        let corr = Correspondence {
            corr_id: corr_id("route", &format!("{:016x}", u64::from(i))),
            kind: CorrespondenceKind::Route,
            rule_version: None,
            endpoints: vec![e0.entity_id, e1.entity_id],
        };
        store.save_correspondence(&corr).expect("save corr");
    }

    assert_eq!(
        store.count_entities(RepoSide::Old).expect("count e"),
        u64::from(N)
    );
    assert_eq!(
        store.count_correspondences().expect("count c"),
        u64::from(M)
    );

    // Sample a few entities + corrs to confirm they survived.
    for sample in [0_u32, 4321, 9_999] {
        let e = sample_entity(sample, &artifact);
        let got = store
            .find_entity(&e.entity_id)
            .expect("find")
            .expect("present");
        assert_eq!(got, e);
    }
    for sample in [0_u32, 1234, 4_999] {
        let id = corr_id("route", &format!("{:016x}", u64::from(sample)));
        let got = store
            .find_correspondence(&id)
            .expect("find")
            .expect("present");
        assert_eq!(got.endpoints.len(), 2);
    }
}
