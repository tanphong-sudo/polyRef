# ADR-009: Sandbox candidate replay and plugin processes; deny network and filesystem escape by default

## Context
PolyRef applies untrusted candidate edits, runs codegen tools, executes build scripts, and invokes plugins that wrap external binaries. None of this should be able to exfiltrate data, escape the sandbox, or modify the host. gpt.md flagged the sandbox decision as missing.

## Decision

### Sandbox layers
1. **Outer container (per validation run).** A rootless OCI container (Docker or Podman) hosts the entire run. Mounts: read-only repo source, writable `.polyref/` cache, no network. CPU/memory cgroup limits per the run.
2. **Per-call sandbox (per plugin/per generator).** Linux: `nsjail` with `--disable_clone_newnet`, `--rlimit_as`, `--rlimit_cpu`, dropped caps (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, etc.), `no-new-privileges=1`, seccomp filter disallowing `ptrace`, `mount`, `chroot`, `kexec`. macOS dev: `sandbox-exec` with the equivalent profile (best effort; CI runs on Linux).
3. **Filesystem mounts.** Source repo: read-only bind mount. Plugin's `log_dir`: writable, scoped to that single call. Tmpfs scratch: writable, capped (default 256 MiB).
4. **Network.** None. Outbound and inbound denied. If a plugin needs a registry (e.g. to fetch generated client deps), it is the user's responsibility to mirror the dep into a local store before running, and PolyRef ships a `prefetch` subcommand to do this declaratively.

### Untrusted-input handling
- All file paths from candidates are normalized and rejected if they escape the sandbox root (path-traversal guard).
- All JSON payloads are size-bounded (default 16 MiB) and JSON-Schema-validated before unmarshaling.
- Plugin stdin payloads are line-bounded (one JSON line per call).
- Plugin stdout is parsed with a streaming parser that aborts on non-JSON.

### Resource limits
| Resource | Default | Configurable |
| --- | --- | --- |
| Per-call CPU time | 60 s | yes (via deadline) |
| Per-call wallclock | `deadline_ms` from request | yes |
| Per-call memory | 1 GiB | yes |
| Per-run memory | 8 GiB | yes |
| Per-call filesystem writes | 256 MiB tmpfs | yes |
| Outbound network | denied | overridable only at run-config level with explicit `--allow-network=domain.com` |

### Secrets
- The run never reads `~/.aws`, `~/.config/gh`, etc. The sandbox starts with `HOME` pointing to a tmpfs.
- Environment variables are an explicit allowlist passed by the operator; the sandbox does not inherit the host env.
- Plugin processes start with empty env unless the plugin's `describe` declares an allowlist.

### Audit
- Every sandbox launch logs `sandbox_started` event with: profile id, mounts, resource limits, command, env keys (values redacted).
- Any sandbox failure (OOM kill, seccomp deny, deadline) becomes `Unknown(reason=PluginFailure|CheckerTimeout)` and is logged.

### Threat model in scope
- Malicious candidate patch attempts to read host secrets / write outside repo / make outbound HTTP. Blocked by sandbox.
- Crafted artifact triggers parser RCE in a plugin. Blocked by per-call memory + seccomp; plugin death does not affect host.
- Plugin attempts privilege escalation. Blocked by `no-new-privileges` + dropped caps.

### Threat model out of scope (v1)
- Side-channel attacks (Spectre, etc.).
- Sophisticated supply-chain attacks on plugin binaries themselves; mitigated by plugin version pinning + checksums in `manifest.json`.

## Consequences

### Positive
- Defense in depth: container + nsjail + seccomp + cgroups + no-network.
- All resource limits become observable via cgroup metrics.
- Replay is reproducible because the sandbox spec is part of `manifest.json`.

### Negative
- Linux-first; macOS dev experience is best-effort. CI must be Linux.
- Operators who want the codegen step to fetch from a private registry must use `prefetch`. Friction; documented.
- Some legacy generators that rely on shell expansion of `HOME`/PATH may need wrapper scripts.

### Alternatives considered
- **Trust the host**: rejected. Untrusted candidate replay is the whole point of PolyRef.
- **VM-per-run (Firecracker)**: stronger isolation, higher cost. Deferred to v2.
- **Run on developer laptop without sandbox**: rejected. Operator-only mode would create a footgun.

## Status
Accepted

## Date
2026-05-19
