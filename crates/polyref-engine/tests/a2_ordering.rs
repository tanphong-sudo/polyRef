//! A2 ordering + precedence lock (L6-02).
//!
//! These tests pin the load-bearing per-item precedence
//! **Broken > Unknown > Migrated > Pres** and the observation-decision meet over
//! `required(o)`. They are written so that reordering the A2 branches (e.g. testing
//! Unknown before Broken) flips an assertion. Pure reducer — no graph, plugin, or IO.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_core::report::ObservationDecision;
use polyref_core::{
    evidence::{Evidence, PredicateId, Version},
    ids::{CorrId, EdgeId},
    status::{BrokenReason, Outcome, UnknownReason},
};
use polyref_engine::a2::{validate_frontier, ItemVerdicts, ValidateFrontierInput};
use polyref_engine::obligation::{FrontierObligationSet, Obligation, ObligationKind};
use polyref_frontier::FrontierItem;
use std::collections::BTreeMap;

const OBS: &str = "obs:api:create-user-visible";

fn corr_item(suffix: &str) -> FrontierItem {
    FrontierItem::Correspondence(CorrId::parse(&format!("corr:route:{suffix}")).unwrap())
}

fn edge_item(suffix: &str) -> FrontierItem {
    FrontierItem::BuildEdge(EdgeId::parse(&format!("edge:build_codegen:{suffix}")).unwrap())
}

fn pres(predicate: &str) -> Evidence {
    Evidence::ok_pres(
        PredicateId::new(predicate),
        vec![],
        vec![],
        Version::new("checker-1"),
        Version::new("rule-1"),
    )
}

fn migrated(predicate: &str) -> Evidence {
    Evidence::ok_migrated(
        PredicateId::new(predicate),
        vec![],
        vec![],
        Version::new("checker-1"),
        Version::new("rule-1"),
    )
}

fn broken(reason: BrokenReason, predicate: &str, checker: &str) -> Evidence {
    Evidence::broken(
        reason,
        PredicateId::new(predicate),
        vec![],
        vec![],
        Version::new(checker),
        Version::new("rule-1"),
    )
}

fn unknown(reason: UnknownReason, predicate: &str, checker: &str) -> Evidence {
    Evidence::unknown(
        reason,
        PredicateId::new(predicate),
        vec![],
        vec![],
        Version::new(checker),
        Version::new("rule-1"),
    )
}

/// Build an obligation set with one Correspondence base obligation per item, all
/// in supp(o) (so they are required).
fn obligations_for(items: &[FrontierItem]) -> FrontierObligationSet {
    let mut obligations = Vec::new();
    for item in items {
        let kind = match item {
            FrontierItem::Correspondence(_) => ObligationKind::Correspondence,
            FrontierItem::BuildEdge(_) => ObligationKind::Build,
        };
        obligations.push(Obligation {
            item: item.clone(),
            kind,
            corr_kind: None,
        });
        obligations.push(Obligation {
            item: item.clone(),
            kind: ObligationKind::ObservationSupport,
            corr_kind: None,
        });
    }
    FrontierObligationSet {
        observation_id: OBS.to_owned(),
        obligations,
        precheck_unknowns: Vec::new(),
    }
}

fn run(
    items: &[FrontierItem],
    verdicts: ItemVerdicts,
) -> polyref_engine::a2::ValidateFrontierOutput {
    let input = ValidateFrontierInput {
        observation_id: OBS.to_owned(),
        required_items: items.to_vec(),
        obligations: obligations_for(items),
        verdicts,
    };
    validate_frontier(&input)
}

#[test]
fn broken_dominates_unknown_on_same_item() {
    // One item, two checker verdicts: a refutation AND a missing-evidence Unknown.
    // Broken must win (precedence), NOT Unknown — this fails if step order is flipped.
    let item = corr_item("0000000000000001");
    let mut verdicts: ItemVerdicts = BTreeMap::new();
    verdicts.insert(
        item.clone(),
        vec![
            unknown(
                UnknownReason::MissingEndpoint,
                "route.compat-v1",
                "checker-a",
            ),
            broken(
                BrokenReason::RoutePathRefuted,
                "route.compat-v1",
                "checker-b",
            ),
        ],
    );

    let out = run(&[item.clone()], verdicts);
    assert!(
        matches!(out.statuses.get(&item), Some(Outcome::Broken { .. })),
        "a refutation must dominate a missing-evidence Unknown on the same item"
    );
    assert_eq!(out.decision, ObservationDecision::Broken);
}

#[test]
fn unknown_blocks_accepting_when_no_refutation() {
    // Missing evidence with no refutation ⇒ Unknown, never Pres/Migrated.
    let item = corr_item("0000000000000001");
    let mut verdicts: ItemVerdicts = BTreeMap::new();
    verdicts.insert(
        item.clone(),
        vec![
            unknown(
                UnknownReason::CheckerTimeout,
                "route.compat-v1",
                "checker-a",
            ),
            pres("route.compat-v1"),
        ],
    );

    let out = run(&[item.clone()], verdicts);
    assert!(
        matches!(out.statuses.get(&item), Some(Outcome::Unknown { .. })),
        "missing/timeout evidence must block accepting even if another checker is Pres"
    );
    assert_eq!(out.decision, ObservationDecision::Unknown);
}

#[test]
fn all_pres_migrated_is_accepted() {
    let a = corr_item("0000000000000001");
    let b = edge_item("0000000000000002");
    let mut verdicts: ItemVerdicts = BTreeMap::new();
    verdicts.insert(a.clone(), vec![migrated("route.migrate-v1")]);
    verdicts.insert(b.clone(), vec![pres("build.compat-v1")]);

    let out = run(&[a.clone(), b.clone()], verdicts);
    assert!(matches!(out.statuses.get(&a), Some(Outcome::Migrated)));
    assert!(matches!(out.statuses.get(&b), Some(Outcome::Pres)));
    assert_eq!(out.decision, ObservationDecision::Accepted);
}

#[test]
fn item_with_no_verdict_falls_back_to_no_accepting_rule() {
    // A required item with no checker verdict cannot be accepted; fail-closed Unknown.
    let item = corr_item("0000000000000001");
    let out = run(&[item.clone()], BTreeMap::new());
    assert!(
        matches!(
            out.statuses.get(&item),
            Some(Outcome::Unknown {
                reason: UnknownReason::NoAcceptingRuleApplied
            })
        ),
        "a required item with no verdict must be Unknown(NoAcceptingRuleApplied)"
    );
    assert_eq!(out.decision, ObservationDecision::Unknown);
}

#[test]
fn precheck_unknown_blocks_acceptance() {
    // A pre-check Unknown (Layer 5 coverage gap) on a required item forces Unknown
    // even if a checker returned Pres.
    let item = corr_item("0000000000000001");
    let mut verdicts: ItemVerdicts = BTreeMap::new();
    verdicts.insert(item.clone(), vec![pres("route.compat-v1")]);

    let mut obligations = obligations_for(&[item.clone()]);
    obligations
        .precheck_unknowns
        .push(polyref_engine::obligation::PrecheckUnknown {
            observation_id: OBS.to_owned(),
            item: "corr:route:0000000000000001".to_owned(),
        });

    let input = ValidateFrontierInput {
        observation_id: OBS.to_owned(),
        required_items: vec![item.clone()],
        obligations,
        verdicts,
    };
    let out = validate_frontier(&input);
    assert!(
        matches!(out.statuses.get(&item), Some(Outcome::Unknown { .. })),
        "a Layer 5 pre-check Unknown must block acceptance of its item"
    );
    assert_eq!(out.decision, ObservationDecision::Unknown);
}

#[test]
fn multi_broken_tie_break_picks_smallest_reason_tag() {
    // Two refutations on one item; the headline reason is the smallest snake_case tag.
    // route_path_refuted vs schema_incompatible ⇒ route_path_refuted (r < s).
    let item = corr_item("0000000000000001");
    let mut verdicts: ItemVerdicts = BTreeMap::new();
    verdicts.insert(
        item.clone(),
        vec![
            broken(
                BrokenReason::SchemaIncompatible,
                "schema.compat-v1",
                "checker-z",
            ),
            broken(
                BrokenReason::RoutePathRefuted,
                "route.compat-v1",
                "checker-a",
            ),
        ],
    );

    let out = run(&[item.clone()], verdicts);
    assert!(
        matches!(
            out.statuses.get(&item),
            Some(Outcome::Broken {
                reason: BrokenReason::RoutePathRefuted
            })
        ),
        "deterministic tie-break must pick the smallest canonical reason tag"
    );
}

#[test]
fn output_is_byte_stable_across_verdict_insertion_order() {
    let a = corr_item("0000000000000001");
    let b = edge_item("0000000000000002");

    let mut v1: ItemVerdicts = BTreeMap::new();
    v1.insert(a.clone(), vec![migrated("route.migrate-v1")]);
    v1.insert(b.clone(), vec![pres("build.compat-v1")]);
    let out1 = run(&[a.clone(), b.clone()], v1);

    let mut v2: ItemVerdicts = BTreeMap::new();
    v2.insert(b.clone(), vec![pres("build.compat-v1")]);
    v2.insert(a.clone(), vec![migrated("route.migrate-v1")]);
    let out2 = run(&[b.clone(), a.clone()], v2);

    assert_eq!(out1, out2);
}
