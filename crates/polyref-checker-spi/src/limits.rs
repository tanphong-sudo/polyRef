//! Hard caps and the `SafePath` newtype.
//!
//! `SafePath` always represents a path **relative to a sandbox or run
//! root**. It never represents a host-absolute path. The plugin host
//! resolves it against the actual sandbox mount in Slice 3.

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Hard caps for plugin SPI payloads.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Max wire-payload size (bytes). 16 MiB per F-6.
    pub max_payload_bytes: usize,
    /// Max JSON nesting depth. 64 per F-6.
    pub max_json_depth: usize,
    /// Max id length (bytes). 16 KiB per F-6.
    pub max_id_bytes: usize,
    /// Max safe-path length (bytes). 4 KiB per F-6.
    pub max_path_bytes: usize,
    /// Max plugin-call deadline (ms).
    pub max_deadline_ms: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_payload_bytes: 16 * 1024 * 1024,
            max_json_depth: 64,
            max_id_bytes: 16 * 1024,
            max_path_bytes: 4 * 1024,
            max_deadline_ms: 600_000,
        }
    }
}

/// Errors enforcing [`Limits`].
#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum LimitsError {
    /// Payload exceeded `max_payload_bytes`.
    #[error("payload exceeds {0} bytes")]
    Payload(usize),
    /// JSON nesting exceeded `max_json_depth`.
    #[error("payload exceeds depth {0}")]
    Depth(usize),
    /// Deadline exceeded `max_deadline_ms`.
    #[error("deadline exceeds {0} ms")]
    Deadline(u32),
}

/// Safe path newtype — always relative to a sandbox / run root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SafePath(String);

/// Errors `SafePath::parse` may emit.
#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum SafePathError {
    /// Empty path.
    #[error("empty path")]
    Empty,
    /// Path exceeded the byte cap.
    #[error("path too long")]
    TooLong,
    /// Absolute path (`/...`).
    #[error("absolute paths are not allowed")]
    Absolute,
    /// Parent traversal (`..`).
    #[error("parent-traversal segments are not allowed")]
    ParentTraversal,
    /// Embedded NUL.
    #[error("path contains NUL")]
    Nul,
    /// Control / bidi / zero-width codepoint.
    #[error("path contains disallowed control or unicode codepoint")]
    Disallowed,
    /// Other syntax error.
    #[error("path syntax: {0}")]
    Syntax(&'static str),
}

/// Hard cap on safe-path length (bytes).
const SAFE_PATH_MAX_LEN: usize = 4 * 1024;

impl SafePath {
    /// Parse a string as a `SafePath`. Rejects absolute paths, parent
    /// traversal, NUL, control chars, bidi overrides, zero-width chars.
    /// Always relative to a sandbox/run root.
    pub fn parse(input: &str) -> Result<Self, SafePathError> {
        if input.is_empty() {
            return Err(SafePathError::Empty);
        }
        if input.len() > SAFE_PATH_MAX_LEN {
            return Err(SafePathError::TooLong);
        }
        if input.starts_with('/') {
            return Err(SafePathError::Absolute);
        }

        for ch in input.chars() {
            if ch == '\0' {
                return Err(SafePathError::Nul);
            }
            // C0/C1 control
            if ch <= '\u{001F}' || ('\u{007F}'..='\u{009F}').contains(&ch) {
                return Err(SafePathError::Disallowed);
            }
            // Bidi overrides
            if ('\u{202A}'..='\u{202E}').contains(&ch) || ('\u{2066}'..='\u{2069}').contains(&ch) {
                return Err(SafePathError::Disallowed);
            }
            // Zero-width
            if ('\u{200B}'..='\u{200D}').contains(&ch) || ch == '\u{FEFF}' || ch == '\u{2060}' {
                return Err(SafePathError::Disallowed);
            }
        }

        // Parent traversal
        for segment in input.split('/') {
            if segment == ".." {
                return Err(SafePathError::ParentTraversal);
            }
        }

        Ok(Self(input.to_owned()))
    }

    /// Read-only view.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for SafePath {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SafePath {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        SafePath::parse(&s).map_err(serde::de::Error::custom)
    }
}
