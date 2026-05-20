//! SQL tag <-> typed enum bridges.
//!
//! `polyref-core` owns the canonical snake-case tags (`as_tag` /
//! `parse` on each enum). This module is just the thin shim that
//! converts those parse errors into [`crate::GraphStoreError`] so the
//! GraphStore can return its own typed error.
//!
//! Keeping every exhaustive enum match inside `polyref-core` lets this
//! crate stay free of wildcard `_ =>` arms on the business enums
//! (paper Def. 1 + Def. 6, ADR-010), per `rust-coding-style.md`.

use crate::error::GraphStoreError;
use crate::model::RepoSide;
use polyref_core::{
    artifact_kind::ArtifactKind, correspondence_kind::CorrespondenceKind, language::Language,
    observation::Visibility,
};

/// Decode an `ArtifactKind` from its SQL tag.
pub(crate) fn parse_artifact_kind(s: &str) -> Result<ArtifactKind, GraphStoreError> {
    ArtifactKind::parse(s).map_err(|e| GraphStoreError::UnsupportedEnum {
        enum_name: "ArtifactKind",
        value: e.0,
    })
}

/// Decode a `Language` from its SQL tag.
pub(crate) fn parse_language(s: &str) -> Result<Language, GraphStoreError> {
    Language::parse(s).map_err(|e| GraphStoreError::UnsupportedEnum {
        enum_name: "Language",
        value: e.0,
    })
}

/// Decode a `CorrespondenceKind` from its SQL tag.
pub(crate) fn parse_correspondence_kind(s: &str) -> Result<CorrespondenceKind, GraphStoreError> {
    CorrespondenceKind::parse(s).map_err(|e| GraphStoreError::UnsupportedEnum {
        enum_name: "CorrespondenceKind",
        value: e.0,
    })
}

/// Decode a `Visibility` from its SQL tag.
///
/// Currently unused at the store layer (round-trip happens through
/// canonical-JSON in the `observation.payload` column). Kept ready for
/// the ADR-010 leakage-prevention filter that lands in Layer 5
/// (`observation_registry`), which queries the `visibility` column
/// directly to avoid loading held-out payloads.
#[allow(dead_code)]
pub(crate) fn parse_visibility(s: &str) -> Result<Visibility, GraphStoreError> {
    Visibility::parse(s).map_err(|e| GraphStoreError::UnsupportedEnum {
        enum_name: "Visibility",
        value: e.0,
    })
}

/// Decode a `RepoSide` from its SQL tag. Unlike the `polyref-core`
/// enums, `RepoSide` is owned by this crate, so the parse logic lives
/// here.
pub(crate) fn parse_repo_side(s: &str) -> Result<RepoSide, GraphStoreError> {
    match s {
        "old" => Ok(RepoSide::Old),
        "new" => Ok(RepoSide::New),
        other => Err(GraphStoreError::UnsupportedEnum {
            enum_name: "RepoSide",
            value: other.to_owned(),
        }),
    }
}
