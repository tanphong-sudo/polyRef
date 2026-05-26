#![allow(clippy::unwrap_used)]

use polyref_core::{
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    observation::SupportRef,
    status::UnknownReason,
};
use polyref_frontier::{
    classify_coverage_risk, CoverageRiskInput, CoverageRiskSource, FrontierDiagnostic,
    FrontierDiagnosticKind, FrontierResult, UnsupportedFeatureRiskNote,
};
use polyref_graph::{
    MigrationMapDiagnostic, MigrationMapDiagnosticKind, ObservationRegistryDiagnostic,
    ObservationRegistryDiagnosticKind,
};

#[test]
fn coverage_risk_clean_fixture_has_no_blocking_risks() {
    let input = CoverageRiskInput {
        observation_id: "obs:api:create-user-visible".to_owned(),
        frontier: FrontierResult {
            entries: Vec::new(),
            diagnostics: Vec::new(),
        },
        support: vec![SupportRef::Corr(corr("corr:route:0000000000000001"))],
        registry_diagnostics: Vec::new(),
        migration_diagnostics: Vec::new(),
        unsupported_features: Vec::new(),
    };

    let report = classify_coverage_risk(input);

    assert_eq!(report.observation_id, "obs:api:create-user-visible");
    assert!(!report.is_blocked);
    assert!(report.risks.is_empty());
}

#[test]
fn missing_support_from_frontier_or_registry_maps_to_missing_endpoint() {
    let report = classify_coverage_risk(CoverageRiskInput {
        observation_id: "obs:api:missing".to_owned(),
        frontier: FrontierResult {
            entries: Vec::new(),
            diagnostics: vec![FrontierDiagnostic {
                kind: FrontierDiagnosticKind::MissingSupport,
                observation_id: "obs:api:missing".to_owned(),
                item: "corr:route:9999999999999999".to_owned(),
            }],
        },
        support: Vec::new(),
        registry_diagnostics: vec![ObservationRegistryDiagnostic {
            kind: ObservationRegistryDiagnosticKind::MissingSupport,
            observation_id: "obs:api:missing".to_owned(),
            item: Some("edge:build_codegen:9999999999999999".to_owned()),
        }],
        migration_diagnostics: Vec::new(),
        unsupported_features: Vec::new(),
    });

    assert!(report.is_blocked);
    assert_eq!(reasons(&report), vec![UnknownReason::MissingEndpoint]);
    assert!(report
        .risks
        .iter()
        .any(|risk| risk.source == CoverageRiskSource::Frontier));
    assert!(report
        .risks
        .iter()
        .any(|risk| risk.source == CoverageRiskSource::ObservationRegistry));
}

#[test]
fn dynamic_route_notes_map_to_dynamic_string() {
    let report = classify_coverage_risk(CoverageRiskInput {
        observation_id: "obs:api:dynamic".to_owned(),
        frontier: FrontierResult {
            entries: Vec::new(),
            diagnostics: Vec::new(),
        },
        support: Vec::new(),
        registry_diagnostics: vec![ObservationRegistryDiagnostic {
            kind: ObservationRegistryDiagnosticKind::UnsupportedEvidence,
            observation_id: "obs:api:dynamic".to_owned(),
            item: Some("dynamic_route_path".to_owned()),
        }],
        migration_diagnostics: Vec::new(),
        unsupported_features: vec![UnsupportedFeatureRiskNote {
            feature: "dynamic_route".to_owned(),
            artifact_id: Some(artifact("artifact:old:handler.py:111100000004")),
            entity_id: None,
        }],
    });

    assert_eq!(reasons(&report), vec![UnknownReason::DynamicString]);
}

#[test]
fn ambiguous_migration_maps_to_migration_map_ambiguous() {
    let report = classify_coverage_risk(CoverageRiskInput {
        observation_id: "obs:api:ambiguous".to_owned(),
        frontier: FrontierResult {
            entries: Vec::new(),
            diagnostics: Vec::new(),
        },
        support: Vec::new(),
        registry_diagnostics: Vec::new(),
        migration_diagnostics: vec![MigrationMapDiagnostic {
            kind: MigrationMapDiagnosticKind::MigrationMapAmbiguous,
            old: entity("old:py:handler:handler.py#createUser:100000000002"),
            targets: Vec::new(),
        }],
        unsupported_features: Vec::new(),
    });

    assert_eq!(reasons(&report), vec![UnknownReason::MigrationMapAmbiguous]);
}

#[test]
fn unsupported_extractor_and_framework_notes_are_classified() {
    let report = classify_coverage_risk(CoverageRiskInput {
        observation_id: "obs:api:unsupported".to_owned(),
        frontier: FrontierResult {
            entries: Vec::new(),
            diagnostics: Vec::new(),
        },
        support: Vec::new(),
        registry_diagnostics: Vec::new(),
        migration_diagnostics: Vec::new(),
        unsupported_features: vec![
            UnsupportedFeatureRiskNote {
                feature: "remote_ref".to_owned(),
                artifact_id: Some(artifact("artifact:old:openapi.yaml:111100000005")),
                entity_id: None,
            },
            UnsupportedFeatureRiskNote {
                feature: "framework_convention_nextjs".to_owned(),
                artifact_id: None,
                entity_id: Some(entity("old:ts:route:app/users/route.ts#POST:123456789abc")),
            },
        ],
    });

    assert_eq!(
        reasons(&report),
        vec![
            UnknownReason::UnsupportedExtractor,
            UnknownReason::UnsupportedFramework,
        ]
    );
}

#[test]
fn non_dynamic_unsupported_observation_evidence_is_rewrite_undefined() {
    let report = classify_coverage_risk(CoverageRiskInput {
        observation_id: "obs:api:opaque".to_owned(),
        frontier: FrontierResult {
            entries: Vec::new(),
            diagnostics: Vec::new(),
        },
        support: Vec::new(),
        registry_diagnostics: vec![ObservationRegistryDiagnostic {
            kind: ObservationRegistryDiagnosticKind::UnsupportedEvidence,
            observation_id: "obs:api:opaque".to_owned(),
            item: Some("opaque_expected_body".to_owned()),
        }],
        migration_diagnostics: Vec::new(),
        unsupported_features: Vec::new(),
    });

    assert_eq!(
        reasons(&report),
        vec![UnknownReason::ObservationRewriteUndefined]
    );
}

#[test]
fn duplicate_and_shuffled_inputs_are_deterministic_and_sanitized() {
    let frontier_diag = FrontierDiagnostic {
        kind: FrontierDiagnosticKind::MissingGraphEndpoint,
        observation_id: "obs:api:stable".to_owned(),
        item: "corr:route:9999999999999999".to_owned(),
    };
    let migration_diag = MigrationMapDiagnostic {
        kind: MigrationMapDiagnosticKind::MissingEndpoint,
        old: entity("old:openapi:route:openapi.yaml#/paths/~1users/post:100000000001"),
        targets: Vec::new(),
    };
    let unsupported = UnsupportedFeatureRiskNote {
        feature: "remote_ref".to_owned(),
        artifact_id: Some(artifact("artifact:old:openapi.yaml:111100000005")),
        entity_id: None,
    };

    let first = classify_coverage_risk(CoverageRiskInput {
        observation_id: "obs:api:stable".to_owned(),
        frontier: FrontierResult {
            entries: Vec::new(),
            diagnostics: vec![frontier_diag.clone(), frontier_diag.clone()],
        },
        support: vec![
            SupportRef::Edge(edge("edge:build_codegen:0000000000000003")),
            SupportRef::Corr(corr("corr:route:0000000000000001")),
        ],
        registry_diagnostics: Vec::new(),
        migration_diagnostics: vec![migration_diag.clone()],
        unsupported_features: vec![unsupported.clone()],
    });
    let second = classify_coverage_risk(CoverageRiskInput {
        observation_id: "obs:api:stable".to_owned(),
        frontier: FrontierResult {
            entries: Vec::new(),
            diagnostics: vec![frontier_diag],
        },
        support: vec![SupportRef::Corr(corr("corr:route:0000000000000001"))],
        registry_diagnostics: Vec::new(),
        migration_diagnostics: vec![migration_diag],
        unsupported_features: vec![unsupported],
    });

    assert_eq!(first, second);
    for risk in &first.risks {
        assert!(!risk.item.starts_with('/'));
        assert!(!risk.item.contains("/Users/"));
        assert!(!risk.item.contains("SECRET"));
        assert!(!risk.item.contains("raw_log"));
    }
}

fn reasons(report: &polyref_frontier::CoverageRiskReport) -> Vec<UnknownReason> {
    report.risks.iter().map(|risk| risk.reason).fold(
        Vec::<UnknownReason>::new(),
        |mut reasons, reason| {
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
            reasons
        },
    )
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
