//! Reproducible local repository checkout for Layer 2.

use polyref_graph::RunReportStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use thiserror::Error;

/// Repository source accepted by the loader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RepoSource {
    /// Existing local repository path. No network fetch is performed.
    LocalPath(PathBuf),
}

/// Commit or snapshot mode for checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CommitRef {
    /// Resolve the current local `HEAD`.
    Head,
    /// Resolve a local exact commit hash or ref without fetching.
    Exact(String),
    /// Copy the current working tree after loader excludes.
    WorkingTreeSnapshot,
}

/// Plan for producing the old workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutPlan {
    /// Repository source.
    pub source: RepoSource,
    /// Commit or snapshot mode.
    pub commit: CommitRef,
    /// Report id this checkout belongs to.
    pub report_id: String,
}

/// Result of producing the old workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutResult {
    /// Stable repository id for this source.
    pub repo_id: String,
    /// Resolved commit hash or working-tree snapshot id.
    pub commit: String,
    /// Workspace path relative to the run directory.
    pub workspace_path: String,
    /// Deterministic sorted file inventory relative to the workspace.
    pub files: Vec<String>,
}

/// Checkout failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CheckoutError {
    /// Source repository does not exist.
    #[error("repository not found: {0}")]
    RepoNotFound(String),
    /// Path is unsafe or cannot be represented safely.
    #[error("unsafe path: {0}")]
    UnsafePath(String),
    /// A symlink points outside the source root.
    #[error("symlink escapes source root: {0}")]
    SymlinkEscape(String),
    /// Git command failed.
    #[error("git command failed: {0}")]
    Git(String),
    /// Host filesystem operation failed.
    #[error("checkout io error: {0}")]
    Io(#[from] std::io::Error),
}

impl CheckoutPlan {
    /// Create a checkout plan.
    #[must_use]
    pub fn new(source: RepoSource, commit: CommitRef, report_id: impl Into<String>) -> Self {
        Self {
            source,
            commit,
            report_id: report_id.into(),
        }
    }
}

/// Copy the selected local repository into `workspace/old`.
///
/// # Errors
///
/// Returns [`CheckoutError`] when the source, git ref, or filesystem
/// inventory is unsafe or unavailable.
pub fn checkout_old_workspace(
    plan: CheckoutPlan,
    run: &RunReportStore,
) -> Result<CheckoutResult, CheckoutError> {
    validate_one_segment(&plan.report_id)?;
    let RepoSource::LocalPath(source) = plan.source;
    let source = canonical_source(&source)?;
    let workspace = run.path().join("workspace").join("old");
    if workspace.exists() {
        fs::remove_dir_all(&workspace)?;
    }
    fs::create_dir_all(&workspace)?;

    let mut files = BTreeSet::new();
    copy_tree(&source, &source, &workspace, &mut files)?;
    let files = files.into_iter().collect::<Vec<_>>();
    let commit = resolve_commit(&source, &plan.commit, &files, &workspace)?;
    let repo_id = repo_id(&source);

    Ok(CheckoutResult {
        repo_id,
        commit,
        workspace_path: "workspace/old".to_owned(),
        files,
    })
}

fn canonical_source(source: &Path) -> Result<PathBuf, CheckoutError> {
    if !source.exists() {
        return Err(CheckoutError::RepoNotFound(source.display().to_string()));
    }
    if !source.is_dir() {
        return Err(CheckoutError::RepoNotFound(source.display().to_string()));
    }
    source.canonicalize().map_err(CheckoutError::Io)
}

fn copy_tree(
    root: &Path,
    current: &Path,
    workspace: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), CheckoutError> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        if should_exclude(&name) {
            continue;
        }

        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            guard_symlink(root, &path)?;
        }

        let relative = safe_relative(root, &path)?;
        let destination = workspace.join(&relative);
        if metadata.is_dir() {
            fs::create_dir_all(&destination)?;
            copy_tree(root, &path, workspace, files)?;
        } else if metadata.is_file() {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &destination)?;
            files.insert(path_to_utf8(&relative)?);
        }
    }
    Ok(())
}

fn should_exclude(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git" | ".polyref" | "target" | ".cache" | "node_modules")
    )
}

fn guard_symlink(root: &Path, path: &Path) -> Result<(), CheckoutError> {
    let target = path.canonicalize()?;
    if target.starts_with(root) {
        Ok(())
    } else {
        Err(CheckoutError::SymlinkEscape(path.display().to_string()))
    }
}

fn safe_relative(root: &Path, path: &Path) -> Result<PathBuf, CheckoutError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| CheckoutError::UnsafePath(path.display().to_string()))?;
    validate_relative(relative)?;
    Ok(relative.to_path_buf())
}

fn validate_one_segment(value: &str) -> Result<(), CheckoutError> {
    if value.is_empty() {
        return Err(CheckoutError::UnsafePath("empty report id".to_owned()));
    }
    validate_relative(Path::new(value))?;
    if Path::new(value).components().count() != 1 {
        return Err(CheckoutError::UnsafePath(value.to_owned()));
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<(), CheckoutError> {
    if path.is_absolute() {
        return Err(CheckoutError::UnsafePath(path.display().to_string()));
    }
    for component in path.components() {
        match component {
            Component::Normal(part) if part.to_str().is_some() => {}
            _ => return Err(CheckoutError::UnsafePath(path.display().to_string())),
        }
    }
    Ok(())
}

fn path_to_utf8(path: &Path) -> Result<String, CheckoutError> {
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| CheckoutError::UnsafePath(path.display().to_string()))
}

fn resolve_commit(
    source: &Path,
    commit: &CommitRef,
    files: &[String],
    workspace: &Path,
) -> Result<String, CheckoutError> {
    match commit {
        CommitRef::Head => git_rev_parse(source, "HEAD"),
        CommitRef::Exact(value) => git_rev_parse(source, value),
        CommitRef::WorkingTreeSnapshot => working_tree_id(files, workspace),
    }
}

fn git_rev_parse(source: &Path, value: &str) -> Result<String, CheckoutError> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", value])
        .current_dir(source)
        .output()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map(|stdout| stdout.trim().to_owned())
            .map_err(|err| CheckoutError::Git(err.to_string()))
    } else {
        Err(CheckoutError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

fn working_tree_id(files: &[String], workspace: &Path) -> Result<String, CheckoutError> {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.as_bytes());
        hasher.update([0]);
        let mut handle = fs::File::open(workspace.join(file))?;
        let mut bytes = Vec::new();
        handle.read_to_end(&mut bytes)?;
        hasher.update(bytes);
        hasher.update([0]);
    }
    Ok(format!("working-tree:{:x}", hasher.finalize()))
}

fn repo_id(source: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.to_string_lossy().as_bytes());
    format!("local:{:x}", hasher.finalize())
}
