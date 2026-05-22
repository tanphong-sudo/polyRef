#![allow(clippy::unwrap_used)]

use polyref_graph::{AuditEventTag, AuditReader, ReportStore};
use polyref_loader::checkout::{CheckoutPlan, CommitRef, RepoSource};
use polyref_loader::manifest::{RunManifest, SandboxBackend};
use polyref_loader::replay::{
    load_repo_with_patch, replay_patch, PatchInput, ReplayError, ReplayPlan,
};
use polyref_loader::sandbox::{
    MountAccess, Sandbox, SandboxCommand, SandboxError, SandboxProfileSpec, SandboxResult,
};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[test]
fn replay_applies_patch_to_new_workspace_and_preserves_old_workspace() {
    let repo = sample_repo();
    write_file(repo.path().join("README.md"), "old\n");
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "initial"]);
    let run = TestRun::new("report-1");
    let patch = patch_for_readme("old", "new");

    let result = replay_patch(
        ReplayPlan::new(
            "report-1",
            CheckoutPlan::new(
                RepoSource::LocalPath(repo.path().to_path_buf()),
                CommitRef::Head,
            ),
            PatchInput::Bytes(patch.into_bytes()),
            SandboxBackend::Unavailable,
        )
        .with_created_at("2026-05-22T00:00:00Z"),
        &run,
        &PatchApplyingSandbox,
    )
    .unwrap();

    assert_eq!(result.old_checkout.workspace_path, "workspace/old");
    assert_eq!(result.new_repo.workspace_path, "workspace/new");
    assert_eq!(result.patch_path, "evidence/candidate.patch");
    assert_eq!(
        fs::read_to_string(run.path().join("workspace/old/README.md")).unwrap(),
        "old\n"
    );
    assert_eq!(
        fs::read_to_string(run.path().join("workspace/new/README.md")).unwrap(),
        "new\n"
    );
    assert_eq!(result.sandbox_profile.env_keys, Vec::<String>::new());
    assert!(!result.sandbox_profile.network_allowed);

    let manifest: RunManifest =
        serde_json::from_slice(&fs::read(run.path().join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest.old_repo.unwrap().workspace_path, "workspace/old");
    assert_eq!(manifest.new_repo.unwrap().workspace_path, "workspace/new");
    assert_eq!(manifest.patch_hash.unwrap(), result.patch_hash);
    assert_eq!(manifest.patch_path.unwrap(), "evidence/candidate.patch");
    assert_eq!(manifest.sandbox.unwrap().env_keys, Vec::<String>::new());

    let tags = audit_tags(&run);
    assert_eq!(
        tags,
        vec![
            AuditEventTag::RepoLoaded,
            AuditEventTag::SandboxStarted,
            AuditEventTag::ReplayCompleted
        ]
    );
}

#[test]
fn replay_without_created_at_does_not_write_fake_manifest_timestamp() {
    let repo = sample_repo();
    write_file(repo.path().join("README.md"), "old\n");
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "initial"]);
    let run = TestRun::new("report-1");

    replay_patch(
        ReplayPlan::new(
            "report-1",
            CheckoutPlan::new(
                RepoSource::LocalPath(repo.path().to_path_buf()),
                CommitRef::Head,
            ),
            PatchInput::Bytes(patch_for_readme("old", "new").into_bytes()),
            SandboxBackend::Unavailable,
        ),
        &run,
        &PatchApplyingSandbox,
    )
    .unwrap();

    let manifest: RunManifest =
        serde_json::from_slice(&fs::read(run.path().join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest.created_at, None);
}

#[test]
fn invalid_patch_returns_typed_error_and_preserves_old_workspace() {
    let repo = sample_repo();
    write_file(repo.path().join("README.md"), "old\n");
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "initial"]);
    let run = TestRun::new("report-1");

    let err = replay_patch(
        ReplayPlan::new(
            "report-1",
            CheckoutPlan::new(
                RepoSource::LocalPath(repo.path().to_path_buf()),
                CommitRef::Head,
            ),
            PatchInput::Bytes(b"not a unified diff\n".to_vec()),
            SandboxBackend::Unavailable,
        ),
        &run,
        &PatchApplyingSandbox,
    )
    .unwrap_err();

    assert!(matches!(err, ReplayError::PatchRejected(_)));
    assert_eq!(
        fs::read_to_string(run.path().join("workspace/old/README.md")).unwrap(),
        "old\n"
    );
    assert_eq!(
        fs::read_to_string(run.path().join("workspace/new/README.md")).unwrap(),
        "old\n"
    );
    assert!(audit_tags(&run).contains(&AuditEventTag::SandboxDenied));
}

#[test]
fn replay_rejects_symlink_left_by_candidate_patch() {
    let repo = sample_repo();
    write_file(repo.path().join("README.md"), "old\n");
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "initial"]);
    let run = TestRun::new("report-1");

    let err = replay_patch(
        ReplayPlan::new(
            "report-1",
            CheckoutPlan::new(
                RepoSource::LocalPath(repo.path().to_path_buf()),
                CommitRef::Head,
            ),
            PatchInput::Bytes(symlink_patch().into_bytes()),
            SandboxBackend::Unavailable,
        ),
        &run,
        &PatchApplyingSandbox,
    )
    .unwrap_err();

    assert!(matches!(err, ReplayError::UnsafePath(_)));
}

#[test]
fn report_id_mismatch_is_rejected_before_checkout() {
    let repo = sample_repo();
    write_file(repo.path().join("README.md"), "old\n");
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "initial"]);
    let run = TestRun::new("actual-report");

    let err = replay_patch(
        ReplayPlan::new(
            "different-report",
            CheckoutPlan::new(
                RepoSource::LocalPath(repo.path().to_path_buf()),
                CommitRef::Head,
            ),
            PatchInput::Bytes(patch_for_readme("old", "new").into_bytes()),
            SandboxBackend::Unavailable,
        ),
        &run,
        &PatchApplyingSandbox,
    )
    .unwrap_err();

    assert!(matches!(err, ReplayError::ReportIdMismatch { .. }));
    assert!(!run.path().join("workspace").exists());
}

#[test]
fn missing_backend_fails_closed_without_host_patch_fallback() {
    let repo = sample_repo();
    write_file(repo.path().join("README.md"), "old\n");
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "initial"]);
    let run = TestRun::new("report-1");

    let err = load_repo_with_patch(
        "report-1",
        RepoSource::LocalPath(repo.path().to_path_buf()),
        PatchInput::Bytes(patch_for_readme("old", "new").into_bytes()),
        SandboxBackend::Unavailable,
        &run,
        &polyref_loader::sandbox::UnavailableSandbox::new(SandboxBackend::Unavailable),
    )
    .unwrap_err();

    assert!(matches!(
        err,
        ReplayError::Sandbox(SandboxError::MissingBackend(SandboxBackend::Unavailable))
    ));
    assert_eq!(
        fs::read_to_string(run.path().join("workspace/old/README.md")).unwrap(),
        "old\n"
    );
    assert_eq!(
        fs::read_to_string(run.path().join("workspace/new/README.md")).unwrap(),
        "old\n"
    );
    assert!(audit_tags(&run).contains(&AuditEventTag::SandboxDenied));
}

#[test]
fn sandbox_denials_for_network_secret_and_root_write_are_logged() {
    for fixture in [
        "curl example.com",
        "cat /etc/passwd",
        "mkdir /polyref-denied",
    ] {
        let repo = sample_repo();
        write_file(repo.path().join("README.md"), "old\n");
        git(&repo, ["add", "README.md"]);
        git(&repo, ["commit", "-m", "initial"]);
        let report_id = format!("report-{}", fixture.split_whitespace().next().unwrap());
        let run = TestRun::new(&report_id);

        let err = replay_patch(
            ReplayPlan::new(
                &report_id,
                CheckoutPlan::new(
                    RepoSource::LocalPath(repo.path().to_path_buf()),
                    CommitRef::Head,
                ),
                PatchInput::Bytes(fixture.as_bytes().to_vec()),
                SandboxBackend::Unavailable,
            ),
            &run,
            &DenyFixtureSandbox,
        )
        .unwrap_err();

        assert!(matches!(err, ReplayError::Sandbox(SandboxError::Denied(_))));
        assert!(audit_tags(&run).contains(&AuditEventTag::SandboxDenied));
    }
}

#[derive(Debug)]
struct PatchApplyingSandbox;

impl Sandbox for PatchApplyingSandbox {
    fn run(
        &self,
        profile: &SandboxProfileSpec,
        command: &SandboxCommand,
    ) -> Result<SandboxResult, SandboxError> {
        assert!(!profile.network_allowed);
        assert!(profile.env_keys.is_empty());
        assert_eq!(command.program(), "git");
        assert_eq!(
            command.args(),
            ["-C", "/work", "apply", "/patch/candidate.patch"]
        );
        assert!(profile
            .mounts
            .iter()
            .any(|mount| mount.target == "/old" && mount.access == MountAccess::ReadOnly));
        let work = mount_source(profile, "/work");
        let patch = mount_source(profile, "/patch/candidate.patch");
        let output = Command::new("git")
            .args([
                "-C",
                work.to_str().unwrap(),
                "apply",
                patch.to_str().unwrap(),
            ])
            .output()
            .map_err(SandboxError::Io)?;
        if !output.status.success() {
            return Err(SandboxError::NonZeroExit(output.status.code().unwrap_or(1)));
        }
        Ok(SandboxResult {
            exit_code: 0,
            stdout: output.stdout,
            stderr: output.stderr,
            duration: Duration::from_millis(1),
            profile: profile.clone(),
        })
    }
}

#[derive(Debug)]
struct DenyFixtureSandbox;

impl Sandbox for DenyFixtureSandbox {
    fn run(
        &self,
        _profile: &SandboxProfileSpec,
        _command: &SandboxCommand,
    ) -> Result<SandboxResult, SandboxError> {
        Err(SandboxError::Denied("fixture denied".to_owned()))
    }
}

struct TestRun {
    _dir: tempfile::TempDir,
    run: polyref_graph::RunReportStore,
}

impl TestRun {
    fn new(report_id: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let run = ReportStore::open(dir.path())
            .unwrap()
            .create_run(report_id)
            .unwrap();
        Self { _dir: dir, run }
    }

    fn path(&self) -> &Path {
        self.run.path()
    }
}

impl std::ops::Deref for TestRun {
    type Target = polyref_graph::RunReportStore;

    fn deref(&self) -> &Self::Target {
        &self.run
    }
}

fn sample_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(&dir, ["init", "-b", "main"]);
    git(&dir, ["config", "user.email", "test@example.com"]);
    git(&dir, ["config", "user.name", "Test User"]);
    dir
}

fn write_file(path: impl AsRef<Path>, contents: &str) {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn git<const N: usize>(repo: &tempfile::TempDir, args: [&str; N]) {
    let status = Command::new("git")
        .current_dir(repo.path())
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn patch_for_readme(old: &str, new: &str) -> String {
    format!(
        "diff --git a/README.md b/README.md\nindex 0000000..1111111 100644\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-{old}\n+{new}\n"
    )
}

fn mount_source<'a>(profile: &'a SandboxProfileSpec, target: &str) -> &'a Path {
    profile
        .mounts
        .iter()
        .find(|mount| mount.target == target)
        .map(|mount| mount.source.as_path())
        .unwrap()
}

fn audit_tags(run: &polyref_graph::RunReportStore) -> Vec<AuditEventTag> {
    AuditReader::open(run.path().join("audit.ndjson"))
        .unwrap()
        .map(|event| event.unwrap().tag)
        .collect()
}

fn symlink_patch() -> String {
    "diff --git a/leak b/leak\nnew file mode 120000\nindex 0000000..1111111\n--- /dev/null\n+++ b/leak\n@@ -0,0 +1 @@\n+/etc/passwd\n".to_owned()
}
