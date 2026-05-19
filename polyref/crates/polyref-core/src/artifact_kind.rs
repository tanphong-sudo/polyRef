//! Closed `ArtifactKind` enum.
//!
//! Mirrors `schemas/artifact-kind.json`. See architecture §1.4 for the
//! 9-member rationale (build_file is its own family).

use serde::{Deserialize, Serialize};

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
