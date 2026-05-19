//! Source span with checked constructor.
//!
//! All fields are private. The only ingress is `try_new`. Serde routes
//! through `try_new` so deserialization cannot bypass the
//! `start <= end` invariant.

use crate::ids::ArtifactId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::num::NonZeroU32;
use thiserror::Error;

/// Errors `SourceSpan::try_new` may emit.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpanError {
    /// `start > end` lexicographically.
    #[error("inverted range: start > end")]
    Inverted,
    /// `utf16_cols.0 > utf16_cols.1`.
    #[error("utf16 cols inverted")]
    Utf16Inverted,
}

/// Line + column position. Line is 1-indexed (`NonZeroU32` rules out
/// line 0 statically); column is the 0-indexed UTF-8 byte offset on
/// that line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LineCol {
    /// 1-indexed line.
    pub line: NonZeroU32,
    /// 0-indexed UTF-8 byte column.
    pub col: u32,
}

impl LineCol {
    /// Build a new `LineCol`. `line` is `NonZeroU32` at the type level.
    #[must_use]
    pub const fn new(line: NonZeroU32, col: u32) -> Self {
        Self { line, col }
    }
}

/// Half-open span `[start, end)` over an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    artifact: ArtifactId,
    start: LineCol,
    end: LineCol,
    utf16_cols: Option<(u32, u32)>,
}

impl SourceSpan {
    /// Construct a new `SourceSpan`. Rejects inverted ranges.
    pub fn try_new(
        artifact: ArtifactId,
        start: LineCol,
        end: LineCol,
        utf16_cols: Option<(u32, u32)>,
    ) -> Result<Self, SpanError> {
        if start > end {
            return Err(SpanError::Inverted);
        }
        if let Some((s, e)) = utf16_cols {
            if s > e {
                return Err(SpanError::Utf16Inverted);
            }
        }
        Ok(Self {
            artifact,
            start,
            end,
            utf16_cols,
        })
    }

    /// Owning artifact.
    #[must_use]
    pub fn artifact(&self) -> &ArtifactId {
        &self.artifact
    }
    /// Span start.
    #[must_use]
    pub fn start(&self) -> LineCol {
        self.start
    }
    /// Span end (exclusive).
    #[must_use]
    pub fn end(&self) -> LineCol {
        self.end
    }
    /// Optional UTF-16 column pair for editor / LSP interop.
    #[must_use]
    pub fn utf16_cols(&self) -> Option<(u32, u32)> {
        self.utf16_cols
    }
}

#[derive(Serialize, Deserialize)]
struct SourceSpanWire {
    artifact: ArtifactId,
    start: LineCol,
    end: LineCol,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    utf16_cols: Option<(u32, u32)>,
}

impl Serialize for SourceSpan {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let wire = SourceSpanWire {
            artifact: self.artifact.clone(),
            start: self.start,
            end: self.end,
            utf16_cols: self.utf16_cols,
        };
        wire.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for SourceSpan {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let wire = SourceSpanWire::deserialize(de)?;
        SourceSpan::try_new(wire.artifact, wire.start, wire.end, wire.utf16_cols)
            .map_err(serde::de::Error::custom)
    }
}
