//! Graph-layer migration-map builder for Layer 5.
//!
//! The builder consumes explicit old-to-new entity rewrite candidates and
//! produces a validated [`polyref_core::MigrationMap`]. It enforces the paper
//! Definition 5 type-respecting rule using graph entity kinds while preserving
//! ambiguous or missing evidence as deterministic diagnostics for later layers.

use crate::blobstore::BlobKey;
use crate::error::{GraphStoreError, Result};
use crate::read_model::GraphReadModel;
use crate::Entity;
use polyref_core::canonical;
use polyref_core::ids::EntityId;
use polyref_core::migration_map::{MigrationConflict, MigrationMap};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Confidence class for one old-to-new rewrite candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[non_exhaustive]
pub enum RewriteConfidence {
    /// Concrete evidence names a single new entity target.
    Concrete,
    /// Evidence exists but does not identify one unique target.
    Ambiguous,
    /// Required endpoint or target evidence is missing.
    Missing,
}

/// Audit-safe provenance for a candidate source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateProvenance {
    /// Short source tag such as `ide`, `llm`, `extractor`, or fixture name.
    pub source: String,
    /// Optional content-addressed payload hash for raw evidence stored elsewhere.
    pub payload_hash: Option<BlobKey>,
}

/// Candidate rewrite from an old entity to an optional new entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityRewriteCandidate {
    /// Old-side entity being migrated.
    pub old: EntityId,
    /// New-side target when known.
    pub new: Option<EntityId>,
    /// Confidence class for this candidate.
    pub confidence: RewriteConfidence,
    /// Audit-safe source metadata.
    pub provenance: CandidateProvenance,
}

/// Deterministic diagnostic category emitted while building `μ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[non_exhaustive]
pub enum MigrationMapDiagnosticKind {
    /// Multiple concrete targets disagree for one old entity.
    MigrationMapConflict,
    /// Candidate evidence is ambiguous and cannot pick one target.
    MigrationMapAmbiguous,
    /// Required old or new endpoint evidence is absent from candidates or graph.
    MissingEndpoint,
    /// Old and new graph entities have different local kinds.
    KindMismatch,
}

/// One deterministic migration-map builder diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationMapDiagnostic {
    /// Diagnostic category.
    pub kind: MigrationMapDiagnosticKind,
    /// Old entity affected by the diagnostic.
    pub old: EntityId,
    /// Candidate target entities relevant to the diagnostic, sorted by id.
    pub targets: Vec<EntityId>,
}

/// Audit-safe summary for a migration-map build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationMapBuildAudit {
    /// Number of input candidates.
    pub candidate_count: usize,
    /// Number of accepted rewrites.
    pub rewrite_count: usize,
    /// Number of diagnostics emitted.
    pub diagnostic_count: usize,
    /// Canonical hash of sanitized candidate ids and confidence classes.
    pub candidate_payload_hash: BlobKey,
    /// Canonical hash of accepted rewrite ids.
    pub rewrite_payload_hash: BlobKey,
    /// Canonical hash of diagnostics.
    pub diagnostic_payload_hash: BlobKey,
}

/// Result of building graph-layer migration map `μ`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationMapBuildResult {
    /// Validated core migration map.
    pub migration_map: MigrationMap,
    /// Deterministic diagnostics preserved for later Layer 6 evidence/statuses.
    pub diagnostics: Vec<MigrationMapDiagnostic>,
    /// Audit-safe summary with counts and payload hashes only.
    pub audit: MigrationMapBuildAudit,
}

/// Build and validate a graph-layer migration map from explicit candidates.
///
/// The builder reads typed graph entities through [`GraphReadModel`] only. It
/// never reads SQLite internals and never infers candidates from patches.
pub fn build_migration_map<R, I>(graph: &R, candidates: I) -> Result<MigrationMapBuildResult>
where
    R: GraphReadModel,
    I: IntoIterator<Item = EntityRewriteCandidate>,
{
    let candidates: Vec<_> = candidates.into_iter().collect();
    let entities = graph
        .list_entities()?
        .into_iter()
        .map(|entity| (entity.entity_id.clone(), entity))
        .collect::<BTreeMap<_, _>>();

    let mut grouped = BTreeMap::<EntityId, Vec<EntityRewriteCandidate>>::new();
    for candidate in candidates.iter().cloned() {
        grouped
            .entry(candidate.old.clone())
            .or_default()
            .push(candidate);
    }

    let mut rewrites = BTreeMap::<EntityId, EntityId>::new();
    let mut conflicts = Vec::<MigrationConflict>::new();
    let mut diagnostics = Vec::<MigrationMapDiagnostic>::new();

    for (old, group) in grouped {
        let mut group_diagnostics = non_concrete_diagnostics(&old, &group);
        if !group_diagnostics.is_empty() {
            diagnostics.append(&mut group_diagnostics);
            continue;
        }

        if !entities.contains_key(&old) {
            diagnostics.push(MigrationMapDiagnostic {
                kind: MigrationMapDiagnosticKind::MissingEndpoint,
                old,
                targets: concrete_targets(&group),
            });
            continue;
        }

        let targets = concrete_targets(&group);
        match targets.as_slice() {
            [] => diagnostics.push(MigrationMapDiagnostic {
                kind: MigrationMapDiagnosticKind::MissingEndpoint,
                old,
                targets,
            }),
            [target] => accept_single_target(
                &entities,
                old,
                target.clone(),
                &mut rewrites,
                &mut diagnostics,
            ),
            [first, second, ..] => {
                conflicts.push(MigrationConflict {
                    old: old.clone(),
                    first: first.clone(),
                    second: second.clone(),
                });
                diagnostics.push(MigrationMapDiagnostic {
                    kind: MigrationMapDiagnosticKind::MigrationMapConflict,
                    old,
                    targets,
                });
            }
        }
    }

    let migration_map = MigrationMap::try_new(rewrites, Vec::new(), conflicts).map_err(|err| {
        GraphStoreError::Canonical {
            message: format!("unexpected migration-map invariant failure: {err}"),
        }
    })?;
    let audit = MigrationMapBuildAudit {
        candidate_count: candidates.len(),
        rewrite_count: migration_map.iter().count(),
        diagnostic_count: diagnostics.len(),
        candidate_payload_hash: canonical_hash(&candidate_summary(&candidates))?,
        rewrite_payload_hash: canonical_hash(&rewrite_summary(&migration_map))?,
        diagnostic_payload_hash: canonical_hash(&diagnostics)?,
    };

    Ok(MigrationMapBuildResult {
        migration_map,
        diagnostics,
        audit,
    })
}

fn non_concrete_diagnostics(
    old: &EntityId,
    group: &[EntityRewriteCandidate],
) -> Vec<MigrationMapDiagnostic> {
    let mut kinds = BTreeSet::new();
    for candidate in group {
        match candidate.confidence {
            RewriteConfidence::Concrete => {}
            RewriteConfidence::Ambiguous => {
                kinds.insert(MigrationMapDiagnosticKind::MigrationMapAmbiguous);
            }
            RewriteConfidence::Missing => {
                kinds.insert(MigrationMapDiagnosticKind::MissingEndpoint);
            }
        }
    }

    let targets = concrete_targets(group);
    kinds
        .into_iter()
        .map(|kind| MigrationMapDiagnostic {
            kind,
            old: old.clone(),
            targets: targets.clone(),
        })
        .collect()
}

fn accept_single_target(
    entities: &BTreeMap<EntityId, Entity>,
    old: EntityId,
    target: EntityId,
    rewrites: &mut BTreeMap<EntityId, EntityId>,
    diagnostics: &mut Vec<MigrationMapDiagnostic>,
) {
    let Some(old_entity) = entities.get(&old) else {
        diagnostics.push(MigrationMapDiagnostic {
            kind: MigrationMapDiagnosticKind::MissingEndpoint,
            old,
            targets: vec![target],
        });
        return;
    };
    let Some(new_entity) = entities.get(&target) else {
        diagnostics.push(MigrationMapDiagnostic {
            kind: MigrationMapDiagnosticKind::MissingEndpoint,
            old,
            targets: vec![target],
        });
        return;
    };
    if old_entity.kind != new_entity.kind {
        diagnostics.push(MigrationMapDiagnostic {
            kind: MigrationMapDiagnosticKind::KindMismatch,
            old,
            targets: vec![target],
        });
        return;
    }
    rewrites.insert(old, target);
}

fn concrete_targets(group: &[EntityRewriteCandidate]) -> Vec<EntityId> {
    group
        .iter()
        .filter(|candidate| candidate.confidence == RewriteConfidence::Concrete)
        .filter_map(|candidate| candidate.new.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Serialize)]
struct CandidateAuditRow<'a> {
    old: &'a str,
    new: Option<&'a str>,
    confidence: RewriteConfidence,
    provenance_payload_hash: Option<String>,
}

fn candidate_summary(candidates: &[EntityRewriteCandidate]) -> Vec<CandidateAuditRow<'_>> {
    let mut rows = candidates
        .iter()
        .map(|candidate| CandidateAuditRow {
            old: candidate.old.as_str(),
            new: candidate.new.as_ref().map(EntityId::as_str),
            confidence: candidate.confidence,
            provenance_payload_hash: candidate
                .provenance
                .payload_hash
                .as_ref()
                .map(BlobKey::to_hex),
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        (
            left.old,
            left.new,
            left.confidence,
            &left.provenance_payload_hash,
        )
            .cmp(&(
                right.old,
                right.new,
                right.confidence,
                &right.provenance_payload_hash,
            ))
    });
    rows
}

#[derive(Serialize)]
struct RewriteAuditRow<'a> {
    old: &'a str,
    new: &'a str,
}

fn rewrite_summary(map: &MigrationMap) -> Vec<RewriteAuditRow<'_>> {
    map.iter()
        .map(|(old, new)| RewriteAuditRow {
            old: old.as_str(),
            new: new.as_str(),
        })
        .collect()
}

fn canonical_hash<T: Serialize>(value: &T) -> Result<BlobKey> {
    let value = serde_json::to_value(value)?;
    let bytes = canonical::canonicalize(&value).map_err(|err| GraphStoreError::Canonical {
        message: err.to_string(),
    })?;
    Ok(BlobKey::from_bytes(&bytes))
}
