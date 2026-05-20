//! Closed `CorrespondenceKind` enum.
//!
//! Mirrors `schemas/correspondence-kind.json`. Paper §3.2 lists the
//! initial set as: call, route, schema, serialization, configuration,
//! build, query/table, event, test-oracle, workflow.

use serde::{Deserialize, Serialize};
use thiserror::Error;

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

/// Failure to parse the snake-case tag string of a
/// [`CorrespondenceKind`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown CorrespondenceKind tag: {0}")]
pub struct CorrespondenceKindParseError(pub String);

impl CorrespondenceKind {
    /// The canonical snake-case tag identical to the serde
    /// representation and `schemas/correspondence-kind.json`.
    ///
    /// Defined here so consumer crates do not need a wildcard `_` arm.
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            CorrespondenceKind::BuildCodegen => "build_codegen",
            CorrespondenceKind::Call => "call",
            CorrespondenceKind::Configuration => "configuration",
            CorrespondenceKind::Event => "event",
            CorrespondenceKind::GeneratedClient => "generated_client",
            CorrespondenceKind::QueryTable => "query_table",
            CorrespondenceKind::Route => "route",
            CorrespondenceKind::Schema => "schema",
            CorrespondenceKind::Serialization => "serialization",
            CorrespondenceKind::TestOracle => "test_oracle",
            CorrespondenceKind::Workflow => "workflow",
        }
    }

    /// Parse the canonical snake-case tag string.
    ///
    /// # Errors
    ///
    /// Returns [`CorrespondenceKindParseError`] when `s` does not match
    /// a closed member.
    pub fn parse(s: &str) -> Result<Self, CorrespondenceKindParseError> {
        match s {
            "build_codegen" => Ok(CorrespondenceKind::BuildCodegen),
            "call" => Ok(CorrespondenceKind::Call),
            "configuration" => Ok(CorrespondenceKind::Configuration),
            "event" => Ok(CorrespondenceKind::Event),
            "generated_client" => Ok(CorrespondenceKind::GeneratedClient),
            "query_table" => Ok(CorrespondenceKind::QueryTable),
            "route" => Ok(CorrespondenceKind::Route),
            "schema" => Ok(CorrespondenceKind::Schema),
            "serialization" => Ok(CorrespondenceKind::Serialization),
            "test_oracle" => Ok(CorrespondenceKind::TestOracle),
            "workflow" => Ok(CorrespondenceKind::Workflow),
            other => Err(CorrespondenceKindParseError(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn correspondence_kind_round_trip() {
        let all = [
            CorrespondenceKind::BuildCodegen,
            CorrespondenceKind::Call,
            CorrespondenceKind::Configuration,
            CorrespondenceKind::Event,
            CorrespondenceKind::GeneratedClient,
            CorrespondenceKind::QueryTable,
            CorrespondenceKind::Route,
            CorrespondenceKind::Schema,
            CorrespondenceKind::Serialization,
            CorrespondenceKind::TestOracle,
            CorrespondenceKind::Workflow,
        ];
        for kind in all {
            assert_eq!(CorrespondenceKind::parse(kind.as_tag()).unwrap(), kind);
        }
    }

    #[test]
    fn correspondence_kind_parse_rejects_unknown() {
        assert!(CorrespondenceKind::parse("custom").is_err());
    }
}
