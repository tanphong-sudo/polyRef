//! Integration tests for `polyref-core`.
//!
//! Tests marked `#[ignore]` are waiting for their corresponding
//! implementation. Un-ignore as each module is implemented.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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

// ════════════════════════════════════════════════════════════════════════
// IDs — IMPLEMENTED (Layer 0, step 1)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn entity_id_parse_accepts_canonical_form() {
    let id = "old:ts:handler:src/users.ts#createUser:0123456789ab";
    let parsed = EntityId::parse(id).expect("canonical form should parse");
    assert_eq!(parsed.repo_side(), "old");
    assert_eq!(parsed.language(), "ts");
    assert_eq!(parsed.kind(), "handler");
    assert_eq!(parsed.local_path(), "src/users.ts#createUser");
    assert_eq!(parsed.stable_hash(), "0123456789ab");
}

#[test]
fn entity_id_parse_rejects_empty() {
    assert!(EntityId::parse("").is_err());
}

#[test]
fn entity_id_parse_rejects_path_with_parent_traversal() {
    assert!(EntityId::parse("old:ts:handler:src/../etc:0123456789ab").is_err());
}

#[test]
fn entity_id_parse_rejects_control_chars() {
    assert!(EntityId::parse("old:ts:handler:src/\u{0007}h.ts:0123456789ab").is_err());
}

#[test]
fn entity_id_parse_rejects_bidi_overrides() {
    assert!(EntityId::parse("old:ts:handler:src/\u{202e}h.ts:0123456789ab").is_err());
}

#[test]
fn entity_id_parse_rejects_zero_width_chars() {
    assert!(EntityId::parse("old:ts:handler:src/\u{200b}h.ts:0123456789ab").is_err());
}

#[test]
fn entity_id_serde_does_not_bypass_parse() {
    let raw = "\"this is not a valid entity id\"";
    let result: Result<EntityId, _> = serde_json::from_str(raw);
    assert!(result.is_err(), "serde must route through EntityId::parse");
}

#[test]
fn artifact_id_parse_rejects_empty() {
    assert!(ArtifactId::parse("").is_err());
}

#[test]
fn artifact_id_parse_accepts_canonical() {
    let id = "artifact:old:src/users.ts:0123456789ab";
    assert!(ArtifactId::parse(id).is_ok());
}

#[test]
fn corr_id_parse_rejects_empty() {
    assert!(CorrId::parse("").is_err());
}

#[test]
fn corr_id_parse_accepts_canonical() {
    assert!(CorrId::parse("corr:route:0123456789abcdef").is_ok());
}

#[test]
fn edge_id_parse_rejects_empty() {
    assert!(EdgeId::parse("").is_err());
}

#[test]
fn edge_id_parse_accepts_canonical() {
    assert!(EdgeId::parse("edge:build_codegen:0123456789abcdef").is_ok());
}

// ════════════════════════════════════════════════════════════════════════
// SourceSpan — IMPLEMENTED (skeleton already has try_new)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn source_span_rejects_inverted_range() {
    let aid = ArtifactId::parse("artifact:old:src/users.ts:0123456789ab").unwrap();
    let start = LineCol::new(NonZeroU32::new(5).unwrap(), 0);
    let end = LineCol::new(NonZeroU32::new(2).unwrap(), 0);
    assert!(SourceSpan::try_new(aid, start, end, None).is_err());
}

#[test]
fn source_span_accepts_valid_range() {
    let aid = ArtifactId::parse("artifact:old:src/users.ts:0123456789ab").unwrap();
    let start = LineCol::new(NonZeroU32::new(1).unwrap(), 0);
    let end = LineCol::new(NonZeroU32::new(5).unwrap(), 10);
    assert!(SourceSpan::try_new(aid, start, end, None).is_ok());
}

// ════════════════════════════════════════════════════════════════════════
// Evidence — IMPLEMENTED (constructors already work)
// ════════════════════════════════════════════════════════════════════════

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

// ════════════════════════════════════════════════════════════════════════
// EvidencePointer — IMPLEMENTED
// ════════════════════════════════════════════════════════════════════════

#[test]
fn evidence_pointer_rejects_path_outside_evidence_dir() {
    assert!(EvidencePointer::parse("../escape").is_err());
    assert!(EvidencePointer::parse("/etc/passwd").is_err());
    assert!(EvidencePointer::parse("logs/foo.log").is_err());
    assert!(EvidencePointer::parse("evidence/ok.log").is_ok());
}

#[test]
fn evidence_pointer_rejects_parent_traversal() {
    assert!(EvidencePointer::parse("evidence/../escape.log").is_err());
}

#[test]
fn evidence_pointer_rejects_empty_suffix() {
    assert!(EvidencePointer::parse("evidence/").is_err());
}

// ════════════════════════════════════════════════════════════════════════
// MigrationMap — IMPLEMENTED
// ════════════════════════════════════════════════════════════════════════

#[test]
fn migration_map_rejects_kind_mismatch() {
    let old = EntityId::parse("old:ts:handler:src/h.ts#h:0123456789ab").unwrap();
    let new = EntityId::parse("new:ts:schema:src/s.ts#S:0123456789ab").unwrap();
    let mut map = BTreeMap::new();
    map.insert(old, new);
    let result = MigrationMap::try_new(map, vec![], vec![]);
    assert!(result.is_err(), "handler -> schema is not type-respecting");
}

#[test]
fn migration_map_allows_language_mismatch_when_kinds_match() {
    let old = EntityId::parse("old:ts:handler:src/h.ts#h:0123456789ab").unwrap();
    let new = EntityId::parse("new:py:handler:src/h.py#h:abcdef012345").unwrap();
    let mut map = BTreeMap::new();
    map.insert(old, new);
    let result = MigrationMap::try_new(map, vec![], vec![]);
    assert!(result.is_ok(), "cross-language migration must succeed when kinds match");
}

#[test]
fn migration_map_iter_is_deterministic() {
    let a = EntityId::parse("old:ts:handler:a:0123456789ab").unwrap();
    let b = EntityId::parse("old:ts:handler:b:0123456789ab").unwrap();
    let a2 = EntityId::parse("new:ts:handler:a2:abcdef012345").unwrap();
    let b2 = EntityId::parse("new:ts:handler:b2:abcdef012345").unwrap();
    let mut map = BTreeMap::new();
    map.insert(b.clone(), b2.clone());
    map.insert(a.clone(), a2.clone());
    let mm = MigrationMap::try_new(map, vec![], vec![]).unwrap();
    let keys: Vec<&str> = mm.iter().map(|(k, _)| k.as_str()).collect();
    // BTreeMap guarantees sorted order
    assert_eq!(keys[0], a.as_str());
    assert_eq!(keys[1], b.as_str());
}

// ════════════════════════════════════════════════════════════════════════
// ValidationReport — IMPLEMENTED
// ════════════════════════════════════════════════════════════════════════

#[test]
fn report_assemble_rejects_accepted_with_missing_endpoint_unknown() {
    use polyref_core::report::{
        CandidateDecision, ObservationDecision, ObservationRow, ReportAuditPointers,
        ReportCandidate, ReportConfigs, ReportParts, ReportRepoRef, ReportRepos,
    };
    use polyref_core::observation::Visibility;

    // Build a ReportParts where all observations are Accepted (so
    // candidate_decision would compute to Accepted) but
    // missing_endpoint_unknown = true.
    let parts = ReportParts {
        report_id: "test-report-1".to_owned(),
        repos: ReportRepos {
            old: ReportRepoRef {
                repo_id: "repo-old".to_owned(),
                commit: "a".repeat(40),
            },
            new: ReportRepoRef {
                repo_id: "repo-new".to_owned(),
                commit: "b".repeat(40),
            },
        },
        candidate: ReportCandidate {
            candidate_id: "cand-1".to_owned(),
            source: "manual".to_owned(),
            patch_hash: "c".repeat(64),
        },
        configs: ReportConfigs {
            extractor_versions: std::collections::BTreeMap::new(),
            checker_versions: std::collections::BTreeMap::new(),
        },
        observations: vec![ObservationRow {
            observation_id: "obs-1".to_owned(),
            obs_kind: "api_call".to_owned(),
            visibility: Visibility::Visible,
            frontier_size: 1,
            items: vec![Evidence::ok_pres(
                PredicateId::new("route.compat-v1"),
                vec![],
                vec![],
                Version::new("1.0.0"),
                Version::new("1.0.0"),
            )],
            status: ObservationDecision::Accepted,
        }],
        missing_endpoint_unknown: true, // ← the violation
        audit_pointers: ReportAuditPointers {
            audit_ndjson: "evidence/audit.ndjson".to_owned(),
            manifest_json: "evidence/manifest.json".to_owned(),
        },
    };

    let err = ValidationReport::assemble(parts).expect_err("invariant must fire");
    assert_eq!(err, ReportInvariantError::MissingEndpointUnknownInAccepted);
}

#[test]
fn report_assemble_accepts_valid_report() {
    use polyref_core::report::{
        CandidateDecision, ObservationDecision, ObservationRow, ReportAuditPointers,
        ReportCandidate, ReportConfigs, ReportParts, ReportRepoRef, ReportRepos,
    };
    use polyref_core::observation::Visibility;

    let parts = ReportParts {
        report_id: "test-report-2".to_owned(),
        repos: ReportRepos {
            old: ReportRepoRef {
                repo_id: "repo-old".to_owned(),
                commit: "a".repeat(40),
            },
            new: ReportRepoRef {
                repo_id: "repo-new".to_owned(),
                commit: "b".repeat(40),
            },
        },
        candidate: ReportCandidate {
            candidate_id: "cand-2".to_owned(),
            source: "ide".to_owned(),
            patch_hash: "d".repeat(64),
        },
        configs: ReportConfigs {
            extractor_versions: std::collections::BTreeMap::new(),
            checker_versions: std::collections::BTreeMap::new(),
        },
        observations: vec![ObservationRow {
            observation_id: "obs-1".to_owned(),
            obs_kind: "api_call".to_owned(),
            visibility: Visibility::Visible,
            frontier_size: 1,
            items: vec![Evidence::ok_migrated(
                PredicateId::new("route.migrate-v1"),
                vec![],
                vec![],
                Version::new("1.0.0"),
                Version::new("1.0.0"),
            )],
            status: ObservationDecision::Accepted,
        }],
        missing_endpoint_unknown: false,
        audit_pointers: ReportAuditPointers {
            audit_ndjson: "evidence/audit.ndjson".to_owned(),
            manifest_json: "evidence/manifest.json".to_owned(),
        },
    };

    let report = ValidationReport::assemble(parts).expect("should assemble");
    assert_eq!(report.candidate_decision(), CandidateDecision::Accepted);
    assert!(!report.missing_endpoint_unknown());
}
