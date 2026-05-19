//! KindChecker SPI types.
//!
//! Mirrors `schemas/checker-spi/{describe,check}.json`. The host
//! enforces resource limits (Slice 3); these types only carry the data.

use crate::limits::SafePath;
use polyref_core::correspondence_kind::CorrespondenceKind;
use polyref_core::evidence::Evidence;
use polyref_core::ids::EntityId;
use polyref_core::status::{BrokenReason, UnknownReason};
use serde::{Deserialize, Serialize};

/// Response for `describe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeResult {
    /// Stable contract id.
    pub contract_id: String,
    /// Kind this contract serves.
    pub kind_id: CorrespondenceKind,
    /// Endpoint signature `Σ_k`.
    pub endpoint_signature: Vec<String>,
    /// Required evidence-field names.
    pub required_evidence_fields: Vec<String>,
    /// Compatibility-rule id.
    pub compat_rule_id: String,
    /// Migration-rule id.
    pub migrate_rule_id: String,
    /// Plugin version string.
    pub plugin_version: String,
    /// Default deadline (ms) the host applies if none is given.
    pub default_timeout_ms: u32,
    /// Reasons this checker may emit as Unknown.
    pub supported_unknown_reasons: Vec<UnknownReason>,
    /// Reasons this checker may emit as Broken.
    pub supported_broken_reasons: Vec<BrokenReason>,
}

/// Endpoint argument carried in a `check` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointArg {
    /// Entity id of the endpoint.
    pub entity_id: EntityId,
    /// Type of the endpoint (matches the corresponding slot in
    /// `DescribeResult::endpoint_signature`).
    pub r#type: String,
}

/// Request body for `check`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRequest {
    /// Contract id (matches `DescribeResult::contract_id`).
    pub contract_id: String,
    /// Kind being checked.
    pub kind: CorrespondenceKind,
    /// Endpoint arguments.
    pub endpoints: Vec<EndpointArg>,
    /// Old repository root **relative to sandbox**.
    pub old_repo_root: SafePath,
    /// New repository root **relative to sandbox**.
    pub new_repo_root: SafePath,
    /// Migration-map excerpt scoped to these endpoints.
    pub migration_map_excerpt: serde_json::Value,
    /// Observation excerpt scoped to these endpoints.
    pub observation_excerpt: serde_json::Value,
    /// Deadline (ms).
    pub deadline_ms: u32,
    /// Log directory **relative to sandbox**.
    pub log_dir: SafePath,
}

/// Response body for `check`. The wire format is exactly
/// [`Evidence`].
pub type CheckResult = Evidence;
