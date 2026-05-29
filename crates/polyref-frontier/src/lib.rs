//! Affected-frontier closure for PolyRef Layer 5.
//!
//! This crate implements the paper Definition 7 least affected frontier over
//! typed graph rows exposed through `polyref-graph`'s read model. The
//! implementation is deterministic: all internal indexes use ordered maps/sets
//! and the public result is sorted by [`FrontierItem`].
//!
//! # Layout
//!
//! - [`closure`] — paper Definition 7 affected-frontier closure
//!   ([`compute_frontier`]) with its input/output DTOs.
//! - [`coverage_risk`] — Layer 5 coverage / fail-closed risk
//!   classification on top of the frontier output.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod closure;
pub mod coverage_risk;

pub use closure::{
    compute_frontier, FrontierDiagnostic, FrontierDiagnosticKind, FrontierEntry, FrontierInput,
    FrontierItem, FrontierReason, FrontierResult, Result,
};
pub use coverage_risk::{
    classify_coverage_risk, CoverageRisk, CoverageRiskInput, CoverageRiskReport,
    CoverageRiskSource, UnsupportedFeatureRiskNote,
};
