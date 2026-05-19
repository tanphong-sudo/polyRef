//! Visibility tag.

use serde::{Deserialize, Serialize};

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
