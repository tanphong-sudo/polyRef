//! PolyRef validation engine (Layer 6).
//!
//! This crate turns Layer 5 frontier outputs into validation statuses and report
//! rows following the paper's A1/A2 engine boundary:
//!
//! - obligation generation ([`obligation`]) — paper Table 15 `GenerateObligations`;
//! - A2 status assignment (later L6-02);
//! - checker execution bridge (later L6-03);
//! - A1 orchestration + report assembly (later L6-04).
//!
//! # Invariants
//!
//! - Obligation generation is deterministic: same graph + frontier ⇒ identical
//!   obligation set, regardless of frontier entry order.
//! - Coverage gaps from Layer 5 become pre-check Unknowns, never silently dropped
//!   (fail-closed).
//! - Payloads carry only validated ids and typed kinds — no raw paths, logs,
//!   source text, env values, or secrets.
//! - `#![forbid(unsafe_code)]` (workspace lint).

#![warn(missing_docs)]

pub mod error;
pub mod obligation;

pub use error::{EngineError, Result};
pub use obligation::{
    generate_obligations, FrontierObligationSet, Obligation, ObligationKind, PrecheckUnknown,
};
