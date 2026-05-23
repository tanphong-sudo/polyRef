//! Layer 3 plugin isolation profile contract tests.

use polyref_checker_spi::cgroup::{
    IsolationBackend, IsolationError, PluginIsolationProfile, SeccompPolicy,
};

#[test]
fn default_isolation_profile_matches_adr_009_security_defaults() {
    let profile = PluginIsolationProfile::default();

    assert_eq!(profile.cpu_seconds, 60);
    assert_eq!(profile.memory_bytes, 1024 * 1024 * 1024);
    assert_eq!(profile.tmpfs_bytes, 256 * 1024 * 1024);
    assert!(!profile.network_allowed);
    assert!(profile.seccomp.no_new_privileges);
    assert!(profile
        .seccomp
        .denied_syscalls
        .contains(&"ptrace".to_owned()));
    assert!(profile
        .seccomp
        .denied_syscalls
        .contains(&"mount".to_owned()));
    assert!(profile
        .seccomp
        .denied_syscalls
        .contains(&"chroot".to_owned()));
    assert!(profile
        .seccomp
        .denied_syscalls
        .contains(&"kexec_load".to_owned()));
}

#[test]
fn isolation_profile_rejects_network_enabled_v1() {
    let profile = PluginIsolationProfile {
        network_allowed: true,
        ..PluginIsolationProfile::default()
    };

    assert!(profile.validate().is_err());
}

#[test]
fn isolation_profile_rejects_empty_seccomp_policy() {
    let profile = PluginIsolationProfile {
        seccomp: SeccompPolicy {
            no_new_privileges: true,
            denied_syscalls: Vec::new(),
        },
        ..PluginIsolationProfile::default()
    };

    assert!(profile.validate().is_err());
}

#[test]
fn nsjail_args_include_no_network_and_resource_limits() -> Result<(), IsolationError> {
    let profile = PluginIsolationProfile::default();
    let args = profile.backend_args(IsolationBackend::Nsjail)?;

    assert!(args.contains(&"--disable_clone_newnet".to_owned()));
    assert!(args.contains(&"--rlimit_cpu".to_owned()));
    assert!(args.contains(&"60".to_owned()));
    assert!(args.contains(&"--rlimit_as".to_owned()));
    assert!(args.contains(&(1024_u64 * 1024 * 1024).to_string()));
    Ok(())
}
