//! Closed `CorrespondenceKind` enum.
//!
//! Mirrors `schemas/correspondence-kind.json`. Paper §3.2 lists the
//! initial set as: call, route, schema, serialization, configuration,
//! build, query/table, event, test-oracle, workflow.

use serde::{Deserialize, Serialize};

/// Correspondence kinds recognised by PolyRef.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CorrespondenceKind {
    /// Build / codegen edge.
    BuildCodegen,
    /// Plain function / method call across artifact boundaries.
    Call,
    /// Application configuration key correspondence.
    Configuration,
    /// Event emitter ↔ consumer.
    Event,
    /// Generated client method ↔ API spec entry.
    GeneratedClient,
    /// Query / table / ORM entity correspondence.
    QueryTable,
    /// HTTP route ↔ handler ↔ test/client call.
    Route,
    /// Schema field / type correspondence.
    Schema,
    /// Serialization (DTO ↔ schema ↔ generated client type).
    Serialization,
    /// Test oracle correspondence.
    TestOracle,
    /// Workflow target correspondence.
    Workflow,
}
