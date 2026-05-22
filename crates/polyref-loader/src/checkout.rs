//! Reproducible local repository checkout for Layer 2.

use polyref_graph::RunReportStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
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
    pub fn new(source: RepoSource, commit: CommitRef) -> Self {
        Self { source, commit }
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
    let RepoSource::LocalPath(source) = plan.source;
    let source = canonical_source(&source)?;
    let workspace = run.path().join("workspace").join("old");
    if workspace.exists() {
        fs::remove_dir_all(&workspace)?;
    }
    fs::create_dir_all(&workspace)?;

    let (commit, files) = materialize_checkout(&source, &plan.commit, &workspace)?;
    let repo_id = repo_id(&source)?;

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
    let source = source.canonicalize().map_err(CheckoutError::Io)?;
    if source.to_str().is_none() {
        return Err(CheckoutError::UnsafePath(source.display().to_string()));
    }
    Ok(source)
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
            return Err(CheckoutError::SymlinkEscape(path.display().to_string()));
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

fn should_exclude_relative(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => should_exclude(name),
        _ => false,
    })
}

fn safe_relative(root: &Path, path: &Path) -> Result<PathBuf, CheckoutError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| CheckoutError::UnsafePath(path.display().to_string()))?;
    validate_relative(relative)?;
    Ok(relative.to_path_buf())
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
        .map(normalize_path_separators)
        .ok_or_else(|| CheckoutError::UnsafePath(path.display().to_string()))
}

#[cfg(windows)]
fn normalize_path_separators(value: &str) -> String {
    value.replace('\\', "/")
}

#[cfg(not(windows))]
fn normalize_path_separators(value: &str) -> String {
    value.to_owned()
}

fn materialize_checkout(
    source: &Path,
    commit: &CommitRef,
    workspace: &Path,
) -> Result<(String, Vec<String>), CheckoutError> {
    match commit {
        CommitRef::Head => {
            let resolved = git_rev_parse(source, "HEAD")?;
            let files = export_commit_tree(source, &resolved, workspace)?;
            Ok((resolved, files))
        }
        CommitRef::Exact(value) => {
            let resolved = git_rev_parse(source, value)?;
            let files = export_commit_tree(source, &resolved, workspace)?;
            Ok((resolved, files))
        }
        CommitRef::WorkingTreeSnapshot => {
            let mut files = BTreeSet::new();
            copy_tree(source, source, workspace, &mut files)?;
            let files = files.into_iter().collect::<Vec<_>>();
            let commit = working_tree_id(&files, workspace)?;
            Ok((commit, files))
        }
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

fn export_commit_tree(
    source: &Path,
    commit: &str,
    workspace: &Path,
) -> Result<Vec<String>, CheckoutError> {
    let output = Command::new("git")
        .args(["ls-tree", "-rz", "-r", commit])
        .current_dir(source)
        .output()?;
    if !output.status.success() {
        return Err(CheckoutError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }

    let mut files = BTreeSet::new();
    for entry in output.stdout.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let tab = entry
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| CheckoutError::Git("malformed git ls-tree output".to_owned()))?;
        let (metadata, path_with_tab) = entry.split_at(tab);
        let path = &path_with_tab[1..];
        let metadata =
            std::str::from_utf8(metadata).map_err(|err| CheckoutError::Git(err.to_string()))?;
        let mut parts = metadata.split_ascii_whitespace();
        let mode = parts
            .next()
            .ok_or_else(|| CheckoutError::Git("missing git object mode".to_owned()))?;
        let kind = parts
            .next()
            .ok_or_else(|| CheckoutError::Git("missing git object kind".to_owned()))?;
        let object = parts
            .next()
            .ok_or_else(|| CheckoutError::Git("missing git object id".to_owned()))?;
        let relative = path_from_git_bytes(path)?;
        validate_relative(&relative)?;
        if should_exclude_relative(&relative) {
            continue;
        }
        if mode == "120000" || kind == "commit" {
            return Err(CheckoutError::SymlinkEscape(path_to_utf8(&relative)?));
        }
        if kind != "blob" {
            return Err(CheckoutError::Git(format!(
                "unsupported git object kind: {kind}"
            )));
        }

        let destination = workspace.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        write_git_blob(source, object, &destination)?;
        files.insert(path_to_utf8(&relative)?);
    }
    Ok(files.into_iter().collect())
}

#[cfg(unix)]
fn path_from_git_bytes(bytes: &[u8]) -> Result<PathBuf, CheckoutError> {
    use std::os::unix::ffi::OsStrExt;

    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn path_from_git_bytes(bytes: &[u8]) -> Result<PathBuf, CheckoutError> {
    let value = std::str::from_utf8(bytes).map_err(|err| CheckoutError::Git(err.to_string()))?;
    Ok(PathBuf::from(value))
}

fn write_git_blob(source: &Path, object: &str, destination: &Path) -> Result<(), CheckoutError> {
    let mut child = Command::new("git")
        .args(["cat-file", "-p", object])
        .current_dir(source)
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| CheckoutError::Git("missing git cat-file stdout".to_owned()))?;
    let mut file = fs::File::create(destination)?;
    std::io::copy(&mut stdout, &mut file)?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(CheckoutError::Git(format!(
            "git cat-file failed for object {object}"
        )))
    }
}

fn working_tree_id(files: &[String], workspace: &Path) -> Result<String, CheckoutError> {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.as_bytes());
        hasher.update([0]);
        let mut handle = fs::File::open(workspace.join(file))?;
        let mut buffer = [0_u8; 8192];
        loop {
            let read = handle.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        hasher.update([0]);
    }
    Ok(format!("working-tree:{:x}", hasher.finalize()))
}

fn repo_id(source: &Path) -> Result<String, CheckoutError> {
    let mut hasher = Sha256::new();
    let source = source
        .to_str()
        .ok_or_else(|| CheckoutError::UnsafePath(source.display().to_string()))?;
    hasher.update(source.as_bytes());
    Ok(format!("local:{:x}", hasher.finalize()))
}
