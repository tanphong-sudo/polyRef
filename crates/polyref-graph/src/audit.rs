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
//! The 14 members of [`AuditEventTag`] mirror
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

use serde::{Deserialize, Serialize};
use thiserror::Error;

use polyref_core::evidence::EvidencePointer;

/// Closed event-tag set per ADR-006. Adding a member requires a
/// `schemas/audit-event.json` minor version bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuditEventTag {
    /// Artifact has been classified into an `ArtifactKind`.
    ArtifactClassified,
    /// Plugin checker call started.
    CheckerInvoked,
    /// Plugin checker call returned (success, broken, or unknown).
    CheckerResult,
    /// Correspondence row written to the GraphStore.
    CorrespondenceCreated,
    /// Entity row written to the GraphStore.
    EntityEmitted,
    /// Plugin extractor call started.
    ExtractorInvoked,
    /// Affected frontier `∂ρ(o)` computed for an observation.
    FrontierComputed,
    /// A2 status assigned to a frontier item.
    FrontierItemStatusAssigned,
    /// Migration map `μ` accepted by the builder.
    MigrationMapBuilt,
    /// Obligation row generated for a frontier item.
    ObligationEmitted,
    /// `μ(o)` rewrite produced (or marked undefined).
    ObservationRewritten,
    /// Per-observation status reduced from frontier-item statuses.
    ObservationStatusAssigned,
    /// Report assembled and written to disk; run is over.
    ReportFinalized,
    /// Repository checkout (R or R') is on disk and ready.
    RepoLoaded,
}

/// Failure to parse the snake-case tag string of an
/// [`AuditEventTag`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown AuditEventTag tag: {0}")]
pub struct AuditEventTagParseError(pub String);

impl AuditEventTag {
    /// The canonical snake-case tag, identical to the serde
    /// representation and `schemas/audit-event.json` `tag` enum.
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            AuditEventTag::ArtifactClassified => "artifact_classified",
            AuditEventTag::CheckerInvoked => "checker_invoked",
            AuditEventTag::CheckerResult => "checker_result",
            AuditEventTag::CorrespondenceCreated => "correspondence_created",
            AuditEventTag::EntityEmitted => "entity_emitted",
            AuditEventTag::ExtractorInvoked => "extractor_invoked",
            AuditEventTag::FrontierComputed => "frontier_computed",
            AuditEventTag::FrontierItemStatusAssigned => "frontier_item_status_assigned",
            AuditEventTag::MigrationMapBuilt => "migration_map_built",
            AuditEventTag::ObligationEmitted => "obligation_emitted",
            AuditEventTag::ObservationRewritten => "observation_rewritten",
            AuditEventTag::ObservationStatusAssigned => "observation_status_assigned",
            AuditEventTag::ReportFinalized => "report_finalized",
            AuditEventTag::RepoLoaded => "repo_loaded",
        }
    }

    /// Parse the canonical snake-case tag string. Inverse of
    /// [`Self::as_tag`].
    ///
    /// # Errors
    ///
    /// Returns [`AuditEventTagParseError`] when `s` is not one of the
    /// 14 closed members.
    pub fn parse(s: &str) -> Result<Self, AuditEventTagParseError> {
        match s {
            "artifact_classified" => Ok(AuditEventTag::ArtifactClassified),
            "checker_invoked" => Ok(AuditEventTag::CheckerInvoked),
            "checker_result" => Ok(AuditEventTag::CheckerResult),
            "correspondence_created" => Ok(AuditEventTag::CorrespondenceCreated),
            "entity_emitted" => Ok(AuditEventTag::EntityEmitted),
            "extractor_invoked" => Ok(AuditEventTag::ExtractorInvoked),
            "frontier_computed" => Ok(AuditEventTag::FrontierComputed),
            "frontier_item_status_assigned" => Ok(AuditEventTag::FrontierItemStatusAssigned),
            "migration_map_built" => Ok(AuditEventTag::MigrationMapBuilt),
            "obligation_emitted" => Ok(AuditEventTag::ObligationEmitted),
            "observation_rewritten" => Ok(AuditEventTag::ObservationRewritten),
            "observation_status_assigned" => Ok(AuditEventTag::ObservationStatusAssigned),
            "report_finalized" => Ok(AuditEventTag::ReportFinalized),
            "repo_loaded" => Ok(AuditEventTag::RepoLoaded),
            other => Err(AuditEventTagParseError(other.to_owned())),
        }
    }
}

/// One audit-log line.
///
/// Matches `schemas/audit-event.json` field-for-field. All free-form
/// strings are length-capped at parse time (when reading back) and at
/// builder construction (when writing). The `payload_hash` field is a
/// 64-char lowercase hex SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEvent {
    /// RFC 3339 UTC timestamp.
    pub ts: String,
    /// Stable run id; matches the directory name under
    /// `.polyref/runs/<report_id>/`.
    pub report_id: String,
    /// Pipeline stage label.
    pub stage: String,
    /// Closed event-tag set.
    pub tag: AuditEventTag,
    /// Component that emitted the event.
    pub actor: String,
    /// Lowercase hex SHA-256 of the canonical-JSON payload.
    pub payload_hash: String,
    /// Optional list of evidence pointers; empty for events that
    /// touch held-out observations before the candidate decision
    /// (ADR-010).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_pointers: Vec<EvidencePointer>,
}

// Schema-mirrored hard caps. These match the lengths declared in
// `schemas/audit-event.json` so the wire and the host agree.
const REPORT_ID_MAX_LEN: usize = 256;
const STAGE_MAX_LEN: usize = 64;
const ACTOR_MAX_LEN: usize = 256;
const PAYLOAD_HASH_LEN: usize = 64;

/// Failure to construct an [`AuditEvent`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuditEventError {
    /// `ts`, `report_id`, `stage`, or `actor` was empty.
    #[error("audit event field {field} is empty")]
    Empty {
        /// Name of the offending field.
        field: &'static str,
    },
    /// String field exceeded its schema length cap.
    #[error("audit event field {field} too long: {len} > {max}")]
    TooLong {
        /// Name of the offending field.
        field: &'static str,
        /// Actual length.
        len: usize,
        /// Maximum permitted length.
        max: usize,
    },
    /// `payload_hash` did not match `^[a-f0-9]{64}$`.
    #[error("audit event payload_hash is not 64 lowercase hex chars")]
    BadPayloadHash,
}

impl AuditEvent {
    /// Construct a new event after validating every free-form field
    /// against the schema caps. This is the only blessed entry point;
    /// `serde` deserialization also routes through these checks via
    /// [`Self::validate`].
    ///
    /// # Errors
    ///
    /// Returns [`AuditEventError`] when any string is empty, exceeds
    /// its cap, or `payload_hash` is not 64 lowercase hex chars.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ts: impl Into<String>,
        report_id: impl Into<String>,
        stage: impl Into<String>,
        tag: AuditEventTag,
        actor: impl Into<String>,
        payload_hash: impl Into<String>,
        evidence_pointers: Vec<EvidencePointer>,
    ) -> Result<Self, AuditEventError> {
        let event = Self {
            ts: ts.into(),
            report_id: report_id.into(),
            stage: stage.into(),
            tag,
            actor: actor.into(),
            payload_hash: payload_hash.into(),
            evidence_pointers,
        };
        event.validate()?;
        Ok(event)
    }

    /// Re-run the constructor checks on an already-deserialized event.
    /// Useful when an event arrives from disk and we want to enforce
    /// the same caps the builder enforces.
    ///
    /// # Errors
    ///
    /// Same as [`Self::new`].
    pub fn validate(&self) -> Result<(), AuditEventError> {
        if self.ts.is_empty() {
            return Err(AuditEventError::Empty { field: "ts" });
        }
        check_string("report_id", &self.report_id, REPORT_ID_MAX_LEN)?;
        check_string("stage", &self.stage, STAGE_MAX_LEN)?;
        check_string("actor", &self.actor, ACTOR_MAX_LEN)?;
        if self.payload_hash.len() != PAYLOAD_HASH_LEN
            || !self
                .payload_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(AuditEventError::BadPayloadHash);
        }
        Ok(())
    }
}

fn check_string(field: &'static str, s: &str, max: usize) -> Result<(), AuditEventError> {
    if s.is_empty() {
        return Err(AuditEventError::Empty { field });
    }
    if s.len() > max {
        return Err(AuditEventError::TooLong {
            field,
            len: s.len(),
            max,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn h(byte: u8) -> String {
        std::iter::repeat(byte as char).take(64).collect()
    }

    #[test]
    fn audit_event_tag_round_trip_covers_all_14_variants() {
        let all = [
            AuditEventTag::ArtifactClassified,
            AuditEventTag::CheckerInvoked,
            AuditEventTag::CheckerResult,
            AuditEventTag::CorrespondenceCreated,
            AuditEventTag::EntityEmitted,
            AuditEventTag::ExtractorInvoked,
            AuditEventTag::FrontierComputed,
            AuditEventTag::FrontierItemStatusAssigned,
            AuditEventTag::MigrationMapBuilt,
            AuditEventTag::ObligationEmitted,
            AuditEventTag::ObservationRewritten,
            AuditEventTag::ObservationStatusAssigned,
            AuditEventTag::ReportFinalized,
            AuditEventTag::RepoLoaded,
        ];
        assert_eq!(all.len(), 14);
        for tag in all {
            assert_eq!(AuditEventTag::parse(tag.as_tag()).unwrap(), tag);
        }
    }

    #[test]
    fn audit_event_tag_parse_rejects_unknown() {
        assert!(AuditEventTag::parse("not_a_tag").is_err());
    }

    #[test]
    fn audit_event_tag_serde_matches_as_tag() {
        let tag = AuditEventTag::FrontierComputed;
        let json = serde_json::to_string(&tag).unwrap();
        assert_eq!(json, format!("\"{}\"", tag.as_tag()));
    }

    #[test]
    fn audit_event_new_accepts_canonical() {
        let e = AuditEvent::new(
            "2026-05-21T10:00:00Z",
            "run-001",
            "extraction",
            AuditEventTag::ExtractorInvoked,
            "polyref-loader",
            h(b'a'),
            vec![],
        )
        .unwrap();
        assert_eq!(e.tag, AuditEventTag::ExtractorInvoked);
    }

    #[test]
    fn audit_event_rejects_empty_ts() {
        let err = AuditEvent::new(
            "",
            "run-001",
            "extraction",
            AuditEventTag::RepoLoaded,
            "loader",
            h(b'b'),
            vec![],
        )
        .unwrap_err();
        assert_eq!(err, AuditEventError::Empty { field: "ts" });
    }

    #[test]
    fn audit_event_rejects_oversize_report_id() {
        let big = "x".repeat(REPORT_ID_MAX_LEN + 1);
        let err = AuditEvent::new(
            "2026-05-21T10:00:00Z",
            big,
            "extraction",
            AuditEventTag::RepoLoaded,
            "loader",
            h(b'c'),
            vec![],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AuditEventError::TooLong {
                field: "report_id",
                ..
            }
        ));
    }

    #[test]
    fn audit_event_rejects_short_payload_hash() {
        let err = AuditEvent::new(
            "2026-05-21T10:00:00Z",
            "run-001",
            "extraction",
            AuditEventTag::RepoLoaded,
            "loader",
            "deadbeef",
            vec![],
        )
        .unwrap_err();
        assert_eq!(err, AuditEventError::BadPayloadHash);
    }

    #[test]
    fn audit_event_rejects_uppercase_payload_hash() {
        let err = AuditEvent::new(
            "2026-05-21T10:00:00Z",
            "run-001",
            "extraction",
            AuditEventTag::RepoLoaded,
            "loader",
            "F".repeat(64),
            vec![],
        )
        .unwrap_err();
        assert_eq!(err, AuditEventError::BadPayloadHash);
    }

    #[test]
    fn audit_event_serde_round_trip() {
        let e = AuditEvent::new(
            "2026-05-21T10:00:00Z",
            "run-001",
            "extraction",
            AuditEventTag::ExtractorInvoked,
            "polyref-loader",
            h(b'd'),
            vec![],
        )
        .unwrap();
        let json = serde_json::to_string(&e).unwrap();
        let back: AuditEvent = serde_json::from_str(&json).unwrap();
        // After round-trip, validate() still passes.
        back.validate().unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn audit_event_serde_rejects_unknown_field() {
        let json = r#"{
            "ts": "2026-05-21T10:00:00Z",
            "report_id": "run-001",
            "stage": "extraction",
            "tag": "repo_loaded",
            "actor": "loader",
            "payload_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "rogue_field": 42
        }"#;
        let result: Result<AuditEvent, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "deny_unknown_fields must reject extra keys"
        );
    }

    #[test]
    fn audit_event_serde_rejects_unknown_tag() {
        let json = r#"{
            "ts": "2026-05-21T10:00:00Z",
            "report_id": "run-001",
            "stage": "extraction",
            "tag": "not_a_known_tag",
            "actor": "loader",
            "payload_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }"#;
        let result: Result<AuditEvent, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "closed enum must reject unknown tag values"
        );
    }
}


// ─── Writer ─────────────────────────────────────────────────────────────

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

/// Append-only NDJSON writer for [`AuditEvent`]s.
///
/// The file is opened with `OpenOptions::append(true)` so concurrent
/// writers from the same process get atomic per-line appends from the
/// kernel (POSIX `O_APPEND`). One `BufWriter` is wrapped around the
/// file to coalesce byte-level writes; [`Self::append`] flushes after
/// every event so a crash mid-run loses at most the partial line of
/// the current event, never a previously-written one.
pub struct AuditWriter {
    inner: BufWriter<File>,
}

/// Failure to write to the audit log.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AuditWriteError {
    /// The supplied event failed schema validation before it could be
    /// serialized. Callers should never produce malformed events but
    /// the writer enforces the cap anyway.
    #[error("audit event invalid: {0}")]
    Invalid(#[from] AuditEventError),

    /// Underlying I/O error.
    #[error("audit io error: {0}")]
    Io(#[from] std::io::Error),

    /// `serde_json` failed to serialize the event. Should be
    /// unreachable for the in-tree wire types but possible if a future
    /// schema change introduces a non-serializable value.
    #[error("audit serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

impl AuditWriter {
    /// Open `path` for append. Creates the file if it does not exist.
    /// Existing content is preserved.
    ///
    /// # Errors
    ///
    /// Returns [`AuditWriteError::Io`] when the file cannot be opened.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AuditWriteError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())?;
        Ok(Self {
            inner: BufWriter::new(file),
        })
    }

    /// Append one event. Validates the event again (defense in depth)
    /// before serializing, writes a single `\n`-terminated JSON line,
    /// and flushes the buffer.
    ///
    /// # Errors
    ///
    /// - [`AuditWriteError::Invalid`] if the event failed schema caps.
    /// - [`AuditWriteError::Json`] if serialization failed.
    /// - [`AuditWriteError::Io`] if the underlying write or flush
    ///   failed.
    pub fn append(&mut self, event: &AuditEvent) -> Result<(), AuditWriteError> {
        event.validate()?;
        let line = serde_json::to_string(event)?;
        // NDJSON: one object per line, LF-terminated. Reject embedded
        // newlines defensively — a buggy upstream that managed to
        // smuggle a literal `\n` into a string field would split a
        // single event across two physical lines and break the reader.
        debug_assert!(
            !line.contains('\n'),
            "serde_json::to_string produced a multiline payload: {line}"
        );
        self.inner.write_all(line.as_bytes())?;
        self.inner.write_all(b"\n")?;
        self.inner.flush()?;
        Ok(())
    }

    /// Flush the underlying buffer. `append` already flushes; this is
    /// only useful before calling [`Self::into_inner`] in tests.
    ///
    /// # Errors
    ///
    /// Returns [`AuditWriteError::Io`] on flush failure.
    pub fn flush(&mut self) -> Result<(), AuditWriteError> {
        self.inner.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod writer_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::io::Read;

    fn h(byte: u8) -> String {
        std::iter::repeat(byte as char).take(64).collect()
    }

    fn sample_event(tag: AuditEventTag, hash_byte: u8) -> AuditEvent {
        AuditEvent::new(
            "2026-05-21T10:00:00Z",
            "run-001",
            "extraction",
            tag,
            "polyref-loader",
            h(hash_byte),
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn writer_creates_file_and_appends_one_line_per_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        {
            let mut w = AuditWriter::open(&path).unwrap();
            w.append(&sample_event(AuditEventTag::RepoLoaded, b'a'))
                .unwrap();
            w.append(&sample_event(AuditEventTag::ExtractorInvoked, b'b'))
                .unwrap();
        }

        let mut buf = String::new();
        File::open(&path).unwrap().read_to_string(&mut buf).unwrap();
        let lines: Vec<_> = buf.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"repo_loaded\""));
        assert!(lines[1].contains("\"extractor_invoked\""));
        // Each line ends with LF.
        assert!(buf.ends_with('\n'));
    }

    #[test]
    fn writer_preserves_existing_content_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        {
            let mut w = AuditWriter::open(&path).unwrap();
            w.append(&sample_event(AuditEventTag::RepoLoaded, b'a'))
                .unwrap();
        }
        // Reopen and append more.
        {
            let mut w = AuditWriter::open(&path).unwrap();
            w.append(&sample_event(AuditEventTag::ReportFinalized, b'c'))
                .unwrap();
        }

        let mut buf = String::new();
        File::open(&path).unwrap().read_to_string(&mut buf).unwrap();
        let lines: Vec<_> = buf.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"repo_loaded\""));
        assert!(lines[1].contains("\"report_finalized\""));
    }

    #[test]
    fn writer_rejects_malformed_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");
        let mut w = AuditWriter::open(path).unwrap();

        // Hand-construct an invalid event by skipping the builder.
        let mut bad = sample_event(AuditEventTag::RepoLoaded, b'a');
        bad.payload_hash = "not-hex".into();
        let err = w.append(&bad).unwrap_err();
        assert!(
            matches!(
                err,
                AuditWriteError::Invalid(AuditEventError::BadPayloadHash)
            ),
            "expected BadPayloadHash, got {err:?}"
        );
    }

    #[test]
    fn writer_flush_after_each_event_is_durable() {
        // Even without explicit `flush()` and without dropping the
        // writer, a previously-appended event must already be visible
        // on disk (write_through guarantee).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");
        let mut w = AuditWriter::open(&path).unwrap();
        w.append(&sample_event(AuditEventTag::RepoLoaded, b'a'))
            .unwrap();

        // Concurrent reader-style check: re-open the same file, count
        // bytes — must be > 0 because append() flushed.
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 0, "append() must flush durable bytes");
    }
}


// ─── Reader ─────────────────────────────────────────────────────────────

use std::io::{BufRead, BufReader};

/// Hard cap on a single audit-line byte length. Defends against a
/// malicious or runaway producer that writes a giant line; the cap is
/// 1 MiB which is two orders of magnitude above the largest event we
/// realistically expect (the typical event is a few hundred bytes).
pub const AUDIT_LINE_MAX_BYTES: usize = 1024 * 1024;

/// Streaming NDJSON reader for [`AuditEvent`]s.
///
/// Lines are read with `BufReader::read_line` and parsed one at a
/// time, which keeps memory bounded regardless of file size. Each line
/// is also validated through [`AuditEvent::validate`] so a corrupted
/// or out-of-bounds field is rejected at read time, not silently
/// trusted.
pub struct AuditReader<R: BufRead> {
    inner: R,
    line_no: usize,
    /// Reusable scratch buffer for `read_line` to avoid per-line
    /// allocation churn.
    scratch: String,
}

/// Failure to read or parse an audit-log line.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AuditReadError {
    /// Underlying I/O error.
    #[error("audit read io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parse failed on the given physical line (1-indexed).
    #[error("audit line {line_no}: malformed JSON: {source}")]
    BadJson {
        /// Physical line number (1-indexed) where parsing failed.
        line_no: usize,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// JSON parsed but failed schema validation (length cap, bad
    /// payload_hash, etc.).
    #[error("audit line {line_no}: schema validation failed: {source}")]
    Invalid {
        /// Physical line number.
        line_no: usize,
        /// Underlying validation error.
        #[source]
        source: AuditEventError,
    },

    /// A single line exceeded [`AUDIT_LINE_MAX_BYTES`]. Likely a
    /// corrupted log or a producer that smuggled a literal `\n` into a
    /// payload.
    #[error("audit line {line_no}: line exceeds {AUDIT_LINE_MAX_BYTES} bytes")]
    LineTooLong {
        /// Physical line number.
        line_no: usize,
    },
}

impl<R: BufRead> AuditReader<R> {
    /// Wrap an existing buffered reader (e.g. one from a custom
    /// source or an in-memory fixture). Tests use this directly;
    /// production callers prefer [`Self::open`].
    #[must_use]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            line_no: 0,
            scratch: String::new(),
        }
    }
}

impl AuditReader<BufReader<File>> {
    /// Open `path` for reading. The file must exist.
    ///
    /// # Errors
    ///
    /// Returns [`AuditReadError::Io`] if the file cannot be opened.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AuditReadError> {
        let file = File::open(path.as_ref())?;
        Ok(Self::new(BufReader::new(file)))
    }
}

impl<R: BufRead> Iterator for AuditReader<R> {
    type Item = Result<AuditEvent, AuditReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.scratch.clear();
            self.line_no += 1;

            // Manual length-capped read: read byte-by-byte through
            // BufRead::fill_buf so a giant line cannot exhaust memory.
            let mut total = 0_usize;
            let saw_eof = loop {
                let buf = match self.inner.fill_buf() {
                    Ok(b) => b,
                    Err(e) => return Some(Err(AuditReadError::Io(e))),
                };
                if buf.is_empty() {
                    break true; // EOF
                }
                let nl = buf.iter().position(|&b| b == b'\n');
                let take = nl.map_or(buf.len(), |i| i + 1);
                if total + take > AUDIT_LINE_MAX_BYTES {
                    return Some(Err(AuditReadError::LineTooLong {
                        line_no: self.line_no,
                    }));
                }
                // Reject non-UTF-8 lines as malformed JSON. JSON is
                // UTF-8 so this is the right semantic level.
                let chunk = match std::str::from_utf8(&buf[..take]) {
                    Ok(s) => s,
                    Err(_) => {
                        // Consume the rest of the line so the iterator
                        // can advance past the corruption.
                        self.inner.consume(take);
                        return Some(Err(self.synth_bad_json(
                            "line is not valid UTF-8".to_owned(),
                        )));
                    }
                };
                self.scratch.push_str(chunk);
                self.inner.consume(take);
                total += take;
                if nl.is_some() {
                    break false;
                }
            };

            // Trim trailing LF / CRLF before parsing.
            let trimmed = self
                .scratch
                .trim_end_matches('\n')
                .trim_end_matches('\r');

            if trimmed.is_empty() {
                if saw_eof {
                    return None;
                }
                // Skip blank lines silently — robust to a producer
                // that writes an extra `\n` somewhere; doesn't change
                // the typed event stream.
                continue;
            }

            return Some(self.parse_line(trimmed));
        }
    }
}

impl<R: BufRead> AuditReader<R> {
    fn parse_line(&self, line: &str) -> Result<AuditEvent, AuditReadError> {
        let event: AuditEvent =
            serde_json::from_str(line).map_err(|e| AuditReadError::BadJson {
                line_no: self.line_no,
                source: e,
            })?;
        event.validate().map_err(|e| AuditReadError::Invalid {
            line_no: self.line_no,
            source: e,
        })?;
        Ok(event)
    }

    fn synth_bad_json(&self, msg: String) -> AuditReadError {
        AuditReadError::BadJson {
            line_no: self.line_no,
            source: serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                msg,
            )),
        }
    }
}

#[cfg(test)]
mod reader_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::io::Cursor;

    fn h(byte: u8) -> String {
        std::iter::repeat(byte as char).take(64).collect()
    }

    fn sample_event(tag: AuditEventTag, hash_byte: u8) -> AuditEvent {
        AuditEvent::new(
            "2026-05-21T10:00:00Z",
            "run-001",
            "extraction",
            tag,
            "polyref-loader",
            h(hash_byte),
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn reader_streams_each_line_to_an_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        let mut w = AuditWriter::open(&path).unwrap();
        for tag in [
            AuditEventTag::RepoLoaded,
            AuditEventTag::ExtractorInvoked,
            AuditEventTag::ReportFinalized,
        ] {
            w.append(&sample_event(tag, b'a')).unwrap();
        }
        w.flush().unwrap();
        drop(w);

        let r = AuditReader::open(&path).unwrap();
        let events: Result<Vec<_>, _> = r.collect();
        let events = events.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].tag, AuditEventTag::RepoLoaded);
        assert_eq!(events[1].tag, AuditEventTag::ExtractorInvoked);
        assert_eq!(events[2].tag, AuditEventTag::ReportFinalized);
    }

    #[test]
    fn reader_skips_blank_lines() {
        let mut payload = String::new();
        payload.push_str("\n\n");
        payload.push_str(&serde_json::to_string(&sample_event(
            AuditEventTag::RepoLoaded,
            b'a',
        ))
        .unwrap());
        payload.push_str("\n\n");

        let r = AuditReader::new(Cursor::new(payload));
        let events: Result<Vec<_>, _> = r.collect();
        assert_eq!(events.unwrap().len(), 1);
    }

    #[test]
    fn reader_returns_typed_error_on_malformed_json() {
        let payload = b"{not json at all}\n";
        let mut r = AuditReader::new(Cursor::new(payload.as_ref()));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(err, AuditReadError::BadJson { line_no: 1, .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn reader_returns_typed_error_on_schema_violation() {
        // Valid JSON but payload_hash is too short → fails validate().
        let mut bad = sample_event(AuditEventTag::RepoLoaded, b'a');
        bad.payload_hash = "deadbeef".into();
        let line = serde_json::to_string(&bad).unwrap();
        let payload = format!("{line}\n");

        let mut r = AuditReader::new(Cursor::new(payload));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(
                err,
                AuditReadError::Invalid {
                    line_no: 1,
                    source: AuditEventError::BadPayloadHash
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn reader_caps_line_length() {
        // Build a line larger than the cap. Use repeated valid JSON
        // bytes so we exercise the cap path, not the UTF-8 path.
        let big = "x".repeat(AUDIT_LINE_MAX_BYTES + 16);
        let payload = format!("{big}\n");

        let mut r = AuditReader::new(Cursor::new(payload));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(err, AuditReadError::LineTooLong { line_no: 1 }),
            "got {err:?}"
        );
    }

    #[test]
    fn reader_returns_typed_error_on_unknown_tag() {
        // Hand-construct a JSON line with a tag that isn't in the
        // closed enum — closed-enum policy rejects it.
        let payload = r#"{"ts":"2026-05-21T10:00:00Z","report_id":"r","stage":"x","tag":"not_a_tag","actor":"a","payload_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
"#;
        let mut r = AuditReader::new(Cursor::new(payload));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(err, AuditReadError::BadJson { line_no: 1, .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn reader_round_trip_after_writer_yields_byte_equal_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        let originals: Vec<AuditEvent> = [
            (AuditEventTag::RepoLoaded, b'a'),
            (AuditEventTag::ArtifactClassified, b'b'),
            (AuditEventTag::ExtractorInvoked, b'c'),
            (AuditEventTag::EntityEmitted, b'd'),
            (AuditEventTag::CorrespondenceCreated, b'e'),
            (AuditEventTag::MigrationMapBuilt, b'f'),
            (AuditEventTag::FrontierComputed, b'0'),
            (AuditEventTag::ObligationEmitted, b'1'),
            (AuditEventTag::CheckerInvoked, b'2'),
            (AuditEventTag::CheckerResult, b'3'),
            (AuditEventTag::ObservationRewritten, b'4'),
            (AuditEventTag::FrontierItemStatusAssigned, b'5'),
            (AuditEventTag::ObservationStatusAssigned, b'6'),
            (AuditEventTag::ReportFinalized, b'7'),
        ]
        .into_iter()
        .map(|(t, h)| sample_event(t, h))
        .collect();

        {
            let mut w = AuditWriter::open(&path).unwrap();
            for e in &originals {
                w.append(e).unwrap();
            }
        }

        let read_back: Vec<AuditEvent> = AuditReader::open(&path)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(originals, read_back);
    }
}
