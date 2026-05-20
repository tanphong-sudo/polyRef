//! Property-based tests for `SqliteGraphStore`.
//!
//! Per `docs/verification.md` Layer 1, property tests are the strongest
//! lock against silent regressions. The properties below pin invariants
//! that are easy to break with a one-line "improvement":
//!
//! - **Endpoint ordering** is significant by paper Definition 3
//!   (`ends(c) = (n_1, ..., n_m)`). Anyone tempted to "tidy up" the
//!   correspondence_endpoint table by sorting endpoints would break the
//!   route checker, which relies on the OpenAPI-path / handler order.
//! - **Idempotent migration** must hold for any number of consecutive
//!   `migrate()` calls — not just the two that the example test covers.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use polyref_core::{
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EntityId},
};
use polyref_graph::{
    model::RepoSide, Artifact, Correspondence, Entity, GraphStore, SqliteGraphStore,
};
use proptest::prelude::*;

// ─── Helpers ───────────────────────────────────────────────────────────

fn artifact_for_test(suffix: u32) -> Artifact {
    let path = format!("src/foo_{suffix}.ts");
    let hash = format!("{:012x}", 0xabcd_1234_u64.wrapping_add(u64::from(suffix)));
    let id_str = format!("artifact:old:{path}:{hash}");
    Artifact {
        artifact_id: ArtifactId::parse(&id_str).unwrap(),
        repo_side: RepoSide::Old,
        kind: polyref_core::ArtifactKind::SourceFile,
        language: polyref_core::Language::Ts,
        local_path: path,
        content_hash: hash,
    }
}

fn entity_for_test(suffix: u32, parent: &Artifact) -> Entity {
    let path = format!("src/foo_{suffix}.ts#h_{suffix}");
    let hash = format!("{:012x}", 0xdead_beef_u64.wrapping_add(u64::from(suffix)));
    let id_str = format!("old:ts:handler:{path}:{hash}");
    Entity {
        entity_id: EntityId::parse(&id_str).unwrap(),
        artifact_id: parent.artifact_id.clone(),
        repo_side: parent.repo_side,
        language: parent.language,
        kind: "handler".into(),
        local_path: path,
        stable_hash: hash,
    }
}

// proptest-shrinking-friendly endpoint count: 1..=8 (paper-realistic
// hyperedges; route correspondence has up to 5 endpoints).
fn endpoint_indices() -> impl Strategy<Value = Vec<u32>> {
    // Emit a permutation of [0, n) so each EntityId is distinct.
    (1usize..=8).prop_flat_map(|n| {
        let base: Vec<u32> = (0..n as u32).collect();
        Just(base).prop_shuffle()
    })
}

// ─── Properties ────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        .. ProptestConfig::default()
    })]

    /// Paper Definition 3: endpoint ordering is part of the
    /// correspondence value. `save` then `find` must return the same
    /// sequence regardless of length or permutation.
    #[test]
    fn prop_correspondence_endpoint_order_is_preserved(indices in endpoint_indices()) {
        let store = SqliteGraphStore::open_in_memory().unwrap();
        store.migrate().unwrap();

        let artifact = artifact_for_test(0);
        store.save_artifact(&artifact).unwrap();

        let endpoints: Vec<EntityId> = indices
            .iter()
            .map(|i| {
                let e = entity_for_test(*i, &artifact);
                store.save_entity(&e).unwrap();
                e.entity_id
            })
            .collect();

        let n = endpoints.len();
        let id = CorrId::parse(&format!("corr:route:{:016x}", n as u64)).unwrap();
        let corr = Correspondence {
            corr_id: id.clone(),
            kind: CorrespondenceKind::Route,
            rule_version: None,
            endpoints: endpoints.clone(),
        };
        store.save_correspondence(&corr).unwrap();

        let got = store
            .find_correspondence(&id)
            .unwrap()
            .expect("present");
        prop_assert_eq!(got.endpoints, endpoints);
    }

    /// `migrate()` is idempotent for any positive number of repeats.
    /// The acceptance gate spec only checks two calls; this widens the
    /// claim.
    #[test]
    fn prop_migrate_is_idempotent(repeats in 1usize..16) {
        let store = SqliteGraphStore::open_in_memory().unwrap();
        for _ in 0..repeats {
            store.migrate().unwrap();
        }
    }
}

// ─── Boundary cases that property tests are unlikely to hit ───────────

#[test]
fn endpoint_ordering_survives_sort_lookalikes() {
    // A regression test specifically for the "let's just sort endpoints"
    // refactor temptation: ids are intentionally chosen so a naive
    // alphanumeric sort would reorder them, and the test asserts the
    // declared order is what comes back.
    let store = SqliteGraphStore::open_in_memory().unwrap();
    store.migrate().unwrap();

    let artifact = artifact_for_test(0);
    store.save_artifact(&artifact).unwrap();

    // Three entities whose stable_hash sort order differs from their
    // declaration order:
    //   declaration: [9..., 0..., 5...]
    //   alphanum:    [0..., 5..., 9...]
    let ids = [9_u32, 0, 5];
    let endpoints: Vec<EntityId> = ids
        .iter()
        .map(|i| {
            let e = entity_for_test(*i, &artifact);
            store.save_entity(&e).unwrap();
            e.entity_id
        })
        .collect();

    let id = CorrId::parse("corr:route:00000000deadbeef").unwrap();
    let corr = Correspondence {
        corr_id: id.clone(),
        kind: CorrespondenceKind::Route,
        rule_version: None,
        endpoints: endpoints.clone(),
    };
    store.save_correspondence(&corr).unwrap();

    let got = store.find_correspondence(&id).unwrap().expect("present");
    assert_eq!(
        got.endpoints, endpoints,
        "endpoint declaration order must survive round-trip"
    );

    // Sanity: confirm the test fixture itself has a non-sorted order
    // so the assertion above is meaningful.
    let mut sorted = endpoints.clone();
    sorted.sort();
    assert_ne!(sorted, endpoints, "fixture must be unsorted to be useful");
}
