# 04 — Verification

Defines what "done" means at each layer of `build-plan.md`.

## Gate matrix

| Layer | Verified | How | Pass criterion |
| --- | --- | --- | --- |
| 0 | Schema integrity, core types | JSON Schema lint, round-trip serde tests | `cargo build` clean; 28 schemas validate against Draft 2020-12 |
| 1 | Persistence round-trips | 10 k-entity stress test; NDJSON replay | Round-trip equals input; cache hit/miss counters wired |
| 2 | Sandbox isolation | Negative tests: candidate that tries `curl example.com`, `cat /etc/passwd`, `mkdir /` | All denied with typed replay/sandbox errors; events `sandbox_denied` logged |
| 3 | Plugin host correctness | Dummy echo, dummy crash, dummy infinite-loop plugins | Each maps to the correct `UnknownReason`; replay deterministic |
| 4 | First extractor + route checker | §2 fixture | Side-local route correspondences emerge; checker returns `Migrated` through the migration map |
| 5 | Migration map + observation registry + frontier closure + coverage risk | Canonical Layer 5 fixture, component tests, and integration gate | `∂ρ(o)` matches 7 correspondence ids + 3 build-edge ids; clean fixture has no coverage risk; fail-closed gaps stay blocked |
| 6 | Engine + A2 ordering | Locked test on A2 step order | Reordering any step makes the test fail |
| 7 | Rewriters + report | Snapshot test on §2 report | Bytes match; invariant abort fires when forced |
| 8 | CLI | End-to-end shell tests | Exit codes 0 (accepted), 1 (broken), 2 (unknown), 3 (internal) |
| 9 | Remaining checkers | Per-checker 4-status fixtures | Each fixture exercises Pres / Migrated / Broken / Unknown |
| 10 | Coq | `dune build`; `Print Assumptions` | No `Admitted`, no axioms beyond stdlib |
| 11 | Empirical pilot | 30-task suite | FA-rate ≤ 0.12, p95 latency ≤ 60 min |
| 12 | Hardening | Fuzzing, dashboards | No panics in 24-h fuzz; dashboards green for 7 nights |

## Test categories

### Unit (per crate)

- `polyref-core` — id parsing, canonical JSON, status arithmetic.
- `polyref-graph` — SQLite migrations idempotent; read model ordering; migration-map diagnostics; observation support registration.
- `polyref-frontier` — affected-frontier closure, coverage-risk classification, and Layer 5 fixture integration.
- `polyref-engine` — A2 step ordering; aggregation rules.
- `polyref-rewriter` — each kind covers all positions in its kind's typed fields.

### Layer 5 invariant tests

Layer 5 uses deterministic bounded property-style tests rather than unbounded
random generators so Rust 1.79 CI remains stable.

- `crates/polyref-graph/tests/layer5_fixture.rs` — canonical fixture id/ref/order contract.
- `crates/polyref-graph/tests/read_model.rs` — read-only graph ordering and typed support loading.
- `crates/polyref-graph/tests/migration_map.rs` — type-respecting `μ`, conflict / ambiguous / missing diagnostics, audit-safe hashes.
- `crates/polyref-graph/tests/observation_registry.rs` — visible and held-out observations, support dedupe, missing support, unsupported evidence.
- `crates/polyref-frontier/tests/closure.rs` — Definition 7 closure rules, missing support diagnostics, idempotence.
- `crates/polyref-frontier/tests/coverage_risk.rs` — fail-closed `UnknownReason` risk mapping without final status assignment.
- `crates/polyref-frontier/tests/layer5_integration.rs` — full fixture graph → `μ` → observation registry → frontier → coverage-risk gate.

Layer 6+ keeps the status-level properties: no missing endpoint may become
`Pres` or `Migrated`, A2 is idempotent, and replayed reports are byte-identical.

### Integration

- `§2-layer5-fixture` — golden Layer 5 gate: 7 correspondence ids + 3 build-edge ids and no clean-fixture coverage risk.
- `seeded-route-client-drift` — stale generated client → `Broken(GeneratedClientStale)` in Layer 6+.
- `seeded-workflow-old-target` — workflow packages old target → `Broken(WorkflowPackagesOldTarget)` in Layer 6+.
- `unknown-dynamic-route` — route built by string concatenation → `Unknown(DynamicString)` in Layer 6+.
- `unknown-cyclic-generator` — OpenAPI ↔ client cycle → `Unknown(CyclicGenerator)` in Layer 6+.
- `migration-map-conflict` — concrete rewrite disagreement becomes Broken evidence in Layer 6+.

### End-to-end

- 30-task nightly pilot in CI.
- Quarterly full 180-task run with paired-bootstrap CIs.

### Replay

After each integration test, the cache directory is preserved and the test reruns with `--replay-from-cache`. Bytes must match.

### Coq

`dune build` is required for any release tag. `Print Assumptions correspondence_closure_preservation.` must print the empty list. Every theorem named in `build-plan.md` Layer 10 must have a paired implementation test.

## Performance verification

- Per-stage timing histograms emitted as Prometheus metrics; nightly CI asserts SLO.
- A 5-task perf microsuite runs on every PR; PR fails if median latency regresses by > 20 %.

## Security verification

- Sandbox negative tests on every PR.
- Pen-test the JSON-RPC parser with `cargo-fuzz` for 8 h before each release.
- Plugin binary digests recorded in `manifest.json`; release process verifies them against an out-of-band manifest.

## Audit verification

For every accepted row in the eval pilot, an automated checker confirms:

- All `endpoint_entity_ids` exist in the graph.
- All `evidence_pointers` resolve to a file under `evidence/`.
- `checker_version` is in the run's `manifest.json`.
- `missing_endpoint_unknown` is `false`.

Any failing row aborts the eval run.

## Definition of done at v1

- All gates above pass.
- The 180-task suite reproduces the paper-style headline (FA-rate reduction with McNemar p < 0.001 vs build/test-only and LLM agents) on at least one curated subject pool.
- Coq is closed.
- A reviewer outside the team can clone the repo, follow `docs/operator-guide.md`, and reproduce the §2 fixture report inside an hour.
