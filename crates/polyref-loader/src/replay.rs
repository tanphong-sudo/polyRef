//! Sandboxed candidate patch replay for Layer 2.

use crate::checkout::{checkout_old_workspace, CheckoutError, CheckoutPlan, CheckoutResult};
use crate::manifest::{
    ManifestRepoRef, ManifestWriteError, RunManifest, SandboxBackend, SandboxProfile,
};
use crate::sandbox::{Sandbox, SandboxCommand, SandboxError, SandboxMount, SandboxProfileSpec};
use polyref_graph::{AuditEvent, AuditEventError, AuditEventTag, ReportStoreError, RunReportStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// Candidate patch bytes or file input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PatchInput {
    /// In-memory unified diff bytes.
    Bytes(Vec<u8>),
    /// Existing unified diff file. The original path is not persisted.
    File(PathBuf),
}

/// Plan for producing old/new replay workspaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayPlan {
    /// Report id expected to match the run directory name.
    pub report_id: String,
    /// Checkout plan used to produce `workspace/old`.
    pub checkout: CheckoutPlan,
    /// Candidate patch input.
    pub patch: PatchInput,
    /// Sandbox backend selected for replay.
    pub sandbox_backend: SandboxBackend,
    /// Optional caller-supplied RFC3339 timestamp string for audit/manifest.
    pub created_at: Option<String>,
}

/// Result of sandboxed candidate replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayResult {
    /// Old workspace checkout result.
    pub old_checkout: CheckoutResult,
    /// New patched workspace metadata.
    pub new_repo: ManifestRepoRef,
    /// Candidate patch SHA-256 hex digest.
    pub patch_hash: String,
    /// Patch path relative to the run directory.
    pub patch_path: String,
    /// Effective sandbox profile recorded for replay.
    pub sandbox_profile: SandboxProfile,
}

/// Replay failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReplayError {
    /// Checkout failed.
    #[error("replay checkout failed: {0}")]
    Checkout(#[from] CheckoutError),
    /// Sandbox failed or denied replay.
    #[error("replay sandbox failed: {0}")]
    Sandbox(#[from] SandboxError),
    /// Manifest writing failed.
    #[error("replay manifest failed: {0}")]
    Manifest(#[from] ManifestWriteError),
    /// Audit writing failed.
    #[error("replay audit failed: {0}")]
    Audit(#[from] ReplayAuditError),
    /// Candidate patch was rejected by the patch tool.
    #[error("candidate patch rejected: {0}")]
    PatchRejected(String),
    /// Path is unsafe or cannot be represented safely.
    #[error("unsafe replay path: {0}")]
    UnsafePath(String),
    /// Plan report id and run directory do not match.
    #[error("replay report id mismatch: plan={plan}, run={run}")]
    ReportIdMismatch {
        /// Report id from the replay plan.
        plan: String,
        /// Report id inferred from the run directory.
        run: String,
    },
    /// Host filesystem operation failed.
    #[error("replay io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Audit event construction/write failures from replay.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReplayAuditError {
    /// Audit event fields failed validation.
    #[error("replay audit event invalid: {0}")]
    Event(#[from] AuditEventError),
    /// Report store failed while appending audit.
    #[error("replay audit write failed: {0}")]
    Store(#[from] ReportStoreError),
}

impl ReplayPlan {
    /// Create a replay plan.
    #[must_use]
    pub fn new(
        report_id: impl Into<String>,
        checkout: CheckoutPlan,
        patch: PatchInput,
        sandbox_backend: SandboxBackend,
    ) -> Self {
        Self {
            report_id: report_id.into(),
            checkout,
            patch,
            sandbox_backend,
            created_at: None,
        }
    }

    /// Set a caller-supplied timestamp string for audit and manifest output.
    #[must_use]
    pub fn with_created_at(mut self, created_at: impl Into<String>) -> Self {
        self.created_at = Some(created_at.into());
        self
    }
}

/// Convenience helper for Layer 2 callers that load a local repo with a patch.
///
/// # Errors
///
/// Returns [`ReplayError`] when checkout, sandbox replay, audit, manifest,
/// or filesystem handling fails.
pub fn load_repo_with_patch<S: Sandbox>(
    report_id: impl Into<String>,
    source: crate::checkout::RepoSource,
    patch: PatchInput,
    sandbox_backend: SandboxBackend,
    run: &RunReportStore,
    sandbox: &S,
) -> Result<ReplayResult, ReplayError> {
    replay_patch(
        ReplayPlan::new(
            report_id,
            CheckoutPlan::new(source, crate::checkout::CommitRef::Head),
            patch,
            sandbox_backend,
        ),
        run,
        sandbox,
    )
}

/// Replay a candidate patch inside an injected sandbox backend.
///
/// # Errors
///
/// Returns [`ReplayError`] if replay cannot complete safely.
pub fn replay_patch<S: Sandbox>(
    plan: ReplayPlan,
    run: &RunReportStore,
    sandbox: &S,
) -> Result<ReplayResult, ReplayError> {
    validate_report_id_match(&plan.report_id, run)?;
    let created_at = plan.created_at.clone();
    let timestamp = created_at
        .clone()
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned());

    let old_checkout = checkout_old_workspace(plan.checkout, run)?;
    append_audit(
        run,
        &timestamp,
        &plan.report_id,
        AuditEventTag::RepoLoaded,
        &audit_hash([
            "repo_loaded",
            &old_checkout.workspace_path,
            &old_checkout.commit,
        ]),
    )?;

    prepare_new_workspace(run)?;
    let patch_bytes = read_patch(plan.patch)?;
    let patch_hash = hash_bytes(&patch_bytes);
    let patch_path = "evidence/candidate.patch".to_owned();
    let patch_file = run.path().join(&patch_path);
    write_patch(&patch_file, &patch_bytes)?;

    let profile = build_profile(run, plan.sandbox_backend, &patch_file)?;
    let command = SandboxCommand::new("git")
        .with_arg("-C")
        .with_arg("/work")
        .with_arg("apply")
        .with_arg("/patch/candidate.patch");

    append_audit(
        run,
        &timestamp,
        &plan.report_id,
        AuditEventTag::SandboxStarted,
        &audit_hash(["sandbox_started", &patch_hash]),
    )?;

    let sandbox_result = sandbox.run(&profile, &command);
    match sandbox_result {
        Ok(_) => {}
        Err(SandboxError::NonZeroExit(code)) => {
            append_audit(
                run,
                &timestamp,
                &plan.report_id,
                AuditEventTag::SandboxDenied,
                &audit_hash(["sandbox_denied", &patch_hash, &code.to_string()]),
            )?;
            return Err(ReplayError::PatchRejected(format!(
                "git apply exited with code {code}"
            )));
        }
        Err(error) => {
            append_audit(
                run,
                &timestamp,
                &plan.report_id,
                AuditEventTag::SandboxDenied,
                &audit_hash(["sandbox_denied", &patch_hash]),
            )?;
            return Err(ReplayError::Sandbox(error));
        }
    }

    let _new_files = inventory_tree(&run.path().join("workspace").join("new"))?;
    let new_repo = ManifestRepoRef {
        repo_id: old_checkout.repo_id.clone(),
        commit: format!("patched:{}:{}", old_checkout.commit, patch_hash),
        workspace_path: "workspace/new".to_owned(),
    };
    let sandbox_profile = manifest_sandbox_profile(&profile);
    let mut manifest = RunManifest::new(&plan.report_id);
    manifest.old_repo = Some(ManifestRepoRef {
        repo_id: old_checkout.repo_id.clone(),
        commit: old_checkout.commit.clone(),
        workspace_path: old_checkout.workspace_path.clone(),
    });
    manifest.new_repo = Some(new_repo.clone());
    manifest.patch_hash = Some(patch_hash.clone());
    manifest.patch_path = Some(patch_path.clone());
    manifest.sandbox = Some(sandbox_profile.clone());
    manifest.created_at = created_at;
    manifest.write_to_report_store(run)?;

    append_audit(
        run,
        &timestamp,
        &plan.report_id,
        AuditEventTag::ReplayCompleted,
        &audit_hash(["replay_completed", &patch_hash]),
    )?;

    Ok(ReplayResult {
        old_checkout,
        new_repo: ManifestRepoRef {
            workspace_path: "workspace/new".to_owned(),
            ..new_repo
        },
        patch_hash,
        patch_path,
        sandbox_profile: SandboxProfile {
            env_keys: profile.env_keys.clone(),
            ..sandbox_profile
        },
    })
}

fn validate_report_id_match(report_id: &str, run: &RunReportStore) -> Result<(), ReplayError> {
    let run_id = run
        .path()
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| ReplayError::UnsafePath(run.path().display().to_string()))?;
    if report_id != run_id {
        return Err(ReplayError::ReportIdMismatch {
            plan: report_id.to_owned(),
            run: run_id.to_owned(),
        });
    }
    Ok(())
}

fn prepare_new_workspace(run: &RunReportStore) -> Result<PathBuf, ReplayError> {
    let run_root = run.path().canonicalize()?;
    let old_workspace = run_root.join("workspace").join("old");
    let new_workspace = run_root.join("workspace").join("new");
    if new_workspace.exists() {
        fs::remove_dir_all(&new_workspace)?;
    }
    fs::create_dir_all(&new_workspace)?;
    copy_tree(&old_workspace, &old_workspace, &new_workspace)?;
    Ok(new_workspace)
}

fn copy_tree(root: &Path, current: &Path, destination_root: &Path) -> Result<(), ReplayError> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(ReplayError::UnsafePath(path.display().to_string()));
        }
        let relative = safe_relative(root, &path)?;
        let destination = destination_root.join(&relative);
        if metadata.is_dir() {
            fs::create_dir_all(&destination)?;
            copy_tree(root, &path, destination_root)?;
        } else if metadata.is_file() {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &destination)?;
        } else {
            return Err(ReplayError::UnsafePath(path.display().to_string()));
        }
    }
    Ok(())
}

fn inventory_tree(root: &Path) -> Result<Vec<String>, ReplayError> {
    let mut files = BTreeSet::new();
    inventory_inner(root, root, &mut files)?;
    Ok(files.into_iter().collect())
}

fn inventory_inner(
    root: &Path,
    current: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), ReplayError> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(ReplayError::UnsafePath(path.display().to_string()));
        }
        let relative = safe_relative(root, &path)?;
        if metadata.is_dir() {
            inventory_inner(root, &path, files)?;
        } else if metadata.is_file() {
            files.insert(path_to_utf8(&relative)?);
        } else {
            return Err(ReplayError::UnsafePath(path.display().to_string()));
        }
    }
    Ok(())
}

fn path_to_utf8(path: &Path) -> Result<String, ReplayError> {
    let value = path
        .to_str()
        .ok_or_else(|| ReplayError::UnsafePath(path.display().to_string()))?;
    #[cfg(windows)]
    {
        Ok(value.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        Ok(value.to_owned())
    }
}

fn safe_relative(root: &Path, path: &Path) -> Result<PathBuf, ReplayError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ReplayError::UnsafePath(path.display().to_string()))?;
    validate_relative(relative)?;
    Ok(relative.to_path_buf())
}

fn validate_relative(path: &Path) -> Result<(), ReplayError> {
    if path.is_absolute() {
        return Err(ReplayError::UnsafePath(path.display().to_string()));
    }
    for component in path.components() {
        match component {
            Component::Normal(value) if value.to_str().is_some() => {}
            _ => return Err(ReplayError::UnsafePath(path.display().to_string())),
        }
    }
    Ok(())
}

fn read_patch(patch: PatchInput) -> Result<Vec<u8>, ReplayError> {
    match patch {
        PatchInput::Bytes(bytes) => Ok(bytes),
        PatchInput::File(path) => {
            if path.to_str().is_none() {
                return Err(ReplayError::UnsafePath(path.display().to_string()));
            }
            let mut handle = fs::File::open(path)?;
            let mut bytes = Vec::new();
            handle.read_to_end(&mut bytes)?;
            Ok(bytes)
        }
    }
}

fn write_patch(path: &Path, bytes: &[u8]) -> Result<(), ReplayError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn build_profile(
    run: &RunReportStore,
    backend: SandboxBackend,
    patch_file: &Path,
) -> Result<SandboxProfileSpec, ReplayError> {
    let old_workspace = run.path().join("workspace").join("old");
    let new_workspace = run.path().join("workspace").join("new");
    Ok(SandboxProfileSpec::default_no_network(backend)
        .with_mount(SandboxMount::read_only(old_workspace, "/old")?)
        .with_mount(SandboxMount::read_write(new_workspace, "/work", run)?)
        .with_mount(SandboxMount::read_only(
            patch_file,
            "/patch/candidate.patch",
        )?))
}

fn manifest_sandbox_profile(profile: &SandboxProfileSpec) -> SandboxProfile {
    SandboxProfile {
        backend: profile.backend,
        profile_id: format!("{:?}-no-network", profile.backend).to_ascii_lowercase(),
        network_allowed: profile.network_allowed,
        env_keys: profile.env_keys.clone(),
        cpu_seconds: Some(profile.limits.cpu_seconds),
        memory_bytes: Some(profile.limits.memory_bytes),
        tmpfs_bytes: Some(profile.limits.tmpfs_bytes),
    }
}

fn append_audit(
    run: &RunReportStore,
    timestamp: &str,
    report_id: &str,
    tag: AuditEventTag,
    payload_hash: &str,
) -> Result<(), ReplayAuditError> {
    let event = AuditEvent::new(
        timestamp,
        report_id,
        "replay",
        tag,
        "polyref-loader",
        payload_hash,
        Vec::new(),
    )?;
    run.append_audit_event(&event)?;
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn audit_hash<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}
