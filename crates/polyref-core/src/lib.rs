//! PolyRef core IR.
//!
//! This crate defines the type substrate that every later slice imports
//! unchanged: ids, source spans, status / outcome, evidence, observation
//! kinds, migration map, and the validation report aggregate root.
//!
//! # Invariants enforced at the type level
//!
//! - `Outcome::Pres` and `Outcome::Migrated` cannot carry a reason; the
//!   reason is the payload of `Broken` and `Unknown`. See [`status`].
//! - `ValidationReport::assemble` rejects a report whose
//!   `candidate_decision` is `Accepted` while `missing_endpoint_unknown`
//!   is `true` — the fail-closed invariant from the paper §3.
//! - All ids ([`ids::EntityId`], [`ids::ArtifactId`], [`ids::CorrId`],
//!   [`ids::EdgeId`]) are newtype-wrapped strings whose constructor
//!   parses against the ADR-003 grammar; serde routes through the parser.
//! - [`migration_map::MigrationMap::try_new`] enforces type-respecting
//!   on the *kind* segment only — paper Definition 5. Cross-language
//!   migrations are first-class.
//! - No runtime I/O; `#![forbid(unsafe_code)]`.
//!
//! Most function bodies in this Slice 1 skeleton are `todo!()` stubs.
//! See `claude/05-handoff-1-core-ir.md` §E for the test list that turns
//! the stubs green.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod artifact_kind;
pub mod canonical;
pub mod correspondence_kind;
pub mod error;
pub mod evidence;
pub mod ids;
pub mod language;
pub mod migration_map;
pub mod observation;
pub mod report;
pub mod source_span;
pub mod status;

pub use artifact_kind::ArtifactKind;
pub use correspondence_kind::CorrespondenceKind;
pub use error::CoreError;
pub use evidence::{Evidence, EvidencePointer};
pub use ids::{ArtifactId, CorrId, EdgeId, EntityId};
pub use language::Language;
pub use migration_map::MigrationMap;
pub use observation::{Observation, Visibility};
pub use report::{CandidateDecision, ReportInvariantError, ValidationReport};
pub use source_span::{LineCol, SourceSpan};
pub use status::{BrokenReason, Outcome, UnknownReason};
