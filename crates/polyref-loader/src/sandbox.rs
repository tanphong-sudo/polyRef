//! Sandbox profile validation and backend command builders.

use crate::manifest::SandboxBackend;
use polyref_graph::RunReportStore;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

/// Command requested inside a sandbox.
#[derive(Clone, PartialEq, Eq)]
pub struct SandboxCommand {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    stdin: Option<Vec<u8>>,
    env: BTreeMap<String, String>,
}

/// Mount access policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MountAccess {
    /// Source is mounted read-only.
    ReadOnly,
    /// Source is mounted read-write.
    ReadWrite,
}

/// A validated sandbox mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxMount {
    /// Host source path.
    pub source: PathBuf,
    /// Sandbox target path.
    pub target: String,
    /// Access policy.
    pub access: MountAccess,
}

/// Sandbox resource limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxLimits {
    /// Per-call CPU time in seconds.
    pub cpu_seconds: u64,
    /// Per-call wallclock limit in milliseconds.
    pub wallclock_ms: u64,
    /// Per-call memory limit in bytes.
    pub memory_bytes: u64,
    /// Per-call writable tmpfs limit in bytes.
    pub tmpfs_bytes: u64,
}

/// Validated sandbox profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxProfileSpec {
    /// Backend selected for this profile.
    pub backend: SandboxBackend,
    /// Whether network is allowed.
    pub network_allowed: bool,
    /// Validated mounts.
    pub mounts: Vec<SandboxMount>,
    /// Resource limits.
    pub limits: SandboxLimits,
    /// Environment keys passed to the sandbox; values are never stored here.
    pub env_keys: Vec<String>,
}

/// Result returned by a sandbox backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxResult {
    /// Process exit code.
    pub exit_code: i32,
    /// Captured stdout bytes.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes.
    pub stderr: Vec<u8>,
    /// Measured duration.
    pub duration: Duration,
    /// Effective profile used by the backend.
    pub profile: SandboxProfileSpec,
}

/// Backend command generated from a sandbox profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCommand {
    /// Host backend executable.
    pub program: String,
    /// Backend arguments.
    pub args: Vec<String>,
}

/// Sandbox failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SandboxError {
    /// Sandbox denied the operation before or during launch.
    #[error("sandbox denied: {0}")]
    Denied(String),
    /// Sandbox exceeded a configured deadline.
    #[error("sandbox timed out")]
    Timeout,
    /// Process exited non-zero.
    #[error("sandbox command exited with code {0}")]
    NonZeroExit(i32),
    /// Requested backend is unavailable.
    #[error("sandbox backend unavailable: {0:?}")]
    MissingBackend(SandboxBackend),
    /// Path is unsafe or cannot be represented safely.
    #[error("unsafe sandbox path: {0}")]
    UnsafePath(String),
    /// Host filesystem operation failed.
    #[error("sandbox io error: {0}")]
    Io(#[from] std::io::Error),
}

/// A sandbox backend.
pub trait Sandbox {
    /// Run a command under the supplied profile.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError`] when validation or execution fails.
    fn run(
        &self,
        profile: &SandboxProfileSpec,
        command: &SandboxCommand,
    ) -> Result<SandboxResult, SandboxError>;
}

/// Wrapper that forces `network_allowed = false`.
#[derive(Debug, Clone)]
pub struct NoNetworkSandbox<S> {
    inner: S,
}

/// Sandbox backend that always reports a missing backend.
#[derive(Debug, Clone, Copy)]
pub struct UnavailableSandbox {
    backend: SandboxBackend,
}

/// Backend command builder for tests and later process launch.
#[derive(Debug, Clone, Copy)]
pub struct BackendCommandBuilder {
    backend: SandboxBackend,
}

impl SandboxCommand {
    /// Create a command with no args, stdin, cwd, or env.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            stdin: None,
            env: BTreeMap::new(),
        }
    }

    /// Add one argument.
    #[must_use]
    pub fn with_arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add an explicitly allowed environment key/value.
    #[must_use]
    pub fn with_allowed_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Return sorted allowlisted environment keys.
    #[must_use]
    pub fn env_keys(&self) -> Vec<String> {
        self.env.keys().cloned().collect()
    }

    /// Command program.
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Command args.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }
}

impl fmt::Debug for SandboxCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SandboxCommand")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("stdin_len", &self.stdin.as_ref().map(std::vec::Vec::len))
            .field("env_keys", &self.env_keys())
            .finish()
    }
}

impl SandboxMount {
    /// Create a read-only mount from an existing host source.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError`] if the source or target is unsafe.
    pub fn read_only(
        source: impl AsRef<Path>,
        target: impl Into<String>,
    ) -> Result<Self, SandboxError> {
        let source = validate_host_source(source.as_ref())?;
        let target = validate_sandbox_target(&target.into())?;
        Ok(Self {
            source,
            target,
            access: MountAccess::ReadOnly,
        })
    }

    /// Create a read-write mount scoped under a run root.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError`] if the source escapes the run root.
    pub fn read_write(
        source: impl AsRef<Path>,
        target: impl Into<String>,
        run: &RunReportStore,
    ) -> Result<Self, SandboxError> {
        let source = validate_host_source(source.as_ref())?;
        let run_root = run.path().canonicalize()?;
        if !source.starts_with(run_root) {
            return Err(SandboxError::UnsafePath(source.display().to_string()));
        }
        let target = validate_sandbox_target(&target.into())?;
        Ok(Self {
            source,
            target,
            access: MountAccess::ReadWrite,
        })
    }

    fn oci_volume_spec(&self) -> String {
        let access = match self.access {
            MountAccess::ReadOnly => "ro",
            MountAccess::ReadWrite => "rw",
        };
        format!("{}:{}:{access}", self.source.display(), self.target)
    }

    fn nsjail_bind_spec(&self) -> String {
        format!("{}:{}", self.source.display(), self.target)
    }
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            cpu_seconds: 60,
            wallclock_ms: 60_000,
            memory_bytes: 1_073_741_824,
            tmpfs_bytes: 268_435_456,
        }
    }
}

impl SandboxProfileSpec {
    /// Build a default no-network profile for a backend.
    #[must_use]
    pub fn default_no_network(backend: SandboxBackend) -> Self {
        Self {
            backend,
            network_allowed: false,
            mounts: Vec::new(),
            limits: SandboxLimits::default(),
            env_keys: Vec::new(),
        }
    }

    /// Append one validated mount.
    #[must_use]
    pub fn with_mount(mut self, mount: SandboxMount) -> Self {
        self.mounts.push(mount);
        self
    }

    /// Copy redacted env keys from a command.
    #[must_use]
    pub fn with_env_from_command(mut self, command: &SandboxCommand) -> Self {
        self.env_keys = command.env_keys();
        self
    }
}

impl<S> NoNetworkSandbox<S> {
    /// Create a no-network wrapper.
    #[must_use]
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: Sandbox> Sandbox for NoNetworkSandbox<S> {
    fn run(
        &self,
        profile: &SandboxProfileSpec,
        command: &SandboxCommand,
    ) -> Result<SandboxResult, SandboxError> {
        let mut effective = profile.clone();
        effective.network_allowed = false;
        self.inner.run(&effective, command)
    }
}

impl UnavailableSandbox {
    /// Create a backend that always returns [`SandboxError::MissingBackend`].
    #[must_use]
    pub fn new(backend: SandboxBackend) -> Self {
        Self { backend }
    }
}

impl Sandbox for UnavailableSandbox {
    fn run(
        &self,
        _profile: &SandboxProfileSpec,
        _command: &SandboxCommand,
    ) -> Result<SandboxResult, SandboxError> {
        Err(SandboxError::MissingBackend(self.backend))
    }
}

impl BackendCommandBuilder {
    /// Docker command builder.
    #[must_use]
    pub fn docker() -> Self {
        Self {
            backend: SandboxBackend::Docker,
        }
    }

    /// Podman command builder.
    #[must_use]
    pub fn podman() -> Self {
        Self {
            backend: SandboxBackend::Podman,
        }
    }

    /// nsjail command builder.
    #[must_use]
    pub fn nsjail() -> Self {
        Self {
            backend: SandboxBackend::Nsjail,
        }
    }

    /// Build a backend command without executing it.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError`] when the requested profile is incompatible.
    pub fn build(
        self,
        profile: &SandboxProfileSpec,
        command: &SandboxCommand,
    ) -> Result<BackendCommand, SandboxError> {
        if profile.network_allowed {
            return Err(SandboxError::Denied("network must be disabled".to_owned()));
        }
        if profile.backend != self.backend {
            return Err(SandboxError::Denied(format!(
                "profile backend {:?} does not match builder backend {:?}",
                profile.backend, self.backend
            )));
        }
        match self.backend {
            SandboxBackend::Docker => Ok(build_oci("docker", profile, command)),
            SandboxBackend::Podman => Ok(build_oci("podman", profile, command)),
            SandboxBackend::Nsjail => Ok(build_nsjail(profile, command)),
            backend => Err(SandboxError::MissingBackend(backend)),
        }
    }
}

fn build_oci(
    program: &str,
    profile: &SandboxProfileSpec,
    command: &SandboxCommand,
) -> BackendCommand {
    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "--network=none".to_owned(),
        "--cpus".to_owned(),
        "1".to_owned(),
        "--memory".to_owned(),
        profile.limits.memory_bytes.to_string(),
        "--tmpfs".to_owned(),
        format!("/tmp:size={}", profile.limits.tmpfs_bytes),
    ];
    for mount in &profile.mounts {
        args.push("--volume".to_owned());
        args.push(mount.oci_volume_spec());
    }
    for key in &profile.env_keys {
        args.push("--env".to_owned());
        args.push(key.clone());
    }
    args.push("polyref-sandbox-runner".to_owned());
    args.push("--timeout-ms".to_owned());
    args.push(profile.limits.wallclock_ms.to_string());
    args.push(command.program().to_owned());
    args.extend(command.args().iter().cloned());
    BackendCommand {
        program: program.to_owned(),
        args,
    }
}

fn build_nsjail(profile: &SandboxProfileSpec, command: &SandboxCommand) -> BackendCommand {
    let mut args = vec![
        "--disable_clone_newnet".to_owned(),
        "--rlimit_cpu".to_owned(),
        profile.limits.cpu_seconds.to_string(),
        "--rlimit_as".to_owned(),
        profile.limits.memory_bytes.to_string(),
        "--rlimit_fsize".to_owned(),
        profile.limits.tmpfs_bytes.to_string(),
        "--time_limit".to_owned(),
        profile.limits.wallclock_ms.div_ceil(1000).to_string(),
    ];
    for mount in &profile.mounts {
        let flag = match mount.access {
            MountAccess::ReadOnly => "--bindmount_ro",
            MountAccess::ReadWrite => "--bindmount",
        };
        args.push(flag.to_owned());
        args.push(mount.nsjail_bind_spec());
    }
    args.push("--".to_owned());
    args.push(command.program().to_owned());
    args.extend(command.args().iter().cloned());
    BackendCommand {
        program: "nsjail".to_owned(),
        args,
    }
}

fn validate_host_source(path: &Path) -> Result<PathBuf, SandboxError> {
    if path.to_str().is_none() {
        return Err(SandboxError::UnsafePath(path.display().to_string()));
    }
    let metadata = fs::symlink_metadata(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            SandboxError::UnsafePath(path.display().to_string())
        } else {
            SandboxError::Io(err)
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(SandboxError::UnsafePath(path.display().to_string()));
    }
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(SandboxError::UnsafePath(path.display().to_string()));
    }
    let canonicalized = path.canonicalize().map_err(SandboxError::Io)?;
    if canonicalized.to_str().is_none() {
        return Err(SandboxError::UnsafePath(
            canonicalized.display().to_string(),
        ));
    }
    Ok(canonicalized)
}

fn validate_sandbox_target(target: &str) -> Result<String, SandboxError> {
    let path = Path::new(target);
    if target.is_empty() || !path.is_absolute() {
        return Err(SandboxError::UnsafePath(target.to_owned()));
    }
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => {}
            _ => return Err(SandboxError::UnsafePath(target.to_owned())),
        }
    }
    Ok(target.to_owned())
}
