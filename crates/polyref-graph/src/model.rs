//! Persistent domain types: [`Artifact`], [`Entity`], [`Correspondence`],
//! [`BuildEdge`].
//!
//! These mirror paper Definition 1 (`R = (A, N, L, C, Build, O, owner,
//! type)`). All ids are validated `polyref-core` newtypes, so untrusted
//! plugin output cannot smuggle path-traversal or break the
//! type-respecting invariant on [`polyref_core::MigrationMap`].
//!
//! Endpoint ordering on a [`Correspondence`] is significant (paper
//! Def. 3: `ends(c) = (n_1, ..., n_m)`); the SQL layer preserves it via
//! the `position` column.

use polyref_core::{
    artifact_kind::ArtifactKind, correspondence_kind::CorrespondenceKind, ids::ArtifactId,
    ids::CorrId, ids::EdgeId, ids::EntityId, language::Language,
};
use serde::{Deserialize, Serialize};

/// One row of `A` in the repository tuple.
///
/// `repo_side` is held implicitly inside the `EntityId` / `ArtifactId`
/// strings (ADR-003 grammar), but cached here to allow indexed lookup
/// without re-parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Stable id for the artifact.
    pub artifact_id: ArtifactId,
    /// Which side of the refactoring this artifact belongs to.
    pub repo_side: RepoSide,
    /// Family this artifact belongs to (paper §3.1, 9 closed members).
    pub kind: ArtifactKind,
    /// Language or format tag.
    pub language: Language,
    /// POSIX path inside the repo, relative to repo root.
    pub local_path: String,
    /// Hash of the artifact bytes (12 lowercase hex chars per ADR-003).
    pub content_hash: String,
}

/// `old` vs `new` repository side. Stored alongside ids for indexed
/// lookups; matches the first segment of the entity-id grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum RepoSide {
    /// Pre-refactoring repository.
    Old,
    /// Post-refactoring repository.
    New,
}

impl RepoSide {
    /// SQL representation: `"old"` or `"new"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RepoSide::Old => "old",
            RepoSide::New => "new",
        }
    }
}

/// One row of `N` in the repository tuple.
///
/// The `EntityId` already encodes `(repo_side, language, kind,
/// local_path, stable_hash)` per ADR-003. The denormalized columns
/// stored alongside are derived from the parsed id; they let the
/// store query without re-parsing every row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// Stable id for the entity.
    pub entity_id: EntityId,
    /// Owner artifact (paper Def. 1 `owner`).
    pub artifact_id: ArtifactId,
    /// Which side of the refactoring.
    pub repo_side: RepoSide,
    /// Language tag of the entity.
    pub language: Language,
    /// Local kind (handler, route, schema field, ...).
    pub kind: String,
    /// POSIX path with optional `#`-anchor inside the artifact.
    pub local_path: String,
    /// 12-char stable hash from the entity-id grammar.
    pub stable_hash: String,
}

/// One row of `C` in the repository tuple.
///
/// Endpoints live in a dedicated table to support ambiguous hyperedges
/// (ADR-005 §2). The `endpoints` field on this struct is the
/// **resolved** view, ordered by `position` ascending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correspondence {
    /// Stable id.
    pub corr_id: CorrId,
    /// Correspondence kind tag.
    pub kind: CorrespondenceKind,
    /// Optional rule version recorded by the extractor.
    pub rule_version: Option<String>,
    /// Endpoints in declaration order.
    pub endpoints: Vec<EntityId>,
}

/// One row of `Build ⊆ A × A`.
///
/// A build edge means `dst_artifact` is generated from or depends on
/// `src_artifact`. Build closure (paper Lemma 2) is grounded here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildEdge {
    /// Stable id.
    pub edge_id: EdgeId,
    /// Source artifact.
    pub src_artifact: ArtifactId,
    /// Destination artifact (generated, packaged, etc.).
    pub dst_artifact: ArtifactId,
}
