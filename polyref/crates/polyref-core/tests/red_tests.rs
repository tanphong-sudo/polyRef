//! TDD red-state checklist for `polyref-core`.
//!
//! Every test below corresponds 1:1 to a test name in
//! `claude/05-handoff-1-core-ir.md` §E-1. They are `#[ignore]`-marked so
//! the workspace ships green during Slice 1 skeleton review; un-ignore
//! them as the Red-Green-Refactor loop turns each stub into real code.
//!
//! All tests panic via `todo!()` from the type stubs they exercise.

use polyref_core::{
    evidence::{Evidence, EvidencePointer, PredicateId, Version},
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    migration_map::MigrationMap,
    report::{ReportInvariantError, ValidationReport},
    source_span::{LineCol, SourceSpan},
    status::{BrokenReason, Outcome, UnknownReason},
};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

// ---------------- IDs ----------------

#[test]
#[ignore = "§E-1: implement EntityId::parse"]
fn entity_id_parse_accepts_canonical_form() {
    let id = "old:ts:handler:src/users.ts#createUser:0123456789ab";
    let _ = EntityId::parse(id).expect("canonical form should parse");
}

#[test]
#[ignore = "§E-1: implement EntityId::parse"]
fn entity_id_parse_rejects_empty() {
    assert!(EntityId::parse("").is_err());
}

#[test]
#[ignore = "§E-1: implement EntityId::parse"]
fn entity_id_parse_rejects_path_with_parent_traversal() {
    assert!(EntityId::parse("old:ts:handler:src/../etc:0123456789ab").is_err());
}

#[test]
#[ignore = "§E-1: implement EntityId::parse"]
fn entity_id_parse_rejects_control_chars() {
    assert!(EntityId::parse("old:ts:handler:src/u\u{0007}.ts:0123456789ab").is_err());
}

#[test]
#[ignore = "§E-1: implement EntityId::parse"]
fn entity_id_parse_rejects_bidi_overrides() {
    assert!(EntityId::parse("old:ts:handler:src/u\u{202e}.ts:0123456789ab").is_err());
}

#[test]
#[ignore = "§E-1: implement EntityId::parse"]
fn entity_id_parse_rejects_zero_width_chars() {
    assert!(EntityId::parse("old:ts:handler:src/u\u{200b}.ts:0123456789ab").is_err());
}

#[test]
#[ignore = "§E-1: implement EntityId::parse"]
fn entity_id_serde_does_not_bypass_parse() {
    let raw = "\"this is not a valid entity id\"";
    let result: Result<EntityId, _> = serde_json::from_str(raw);
    assert!(result.is_err(), "serde must route through EntityId::parse");
}

#[test]
#[ignore = "§E-1: implement ArtifactId::parse"]
fn artifact_id_parse_rejects_empty() {
    assert!(ArtifactId::parse("").is_err());
}

#[test]
#[ignore = "§E-1: implement CorrId::parse"]
fn corr_id_parse_rejects_empty() {
    assert!(CorrId::parse("").is_err());
}

#[test]
#[ignore = "§E-1: implement EdgeId::parse"]
fn edge_id_parse_rejects_empty() {
    assert!(EdgeId::parse("").is_err());
}

// ---------------- SourceSpan ----------------

#[test]
fn source_span_rejects_inverted_range() {
    let aid_str = "artifact:old:src/users.ts:0123456789ab";
    // We can't construct a real ArtifactId until parsing lands; this
    // test is enabled only after artifact-id parsing is implemented.
    let aid_res = ArtifactId::parse(aid_str);
    let Ok(aid) = aid_res else {
        eprintln!("skipping until ArtifactId::parse is implemented");
        return;
    };
    let start = LineCol::new(NonZeroU32::new(5).unwrap(), 0);
    let end = LineCol::new(NonZeroU32::new(2).unwrap(), 0);
    assert!(SourceSpan::try_new(aid, start, end, None).is_err());
}

// ---------------- Evidence ----------------

#[test]
fn evidence_ok_pres_has_outcome_pres() {
    let ev = Evidence::ok_pres(
        PredicateId::new("test.predicate-v1"),
        vec![],
        vec![],
        Version::new("1.0.0"),
        Version::new("1.0.0"),
    );
    assert!(matches!(ev.outcome(), Outcome::Pres));
}

#[test]
fn evidence_broken_carries_reason() {
    let ev = Evidence::broken(
        BrokenReason::SchemaIncompatible,
        PredicateId::new("schema.compat-v1"),
        vec![],
        vec![],
        Version::new("1.0.0"),
        Version::new("1.0.0"),
    );
    match ev.outcome() {
        Outcome::Broken { reason } => assert_eq!(*reason, BrokenReason::SchemaIncompatible),
        _ => panic!("expected Broken"),
    }
}

#[test]
fn evidence_unknown_carries_reason() {
    let ev = Evidence::unknown(
        UnknownReason::MissingEndpoint,
        PredicateId::new("route.compat-v1"),
        vec![],
        vec![],
        Version::new("1.0.0"),
        Version::new("1.0.0"),
    );
    match ev.outcome() {
        Outcome::Unknown { reason } => assert_eq!(*reason, UnknownReason::MissingEndpoint),
        _ => panic!("expected Unknown"),
    }
}

#[test]
#[ignore = "§E-1: implement EvidencePointer::parse"]
fn evidence_pointer_rejects_path_outside_evidence_dir() {
    assert!(EvidencePointer::parse("../escape").is_err());
    assert!(EvidencePointer::parse("/etc/passwd").is_err());
    assert!(EvidencePointer::parse("logs/foo.log").is_err());
    assert!(EvidencePointer::parse("evidence/ok.log").is_ok());
}

// ---------------- MigrationMap ----------------

#[test]
#[ignore = "§E-1: implement MigrationMap::try_new"]
fn migration_map_rejects_kind_mismatch() {
    let old = EntityId::parse("old:ts:handler:src/h.ts#h:0123456789ab").unwrap();
    let new = EntityId::parse("new:ts:schema:src/s.ts#S:0123456789ab").unwrap();
    let mut map = BTreeMap::new();
    map.insert(old, new);
    let result = MigrationMap::try_new(map, vec![], vec![]);
    assert!(result.is_err(), "handler -> schema is not type-respecting");
}

#[test]
#[ignore = "§E-1: implement MigrationMap::try_new"]
fn migration_map_allows_language_mismatch_when_kinds_match() {
    // TS handler ↔ JS handler is paper Definition 5: type(n) = type(μ(n))
    // where type is the local kind. The kind segment matches; the
    // language segment differs; this MUST succeed.
    let old = EntityId::parse("old:ts:handler:src/h.ts#h:0123456789ab").unwrap();
    let new = EntityId::parse("new:js:handler:src/h.js#h:0123456789ab").unwrap();
    let mut map = BTreeMap::new();
    map.insert(old, new);
    let result = MigrationMap::try_new(map, vec![], vec![]);
    assert!(result.is_ok(), "cross-language migration must be accepted when kinds match");
}

// ---------------- Report (the load-bearing fail-closed test) ----------------

#[test]
#[ignore = "§E-1: implement ValidationReport::assemble"]
fn report_assemble_rejects_accepted_with_missing_endpoint_unknown() {
    use polyref_core::report::ReportParts;
    // The test author constructs `ReportParts` such that
    // candidate_decision would compute to Accepted (every observation
    // has all items Pres/Migrated) but missing_endpoint_unknown=true.
    // The assemble call MUST return MissingEndpointUnknownInAccepted.
    let parts: ReportParts = todo!("§E-1 fixture for report assembly");
    let err = ValidationReport::assemble(parts).expect_err("invariant must fire");
    assert_eq!(err, ReportInvariantError::MissingEndpointUnknownInAccepted);
}
