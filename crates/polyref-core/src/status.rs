//! Validation status model — the load-bearing fail-closed type.
//!
//! `Outcome` is a sum type whose reason payloads live inside `Broken`
//! and `Unknown`. Code that tries to attach an `UnknownReason` to a
//! `Pres` or `Migrated` cannot compile — that is the type-level
//! enforcement of paper §3's fail-closed convention.

use serde::{Deserialize, Serialize};

/// Closed set of frontier-item outcomes.
///
/// Per paper Definition 8 + the fail-closed convention:
///
/// - `Pres` — endpoints unchanged, well-typed in `R'`, compatibility
///   predicate holds.
/// - `Migrated` — endpoints rewritten by μ; migration predicate holds.
/// - `Broken` — a checker refuted a concrete predicate.
/// - `Unknown` — evidence is missing / unsupported / ambiguous.
///
/// Note that `Pres` and `Migrated` carry no payload, while `Broken`
/// and `Unknown` carry their reason. Attempting `Outcome::Pres` with a
/// `BrokenReason` is a compile error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tag", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Outcome {
    /// Item compatible without endpoint identity rewrite.
    Pres,
    /// Item consistently rewritten by μ.
    Migrated,
    /// A checker refuted a concrete predicate.
    Broken {
        /// Concrete refutation reason.
        reason: BrokenReason,
    },
    /// Evidence is missing, unsupported, ambiguous, or timed-out.
    Unknown {
        /// Reason a checker could not return Pres / Migrated / Broken.
        reason: UnknownReason,
    },
}

/// Closed set of reasons a checker may emit `Unknown`.
///
/// Source of truth: `schemas/unknown-reason.json`. The wire format uses
/// `snake_case` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UnknownReason {
    /// Multiple candidate endpoints remain after evidence collection.
    AmbiguousEndpoint,
    /// Plugin exceeded its deadline.
    CheckerTimeout,
    /// Generator graph contains a cycle.
    CyclicGenerator,
    /// Dynamic trace did not pass ADR-004 admission.
    DynamicEvidenceUnverified,
    /// Endpoint built from a dynamic string at runtime.
    DynamicString,
    /// Generated artifact lacks all three pillars (no source map, no
    /// re-execution, no checksum).
    GeneratedEvidenceMissing,
    /// Generated artifact has only one of the three pillars.
    GeneratedEvidenceWeak,
    /// Migration map has multiple candidate targets for a shared
    /// entity but they are not concrete enough to reject as Broken.
    MigrationMapAmbiguous,
    /// At least one endpoint slot has no candidate entity.
    MissingEndpoint,
    /// Algorithm A2 fallback: no rule applied.
    NoAcceptingRuleApplied,
    /// μ(o) cannot be defined for a required position.
    ObservationRewriteUndefined,
    /// Build target produced by a non-introspectable cache.
    OpaqueBuildCache,
    /// Plugin process crashed or returned malformed output.
    PluginFailure,
    /// Endpoint resolved by reflection / metaprogramming.
    Reflection,
    /// Extractor reported `unsupported_features` for the artifact.
    UnsupportedExtractor,
    /// Framework convention not modeled by any kind.
    UnsupportedFramework,
}

/// Closed set of reasons a checker may emit `Broken`.
///
/// Source of truth: `schemas/broken-reason.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BrokenReason {
    /// Build target unreachable from the migrated source.
    BuildTargetUnreachable,
    /// Event payload incompatible with consumer.
    EventPayloadIncompatible,
    /// Generated client method or package target stale.
    GeneratedClientStale,
    /// Re-running the generator produced a target that does not match
    /// the committed file.
    GeneratorMismatch,
    /// Route handler binding mismatch.
    HandlerBindingMismatch,
    /// A language- or format-specific local checker refuted local
    /// preservation.
    LocalCheckerFailure,
    /// Two proposers proposed conflicting concrete targets for the
    /// same entity.
    MigrationMapConflict,
    /// Query refers to a missing table or column after migration.
    QueryTableMissing,
    /// Required schema field changed in a backwards-incompatible way.
    RequiredFieldDrift,
    /// Route path was refuted by the route checker.
    RoutePathRefuted,
    /// Schema diff reports an incompatible change.
    SchemaIncompatible,
    /// Workflow still packages the old target after the route was
    /// rewritten.
    WorkflowPackagesOldTarget,
}

impl Outcome {
    /// Returns `true` for `Pres` or `Migrated`.
    #[must_use]
    pub fn is_accepting(&self) -> bool {
        matches!(self, Outcome::Pres | Outcome::Migrated)
    }
}

impl UnknownReason {
    /// Canonical snake-case tag, identical to the serde representation and
    /// `schemas/unknown-reason.json`. Defined here so consumer crates can order
    /// or display reasons without a wildcard `_` arm on this `#[non_exhaustive]`
    /// enum (used by the engine's deterministic Unknown tie-break, ADR-005 §3).
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            UnknownReason::AmbiguousEndpoint => "ambiguous_endpoint",
            UnknownReason::CheckerTimeout => "checker_timeout",
            UnknownReason::CyclicGenerator => "cyclic_generator",
            UnknownReason::DynamicEvidenceUnverified => "dynamic_evidence_unverified",
            UnknownReason::DynamicString => "dynamic_string",
            UnknownReason::GeneratedEvidenceMissing => "generated_evidence_missing",
            UnknownReason::GeneratedEvidenceWeak => "generated_evidence_weak",
            UnknownReason::MigrationMapAmbiguous => "migration_map_ambiguous",
            UnknownReason::MissingEndpoint => "missing_endpoint",
            UnknownReason::NoAcceptingRuleApplied => "no_accepting_rule_applied",
            UnknownReason::ObservationRewriteUndefined => "observation_rewrite_undefined",
            UnknownReason::OpaqueBuildCache => "opaque_build_cache",
            UnknownReason::PluginFailure => "plugin_failure",
            UnknownReason::Reflection => "reflection",
            UnknownReason::UnsupportedExtractor => "unsupported_extractor",
            UnknownReason::UnsupportedFramework => "unsupported_framework",
        }
    }
}

impl BrokenReason {
    /// Canonical snake-case tag, identical to the serde representation and
    /// `schemas/broken-reason.json`. Defined here so consumer crates can order
    /// or display reasons without a wildcard `_` arm on this `#[non_exhaustive]`
    /// enum (used by the engine's deterministic Broken tie-break, ADR-005 §3).
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            BrokenReason::BuildTargetUnreachable => "build_target_unreachable",
            BrokenReason::EventPayloadIncompatible => "event_payload_incompatible",
            BrokenReason::GeneratedClientStale => "generated_client_stale",
            BrokenReason::GeneratorMismatch => "generator_mismatch",
            BrokenReason::HandlerBindingMismatch => "handler_binding_mismatch",
            BrokenReason::LocalCheckerFailure => "local_checker_failure",
            BrokenReason::MigrationMapConflict => "migration_map_conflict",
            BrokenReason::QueryTableMissing => "query_table_missing",
            BrokenReason::RequiredFieldDrift => "required_field_drift",
            BrokenReason::RoutePathRefuted => "route_path_refuted",
            BrokenReason::SchemaIncompatible => "schema_incompatible",
            BrokenReason::WorkflowPackagesOldTarget => "workflow_packages_old_target",
        }
    }
}
