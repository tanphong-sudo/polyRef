//! `Language` enum tagging the EntityId language segment.
//!
//! Mirrors `schemas/language.json`. The literal `Build` covers package
//! manifests + build scripts (see `ArtifactKind::BuildFile`).

use serde::{Deserialize, Serialize};

/// Closed set of language tags recognised by PolyRef.
///
/// Cross-language source of truth: `schemas/language.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Language {
    /// Build manifests + build scripts.
    Build,
    /// Container image definitions.
    Dockerfile,
    /// Java source.
    Java,
    /// JSON.
    Json,
    /// JSON Schema.
    Jsonschema,
    /// OpenAPI YAML / JSON.
    Openapi,
    /// Python source.
    Py,
    /// SQL files.
    Sql,
    /// TypeScript / JavaScript source.
    Ts,
    /// Generic YAML (workflows, configs).
    Yaml,
}
