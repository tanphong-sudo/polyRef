#![allow(clippy::unwrap_used)]

use polyref_graph::ReportStore;
use polyref_loader::checkout::{
    checkout_old_workspace, CheckoutError, CheckoutPlan, CommitRef, RepoSource,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn copies_local_repo_into_old_workspace_with_sorted_inventory() {
    let repo = sample_repo();
    write_file(
        repo.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    );
    write_file(repo.path().join("README.md"), "# sample\n");
    write_file(repo.path().join(".polyref/runs/old/report.json"), "{}\n");
    write_file(repo.path().join("target/debug/cache"), "ignored\n");
    git(&repo, ["add", "README.md", "src/lib.rs"]);
    git(&repo, ["commit", "-m", "initial"]);
    let commit = git_stdout(&repo, ["rev-parse", "HEAD"]);
    let run = run_store("report-1");

    let result = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(repo.path().to_path_buf()),
            CommitRef::Head,
        ),
        &run,
    )
    .unwrap();

    assert_eq!(result.workspace_path, "workspace/old");
    assert_eq!(result.commit, commit);
    assert_eq!(result.files, vec!["README.md", "src/lib.rs"]);
    assert_eq!(
        fs::read_to_string(run.path().join("workspace/old/src/lib.rs")).unwrap(),
        "pub fn answer() -> u8 { 42 }\n"
    );
    assert!(!run.path().join("workspace/old/.git").exists());
    assert!(!run.path().join("workspace/old/.polyref").exists());
    assert!(!run.path().join("workspace/old/target").exists());
}

#[test]
fn exact_commit_resolves_without_network() {
    let repo = sample_repo();
    write_file(repo.path().join("a.txt"), "a\n");
    git(&repo, ["add", "a.txt"]);
    git(&repo, ["commit", "-m", "initial"]);
    let commit = git_stdout(&repo, ["rev-parse", "HEAD"]);
    let run = run_store("report-1");

    let result = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(repo.path().to_path_buf()),
            CommitRef::Exact(commit.clone()),
        ),
        &run,
    )
    .unwrap();

    assert_eq!(result.commit, commit);
    assert_eq!(result.files, vec!["a.txt"]);
}

#[test]
fn commit_checkout_excludes_tracked_cache_dirs() {
    let repo = sample_repo();
    write_file(repo.path().join("README.md"), "kept\n");
    write_file(repo.path().join(".polyref/runs/report.json"), "ignored\n");
    write_file(repo.path().join("target/debug/cache"), "ignored\n");
    write_file(repo.path().join(".cache/tool/cache"), "ignored\n");
    write_file(repo.path().join("node_modules/pkg/index.js"), "ignored\n");
    git(&repo, ["add", "."]);
    git(&repo, ["commit", "-m", "initial"]);
    let commit = git_stdout(&repo, ["rev-parse", "HEAD"]);
    let run = run_store("report-1");

    let result = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(repo.path().to_path_buf()),
            CommitRef::Head,
        ),
        &run,
    )
    .unwrap();

    assert_eq!(result.commit, commit);
    assert_eq!(result.files, vec!["README.md"]);
    assert!(run.path().join("workspace/old/README.md").exists());
    assert!(!run.path().join("workspace/old/.polyref").exists());
    assert!(!run.path().join("workspace/old/target").exists());
    assert!(!run.path().join("workspace/old/.cache").exists());
    assert!(!run.path().join("workspace/old/node_modules").exists());
}

#[test]
fn exact_commit_ignores_dirty_working_tree() {
    let repo = sample_repo();
    write_file(repo.path().join("a.txt"), "committed\n");
    git(&repo, ["add", "a.txt"]);
    git(&repo, ["commit", "-m", "initial"]);
    let commit = git_stdout(&repo, ["rev-parse", "HEAD"]);
    write_file(repo.path().join("a.txt"), "dirty\n");
    write_file(repo.path().join("untracked.txt"), "untracked\n");
    let run = run_store("report-1");

    let result = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(repo.path().to_path_buf()),
            CommitRef::Exact(commit),
        ),
        &run,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(run.path().join("workspace/old/a.txt")).unwrap(),
        "committed\n"
    );
    assert_eq!(result.files, vec!["a.txt"]);
    assert!(!run.path().join("workspace/old/untracked.txt").exists());
}

#[test]
fn exact_commit_can_checkout_non_head_commit() {
    let repo = sample_repo();
    write_file(repo.path().join("a.txt"), "v1\n");
    git(&repo, ["add", "a.txt"]);
    git(&repo, ["commit", "-m", "first"]);
    let first = git_stdout(&repo, ["rev-parse", "HEAD"]);
    write_file(repo.path().join("a.txt"), "v2\n");
    git(&repo, ["add", "a.txt"]);
    git(&repo, ["commit", "-m", "second"]);
    let run = run_store("report-1");

    checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(repo.path().to_path_buf()),
            CommitRef::Exact(first),
        ),
        &run,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(run.path().join("workspace/old/a.txt")).unwrap(),
        "v1\n"
    );
}

#[test]
fn working_tree_snapshot_includes_untracked_files_with_stable_id() {
    let repo = sample_repo();
    write_file(repo.path().join("tracked.txt"), "tracked\n");
    git(&repo, ["add", "tracked.txt"]);
    git(&repo, ["commit", "-m", "initial"]);
    write_file(repo.path().join("untracked.txt"), "untracked\n");
    let run = run_store("report-1");

    let first = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(repo.path().to_path_buf()),
            CommitRef::WorkingTreeSnapshot,
        ),
        &run,
    )
    .unwrap();

    let second = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(repo.path().to_path_buf()),
            CommitRef::WorkingTreeSnapshot,
        ),
        &run,
    )
    .unwrap();

    assert_eq!(first.commit, second.commit);
    assert!(first.commit.starts_with("working-tree:"));
    assert_eq!(first.files, vec!["tracked.txt", "untracked.txt"]);
}

#[test]
fn rejects_nonexistent_repo() {
    let run = run_store("report-1");
    let err = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(PathBuf::from("/definitely/not/polyref/repo")),
            CommitRef::Head,
        ),
        &run,
    )
    .unwrap_err();

    assert!(matches!(err, CheckoutError::RepoNotFound(_)));
}

#[test]
fn rejects_symlink_escape() {
    let repo = sample_repo();
    let outside = tempfile::tempdir().unwrap();
    write_file(outside.path().join("secret.txt"), "secret\n");
    symlink_file(
        outside.path().join("secret.txt"),
        repo.path().join("secret-link"),
    );
    let run = run_store("report-1");

    let err = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(repo.path().to_path_buf()),
            CommitRef::WorkingTreeSnapshot,
        ),
        &run,
    )
    .unwrap_err();

    assert!(matches!(err, CheckoutError::SymlinkEscape(_)));
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn rejects_non_utf8_source_path() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let parent = tempfile::tempdir().unwrap();
    let source = parent
        .path()
        .join(PathBuf::from(OsString::from_vec(vec![b'r', 0xff])));
    fs::create_dir_all(&source).unwrap();
    let run = run_store("report-1");

    let err = checkout_old_workspace(
        CheckoutPlan::new(
            RepoSource::LocalPath(source),
            CommitRef::WorkingTreeSnapshot,
        ),
        &run,
    )
    .unwrap_err();

    assert!(matches!(err, CheckoutError::UnsafePath(_)));
}

fn run_store(report_id: &str) -> polyref_graph::RunReportStore {
    let dir = tempfile::tempdir().unwrap().into_path();
    ReportStore::open(dir)
        .unwrap()
        .create_run(report_id)
        .unwrap()
}

fn sample_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(&repo, ["init"]);
    git(&repo, ["config", "user.email", "polyref@example.invalid"]);
    git(&repo, ["config", "user.name", "PolyRef"]);
    repo
}

fn write_file(path: impl AsRef<Path>, contents: &str) {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn git<const N: usize>(repo: &tempfile::TempDir, args: [&str; N]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo.path())
        .status()
        .unwrap();
    assert!(status.success());
}

fn git_stdout<const N: usize>(repo: &tempfile::TempDir, args: [&str; N]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[cfg(unix)]
fn symlink_file(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn symlink_file(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    std::os::windows::fs::symlink_file(target, link).unwrap();
}
