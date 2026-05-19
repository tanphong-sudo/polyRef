//! Extractor SPI types.
//!
//! Mirrors `schemas/extractor-spi/extract.json`. Plugin host (Slice 3)
//! enforces deadlines + cgroup limits.

use crate::limits::SafePath;
use polyref_core::ids::EntityId;
use polyref_core::language::Language;
use polyref_core::source_span::SourceSpan;
use serde::{Deserialize, Serialize};

/// Request body for `extract`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRequest {
    /// Path of the artifact to extract, **relative to the sandbox root**.
    pub artifact_path: SafePath,
    /// SHA-256 of the artifact content (lowercase hex, 64 chars).
    pub content_hash: String,
    /// Language tag.
    pub language: Language,
    /// Plugin-specific options.
    pub options: serde_json::Value,
    /// Plugin deadline in milliseconds.
    pub deadline_ms: u32,
    /// Directory the plugin may write logs to. **Relative.**
    pub log_dir: SafePath,
}

/// Response body for `extract`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractResult {
    /// Extracted entities.
    pub entities: Vec<ExtractedEntity>,
    /// Local-fact rows (free-form per language).
    pub local_facts: Vec<serde_json::Value>,
    /// Features the extractor saw but cannot model.
    pub unsupported_features: Vec<UnsupportedFeatureNote>,
    /// Plugin version string.
    pub extractor_version: String,
}

/// One extracted entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    /// Stable entity id.
    pub entity_id: EntityId,
    /// Local kind tag (lowercase snake_case, e.g. `handler`).
    pub kind: String,
    /// Local name (e.g. `createUser`).
    pub local_name: String,
    /// Optional local type.
    pub r#type: Option<String>,
    /// Source span.
    pub source_span: SourceSpan,
}

/// Note that an unsupported feature was encountered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsupportedFeatureNote {
    /// Short feature tag (`reflection`, `dynamic_route`, …).
    pub feature: String,
    /// Span pointing at the offending location.
    pub span: SourceSpan,
    /// Optional human note.
    pub note: Option<String>,
}
