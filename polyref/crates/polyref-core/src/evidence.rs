//! Evidence + EvidencePointer.
//!
//! `Evidence` is a value object. The four constructors (`ok_pres`,
//! `ok_migrated`, `broken`, `unknown`) are the only ingress; the inner
//! fields are private. This makes "Evidence with outcome=Pres but
//! reason=…" unrepresentable.
//!
//! `EvidencePointer` accepts only relative paths under `evidence/`,
//! per hard blocker F-7.

use crate::source_span::SourceSpan;
use crate::status::{BrokenReason, Outcome, UnknownReason};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Validated relative path under `evidence/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EvidencePointer(String);

/// Errors `EvidencePointer::parse` may emit.
#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum EvidencePointerError {
    /// Path is empty, too long, contains traversal, has bad chars, or
    /// is not under `evidence/`.
    #[error("invalid evidence pointer: {0}")]
    Invalid(&'static str),
}

impl EvidencePointer {
    /// Parse a pointer string. Slice 1 stub.
    pub fn parse(_input: &str) -> Result<Self, EvidencePointerError> {
        todo!(
            "§E-1 evidence_pointer_rejects_*; \
             enforce ^evidence/[A-Za-z0-9_./-]{{1,512}}$ per F-7"
        )
    }

    /// Read-only view.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for EvidencePointer {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EvidencePointer {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        EvidencePointer::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Versioned identifier of a checker rule (e.g. `route.migrate-v1`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PredicateId(String);

impl PredicateId {
    /// Build a new `PredicateId`. Slice 1 accepts any non-empty string;
    /// the parser is tightened in Slice 2.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Read-only view.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Versioned identifier of a checker plugin or rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Version(String);

impl Version {
    /// Build a new `Version`.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Read-only view.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Evidence record produced by a checker call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    outcome: Outcome,
    predicate: PredicateId,
    spans: Vec<SourceSpan>,
    pointers: Vec<EvidencePointer>,
    checker_version: Version,
    rule_version: Version,
}

impl Evidence {
    /// Build a `Pres` evidence record.
    #[must_use]
    pub fn ok_pres(
        predicate: PredicateId,
        spans: Vec<SourceSpan>,
        pointers: Vec<EvidencePointer>,
        checker_version: Version,
        rule_version: Version,
    ) -> Self {
        Self {
            outcome: Outcome::Pres,
            predicate,
            spans,
            pointers,
            checker_version,
            rule_version,
        }
    }

    /// Build a `Migrated` evidence record.
    #[must_use]
    pub fn ok_migrated(
        predicate: PredicateId,
        spans: Vec<SourceSpan>,
        pointers: Vec<EvidencePointer>,
        checker_version: Version,
        rule_version: Version,
    ) -> Self {
        Self {
            outcome: Outcome::Migrated,
            predicate,
            spans,
            pointers,
            checker_version,
            rule_version,
        }
    }

    /// Build a `Broken` evidence record.
    #[must_use]
    pub fn broken(
        reason: BrokenReason,
        predicate: PredicateId,
        spans: Vec<SourceSpan>,
        pointers: Vec<EvidencePointer>,
        checker_version: Version,
        rule_version: Version,
    ) -> Self {
        Self {
            outcome: Outcome::Broken { reason },
            predicate,
            spans,
            pointers,
            checker_version,
            rule_version,
        }
    }

    /// Build an `Unknown` evidence record.
    #[must_use]
    pub fn unknown(
        reason: UnknownReason,
        predicate: PredicateId,
        spans: Vec<SourceSpan>,
        pointers: Vec<EvidencePointer>,
        checker_version: Version,
        rule_version: Version,
    ) -> Self {
        Self {
            outcome: Outcome::Unknown { reason },
            predicate,
            spans,
            pointers,
            checker_version,
            rule_version,
        }
    }

    /// View the outcome.
    #[must_use]
    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    /// View the predicate id.
    #[must_use]
    pub fn predicate(&self) -> &PredicateId {
        &self.predicate
    }

    /// View the spans.
    #[must_use]
    pub fn spans(&self) -> &[SourceSpan] {
        &self.spans
    }

    /// View the evidence pointers.
    #[must_use]
    pub fn pointers(&self) -> &[EvidencePointer] {
        &self.pointers
    }

    /// View the checker version.
    #[must_use]
    pub fn checker_version(&self) -> &Version {
        &self.checker_version
    }

    /// View the rule version.
    #[must_use]
    pub fn rule_version(&self) -> &Version {
        &self.rule_version
    }
}
