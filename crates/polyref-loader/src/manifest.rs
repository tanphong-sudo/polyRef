//! Layer 2 run manifest DTOs.
//!
//! The manifest is intentionally open-ended: `schemas/manifest.json`
//! still permits additional properties while later loader branches add
//! checkout, sandbox, and replay details.

use polyref_graph::RunReportStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;
use thiserror::Error;

/// Run manifest written under `.polyref/runs/<report_id>/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunManifest {
    /// Stable run id; matches the report directory name.
    pub report_id: String,
    /// Manifest schema version.
    pub schema_version: String,
    /// Old repository checkout metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_repo: Option<ManifestRepoRef>,
    /// New repository checkout metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_repo: Option<ManifestRepoRef>,
    /// Candidate patch SHA-256 hex digest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_hash: Option<String>,
    /// Candidate patch path relative to the run directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_path: Option<String>,
    /// Sandbox profile selected for replay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxProfile>,
    /// Creation timestamp string supplied by the caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Tool id to version string map.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_versions: BTreeMap<String, String>,
}

/// Repository metadata captured in the run manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestRepoRef {
    /// Repository id used by reports and audit events.
    pub repo_id: String,
    /// Commit hash or snapshot id.
    pub commit: String,
    /// Workspace path relative to the run directory.
    pub workspace_path: String,
}

/// Sandbox profile captured in the run manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProfile {
    /// Sandbox backend used by this run.
    pub backend: SandboxBackend,
    /// Profile identifier.
    pub profile_id: String,
    /// Whether outbound network was allowed.
    pub network_allowed: bool,
    /// Environment keys passed to the sandbox; values are never logged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
    /// CPU limit in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_seconds: Option<u64>,
    /// Memory limit in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
    /// Tmpfs/write limit in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmpfs_bytes: Option<u64>,
}

/// Sandbox backend tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SandboxBackend {
    /// Docker or another OCI-compatible runtime.
    Docker,
    /// Podman rootless runtime.
    Podman,
    /// nsjail process sandbox.
    Nsjail,
    /// macOS sandbox-exec development fallback.
    SandboxExec,
    /// No executable backend; tests may use this for manifest-only paths.
    Unavailable,
}

impl RunManifest {
    /// Manifest schema version from `schemas/manifest.json`.
    pub const SCHEMA_VERSION: &'static str = "0.1.0";

    /// Create a minimal manifest.
    #[must_use]
    pub fn new(report_id: impl Into<String>) -> Self {
        Self {
            report_id: report_id.into(),
            schema_version: Self::SCHEMA_VERSION.to_owned(),
            old_repo: None,
            new_repo: None,
            patch_hash: None,
            patch_path: None,
            sandbox: None,
            created_at: None,
            tool_versions: BTreeMap::new(),
        }
    }

    /// Write root and mirrored evidence manifest files through `ReportStore`.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError`] if serialization or writing fails.
    pub fn write_to_report_store(&self, run: &RunReportStore) -> Result<(), ManifestWriteError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        atomic_write(run.path().join("manifest.json"), &bytes)?;
        atomic_write(run.path().join("evidence").join("manifest.json"), &bytes)
    }
}

/// Expanded manifest write failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ManifestWriteError {
    /// Host filesystem write failed.
    #[error("manifest io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization failed.
    #[error("manifest json error: {0}")]
    Json(#[from] serde_json::Error),
}

fn atomic_write(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), ManifestWriteError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        let mut temp = NamedTempFile::new_in(parent)?;
        temp.write_all(bytes)?;
        temp.flush()?;
        temp.persist(path).map_err(|err| err.error)?;
        Ok(())
    } else {
        fs::write(path, bytes)?;
        Ok(())
    }
}
