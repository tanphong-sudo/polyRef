//! Observation kinds + visibility.
//!
//! Visibility (visible / held_out / evaluation_only) is *immutable* on
//! an `Observation` — promotion happens upstream by producing a new
//! `Observation`, per ADR-010 leakage-prevention rule.

use crate::ids::{CorrId, EdgeId, EntityId};
use serde::{Deserialize, Serialize};

pub mod visibility;
pub use visibility::Visibility;

/// HTTP method tag for `ApiCallObs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
#[non_exhaustive]
pub enum HttpMethod {
    /// GET
    Get,
    /// POST
    Post,
    /// PUT
    Put,
    /// PATCH
    Patch,
    /// DELETE
    Delete,
    /// HEAD
    Head,
    /// OPTIONS
    Options,
}

/// One element of an observation's support set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum SupportRef {
    /// Reference to a correspondence.
    Corr(CorrId),
    /// Reference to a build edge.
    Edge(EdgeId),
}

/// Observation kinds recognised in v1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "obs_kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Observation {
    /// HTTP API call observation.
    ApiCall(ApiCallObs),
    /// Test invocation observation.
    TestInvocation(TestObs),
    /// Build target observation.
    BuildTarget(BuildTargetObs),
    /// Workflow run observation.
    WorkflowRun(WorkflowObs),
    /// Schema validation observation.
    SchemaValidation(SchemaObs),
}

impl Observation {
    /// The canonical snake-case tag identical to the serde
    /// `obs_kind` discriminator and `schemas/observation/_kind.json`.
    ///
    /// Defined here so consumer crates do not need a wildcard `_` arm
    /// on this `#[non_exhaustive]` business enum (paper Def. 6).
    #[must_use]
    pub fn kind_tag(&self) -> &'static str {
        match self {
            Observation::ApiCall(_) => "api_call",
            Observation::TestInvocation(_) => "test_invocation",
            Observation::BuildTarget(_) => "build_target",
            Observation::WorkflowRun(_) => "workflow_run",
            Observation::SchemaValidation(_) => "schema_validation",
        }
    }

    /// Borrow the common observation header (visibility, support,
    /// defined-semantics flag) regardless of kind.
    #[must_use]
    pub fn header(&self) -> &ObsHeader {
        match self {
            Observation::ApiCall(o) => &o.header,
            Observation::TestInvocation(o) => &o.header,
            Observation::BuildTarget(o) => &o.header,
            Observation::WorkflowRun(o) => &o.header,
            Observation::SchemaValidation(o) => &o.header,
        }
    }
}

/// Common header on every observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObsHeader {
    /// Visibility class.
    pub visibility: Visibility,
    /// Resolved support set; see paper Definition 6.
    pub support: Vec<SupportRef>,
    /// Whether `supp(o)` resolves to typed entities. If false the
    /// observation status is `Unknown(ObservationRewriteUndefined)`.
    pub defined_semantics: bool,
}

/// API call observation typed fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiCallObs {
    /// HTTP method.
    pub method: HttpMethod,
    /// Path or path pattern.
    pub path: String,
    /// Optional request schema entity.
    pub request_schema_id: Option<EntityId>,
    /// Optional response schema entity.
    pub response_schema_id: Option<EntityId>,
    /// Optional generated-client method entity.
    pub client_id: Option<EntityId>,
    /// Common observation header.
    pub header: ObsHeader,
}

/// Test invocation observation typed fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestObs {
    /// Test entity.
    pub test_id: EntityId,
    /// Optional public entrypoint targeted by the test.
    pub public_entrypoint: Option<EntityId>,
    /// Common observation header.
    pub header: ObsHeader,
}

/// Build target observation typed fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildTargetObs {
    /// Target name.
    pub target_name: String,
    /// Optional generator command.
    pub generator_command: Option<String>,
    /// Optional expected artifact path.
    pub expected_artifact_path: Option<String>,
    /// Common observation header.
    pub header: ObsHeader,
}

/// Workflow run observation typed fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowObs {
    /// Workflow entity.
    pub workflow_id: EntityId,
    /// Optional packaged target name.
    pub packaged_target_name: Option<String>,
    /// Optional list of consulted env keys.
    pub env_keys: Vec<String>,
    /// Common observation header.
    pub header: ObsHeader,
}

/// Schema validation observation typed fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaObs {
    /// Schema entity.
    pub schema_id: EntityId,
    /// Sample payload, referenced via evidence pointer in v2.
    pub sample_payload_ref: Option<String>,
    /// Expected outcome (`valid` or `invalid`).
    pub expected_outcome: Option<SchemaExpectedOutcome>,
    /// Common observation header.
    pub header: ObsHeader,
}

/// Expected outcome of a schema validation observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SchemaExpectedOutcome {
    /// Payload is expected to validate.
    Valid,
    /// Payload is expected to reject.
    Invalid,
}
