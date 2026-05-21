//! Report artifact store for Layer 1.
//!
//! Owns the `.polyref/runs/<report_id>/` layout from ADR-006:
//!
//! ```text
//! runs/<report_id>/
//!   report.json
//!   report.md
//!   audit.ndjson
//!   manifest.json
//!   evidence/
//! ```
//!
//! The report schema also models `manifest_json` as an
//! `EvidencePointer`, so Layer 1 writes a mirrored
//! `evidence/manifest.json` beside the ADR-006 root manifest.

use crate::{AuditEvent, AuditWriteError, AuditWriter};
use polyref_core::{evidence::EvidencePointer, ValidationReport};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tempfile::NamedTempFile;
use thiserror::Error;

/// Store for validation report run directories.
#[derive(Debug, Clone)]
pub struct ReportStore {
    runs_root: PathBuf,
}

/// One concrete `.polyref/runs/<report_id>/` directory.
#[derive(Debug, Clone)]
pub struct RunReportStore {
    path: PathBuf,
}

/// Minimal Layer 1 run manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunManifest {
    /// Stable report id; matches the run directory name.
    pub report_id: String,
    /// Schema version for the manifest placeholder.
    pub schema_version: String,
}

/// Report store failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReportStoreError {
    /// Host filesystem operation failed.
    #[error("report store io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization or deserialization failed.
    #[error("report store json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Audit writer failed.
    #[error("report store audit error: {0}")]
    Audit(#[from] AuditWriteError),
    /// Report id is unsafe for a single run directory name.
    #[error("invalid report_id: {0}")]
    InvalidReportId(String),
}

impl RunManifest {
    /// Schema version from `schemas/manifest.json`.
    pub const SCHEMA_VERSION: &'static str = "0.1.0";

    /// Build a minimal Layer 1 manifest.
    #[must_use]
    pub fn new(report_id: impl Into<String>) -> Self {
        Self {
            report_id: report_id.into(),
            schema_version: Self::SCHEMA_VERSION.to_owned(),
        }
    }
}

impl ReportStore {
    /// Open a report store rooted at `.polyref` or an equivalent test root.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError::Io`] if the `runs/` directory cannot
    /// be created.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ReportStoreError> {
        let runs_root = root.as_ref().join("runs");
        fs::create_dir_all(&runs_root)?;
        Ok(Self { runs_root })
    }

    /// Create a fresh run directory.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError::InvalidReportId`] if `report_id` is
    /// empty or not a single safe path segment. Returns
    /// [`ReportStoreError::Io`] if the directory already exists or
    /// cannot be created.
    pub fn create_run(&self, report_id: &str) -> Result<RunReportStore, ReportStoreError> {
        validate_report_id(report_id)?;
        let path = self.runs_root.join(report_id);
        fs::create_dir(&path)?;
        fs::create_dir(path.join("evidence"))?;
        Ok(RunReportStore { path })
    }
}

impl RunReportStore {
    /// Run directory path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write `report.json` using pretty JSON.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError`] if serialization or writing fails.
    pub fn write_report_json(&self, report: &ValidationReport) -> Result<(), ReportStoreError> {
        let bytes = serde_json::to_vec_pretty(report)?;
        atomic_write(self.path.join("report.json"), &bytes)
    }

    /// Read `report.json`.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError`] if reading or deserialization fails.
    pub fn read_report_json(&self) -> Result<ValidationReport, ReportStoreError> {
        let bytes = fs::read(self.path.join("report.json"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Write `report.md`.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError`] if writing fails.
    pub fn write_report_markdown(&self, markdown: &str) -> Result<(), ReportStoreError> {
        atomic_write(self.path.join("report.md"), markdown.as_bytes())
    }

    /// Write root and evidence-pointer manifest copies.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError`] if serialization or writing fails.
    pub fn write_manifest_json(&self, manifest: &RunManifest) -> Result<(), ReportStoreError> {
        let bytes = serde_json::to_vec_pretty(manifest)?;
        atomic_write(self.path.join("manifest.json"), &bytes)?;
        atomic_write(self.path.join("evidence").join("manifest.json"), &bytes)
    }

    /// Write evidence bytes under the run `evidence/` directory.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError`] if writing fails.
    pub fn write_evidence(
        &self,
        pointer: &EvidencePointer,
        bytes: &[u8],
    ) -> Result<(), ReportStoreError> {
        let relative = pointer
            .as_str()
            .strip_prefix("evidence/")
            .ok_or_else(|| ReportStoreError::InvalidReportId(pointer.as_str().to_owned()))?;
        atomic_write(self.path.join("evidence").join(relative), bytes)
    }

    /// Append one event to `audit.ndjson`.
    ///
    /// # Errors
    ///
    /// Returns [`ReportStoreError`] if opening or appending fails.
    pub fn append_audit_event(&self, event: &AuditEvent) -> Result<(), ReportStoreError> {
        let mut writer = AuditWriter::open(self.path.join("audit.ndjson"))?;
        writer.append(event)?;
        Ok(())
    }
}

fn validate_report_id(report_id: &str) -> Result<(), ReportStoreError> {
    if report_id.is_empty() {
        return Err(ReportStoreError::InvalidReportId(
            "must not be empty".to_owned(),
        ));
    }
    if report_id.contains('/') || report_id.contains('\\') {
        return Err(ReportStoreError::InvalidReportId(
            "must be one path segment".to_owned(),
        ));
    }
    let path = Path::new(report_id);
    if path.is_absolute() {
        return Err(ReportStoreError::InvalidReportId(
            "must not be absolute".to_owned(),
        ));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(ReportStoreError::InvalidReportId(
                    "contains unsafe path component".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn atomic_write(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), ReportStoreError> {
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
