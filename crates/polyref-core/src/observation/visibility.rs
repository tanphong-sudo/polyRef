//! Visibility tag.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Visibility class for an observation, per ADR-010.
///
/// `Visible` observations are consumed by proposal methods, build/test
/// baselines, and PolyRef. `HeldOut` observations are consulted only
/// after the candidate decision is computed, by the evaluator.
/// `EvaluationOnly` observations are never consulted by any method;
/// they are oracle inputs to the empirical harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Visibility {
    /// Used by proposal + validation paths.
    Visible,
    /// Reserved for the post-acceptance evaluator.
    HeldOut,
    /// Reserved for the empirical harness oracle.
    EvaluationOnly,
}

impl Default for Visibility {
    /// Default visibility per ADR-010 is `Visible`.
    fn default() -> Self {
        Visibility::Visible
    }
}

/// Failure to parse the snake-case tag string of a [`Visibility`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown Visibility tag: {0}")]
pub struct VisibilityParseError(pub String);

impl Visibility {
    /// The canonical snake-case tag identical to the serde
    /// representation and `schemas/observation/visibility.json`.
    ///
    /// Defined here so consumer crates do not need a wildcard `_` arm
    /// on this `#[non_exhaustive]` enum that drives ADR-010 leakage
    /// prevention.
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            Visibility::Visible => "visible",
            Visibility::HeldOut => "held_out",
            Visibility::EvaluationOnly => "evaluation_only",
        }
    }

    /// Parse the canonical snake-case tag string.
    ///
    /// # Errors
    ///
    /// Returns [`VisibilityParseError`] when `s` is not one of the
    /// three closed members.
    pub fn parse(s: &str) -> Result<Self, VisibilityParseError> {
        match s {
            "visible" => Ok(Visibility::Visible),
            "held_out" => Ok(Visibility::HeldOut),
            "evaluation_only" => Ok(Visibility::EvaluationOnly),
            other => Err(VisibilityParseError(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn visibility_round_trip_covers_all_three_variants() {
        for v in [
            Visibility::Visible,
            Visibility::HeldOut,
            Visibility::EvaluationOnly,
        ] {
            assert_eq!(Visibility::parse(v.as_tag()).unwrap(), v);
        }
    }

    #[test]
    fn visibility_parse_rejects_unknown() {
        assert!(Visibility::parse("public").is_err());
    }

    #[test]
    fn visibility_default_is_visible() {
        assert_eq!(Visibility::default(), Visibility::Visible);
    }
}
