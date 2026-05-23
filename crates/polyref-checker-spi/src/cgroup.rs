//! Declarative plugin isolation profile for Layer 3.
//!
//! The process runner is intentionally separate from OS-specific sandbox
//! backends. This module captures the cgroup/seccomp requirements that a Linux
//! backend must enforce and gives tests a stable, auditable contract.

use thiserror::Error;

/// Backend family that can enforce a plugin isolation profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IsolationBackend {
    /// Linux nsjail per-call sandbox.
    Nsjail,
    /// macOS development fallback using sandbox-exec.
    SandboxExec,
}

impl IsolationBackend {
    /// Backend executable name.
    #[must_use]
    pub fn program(self) -> &'static str {
        match self {
            Self::Nsjail => "nsjail",
            Self::SandboxExec => "sandbox-exec",
        }
    }
}

/// Seccomp policy requirements for plugin processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeccompPolicy {
    /// Require Linux no-new-privileges / equivalent backend posture.
    pub no_new_privileges: bool,
    /// Syscalls the backend profile must deny.
    pub denied_syscalls: Vec<String>,
}

/// Resource and security profile for one plugin call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginIsolationProfile {
    /// Per-call CPU seconds.
    pub cpu_seconds: u64,
    /// Per-call wallclock milliseconds.
    pub wallclock_ms: u64,
    /// Per-call memory cap in bytes.
    pub memory_bytes: u64,
    /// Per-call writable tmpfs cap in bytes.
    pub tmpfs_bytes: u64,
    /// Whether network is allowed. V1 requires false.
    pub network_allowed: bool,
    /// Seccomp/no-new-privileges policy.
    pub seccomp: SeccompPolicy,
}

/// Isolation profile validation/build errors.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IsolationError {
    /// Network is not allowed for v1 plugin execution.
    #[error("plugin isolation must deny network")]
    NetworkAllowed,
    /// A numeric resource limit must be non-zero.
    #[error("plugin isolation limit must be non-zero: {0}")]
    ZeroLimit(&'static str),
    /// Seccomp policy is missing required hardening.
    #[error("plugin isolation seccomp policy is incomplete: {0}")]
    WeakSeccomp(&'static str),
}

impl Default for SeccompPolicy {
    fn default() -> Self {
        Self {
            no_new_privileges: true,
            denied_syscalls: ["ptrace", "mount", "chroot", "kexec_load"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    }
}

impl Default for PluginIsolationProfile {
    fn default() -> Self {
        Self {
            cpu_seconds: 60,
            wallclock_ms: 60_000,
            memory_bytes: 1024 * 1024 * 1024,
            tmpfs_bytes: 256 * 1024 * 1024,
            network_allowed: false,
            seccomp: SeccompPolicy::default(),
        }
    }
}

impl PluginIsolationProfile {
    /// Validate fail-closed v1 plugin isolation requirements.
    ///
    /// # Errors
    ///
    /// Returns [`IsolationError`] when the profile weakens ADR-009 defaults.
    pub fn validate(&self) -> Result<(), IsolationError> {
        if self.network_allowed {
            return Err(IsolationError::NetworkAllowed);
        }
        if self.cpu_seconds == 0 {
            return Err(IsolationError::ZeroLimit("cpu_seconds"));
        }
        if self.wallclock_ms == 0 {
            return Err(IsolationError::ZeroLimit("wallclock_ms"));
        }
        if self.memory_bytes == 0 {
            return Err(IsolationError::ZeroLimit("memory_bytes"));
        }
        if self.tmpfs_bytes == 0 {
            return Err(IsolationError::ZeroLimit("tmpfs_bytes"));
        }
        if !self.seccomp.no_new_privileges {
            return Err(IsolationError::WeakSeccomp("no_new_privileges"));
        }
        for required in ["ptrace", "mount", "chroot", "kexec_load"] {
            if !self
                .seccomp
                .denied_syscalls
                .iter()
                .any(|syscall| syscall == required)
            {
                return Err(IsolationError::WeakSeccomp(required));
            }
        }
        Ok(())
    }

    /// Build backend arguments for the selected isolation backend.
    ///
    /// # Errors
    ///
    /// Returns [`IsolationError`] when the profile is not safe enough to build.
    pub fn backend_args(&self, backend: IsolationBackend) -> Result<Vec<String>, IsolationError> {
        self.validate()?;
        Ok(match backend {
            IsolationBackend::Nsjail => self.nsjail_args(),
            IsolationBackend::SandboxExec => self.sandbox_exec_args(),
        })
    }

    /// Build a backend command prefix that runs `plugin_path` under isolation.
    ///
    /// # Errors
    ///
    /// Returns [`IsolationError`] when the profile is not safe enough to build.
    pub fn backend_command(
        &self,
        backend: IsolationBackend,
        plugin_path: &str,
    ) -> Result<(String, Vec<String>), IsolationError> {
        let mut args = self.backend_args(backend)?;
        match backend {
            IsolationBackend::Nsjail => {
                args.push("--".to_owned());
                args.push(plugin_path.to_owned());
            }
            IsolationBackend::SandboxExec => args.push(plugin_path.to_owned()),
        }
        Ok((backend.program().to_owned(), args))
    }

    fn nsjail_args(&self) -> Vec<String> {
        vec![
            "--disable_clone_newnet".to_owned(),
            "--rlimit_cpu".to_owned(),
            self.cpu_seconds.to_string(),
            "--rlimit_as".to_owned(),
            self.memory_bytes.to_string(),
            "--time_limit".to_owned(),
            self.wallclock_ms.div_ceil(1000).to_string(),
        ]
    }

    fn sandbox_exec_args(&self) -> Vec<String> {
        vec![
            "-p".to_owned(),
            "(version 1) (deny default) (allow file-read*)".to_owned(),
        ]
    }
}
