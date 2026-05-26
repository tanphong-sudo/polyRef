//! Coverage-risk classification for Layer 5.
//!
//! This module is intentionally pure: it consumes already-computed frontier,
//! registry, migration-map, and extractor diagnostics and reports deterministic
//! fail-closed `UnknownReason` risks for later Layer 6 status assignment.

use crate::{FrontierDiagnosticKind, FrontierResult};
use polyref_core::{
    ids::{ArtifactId, EntityId},
    observation::SupportRef,
    status::UnknownReason,
};
use polyref_graph::{
    MigrationMapDiagnostic, MigrationMapDiagnosticKind, ObservationRegistryDiagnostic,
    ObservationRegistryDiagnosticKind,
};
use std::collections::BTreeMap;

/// Sanitized unsupported-feature note accepted by coverage-risk classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedFeatureRiskNote {
    /// Short feature tag such as `dynamic_route`, `remote_ref`, or `framework_*`.
    pub feature: String,
    /// Optional artifact id associated with the note.
    pub artifact_id: Option<ArtifactId>,
    /// Optional entity id associated with the note.
    pub entity_id: Option<EntityId>,
}

/// Source subsystem that produced a coverage risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CoverageRiskSource {
    /// Frontier closure diagnostics.
    Frontier,
    /// Observation registry diagnostics.
    ObservationRegistry,
    /// Migration-map builder diagnostics.
    MigrationMap,
    /// Extractor unsupported-feature notes.
    Extractor,
}

/// Input to the pure coverage-risk classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRiskInput {
    /// Observation being classified.
    pub observation_id: String,
    /// Computed frontier result.
    pub frontier: FrontierResult,
    /// Observation support set used by the frontier.
    pub support: Vec<SupportRef>,
    /// Diagnostics emitted by the observation registry.
    pub registry_diagnostics: Vec<ObservationRegistryDiagnostic>,
    /// Diagnostics emitted while building `μ`.
    pub migration_diagnostics: Vec<MigrationMapDiagnostic>,
    /// Sanitized extractor unsupported-feature notes.
    pub unsupported_features: Vec<UnsupportedFeatureRiskNote>,
}

/// One fail-closed coverage risk prepared for Layer 6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRisk {
    /// Unknown reason that best represents the coverage gap.
    pub reason: UnknownReason,
    /// Source subsystem that produced the risk.
    pub source: CoverageRiskSource,
    /// Observation being classified.
    pub observation_id: String,
    /// Sanitized id/tag associated with the risk.
    pub item: String,
}

/// Deterministic coverage-risk report for one observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRiskReport {
    /// Observation being classified.
    pub observation_id: String,
    /// Sorted, deduped coverage risks.
    pub risks: Vec<CoverageRisk>,
    /// Whether any coverage risk blocks later acceptance.
    pub is_blocked: bool,
}

/// Classify fail-closed coverage risks without assigning final statuses.
#[must_use]
pub fn classify_coverage_risk(input: CoverageRiskInput) -> CoverageRiskReport {
    let mut risks = BTreeMap::<RiskKey, CoverageRisk>::new();
    let observation_id = input.observation_id;

    for diagnostic in input.frontier.diagnostics {
        match diagnostic.kind {
            FrontierDiagnosticKind::MissingSupport
            | FrontierDiagnosticKind::MissingGraphEndpoint => {
                insert_risk(
                    &mut risks,
                    UnknownReason::MissingEndpoint,
                    CoverageRiskSource::Frontier,
                    &observation_id,
                    diagnostic.item,
                );
            }
        }
    }

    for diagnostic in input.registry_diagnostics {
        if diagnostic.observation_id != observation_id {
            continue;
        }
        match diagnostic.kind {
            ObservationRegistryDiagnosticKind::MissingSupport => insert_risk(
                &mut risks,
                UnknownReason::MissingEndpoint,
                CoverageRiskSource::ObservationRegistry,
                &observation_id,
                diagnostic
                    .item
                    .unwrap_or_else(|| "missing_support".to_owned()),
            ),
            ObservationRegistryDiagnosticKind::UnsupportedEvidence => {
                let item = diagnostic
                    .item
                    .unwrap_or_else(|| "unsupported_observation_evidence".to_owned());
                let reason = if is_dynamic_feature(&item) {
                    UnknownReason::DynamicString
                } else {
                    UnknownReason::ObservationRewriteUndefined
                };
                insert_risk(
                    &mut risks,
                    reason,
                    CoverageRiskSource::ObservationRegistry,
                    &observation_id,
                    item,
                );
            }
            ObservationRegistryDiagnosticKind::DuplicateSupport
            | ObservationRegistryDiagnosticKind::InvalidObservationId => {}
            _ => {}
        }
    }

    for diagnostic in input.migration_diagnostics {
        match diagnostic.kind {
            MigrationMapDiagnosticKind::MigrationMapAmbiguous => insert_risk(
                &mut risks,
                UnknownReason::MigrationMapAmbiguous,
                CoverageRiskSource::MigrationMap,
                &observation_id,
                diagnostic.old.as_str().to_owned(),
            ),
            MigrationMapDiagnosticKind::MissingEndpoint => insert_risk(
                &mut risks,
                UnknownReason::MissingEndpoint,
                CoverageRiskSource::MigrationMap,
                &observation_id,
                diagnostic.old.as_str().to_owned(),
            ),
            MigrationMapDiagnosticKind::MigrationMapConflict
            | MigrationMapDiagnosticKind::KindMismatch => {}
            _ => {}
        }
    }

    for note in input.unsupported_features {
        let reason = classify_unsupported_feature(&note.feature);
        insert_risk(
            &mut risks,
            reason,
            CoverageRiskSource::Extractor,
            &observation_id,
            unsupported_item(&note),
        );
    }

    let risks = risks.into_values().collect::<Vec<_>>();
    CoverageRiskReport {
        observation_id,
        is_blocked: !risks.is_empty(),
        risks,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RiskKey {
    observation_id: String,
    reason: &'static str,
    source: &'static str,
    item: String,
}

fn insert_risk(
    risks: &mut BTreeMap<RiskKey, CoverageRisk>,
    reason: UnknownReason,
    source: CoverageRiskSource,
    observation_id: &str,
    item: String,
) {
    let sanitized_item = sanitize_item(item);
    let key = RiskKey {
        observation_id: observation_id.to_owned(),
        reason: reason_key(reason),
        source: source_key(source),
        item: sanitized_item.clone(),
    };
    risks.entry(key).or_insert_with(|| CoverageRisk {
        reason,
        source,
        observation_id: observation_id.to_owned(),
        item: sanitized_item,
    });
}

fn classify_unsupported_feature(feature: &str) -> UnknownReason {
    if is_dynamic_feature(feature) {
        UnknownReason::DynamicString
    } else if feature.contains("framework") {
        UnknownReason::UnsupportedFramework
    } else {
        UnknownReason::UnsupportedExtractor
    }
}

fn is_dynamic_feature(feature: &str) -> bool {
    feature.contains("dynamic_route")
        || feature.contains("dynamic_string")
        || feature == "dynamic"
        || feature.contains("dynamic_route_path")
}

fn unsupported_item(note: &UnsupportedFeatureRiskNote) -> String {
    if let Some(entity_id) = &note.entity_id {
        format!("{}:{}", note.feature, entity_id.as_str())
    } else if let Some(artifact_id) = &note.artifact_id {
        format!("{}:{}", note.feature, artifact_id.as_str())
    } else {
        note.feature.clone()
    }
}

fn sanitize_item(item: String) -> String {
    let trimmed = item.trim();
    if trimmed.starts_with('/') || trimmed.contains("/Users/") || trimmed.contains("SECRET") {
        "redacted".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn reason_key(reason: UnknownReason) -> &'static str {
    match reason {
        UnknownReason::DynamicString => "dynamic_string",
        UnknownReason::MigrationMapAmbiguous => "migration_map_ambiguous",
        UnknownReason::MissingEndpoint => "missing_endpoint",
        UnknownReason::ObservationRewriteUndefined => "observation_rewrite_undefined",
        UnknownReason::UnsupportedExtractor => "unsupported_extractor",
        UnknownReason::UnsupportedFramework => "unsupported_framework",
        _ => "other",
    }
}

fn source_key(source: CoverageRiskSource) -> &'static str {
    match source {
        CoverageRiskSource::Extractor => "extractor",
        CoverageRiskSource::Frontier => "frontier",
        CoverageRiskSource::MigrationMap => "migration_map",
        CoverageRiskSource::ObservationRegistry => "observation_registry",
    }
}
