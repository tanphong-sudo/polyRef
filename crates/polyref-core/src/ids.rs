//! Newtype IDs with validated construction.
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
use unicode_normalization::UnicodeNormalization;

/// Hard cap on parsed id length (bytes). Per hard blocker F-6.
pub const ID_MAX_LEN: usize = 16 * 1024;

/// Valid language tags per ADR-003 / `schemas/language.json`.
const VALID_LANGUAGES: &[&str] = &[
    "build",
    "dockerfile",
    "java",
    "json",
    "jsonschema",
    "openapi",
    "py",
    "sql",
    "ts",
    "yaml",
];

/// Valid repo-side tags.
const VALID_REPO_SIDES: &[&str] = &["old", "new"];

/// Errors an id parser may emit.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
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

// ─── Security validation helpers ───────────────────────────────────────

/// Check for disallowed codepoints. Returns the first error found.
fn check_disallowed_chars(s: &str) -> Result<(), IdParseError> {
    for ch in s.chars() {
        // NUL
        if ch == '\0' {
            return Err(IdParseError::Nul);
        }
        // C0 control (U+0001–U+001F) and C1 control (U+007F–U+009F)
        if ch <= '\u{001F}' || ('\u{007F}'..='\u{009F}').contains(&ch) {
            return Err(IdParseError::ControlChar);
        }
        // Bidi overrides (U+202A–U+202E, U+2066–U+2069)
        if ('\u{202A}'..='\u{202E}').contains(&ch) || ('\u{2066}'..='\u{2069}').contains(&ch) {
            return Err(IdParseError::BidiOverride);
        }
        // Zero-width chars (U+200B–U+200D, U+FEFF, U+2060)
        if ('\u{200B}'..='\u{200D}').contains(&ch) || ch == '\u{FEFF}' || ch == '\u{2060}' {
            return Err(IdParseError::ZeroWidth);
        }
    }
    Ok(())
}

/// Check NFC normalization. Rejects non-NFC input (does not silently
/// normalize, to keep parsing reproducible).
fn check_nfc(s: &str) -> Result<(), IdParseError> {
    let nfc: String = s.nfc().collect();
    if nfc != s {
        return Err(IdParseError::NotNfc);
    }
    Ok(())
}

/// Check for path-traversal segments (`..`) and absolute paths.
fn check_path_safety(s: &str) -> Result<(), IdParseError> {
    if s.starts_with('/') {
        return Err(IdParseError::AbsolutePath);
    }
    // Check for `..` as a path segment in the local_path portion.
    // We check the entire string since `:` separators prevent false
    // positives on the structured segments.
    for segment in s.split('/') {
        if segment == ".." {
            return Err(IdParseError::ParentTraversal);
        }
    }
    Ok(())
}

/// Common validation applied to all id types.
fn validate_common(input: &str) -> Result<(), IdParseError> {
    if input.is_empty() {
        return Err(IdParseError::Empty);
    }
    if input.len() > ID_MAX_LEN {
        return Err(IdParseError::TooLong);
    }
    check_disallowed_chars(input)?;
    check_nfc(input)?;
    check_path_safety(input)?;
    Ok(())
}

// ─── EntityId ──────────────────────────────────────────────────────────

/// Parsed segment offsets cached inside EntityId for O(1) accessor calls.
/// Stored as byte offsets into the inner string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct EntitySegments {
    /// End of repo_side (exclusive; start is 0).
    repo_side_end: usize,
    /// End of language (exclusive; start is repo_side_end + 1).
    language_end: usize,
    /// End of kind (exclusive; start is language_end + 1).
    kind_end: usize,
    /// End of local_path (exclusive; start is kind_end + 1).
    local_path_end: usize,
    // stable_hash starts at local_path_end + 1 and goes to end.
}

/// EntityId: `<repo_side>:<language>:<kind>:<local_path>:<stable_hash>`.
///
/// Grammar per ADR-003:
/// - `repo_side` ∈ {"old", "new"}
/// - `language` ∈ language.json enum
/// - `kind` = lowercase EntityKind discriminant (e.g. "handler", "route")
/// - `local_path` = POSIX path with optional `#` anchor
/// - `stable_hash` = 12 hex chars of SHA-256
///
/// Type-respecting check on `MigrationMap` compares the **kind** segment
/// only — not language. Cross-language migrations are first-class.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId {
    inner: String,
    segments: EntitySegments,
}

impl EntityId {
    /// Parse an EntityId, applying security rules from ADR-003 and the
    /// security steering file.
    pub fn parse(input: &str) -> Result<Self, IdParseError> {
        validate_common(input)?;

        // Split into exactly 5 segments by `:`.
        // But local_path may contain `:` (for scope separators like
        // `User#methods:setName`), so we split from the left for the
        // first 3 segments, then from the right for the last segment
        // (stable_hash), and everything in between is local_path.
        let mut parts = input.splitn(4, ':');
        let repo_side = parts
            .next()
            .ok_or(IdParseError::Syntax("missing repo_side"))?;
        let language = parts
            .next()
            .ok_or(IdParseError::Syntax("missing language"))?;
        let kind = parts.next().ok_or(IdParseError::Syntax("missing kind"))?;
        let rest = parts
            .next()
            .ok_or(IdParseError::Syntax("missing local_path and stable_hash"))?;

        // rest = "<local_path>:<stable_hash>" where stable_hash is last
        // 12 chars after the last `:`.
        let last_colon = rest
            .rfind(':')
            .ok_or(IdParseError::Syntax("missing stable_hash separator"))?;
        let local_path = &rest[..last_colon];
        let stable_hash = &rest[last_colon + 1..];

        // Validate repo_side
        if !VALID_REPO_SIDES.contains(&repo_side) {
            return Err(IdParseError::Syntax("invalid repo_side"));
        }

        // Validate language
        if !VALID_LANGUAGES.contains(&language) {
            return Err(IdParseError::Syntax("invalid language"));
        }

        // Validate kind: must be non-empty, lowercase ascii + underscore
        if kind.is_empty() {
            return Err(IdParseError::Syntax("empty kind"));
        }
        if !kind.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(IdParseError::Syntax(
                "kind must be lowercase ascii + underscore",
            ));
        }

        // Validate local_path: must be non-empty
        if local_path.is_empty() {
            return Err(IdParseError::Syntax("empty local_path"));
        }

        // Validate stable_hash: exactly 12 lowercase hex chars
        if stable_hash.len() != 12 {
            return Err(IdParseError::Syntax("stable_hash must be 12 hex chars"));
        }
        if !stable_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(IdParseError::Syntax("stable_hash must be lowercase hex"));
        }

        // Compute segment offsets
        let repo_side_end = repo_side.len();
        let language_end = repo_side_end + 1 + language.len();
        let kind_end = language_end + 1 + kind.len();
        let local_path_end = kind_end + 1 + local_path.len();

        Ok(Self {
            inner: input.to_owned(),
            segments: EntitySegments {
                repo_side_end,
                language_end,
                kind_end,
                local_path_end,
            },
        })
    }

    /// Read-only view of the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// Returns the `repo_side` segment ("old" or "new").
    #[must_use]
    pub fn repo_side(&self) -> &str {
        &self.inner[..self.segments.repo_side_end]
    }

    /// Returns the `language` segment.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.inner[self.segments.repo_side_end + 1..self.segments.language_end]
    }

    /// Returns the `kind` segment. **Type-respecting check on
    /// `MigrationMap` compares this segment only.**
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.inner[self.segments.language_end + 1..self.segments.kind_end]
    }

    /// Returns the `local_path` segment.
    #[must_use]
    pub fn local_path(&self) -> &str {
        &self.inner[self.segments.kind_end + 1..self.segments.local_path_end]
    }

    /// Returns the 12-hex `stable_hash` segment.
    #[must_use]
    pub fn stable_hash(&self) -> &str {
        &self.inner[self.segments.local_path_end + 1..]
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl Serialize for EntityId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.inner)
    }
}

impl<'de> Deserialize<'de> for EntityId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        EntityId::parse(&s).map_err(serde::de::Error::custom)
    }
}

// ─── ArtifactId ────────────────────────────────────────────────────────

/// ArtifactId: `artifact:<repo_side>:<path>:<content_hash>`.
///
/// - `repo_side` ∈ {"old", "new"}
/// - `path` = relative POSIX path (no traversal, no absolute)
/// - `content_hash` = 12 lowercase hex chars
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArtifactId(String);

impl ArtifactId {
    /// Parse an ArtifactId per ADR-003.
    pub fn parse(input: &str) -> Result<Self, IdParseError> {
        validate_common(input)?;

        // Format: artifact:<repo_side>:<path>:<content_hash>
        let stripped = input
            .strip_prefix("artifact:")
            .ok_or(IdParseError::Syntax("must start with 'artifact:'"))?;

        let mut parts = stripped.splitn(3, ':');
        let repo_side = parts
            .next()
            .ok_or(IdParseError::Syntax("missing repo_side"))?;
        let _rest = parts.next().ok_or(IdParseError::Syntax("missing path"))?;

        // If there's a third part from splitn(3), combine with rest
        // Actually we need: repo_side, then path (may contain colons? no — paths use / not :),
        // then content_hash (last 12 hex after last colon).
        // Re-parse: after repo_side, everything until last colon is path, after is hash.
        let after_repo_side = &stripped[repo_side.len() + 1..];
        let last_colon = after_repo_side
            .rfind(':')
            .ok_or(IdParseError::Syntax("missing content_hash separator"))?;
        let path = &after_repo_side[..last_colon];
        let content_hash = &after_repo_side[last_colon + 1..];

        if !VALID_REPO_SIDES.contains(&repo_side) {
            return Err(IdParseError::Syntax("invalid repo_side"));
        }
        if path.is_empty() {
            return Err(IdParseError::Syntax("empty path"));
        }
        if content_hash.len() != 12 {
            return Err(IdParseError::Syntax("content_hash must be 12 hex chars"));
        }
        if !content_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(IdParseError::Syntax("content_hash must be lowercase hex"));
        }

        Ok(Self(input.to_owned()))
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

// ─── CorrId ───────────────────────────────────────────────────────────

/// Correspondence id: `corr:<kind>:<hash>`.
///
/// - `kind` = lowercase correspondence kind tag
/// - `hash` = 16–64 lowercase hex chars
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CorrId(String);

impl CorrId {
    /// Parse a CorrId per ADR-003.
    pub fn parse(input: &str) -> Result<Self, IdParseError> {
        validate_common(input)?;

        let stripped = input
            .strip_prefix("corr:")
            .ok_or(IdParseError::Syntax("must start with 'corr:'"))?;

        let colon_pos = stripped
            .find(':')
            .ok_or(IdParseError::Syntax("missing hash separator"))?;
        let kind = &stripped[..colon_pos];
        let hash = &stripped[colon_pos + 1..];

        if kind.is_empty() {
            return Err(IdParseError::Syntax("empty kind"));
        }
        if !kind.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(IdParseError::Syntax(
                "kind must be lowercase ascii + underscore",
            ));
        }
        if hash.len() < 16 || hash.len() > 64 {
            return Err(IdParseError::Syntax("hash must be 16–64 hex chars"));
        }
        if !hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(IdParseError::Syntax("hash must be lowercase hex"));
        }

        Ok(Self(input.to_owned()))
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

// ─── EdgeId ───────────────────────────────────────────────────────────

/// Build-edge id: `edge:<kind>:<hash>`.
///
/// - `kind` = lowercase edge kind tag
/// - `hash` = 16–64 lowercase hex chars
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeId(String);

impl EdgeId {
    /// Parse an EdgeId per ADR-003.
    pub fn parse(input: &str) -> Result<Self, IdParseError> {
        validate_common(input)?;

        let stripped = input
            .strip_prefix("edge:")
            .ok_or(IdParseError::Syntax("must start with 'edge:'"))?;

        let colon_pos = stripped
            .find(':')
            .ok_or(IdParseError::Syntax("missing hash separator"))?;
        let kind = &stripped[..colon_pos];
        let hash = &stripped[colon_pos + 1..];

        if kind.is_empty() {
            return Err(IdParseError::Syntax("empty kind"));
        }
        if !kind.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(IdParseError::Syntax(
                "kind must be lowercase ascii + underscore",
            ));
        }
        if hash.len() < 16 || hash.len() > 64 {
            return Err(IdParseError::Syntax("hash must be 16–64 hex chars"));
        }
        if !hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(IdParseError::Syntax("hash must be lowercase hex"));
        }

        Ok(Self(input.to_owned()))
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

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    // ── EntityId ──

    #[test]
    fn entity_id_parse_accepts_canonical_form() {
        let id = "old:ts:handler:src/users.ts#createUser:0123456789ab";
        let parsed = EntityId::parse(id).expect("canonical form should parse");
        assert_eq!(parsed.as_str(), id);
        assert_eq!(parsed.repo_side(), "old");
        assert_eq!(parsed.language(), "ts");
        assert_eq!(parsed.kind(), "handler");
        assert_eq!(parsed.local_path(), "src/users.ts#createUser");
        assert_eq!(parsed.stable_hash(), "0123456789ab");
    }

    #[test]
    fn entity_id_parse_accepts_local_path_with_colon_scope() {
        let id = "new:py:handler:src/users.py#User#methods:setName:abcdef012345";
        let parsed = EntityId::parse(id).expect("colon in local_path should parse");
        assert_eq!(parsed.local_path(), "src/users.py#User#methods:setName");
        assert_eq!(parsed.stable_hash(), "abcdef012345");
    }

    #[test]
    fn entity_id_parse_rejects_empty() {
        assert_eq!(EntityId::parse(""), Err(IdParseError::Empty));
    }

    #[test]
    fn entity_id_parse_rejects_no_separator() {
        assert_eq!(
            EntityId::parse("noseparators"),
            Err(IdParseError::Syntax("missing language"))
        );
    }

    #[test]
    fn entity_id_parse_rejects_invalid_repo_side() {
        assert_eq!(
            EntityId::parse("bad:ts:handler:src/h.ts:0123456789ab"),
            Err(IdParseError::Syntax("invalid repo_side"))
        );
    }

    #[test]
    fn entity_id_parse_rejects_invalid_language() {
        assert_eq!(
            EntityId::parse("old:csharp:handler:src/h.cs:0123456789ab"),
            Err(IdParseError::Syntax("invalid language"))
        );
    }

    #[test]
    fn entity_id_parse_rejects_invalid_kind() {
        assert_eq!(
            EntityId::parse("old:ts:HANDLER:src/h.ts:0123456789ab"),
            Err(IdParseError::Syntax(
                "kind must be lowercase ascii + underscore"
            ))
        );
    }

    #[test]
    fn entity_id_parse_rejects_path_with_null_byte() {
        assert_eq!(
            EntityId::parse("old:ts:handler:src/\0h.ts:0123456789ab"),
            Err(IdParseError::Nul)
        );
    }

    #[test]
    fn entity_id_parse_rejects_path_with_parent_traversal() {
        assert_eq!(
            EntityId::parse("old:ts:handler:src/../etc:0123456789ab"),
            Err(IdParseError::ParentTraversal)
        );
    }

    #[test]
    fn entity_id_parse_rejects_control_chars() {
        assert_eq!(
            EntityId::parse("old:ts:handler:src/\u{0007}h.ts:0123456789ab"),
            Err(IdParseError::ControlChar)
        );
    }

    #[test]
    fn entity_id_parse_rejects_bidi_overrides() {
        assert_eq!(
            EntityId::parse("old:ts:handler:src/\u{202E}h.ts:0123456789ab"),
            Err(IdParseError::BidiOverride)
        );
    }

    #[test]
    fn entity_id_parse_rejects_zero_width_chars() {
        assert_eq!(
            EntityId::parse("old:ts:handler:src/\u{200B}h.ts:0123456789ab"),
            Err(IdParseError::ZeroWidth)
        );
    }

    #[test]
    fn entity_id_parse_rejects_oversize_input() {
        let long = format!("old:ts:handler:{}:0123456789ab", "a".repeat(ID_MAX_LEN));
        assert_eq!(EntityId::parse(&long), Err(IdParseError::TooLong));
    }

    #[test]
    fn entity_id_serde_roundtrip() {
        let id = "old:ts:handler:src/users.ts#createUser:0123456789ab";
        let parsed = EntityId::parse(id).expect("should parse");
        let json = serde_json::to_string(&parsed).expect("serialize");
        let back: EntityId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, back);
    }

    #[test]
    fn entity_id_serde_does_not_bypass_parse() {
        let raw = "\"this is not a valid entity id\"";
        let result: Result<EntityId, _> = serde_json::from_str(raw);
        assert!(result.is_err(), "serde must route through EntityId::parse");
    }

    #[test]
    fn entity_id_eq_is_segment_aware() {
        let a = EntityId::parse("old:ts:handler:src/h.ts:0123456789ab").unwrap();
        let b = EntityId::parse("old:ts:handler:src/h.ts:abcdef012345").unwrap();
        assert_ne!(a, b, "different stable_hash means different id");
    }

    // ── ArtifactId ──

    #[test]
    fn artifact_id_parse_accepts_canonical() {
        let id = "artifact:old:src/users.ts:0123456789ab";
        let parsed = ArtifactId::parse(id).expect("should parse");
        assert_eq!(parsed.as_str(), id);
    }

    #[test]
    fn artifact_id_parse_rejects_empty() {
        assert_eq!(ArtifactId::parse(""), Err(IdParseError::Empty));
    }

    #[test]
    fn artifact_id_parse_rejects_wrong_prefix() {
        assert!(ArtifactId::parse("entity:old:src/h.ts:0123456789ab").is_err());
    }

    // ── CorrId ──

    #[test]
    fn corr_id_parse_accepts_canonical() {
        let id = "corr:route:0123456789abcdef";
        let parsed = CorrId::parse(id).expect("should parse");
        assert_eq!(parsed.as_str(), id);
    }

    #[test]
    fn corr_id_parse_rejects_empty() {
        assert_eq!(CorrId::parse(""), Err(IdParseError::Empty));
    }

    #[test]
    fn corr_id_parse_rejects_short_hash() {
        assert!(CorrId::parse("corr:route:0123").is_err());
    }

    // ── EdgeId ──

    #[test]
    fn edge_id_parse_accepts_canonical() {
        let id = "edge:build_codegen:0123456789abcdef";
        let parsed = EdgeId::parse(id).expect("should parse");
        assert_eq!(parsed.as_str(), id);
    }

    #[test]
    fn edge_id_parse_rejects_empty() {
        assert_eq!(EdgeId::parse(""), Err(IdParseError::Empty));
    }

    #[test]
    fn edge_id_parse_rejects_uppercase_hash() {
        assert!(EdgeId::parse("edge:build:0123456789ABCDEF").is_err());
    }
}
