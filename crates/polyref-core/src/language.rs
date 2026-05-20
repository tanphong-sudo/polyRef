//! `Language` enum tagging the EntityId language segment.
//!
//! Mirrors `schemas/language.json`. The literal `Build` covers package
//! manifests + build scripts (see `ArtifactKind::BuildFile`).

use serde::{Deserialize, Serialize};
use thiserror::Error;

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

/// Failure to parse the lowercase tag string of a [`Language`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown Language tag: {0}")]
pub struct LanguageParseError(pub String);

impl Language {
    /// The canonical lowercase tag, identical to the serde
    /// representation and the `schemas/language.json` enum value.
    ///
    /// Defined here so consumer crates do not need a wildcard `_` arm
    /// on this `#[non_exhaustive]` business enum.
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            Language::Build => "build",
            Language::Dockerfile => "dockerfile",
            Language::Java => "java",
            Language::Json => "json",
            Language::Jsonschema => "jsonschema",
            Language::Openapi => "openapi",
            Language::Py => "py",
            Language::Sql => "sql",
            Language::Ts => "ts",
            Language::Yaml => "yaml",
        }
    }

    /// Parse the canonical lowercase tag string. Inverse of
    /// [`Self::as_tag`].
    ///
    /// # Errors
    ///
    /// Returns [`LanguageParseError`] when `s` does not match a closed
    /// member.
    pub fn parse(s: &str) -> Result<Self, LanguageParseError> {
        match s {
            "build" => Ok(Language::Build),
            "dockerfile" => Ok(Language::Dockerfile),
            "java" => Ok(Language::Java),
            "json" => Ok(Language::Json),
            "jsonschema" => Ok(Language::Jsonschema),
            "openapi" => Ok(Language::Openapi),
            "py" => Ok(Language::Py),
            "sql" => Ok(Language::Sql),
            "ts" => Ok(Language::Ts),
            "yaml" => Ok(Language::Yaml),
            other => Err(LanguageParseError(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn language_tag_round_trip_covers_all_variants() {
        let all = [
            Language::Build,
            Language::Dockerfile,
            Language::Java,
            Language::Json,
            Language::Jsonschema,
            Language::Openapi,
            Language::Py,
            Language::Sql,
            Language::Ts,
            Language::Yaml,
        ];
        for lang in all {
            assert_eq!(Language::parse(lang.as_tag()).unwrap(), lang);
        }
    }

    #[test]
    fn language_parse_rejects_unknown() {
        assert!(Language::parse("rust").is_err());
    }

    #[test]
    fn language_tag_matches_serde() {
        let lang = Language::Jsonschema;
        let json = serde_json::to_string(&lang).unwrap();
        assert_eq!(json, format!("\"{}\"", lang.as_tag()));
    }
}
