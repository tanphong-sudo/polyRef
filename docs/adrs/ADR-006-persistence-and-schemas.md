# ADR-006: SQLite GraphStore + content-addressed ArtifactStore + NDJSON AuditLog; report schemas frozen

## Context
gpt.md missing decision (7): concrete JSON schemas. Also: no decision on where graphs live, how memoization works, where audit logs sit. Choices need to balance simple-deploy (CLI on a laptop) with reproducibility for CI.

## Decision

### Storage layout per validation run
```
.polyref/
├── cache/
│   ├── blobs/sha256/<hash[:2]>/<hash>           # ArtifactStore: extractor outputs, raw checker logs, replay artifacts
│   └── memo/<plugin_id>/<call_hash>.json         # KindChecker memoization
├── runs/<report_id>/
│   ├── repo-old.sqlite                          # GraphStore for R
│   ├── repo-new.sqlite                          # GraphStore for R'
│   ├── audit.ndjson                             # append-only event log
│   ├── evidence/                                # evidence pointers + per-call raw logs from plugins
│   │   └── manifest.json                        # schema-valid mirror for report audit_pointers
│   ├── report.json                              # canonical machine-readable report
│   ├── report.md                                # human-readable rendering
│   └── manifest.json                            # canonical versions, hashes, env capture
└── schemas/                                     # frozen JSON Schemas under semver
```

`runs/<report_id>/manifest.json` is the canonical run manifest for operators
and replay. The report schema models `audit_pointers.manifest_json` as an
`EvidencePointer`, so the ReportStore also writes a byte-identical mirror at
`runs/<report_id>/evidence/manifest.json`. Layer 2 may expand the manifest
contents, but it must preserve this report-schema compatibility unless the
report schema is versioned.

### GraphStore (SQLite) tables
- `artifact(artifact_id PK, repo_side, path, language, kind, content_hash, generated)`
- `entity(entity_id PK, artifact_id FK, kind, local_name, type, span, extractor_id, extractor_version, confidence)`
- `correspondence(corr_id PK, kind_id, extraction_rule, ambiguity_json)`
- `correspondence_endpoint(corr_id FK, slot, entity_id FK)` (composite PK on `(corr_id, slot)`)
- `build_edge(edge_id PK, source_artifact_id, target_artifact_id, generator_command, declared, source_map_pointer)`
- `observation(obs_id PK, kind, fields_json, support_json, visibility, defined_semantics)`
- Indexes on `(corr_id, slot)`, `(entity_id)`, `(kind_id)`, `(content_hash)`.

### AuditLog (NDJSON) event types
One JSON object per line. Closed event tag set:
- `repo_loaded`
- `artifact_classified`
- `extractor_invoked`
- `entity_emitted`
- `correspondence_created`
- `migration_map_built`
- `frontier_computed`
- `obligation_emitted`
- `checker_invoked`
- `checker_result`
- `observation_rewritten`
- `frontier_item_status_assigned`
- `observation_status_assigned`
- `report_finalized`

Every event carries `ts`, `report_id`, `stage`, `actor`, `payload_hash`, optional `evidence_pointers`. Replay verifier checks the chain of `payload_hash` values is reconstructable from cache.

### Report schema (canonical, semver-locked)
```json
{
  "schema_version": "1.0.0",
  "report_id": "...",
  "candidate": { "candidate_id": "...", "source": "llm|ide|template|manual", "patch_hash": "..." },
  "repos": { "old": { "repo_id": "...", "commit": "..." }, "new": { "repo_id": "...", "commit": "..." } },
  "configs": { "extractor_versions": {...}, "checker_versions": {...} },
  "observations": [
    {
      "observation_id": "...",
      "obs_kind": "api_call",
      "visibility": "visible|held_out|evaluation_only",
      "frontier_size": 7,
      "status_counts": { "Pres": 4, "Migrated": 2, "Broken": 0, "Unknown": 1 },
      "items": [
        {
          "item_id": "...",
          "item_kind": "correspondence",
          "kind_id": "route",
          "endpoint_entity_ids": ["old:py:route:...", "new:py:route:..."],
          "status": "Migrated",
          "predicate": "route.migrate-v1",
          "spans": [...],
          "evidence_pointers": ["evidence/.../router.json"],
          "checker_version": "...",
          "rule_version": "...",
          "unknown_reason": null,
          "broken_reason": null
        }
      ],
      "observation_rewrite": {
        "old": "...",
        "new": "...",
        "rewriter_version": "...",
        "defined": true
      },
      "status": "accepted|broken|unknown"
    }
  ],
  "candidate_decision": "accepted|broken|unknown",
  "missing_endpoint_unknown": false,
  "audit_pointers": { "audit_ndjson": "...", "manifest_json": "..." }
}
```

The audit invariant `accepted_implies_no_missing_endpoint_unknown` is verified at report finalization; if violated, the run aborts with an internal error (this is a fail-closed invariant).

### Memoization keys
- Extractor: `H(content_hash, extractor_id, extractor_version, options_canonical)`.
- KindChecker: `H(plugin_version, contract_id, sorted endpoint_entity_ids, evidence_inputs_hash, deadline_ms)`.

Cache hits return the stored JSON response; misses invoke the plugin process. Cache is per-machine; CI nodes can warm-share via a tarball under `cache/` if desired.

## Consequences

### Positive
- Single-file SQLite GraphStore: zero deploy ceremony; `polyref` runs on a laptop and on a CI runner with no service dependency.
- Content-addressed cache makes the 43-min budget achievable and reproducibility trivial.
- NDJSON audit log streams; tail-friendly for live dashboards.
- Frozen JSON schema versioned with semver; consumers (eval harness, CI bot) can pin.

### Negative
- SQLite single-writer limits parallel ingestion. Mitigated by writing per-repo (R and R' have separate SQLite files); concurrency happens across observations, not graph writes.
- Cache invalidation on plugin upgrade is coarse (any version change invalidates). Acceptable; correctness > cache hit rate.
- Schema versioning discipline must be enforced; CI lints `schemas/` for breaking changes.

### Alternatives considered
- **Postgres or Neo4j**: rejected. Adds a service to deploy; not justified at expected graph sizes.
- **In-memory only**: rejected. No replayability, no cross-run memoization.
- **Avro/Protobuf for evidence**: rejected. Plain JSON Schema is enough and human-readable; performance not a bottleneck at JSON size we emit.

## Status
Accepted

## Date
2026-05-19
