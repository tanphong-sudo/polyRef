//! PolyRef GraphStore — persistent typed correspondence graph.
//!
//! This crate provides the SQLite-backed `GraphStore` that holds a
//! repository graph as defined by paper Definition 1:
//!
//! ```text
//! R = (A, N, L, C, Build, O, owner, type)
//! ```
//!
//! - `A` = [`Artifact`]s (one row per file family)
//! - `N` = [`Entity`]s (typed handles extracted from artifacts)
//! - `C` = [`Correspondence`]s (typed edges/hyperedges over entities)
//! - `Build` = [`BuildEdge`]s (artifact → artifact build deps)
//! - `O` = [`polyref_core::Observation`]s
//!
//! # Invariants
//!
//! - All ids ([`polyref_core::EntityId`] etc.) are validated at parse
//!   time. SQL only stores their `as_str()` form; no constructor
//!   bypass possible.
//! - `Correspondence` endpoints are stored as a one-to-many table
//!   (`correspondence_endpoint`) per ADR-005 §2 to avoid combinatorial
//!   blowup on ambiguous hyperedges.
//! - The store is `Send + Sync` so it can back the bounded plugin pool
//!   in Layer 3.
//! - `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod audit;
pub mod blobstore;
pub mod error;
pub mod model;
pub mod report_store;
pub mod store;
mod tags;

pub use audit::{
    AuditEvent, AuditEventError, AuditEventTag, AuditEventTagParseError, AuditReadError,
    AuditReader, AuditWriteError, AuditWriter, AUDIT_LINE_MAX_BYTES,
};
pub use blobstore::{
    checker_memo_key, extractor_memo_key, BlobKey, BlobKeyError, BlobStore, BlobStoreError,
    CacheStats, FsBlobStore,
};
pub use error::GraphStoreError;
pub use model::{Artifact, BuildEdge, Correspondence, Entity};
pub use report_store::{ReportStore, ReportStoreError, RunManifest, RunReportStore};
pub use store::{GraphStore, SqliteGraphStore};
