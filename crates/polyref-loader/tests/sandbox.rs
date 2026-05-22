#![allow(clippy::unwrap_used)]

use polyref_graph::ReportStore;
use polyref_loader::manifest::SandboxBackend;
use polyref_loader::sandbox::{
    BackendCommandBuilder, NoNetworkSandbox, Sandbox, SandboxCommand, SandboxError, SandboxLimits,
    SandboxMount, SandboxProfileSpec, SandboxResult, UnavailableSandbox,
};
use std::fs;
use std::path::Path;
use std::time::Duration;

#[test]
fn default_profile_matches_adr_009_limits_and_denies_network() {
    let profile = SandboxProfileSpec::default_no_network(SandboxBackend::Docker);

    assert_eq!(profile.backend, SandboxBackend::Docker);
    assert!(!profile.network_allowed);
    assert_eq!(profile.limits.cpu_seconds, 60);
    assert_eq!(profile.limits.wallclock_ms, 60_000);
    assert_eq!(profile.limits.memory_bytes, 1_073_741_824);
    assert_eq!(profile.limits.tmpfs_bytes, 268_435_456);
    assert!(profile.env_keys.is_empty());
}

#[test]
fn docker_podman_and_nsjail_builders_include_no_network_controls() {
    let run = TestRun::new("report-1");
    let source = temp_dir_with_file("src.txt");
    fs::create_dir_all(run.path().join("scratch")).unwrap();
    let docker_profile = profile_with_mounts(SandboxBackend::Docker, source.path(), &run);
    let podman_profile = profile_with_mounts(SandboxBackend::Podman, source.path(), &run);
    let nsjail_profile = profile_with_mounts(SandboxBackend::Nsjail, source.path(), &run);
    let command = SandboxCommand::new("true");

    let docker = BackendCommandBuilder::docker()
        .build(&docker_profile, &command)
        .unwrap();
    let podman = BackendCommandBuilder::podman()
        .build(&podman_profile, &command)
        .unwrap();
    let nsjail = BackendCommandBuilder::nsjail()
        .build(&nsjail_profile, &command)
        .unwrap();

    assert!(docker.args.contains(&"--network=none".to_owned()));
    assert!(docker.args.iter().any(|arg| arg.contains(":ro")));
    assert!(docker.args.contains(&"--cpus".to_owned()));
    assert!(docker.args.contains(&"--memory".to_owned()));
    assert!(docker.args.contains(&"--tmpfs".to_owned()));
    assert!(podman.args.contains(&"--network=none".to_owned()));
    assert!(podman.args.iter().any(|arg| arg.contains(":ro")));
    assert!(podman.args.contains(&"--cpus".to_owned()));
    assert!(podman.args.contains(&"--memory".to_owned()));
    assert!(podman.args.contains(&"--tmpfs".to_owned()));
    assert!(nsjail.args.contains(&"--disable_clone_newnet".to_owned()));
    assert!(nsjail.args.contains(&"--rlimit_cpu".to_owned()));
    assert!(nsjail.args.contains(&"--rlimit_as".to_owned()));
    assert!(nsjail.args.contains(&"--rlimit_fsize".to_owned()));
    let ro_index = nsjail
        .args
        .iter()
        .position(|arg| arg == "--bindmount_ro")
        .unwrap();
    assert!(nsjail.args[ro_index + 1].contains(":/src"));
    assert!(!nsjail.args[ro_index + 1].ends_with(":ro"));
}

#[test]
fn backend_builder_rejects_profile_backend_mismatch() {
    let profile = SandboxProfileSpec::default_no_network(SandboxBackend::Docker);
    let err = BackendCommandBuilder::nsjail()
        .build(&profile, &SandboxCommand::new("true"))
        .unwrap_err();

    assert!(matches!(err, SandboxError::Denied(_)));
}

#[test]
fn env_redaction_exposes_keys_but_never_values() {
    let command = SandboxCommand::new("printenv")
        .with_allowed_env("PATH", "/usr/bin")
        .with_allowed_env("OPENAI_API_KEY", "sk-secret")
        .with_allowed_env("HOME", "/Users/example");
    let profile = SandboxProfileSpec::default_no_network(SandboxBackend::Docker)
        .with_env_from_command(&command);

    assert_eq!(profile.env_keys, vec!["HOME", "OPENAI_API_KEY", "PATH"]);
    let debug = format!("{profile:?} {command:?}");
    assert!(debug.contains("OPENAI_API_KEY"));
    assert!(!debug.contains("sk-secret"));
    assert!(!debug.contains("/Users/example"));
}

#[test]
fn write_mount_outside_run_root_is_rejected_before_launch() {
    let run = TestRun::new("report-1");
    let outside = temp_dir_with_file("outside.txt");

    let err = SandboxMount::read_write(outside.path(), "/work", &run).unwrap_err();

    assert!(matches!(err, SandboxError::UnsafePath(_)));
}

#[test]
fn unsafe_mount_paths_are_rejected() {
    let run = TestRun::new("report-1");
    let source = temp_dir_with_file("src.txt");
    let missing = source.path().join("missing");

    assert!(matches!(
        SandboxMount::read_only(missing, "/src"),
        Err(SandboxError::UnsafePath(_))
    ));
    assert!(matches!(
        SandboxMount::read_only(source.path(), "../escape"),
        Err(SandboxError::UnsafePath(_))
    ));
    assert!(matches!(
        SandboxMount::read_write(run.path(), "relative-target", &run),
        Err(SandboxError::UnsafePath(_))
    ));
}

#[test]
fn symlink_source_mount_is_rejected() {
    let run = TestRun::new("report-1");
    let outside = temp_dir_with_file("secret.txt");
    let link = run.path().join("scratch-link");
    symlink_dir(outside.path(), &link);

    let err = SandboxMount::read_write(&link, "/work", &run).unwrap_err();

    assert!(matches!(err, SandboxError::UnsafePath(_)));
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_utf8_mount_source_is_rejected() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let run = TestRun::new("report-1");
    let source = run
        .path()
        .join(PathBuf::from(OsString::from_vec(vec![0xff])));
    fs::create_dir_all(&source).unwrap();

    let err = SandboxMount::read_write(&source, "/work", &run).unwrap_err();

    assert!(matches!(err, SandboxError::UnsafePath(_)));
}

#[test]
fn missing_backend_returns_typed_error() {
    let sandbox = UnavailableSandbox::new(SandboxBackend::Nsjail);
    let profile = SandboxProfileSpec::default_no_network(SandboxBackend::Nsjail);
    let command = SandboxCommand::new("true");

    let err = sandbox.run(&profile, &command).unwrap_err();

    assert!(matches!(
        err,
        SandboxError::MissingBackend(SandboxBackend::Nsjail)
    ));
}

#[test]
fn no_network_wrapper_forces_network_denied() {
    let sandbox = NoNetworkSandbox::new(FakeSandbox);
    let profile = SandboxProfileSpec {
        backend: SandboxBackend::Docker,
        network_allowed: true,
        mounts: Vec::new(),
        limits: SandboxLimits::default(),
        env_keys: Vec::new(),
    };

    let result = sandbox.run(&profile, &SandboxCommand::new("true")).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(!result.profile.network_allowed);
}

#[derive(Debug)]
struct FakeSandbox;

impl polyref_loader::sandbox::Sandbox for FakeSandbox {
    fn run(
        &self,
        profile: &SandboxProfileSpec,
        _command: &SandboxCommand,
    ) -> Result<SandboxResult, SandboxError> {
        Ok(SandboxResult {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            profile: profile.clone(),
        })
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

fn profile_with_mounts(
    backend: SandboxBackend,
    source_path: &Path,
    run: &polyref_graph::RunReportStore,
) -> SandboxProfileSpec {
    SandboxProfileSpec::default_no_network(backend)
        .with_mount(SandboxMount::read_only(source_path, "/src").unwrap())
        .with_mount(SandboxMount::read_write(run.path().join("scratch"), "/work", run).unwrap())
}

fn temp_dir_with_file(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(name), "content\n").unwrap();
    dir
}

#[cfg(unix)]
fn symlink_dir(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn symlink_dir(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}
