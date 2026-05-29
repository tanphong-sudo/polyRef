//! `AuditEvent` and `AuditEventTag` — the wire DTOs for the NDJSON
//! audit log defined by ADR-006.
//!
//! See [`crate::audit`] for the module-level overview.

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
    /// Candidate patch replay completed successfully.
    ReplayCompleted,
    /// Repository checkout (R or R') is on disk and ready.
    RepoLoaded,
    /// Report assembled and written to disk; run is over.
    ReportFinalized,
    /// Sandbox denied or failed a replay/plugin operation.
    SandboxDenied,
    /// Sandbox replay/plugin operation started.
    SandboxStarted,
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
            AuditEventTag::ReplayCompleted => "replay_completed",
            AuditEventTag::RepoLoaded => "repo_loaded",
            AuditEventTag::ReportFinalized => "report_finalized",
            AuditEventTag::SandboxDenied => "sandbox_denied",
            AuditEventTag::SandboxStarted => "sandbox_started",
        }
    }

    /// Parse the canonical snake-case tag string. Inverse of
    /// [`Self::as_tag`].
    ///
    /// # Errors
    ///
    /// Returns [`AuditEventTagParseError`] when `s` is not one of the
    /// closed members.
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
            "replay_completed" => Ok(AuditEventTag::ReplayCompleted),
            "repo_loaded" => Ok(AuditEventTag::RepoLoaded),
            "report_finalized" => Ok(AuditEventTag::ReportFinalized),
            "sandbox_denied" => Ok(AuditEventTag::SandboxDenied),
            "sandbox_started" => Ok(AuditEventTag::SandboxStarted),
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
pub(super) const REPORT_ID_MAX_LEN: usize = 256;
pub(super) const STAGE_MAX_LEN: usize = 64;
pub(super) const ACTOR_MAX_LEN: usize = 256;
pub(super) const PAYLOAD_HASH_LEN: usize = 64;

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
    fn audit_event_tag_round_trip_covers_all_17_variants() {
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
            AuditEventTag::ReplayCompleted,
            AuditEventTag::RepoLoaded,
            AuditEventTag::ReportFinalized,
            AuditEventTag::SandboxDenied,
            AuditEventTag::SandboxStarted,
        ];
        assert_eq!(all.len(), 17);
        let tags = all.map(AuditEventTag::as_tag);
        let mut sorted = tags;
        sorted.sort_unstable();
        assert_eq!(tags, sorted);
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
