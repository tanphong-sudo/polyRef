//! Closed `ArtifactKind` enum.
//!
//! Mirrors `schemas/artifact-kind.json`. See architecture §1.4 for the
//! 9-member rationale (build_file is its own family).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Closed set of artifact families recognised by PolyRef.
///
/// Cross-language source of truth: `schemas/artifact-kind.json`.
/// Adding a variant requires a schema minor bump per ADR-006.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ArtifactKind {
    /// Package manifests + build scripts (package.json, pyproject.toml,
    /// pom.xml, build.gradle, Bazel BUILD, Makefile, CMakeLists.txt,
    /// lockfiles).
    BuildFile,
    /// Application configuration (env files, application.yaml, JSON
    /// config).
    Config,
    /// Container image definitions.
    Dockerfile,
    /// Generated artifacts (codegen output, compiled bundles).
    Generated,
    /// SQL files or ORM model files.
    Query,
    /// API / data schemas (OpenAPI, JSON Schema, Avro, Protobuf).
    Schema,
    /// Application source files (TypeScript, Python, Java, Go, …).
    SourceFile,
    /// Test files.
    Test,
    /// Workflow / CI definitions (GitHub Actions, Jenkinsfile).
    Workflow,
}

/// Failure to parse the snake-case tag string of an [`ArtifactKind`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown ArtifactKind tag: {0}")]
pub struct ArtifactKindParseError(pub String);

impl ArtifactKind {
    /// The canonical snake-case tag, identical to the serde
    /// representation and the `schemas/artifact-kind.json` enum value.
    ///
    /// Defined here (not in consumer crates) so the exhaustive match
    /// stays inside the module that owns the enum. Keeps consumer
    /// crates free of the wildcard `_` arm that
    /// `#[non_exhaustive]` would otherwise force on a business enum
    /// (see `rust-coding-style.md`).
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            ArtifactKind::BuildFile => "build_file",
            ArtifactKind::Config => "config",
            ArtifactKind::Dockerfile => "dockerfile",
            ArtifactKind::Generated => "generated",
            ArtifactKind::Query => "query",
            ArtifactKind::Schema => "schema",
            ArtifactKind::SourceFile => "source_file",
            ArtifactKind::Test => "test",
            ArtifactKind::Workflow => "workflow",
        }
    }

    /// Parse the canonical snake-case tag string. Inverse of
    /// [`Self::as_tag`].
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactKindParseError`] when `s` does not exactly
    /// match one of the 9 closed members.
    pub fn parse(s: &str) -> Result<Self, ArtifactKindParseError> {
        match s {
            "build_file" => Ok(ArtifactKind::BuildFile),
            "config" => Ok(ArtifactKind::Config),
            "dockerfile" => Ok(ArtifactKind::Dockerfile),
            "generated" => Ok(ArtifactKind::Generated),
            "query" => Ok(ArtifactKind::Query),
            "schema" => Ok(ArtifactKind::Schema),
            "source_file" => Ok(ArtifactKind::SourceFile),
            "test" => Ok(ArtifactKind::Test),
            "workflow" => Ok(ArtifactKind::Workflow),
            other => Err(ArtifactKindParseError(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// All 9 variants must round-trip through `as_tag` / `parse`. Adding
    /// a new variant fails this test until the helper is updated, which
    /// is the intended drift signal.
    #[test]
    fn artifact_kind_tag_round_trip_covers_all_nine_variants() {
        let all = [
            ArtifactKind::BuildFile,
            ArtifactKind::Config,
            ArtifactKind::Dockerfile,
            ArtifactKind::Generated,
            ArtifactKind::Query,
            ArtifactKind::Schema,
            ArtifactKind::SourceFile,
            ArtifactKind::Test,
            ArtifactKind::Workflow,
        ];
        assert_eq!(all.len(), 9, "ArtifactKind has exactly 9 members");
        for kind in all {
            let tag = kind.as_tag();
            let parsed = ArtifactKind::parse(tag).expect("round-trip");
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn artifact_kind_parse_rejects_unknown_tag() {
        let err = ArtifactKind::parse("not-a-kind").unwrap_err();
        assert_eq!(err.0, "not-a-kind");
    }

    #[test]
    fn artifact_kind_tag_matches_serde_representation() {
        // serde emits snake_case; `as_tag` must match byte-for-byte.
        for kind in [
            ArtifactKind::BuildFile,
            ArtifactKind::SourceFile,
            ArtifactKind::Workflow,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize");
            // strip surrounding JSON quotes
            let expected = format!("\"{}\"", kind.as_tag());
            assert_eq!(json, expected);
        }
    }
}
