# 03 — Build Plan (Dependency-Ordered)

The order is by dependency, not calendar. Each layer must compile and pass its acceptance gate before the next begins. Unrelated layers may be developed in parallel where called out.

## Layer 0 — Schemas + core types  *(landed)*

```
schemas/
  _meta/version.json
  ids/{entity-id, artifact-id, correspondence-id, edge-id}.json
  source-span.json  artifact-kind.json  language.json
  correspondence-kind.json  validation-status.json
  unknown-reason.json  broken-reason.json
  evidence.json  evidence-pointer.json
  migration-map.json
  observation/{_kind, visibility, api-call, test-invocation,
               build-target, workflow-run, schema-validation}.json
  checker-spi/{describe, check}.json
  extractor-spi/extract.json
  report.json  audit-event.json  manifest.json

crates/polyref-core/src/{ids, source_span, artifact_kind, language,
                         correspondence_kind, status, evidence,
                         migration_map, observation/{mod,visibility},
                         canonical, error, report}.rs
crates/polyref-checker-spi/src/{envelope, limits, error, extractor, checker}.rs
```

Acceptance: `cargo build --workspace` clean; `cargo test --workspace` green; `bash scripts/verify-schemas.sh` reports 28 schemas validate against Draft 2020-12.

Status: **complete** (v0.1.0). All parsers implemented, 69 tests green, CI green.

## Layer 1 — Persistence

| File | Purpose |
| --- | --- |
| `crates/polyref-graph/src/store.rs` | SQLite GraphStore + migrations |
| `crates/polyref-graph/src/blobstore.rs` | Content-addressed `cache/blobs/` |
| `crates/polyref-graph/src/audit.rs` | NDJSON AuditLog writer + reader |
| `crates/polyref-graph/src/report_store.rs` | `runs/<report_id>/` layout |
| `crates/polyref-graph/migrations/0001_init.sql` | Initial schema |

Acceptance: round-trip 10 k entities + correspondences; replay reads NDJSON back into typed events; cache hit/miss counters wired.

Status: **complete**. `polyref-graph` now includes SQLite GraphStore, content-addressed BlobStore, NDJSON AuditLog, and ReportStore run layout. Layer 1 quality gate passes locally with `RUSTFLAGS="-D warnings"`.

## Layer 2 — Loader + sandbox

Status: complete as of `polyref-loader` library support. Layer 2 exposes
Rust helper APIs (`replay_patch` / `load_repo_with_patch`) for loading a
local repo and patch into sandboxed old/new workspaces. The end-to-end
`polyref load <repo> <patch>` CLI remains Layer 8 scope.

| File | Purpose |
| --- | --- |
| `crates/polyref-loader/src/checkout.rs` | Reproducible repo + commit checkout |
| `crates/polyref-loader/src/sandbox.rs` | backend-neutral sandbox abstraction and no-network command builders |
| `crates/polyref-loader/src/replay.rs` | Apply candidate ρ inside sandbox |
| `crates/polyref-loader/src/manifest.rs` | `.polyref/runs/<id>/manifest.json` |

Acceptance: `load_repo_with_patch(...)` produces R and R' under a sandbox
abstraction with no host-side patch fallback. Negative replay attempts are
denied and logged; mapping denials to final `Unknown` statuses is handled by
later engine/checker layers.

## Layer 3 — Plugin host

| File | Purpose |
| --- | --- |
| `crates/polyref-checker-spi/src/host.rs` | Plugin process pool + JSON-RPC dispatcher |
| `crates/polyref-checker-spi/src/cgroup.rs` | cgroup + seccomp glue |
| `crates/polyref-checker-spi/src/memo.rs` | Plugin response memoization |

Acceptance: dummy echo plugin succeeds; crashing plugin yields `Unknown(PluginFailure)`; deadline overrun yields `Unknown(CheckerTimeout)`; replay test produces byte-identical bytes.

## Layer 4 — First extractors + first checker

| Component | Purpose |
| --- | --- |
| `plugins/extractor-typescript/` | Rust tree-sitter extractor for fixture-owned route metadata and handlers |
| `plugins/extractor-openapi/` | OpenAPI 3.x parser + ref resolver |
| `crates/polyref-graph/src/builder.rs` | Normalize extractor outputs into `Correspondence` rows |
| `plugins/checker-route/` | Route compat + migrate predicates |

Acceptance: §2 fixture (`POST /users → /v2/users`) loads; side-local route correspondences emerge for old route→old handler and new route→new handler; route checker returns `Migrated` after the canonical migration map is supplied.

Layer 4 intentionally covers the first OpenAPI + TypeScript route slice only. Python extraction remains future plugin work unless a later layer or fixture explicitly schedules it.

## Layer 5 — Migration + observation registry + frontier

| File | Purpose |
| --- | --- |
| `crates/polyref-graph/src/read_model.rs` | Typed read-only graph view for Layer 5 algorithms |
| `crates/polyref-graph/src/migration_map.rs` | Build + validate migration map `μ`; conflict / ambiguity diagnostics |
| `crates/polyref-graph/src/observation_registry.rs` | Declarative observation registration; visible / held-out support sets |
| `crates/polyref-frontier/src/closure.rs` | Definition 7 deterministic least affected-frontier closure (o-relative) |
| `crates/polyref-frontier/src/coverage_risk.rs` | Fail-closed coverage-risk classification for later A2 |
| `crates/polyref-frontier/tests/layer5_integration.rs` | Full Layer 5 acceptance gate over the §2 fixture |

Acceptance: §2 fixture produces a frontier of 7 correspondences + 3 build edges;
Layer 5 integration proves migration-map construction, observation registration,
frontier closure, and coverage-risk classification compose without assigning
Layer 6 final statuses. The frontier is o-relative and closed under reachability to
`supp(o)`: it includes the intermediate build/codegen edges on paths from an edit to a
`supp(o)` element even when those edges are not themselves in `supp(o)`, so the
`required(o)` set the engine consumes in Layer 6 is complete (see architecture.md,
"Affected frontier and required(o)"). Property-style tests cover deterministic replay,
frontier idempotence, closure-under-reachability, missing-support blocking, and
type-respecting migration-map rewrites.

## Layer 6 — Engine

| File | Purpose |
| --- | --- |
| `crates/polyref-engine/src/obligation.rs` | Obligation generator |
| `crates/polyref-engine/src/a2.rs` | Algorithm A2; ordering test-locked |
| `crates/polyref-engine/src/a1.rs` | Algorithm A1 driver |
| `crates/polyref-engine/src/concurrency.rs` | ADR-007 task layout |
| `crates/polyref-engine/tests/a2_ordering.rs` | Locks the load-bearing ordering |

Acceptance: integration test reproduces the §5.7 paper report from §2 fixture; ordering test fails on any reordering of A2 steps.

## Layer 7 — Rewriters + report

| File | Purpose |
| --- | --- |
| `crates/polyref-rewriter/src/api_call.rs` | API observation rewriter |
| `crates/polyref-rewriter/src/build.rs` | Build / workflow rewriter |
| `crates/polyref-rewriter/src/test.rs` | Test observation rewriter |
| `crates/polyref-report/src/json.rs` | Canonical JSON |
| `crates/polyref-report/src/md.rs` | Operator-friendly markdown |
| `crates/polyref-report/src/invariant.rs` | `accepted_no_missing_endpoint_unknown` invariant |

Acceptance: §2 fixture produces JSON + Markdown that match snapshots; invariant abort triggers in a fault-injection test.

## Layer 8 — CLI

| File | Purpose |
| --- | --- |
| `crates/polyref-cli/src/main.rs` | `polyref validate / replay / explain / prefetch` |
| `crates/polyref-cli/src/config.rs` | `polyref.toml` loader |
| `docs/operator-guide.md` | Run, interpret, debug |
| `docs/troubleshooting-unknowns.md` | Per-`UnknownReason` remediation |

Acceptance: end-to-end CLI run on §2 fixture; `polyref explain <unknown_id>` returns actionable info.

## Layer 9 — Remaining checkers (parallel)

`plugins/checker-{schema, build-codegen, workflow, query-table, event, generated-client, test-oracle, serialization, configuration}/`. Each ships:
- a `describe()` returning a contract for its kind,
- four `check()` fixtures (Pres / Migrated / Broken / Unknown),
- replayable byte-identical responses for the same input.

`checker-build-codegen` reads the `build_file` ArtifactKind and applies the ADR-004 two-of-three rule (source map, re-execution, checksum manifest).

## Layer 10 — Coq mechanization (parallel from Layer 0)

| File | Theorem |
| --- | --- |
| `coq/PolyRef/Repository.v` | Inductive `Repository`, `Correspondence`, … |
| `coq/PolyRef/Status.v` | `ValidationStatus`, fail-closed lemma |
| `coq/PolyRef/Frontier.v` | `compute_frontier` + `frontier_adequacy` |
| `coq/PolyRef/CorrespondenceClosure.v` | `correspondence_closure` |
| `coq/PolyRef/BuildClosure.v` | `build_closure_preservation` |
| `coq/PolyRef/Compositional.v` | `compositional_closure` |
| `coq/PolyRef/Preservation.v` | `correspondence_closure_preservation` |
| `coq/PolyRef/Acceptance.v` | `accepted_no_broken_unknown` |
| `coq/PolyRef/NonAcceptance.v` | `nonaccepted_not_preservation_claim` |

Acceptance: `dune build` clean; `Print Assumptions correspondence_closure_preservation.` empty.

## Layer 11 — Empirical harness

`eval/{subjects, tasks/{historical, seeded}, baselines/{build-test, llm-agent, static-tools}, analysis, figures}/`. Acceptance: 30-task pilot runs end-to-end; FA-rate paired CI computed; nightly CI keeps SLOs green.

## Layer 12 — Hardening

Memoization tuning, plugin pool sizing, sandbox profile audit, observability dashboards, fuzzing on JSON-RPC parser, EntityId parser, and schema differs.
