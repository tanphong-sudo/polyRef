//! NDJSON audit log per ADR-006.
//!
//! The audit log is the replay anchor for a validation run: every
//! stage transition emits one [`AuditEvent`] line, and the
//! `payload_hash` chain lets the replay verifier reconstruct decisions
//! from cache without re-running plugins.
//!
//! # Layout
//!
//! ```text
//! .polyref/runs/<report_id>/audit.ndjson
//! ```
//!
//! One JSON object per line; lines are separated by `\n` (LF only,
//! never CRLF) so the file is `tail -f`-friendly and round-trips
//! through `serde_json::Deserializer::from_reader`.
//!
//! # Closed tag set
//!
//! The members of [`AuditEventTag`] mirror
//! `schemas/audit-event.json` (schema package version 0.2.0). The
//! `as_tag()` / `parse()` helpers live on the enum itself so consumer
//! crates never need a wildcard `_ =>` arm on the `#[non_exhaustive]`
//! type (per `rust-coding-style.md`).
//!
//! # Security
//!
//! - Free-form `payload` fields are **not** stored in the audit log;
//!   only the SHA-256 hash. Held-out observation typed fields are
//!   therefore never leaked through this channel (ADR-010).
//! - `payload_hash` is constrained to 64 lowercase hex characters at
//!   parse time so callers cannot smuggle non-hash strings.
//! - The writer is append-only and `flush()`es after every event; a
//!   crash mid-write loses at most one event but never corrupts the
//!   prefix.
//!
//! # Layout (this module)
//!
//! - [`event`] — wire DTOs ([`AuditEvent`], [`AuditEventTag`]) and
//!   schema-mirrored validation.
//! - [`writer`] — append-only [`AuditWriter`] with flush-per-event
//!   durability.
//! - [`reader`] — streaming [`AuditReader`] with line-length cap and
//!   typed errors.

pub mod event;
pub mod reader;
pub mod writer;

pub use event::{AuditEvent, AuditEventError, AuditEventTag, AuditEventTagParseError};
pub use reader::{AuditReadError, AuditReader, AUDIT_LINE_MAX_BYTES};
pub use writer::{AuditWriteError, AuditWriter};
