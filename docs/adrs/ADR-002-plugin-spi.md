# ADR-002: Plugin SPI is JSON-RPC over stdio with strict schemas

## Context
gpt.md flagged the plugin API as a missing decision. PolyRef must isolate untrusted extractor and checker code, support multiple host languages, enforce per-call timeouts, and produce byte-identical replay. We need a transport that survives plugin crashes, supports cgroup limits, and does not require shared memory.

## Decision
Plugins are separate processes spawned by the host. The host writes JSON-RPC 2.0 messages on stdin; the plugin replies on stdout. Logs go to a host-managed log directory passed in the request. Each request has a deadline; the host kills the plugin process on deadline expiry and synthesizes `Unknown(reason=PluginTimeout)`.

### Two SPI families

**Extractor SPI** (one method):
```
Method:  extract
Params:  { artifact_path, content_hash, language, options, deadline_ms, log_dir }
Result:  { entities[], local_facts[], unsupported_features[], extractor_version }
```

**KindChecker SPI** (two methods):
```
Method:  describe
Params:  {}
Result:  {
  contract_id, kind_id, endpoint_signature[], required_evidence_fields[],
  compat_rule_id, migrate_rule_id, plugin_version, default_timeout_ms,
  supported_unknown_reasons[], supported_broken_reasons[]
}

Method:  check
Params:  {
  contract_id, kind, endpoints[],
  old_repo_root, new_repo_root, migration_map_excerpt,
  observation_excerpt, deadline_ms, log_dir
}
Result:  {
  outcome,                          // "Pres" | "Migrated" | "Broken" | "Unknown"
  predicate,                        // rule id used
  evidence_pointers[],              // file paths under log_dir
  spans[],                          // SourceSpan
  checker_version, rule_version,
  unknown_reason | null,            // canonical UnknownReason from ADR-005
  broken_reason | null              // canonical BrokenReason from ADR-005
}
```

### Host enforcement
- Plugin process started under cgroup with CPU/memory/wallclock limits.
- Read-only mount of the repo source(s); `log_dir` is the only writable path.
- Stdin payload is a single line of canonical JSON (sorted keys, no trailing whitespace). The SHA-256 of that payload + plugin binary digest forms the cache key.
- Stdout response must be a single JSON object on a single line (LSP-style framing rejected for v1; simpler).
- Any non-zero exit, malformed JSON, schema violation, or deadline overrun → `Unknown(reason)`.

## Consequences

### Positive
- Language-agnostic plugins.
- Crash isolation; one bad plugin cannot poison the host.
- Per-call cgroup limits + deterministic stdin hashing make replay trivial.
- Schema-versioned: plugins declare their `plugin_version`; host writes it into evidence.
- Easy to test: snapshot input/output JSON in fixtures.

### Negative
- Process startup cost per call. Mitigated by long-lived plugin processes (one per kind), reused across many `check` calls in a run.
- Stdio contention if a plugin produces large logs. Mitigated by: logs go to `log_dir`, not stdout.
- JSON-RPC framing is text-heavy. Acceptable; profiling shows graph traversal dominates.

### Alternatives considered
- **gRPC**: more typed but adds a proto compiler dependency and TLS surface area we do not need on localhost.
- **Shared library (FFI)**: rejected. No crash isolation, no per-call cgroup, harder to write Python plugins safely.
- **Embedded Python interpreter**: rejected. GIL contention; couples host to a single Python version.
- **WASM plugins**: appealing for sandboxing but immature toolchain support for tree-sitter, oasdiff, sqlfluff.

## Status
Accepted

## Date
2026-05-19
