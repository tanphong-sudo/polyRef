//! Newtype IDs.
//!
//! Each id is a newtype wrapping `String` with private inner; the only
//! way to construct one is via `parse(...)` (also used by `serde`),
//! which validates against the ADR-003 grammar. This is the security
//! boundary: untrusted plugin output and candidate edits cannot
//! fabricate ids that smuggle path traversal or break the type-respecting
//! invariant on `MigrationMap`.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use thiserror::Error;

/// Hard cap on parsed id length (bytes). Per hard blocker F-6.
pub const ID_MAX_LEN: usize = 16 * 1024;

/// Errors a id parser may emit.
#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum IdParseError {
    /// Empty input.
    #[error("empty id")]
    Empty,
    /// Input exceeds [`ID_MAX_LEN`].
    #[error("id too long")]
    TooLong,
    /// Embedded NUL.
    #[error("id contains NUL")]
    Nul,
    /// C0/C1 control character (U+0000–U+001F, U+007F–U+009F).
    #[error("id contains control character")]
    ControlChar,
    /// Bidi override codepoint (U+202A–U+202E, U+2066–U+2069).
    #[error("id contains bidi override codepoint")]
    BidiOverride,
    /// Zero-width codepoint (U+200B–U+200D, U+FEFF, U+2060).
    #[error("id contains zero-width codepoint")]
    ZeroWidth,
    /// Input is not in NFC.
    #[error("id is not NFC-normalised")]
    NotNfc,
    /// Path-traversal segment (`..`) anywhere in the id.
    #[error("id contains path-traversal segment")]
    ParentTraversal,
    /// Absolute path segment.
    #[error("id contains absolute path")]
    AbsolutePath,
    /// Generic syntax error against the ADR-003 grammar.
    #[error("id does not match grammar: {0}")]
    Syntax(&'static str),
}

/// EntityId: `<repo_side>:<language>:<kind>:<local_path>:<stable_hash>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(String);

impl EntityId {
    /// Parse an EntityId, applying the security rules from
    /// `claude/05-handoff-1-core-ir.md` §F. Slice 1 stub returns
    /// `todo!()`; the implementer fills the parser during the
    /// Red-Green-Refactor loop in §E-1.
    pub fn parse(_input: &str) -> Result<Self, IdParseError> {
        todo!("§E-1 entity_id_parse_*; implement validator + NFC + grammar")
    }

    /// Read-only view of the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    // ---------- segment accessors ----------
    /// Returns the `repo_side` segment view.
    #[must_use]
    pub fn repo_side(&self) -> &str {
        // Slice 1 stub. Real impl indexes the parsed segments cached at
        // parse time; not reachable until §E-1 lands EntityId::parse.
        unimplemented!("EntityId segment accessors are §E-1 work")
    }

    /// Returns the `language` segment view.
    #[must_use]
    pub fn language(&self) -> &str {
        unimplemented!("EntityId segment accessors are §E-1 work")
    }

    /// Returns the `kind` segment view. **Type-respecting check on
    /// `MigrationMap` compares this segment only.**
    #[must_use]
    pub fn kind(&self) -> &str {
        unimplemented!("EntityId segment accessors are §E-1 work")
    }

    /// Returns the `local_path` segment view.
    #[must_use]
    pub fn local_path(&self) -> &str {
        unimplemented!("EntityId segment accessors are §E-1 work")
    }

    /// Returns the 12-hex `stable_hash` segment view.
    #[must_use]
    pub fn stable_hash(&self) -> &str {
        unimplemented!("EntityId segment accessors are §E-1 work")
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for EntityId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EntityId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        EntityId::parse(&s).map_err(serde::de::Error::custom)
    }
}

// --------------------------------------------------------------- ArtifactId

/// ArtifactId: `artifact:<repo_side>:<path>:<content_hash>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArtifactId(String);

impl ArtifactId {
    /// Parse an ArtifactId per ADR-003.
    pub fn parse(_input: &str) -> Result<Self, IdParseError> {
        todo!("§E-1 artifact_id_parse_*; mirrors EntityId parser")
    }

    /// Read-only view.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ArtifactId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ArtifactId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        ArtifactId::parse(&s).map_err(serde::de::Error::custom)
    }
}

// --------------------------------------------------------------- CorrId

/// Correspondence id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CorrId(String);

impl CorrId {
    /// Parse a CorrId per ADR-003.
    pub fn parse(_input: &str) -> Result<Self, IdParseError> {
        todo!("§E-1 corr_id_parse_*; mirrors EntityId parser")
    }

    /// Read-only view.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CorrId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for CorrId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CorrId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        CorrId::parse(&s).map_err(serde::de::Error::custom)
    }
}

// --------------------------------------------------------------- EdgeId

/// Build-edge id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeId(String);

impl EdgeId {
    /// Parse an EdgeId per ADR-003.
    pub fn parse(_input: &str) -> Result<Self, IdParseError> {
        todo!("§E-1 edge_id_parse_*; mirrors EntityId parser")
    }

    /// Read-only view.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for EdgeId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EdgeId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        EdgeId::parse(&s).map_err(serde::de::Error::custom)
    }
}
