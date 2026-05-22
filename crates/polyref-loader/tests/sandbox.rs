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
    let run = run_store("report-1");
    let source = temp_dir_with_file("src.txt");
    fs::create_dir_all(run.path().join("scratch")).unwrap();
    let profile = SandboxProfileSpec::default_no_network(SandboxBackend::Docker)
        .with_mount(SandboxMount::read_only(source.path(), "/src").unwrap())
        .with_mount(SandboxMount::read_write(run.path().join("scratch"), "/work", &run).unwrap());
    let command = SandboxCommand::new("true");

    let docker = BackendCommandBuilder::docker()
        .build(&profile, &command)
        .unwrap();
    let podman = BackendCommandBuilder::podman()
        .build(&profile, &command)
        .unwrap();
    let nsjail = BackendCommandBuilder::nsjail()
        .build(&profile, &command)
        .unwrap();

    assert!(docker.args.contains(&"--network=none".to_owned()));
    assert!(docker.args.iter().any(|arg| arg.contains(":ro")));
    assert!(podman.args.contains(&"--network=none".to_owned()));
    assert!(podman.args.iter().any(|arg| arg.contains(":ro")));
    assert!(nsjail.args.contains(&"--disable_clone_newnet".to_owned()));
    assert!(nsjail.args.iter().any(|arg| arg.contains(":ro")));
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
    let run = run_store("report-1");
    let outside = temp_dir_with_file("outside.txt");

    let err = SandboxMount::read_write(outside.path(), "/work", &run).unwrap_err();

    assert!(matches!(err, SandboxError::UnsafePath(_)));
}

#[test]
fn unsafe_mount_paths_are_rejected() {
    let run = run_store("report-1");
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
    let run = run_store("report-1");
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
    use std::path::PathBuf;

    let run = run_store("report-1");
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

fn run_store(report_id: &str) -> polyref_graph::RunReportStore {
    let dir = tempfile::tempdir().unwrap().into_path();
    ReportStore::open(dir)
        .unwrap()
        .create_run(report_id)
        .unwrap()
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
