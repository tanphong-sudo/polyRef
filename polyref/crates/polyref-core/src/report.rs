//! Validation report aggregate root.
//!
//! `ValidationReport` is the only path by which a candidate decision
//! reaches the world. Its constructor (`assemble`) is also the
//! type-system enforcement point for the fail-closed invariant from
//! paper §3:
//!
//! > A candidate decision of `Accepted` cannot coexist with
//! > `missing_endpoint_unknown == true`.
//!
//! Code that tries to construct such a report must take the
//! `Err(ReportInvariantError::MissingEndpointUnknownInAccepted)` path.

use crate::evidence::Evidence;
use crate::observation::Visibility;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Per-candidate decision computed as the meet over visible
/// observations (ADR-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum CandidateDecision {
    /// Every visible observation is `accepted`.
    Accepted,
    /// At least one visible observation is `broken`.
    Broken,
    /// Otherwise.
    Unknown,
}

/// Per-observation decision in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ObservationDecision {
    /// All required frontier items are `Pres` or `Migrated` and
    /// build / observation obligations validate.
    Accepted,
    /// At least one frontier item is `Broken`.
    Broken,
    /// Otherwise.
    Unknown,
}

/// One row in the report — a single observation's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationRow {
    /// Stable id for the observation.
    pub observation_id: String,
    /// Closed kind tag (api_call / test_invocation / build_target /
    /// workflow_run / schema_validation).
    pub obs_kind: String,
    /// Visibility class.
    pub visibility: Visibility,
    /// Number of frontier items considered.
    pub frontier_size: u32,
    /// Per-item evidence. Each `Evidence::outcome()` lives in
    /// `{Pres, Migrated, Broken, Unknown}`.
    pub items: Vec<Evidence>,
    /// Per-observation status.
    pub status: ObservationDecision,
}

/// Inputs to [`ValidationReport::assemble`].
#[derive(Debug, Clone)]
pub struct ReportParts {
    /// Stable report id.
    pub report_id: String,
    /// Repository identifiers (old / new).
    pub repos: ReportRepos,
    /// Candidate metadata.
    pub candidate: ReportCandidate,
    /// Configurations.
    pub configs: ReportConfigs,
    /// Per-observation rows.
    pub observations: Vec<ObservationRow>,
    /// Whether any frontier item was rejected as Unknown specifically
    /// because of a missing endpoint. The fail-closed invariant uses
    /// this to refuse `Accepted`.
    pub missing_endpoint_unknown: bool,
    /// Audit pointer references.
    pub audit_pointers: ReportAuditPointers,
}

/// Repo references in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRepos {
    /// Old repository.
    pub old: ReportRepoRef,
    /// New repository.
    pub new: ReportRepoRef,
}

/// One repo reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRepoRef {
    /// Stable repo id.
    pub repo_id: String,
    /// Commit sha (40 or 64 hex chars).
    pub commit: String,
}

/// Candidate metadata in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportCandidate {
    /// Candidate id.
    pub candidate_id: String,
    /// Source kind: `llm` / `ide` / `template` / `manual`.
    pub source: String,
    /// Patch hash (sha256 hex).
    pub patch_hash: String,
}

/// Pinned tool configurations in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportConfigs {
    /// Map from extractor id to version.
    pub extractor_versions: std::collections::BTreeMap<String, String>,
    /// Map from checker id to version.
    pub checker_versions: std::collections::BTreeMap<String, String>,
}

/// Audit pointer references in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportAuditPointers {
    /// Path to the audit NDJSON.
    pub audit_ndjson: String,
    /// Path to the manifest.
    pub manifest_json: String,
}

/// Failure modes of [`ValidationReport::assemble`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportInvariantError {
    /// `candidate_decision == Accepted` with `missing_endpoint_unknown
    /// == true`. The fail-closed invariant.
    #[error("candidate decision 'accepted' is incompatible with missing_endpoint_unknown=true")]
    MissingEndpointUnknownInAccepted,
    /// An observation has status `Accepted` but contains a non-accepting item.
    #[error("observation has Accepted status but a non-accepting item")]
    NonAcceptingItemInAcceptedObservation,
    /// An evidence pointer escaped the `evidence/` subtree. Slice 7
    /// also re-checks this.
    #[error("evidence pointer escaped evidence/ subtree")]
    EvidencePointerOutsideEvidenceDir,
    /// An id field has invalid syntax. Cross-graph reference resolution
    /// is *not* performed in this slice; that lives in `polyref-graph`.
    #[error("id field has invalid syntax")]
    InvalidIdSyntax,
}

/// Validation report aggregate root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    schema_version: String,
    report_id: String,
    candidate: ReportCandidate,
    repos: ReportRepos,
    configs: ReportConfigs,
    observations: Vec<ObservationRow>,
    candidate_decision: CandidateDecision,
    missing_endpoint_unknown: bool,
    audit_pointers: ReportAuditPointers,
}

impl ValidationReport {
    /// Schema version this aggregate root produces.
    pub const SCHEMA_VERSION: &'static str = "0.1.0";

    /// Assemble a validation report from `ReportParts`. Computes
    /// `candidate_decision` as the meet over visible observations, and
    /// rejects assembly if the fail-closed invariant would be violated.
    ///
    /// Slice 1 stub: returns `todo!()`. Implement during §E-1
    /// `report_assemble_*` test pass.
    pub fn assemble(_parts: ReportParts) -> Result<Self, ReportInvariantError> {
        todo!(
            "§E-1 report_assemble_rejects_accepted_with_missing_endpoint_unknown; \
             compute meet rule; reject EvidencePointerOutsideEvidenceDir"
        )
    }

    /// Computed candidate decision.
    #[must_use]
    pub fn candidate_decision(&self) -> CandidateDecision {
        self.candidate_decision
    }

    /// Whether any frontier item was Unknown due to a missing endpoint.
    #[must_use]
    pub fn missing_endpoint_unknown(&self) -> bool {
        self.missing_endpoint_unknown
    }

    /// Per-observation rows.
    #[must_use]
    pub fn observations(&self) -> &[ObservationRow] {
        &self.observations
    }

    /// Schema version string.
    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }
}
