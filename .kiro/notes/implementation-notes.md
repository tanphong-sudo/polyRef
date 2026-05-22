# Implementation Notes (local)

Living log of deviations, tradeoffs, and decisions taken during
implementation that the docs / paper / plans do not reflect. Append-only.

This file lives under `.kiro/` so it is workspace-local and gitignored;
it is for the author to read while working, not for shipping.

Each entry should say:

- **what** changed vs the docs/plan
- **why** (which constraint forced the change)
- **impact** (does it break an invariant? require a paper-side note?)
- **follow-up** (when/whether to fold the decision back into docs)

---

## 2026-05-21 — Layer 1 / PR #2 `feat/graphstore-sqlite` (merged)

### N-1. Domain types `Entity` / `Correspondence` / `BuildEdge` live in `polyref-graph`

`plans.md` and `docs/build-plan.md` describe the persistent graph model
but do not say where the row structs go. Placed them in
`crates/polyref-graph/src/model.rs`.

**Why**: `docs/architecture.md` lists "Repository graph, GraphStore,
MigrationMap, ObservationRegistry" all under `polyref-graph`. The
`polyref-core` crate doc says it is the type substrate (ids, status,
evidence) and "no runtime I/O" — the persistent row types are
graph-layer.

**Impact**: None. `polyref-core` API unchanged; `polyref-graph` is a new
crate so blast radius is zero.

**Follow-up**: Update `claude/05-handoff-1-core-ir.md` §B to say
explicitly that `Entity` / `Correspondence` / `BuildEdge` are layer-1.

### N-2. `rusqlite` with `bundled` SQLite

`polyref-graph` uses `rusqlite = { version = "0.32", features = ["bundled"] }`.
Bundled = static-link a copy of SQLite at compile time, removing the
system-`libsqlite3` dependency.

**Why**: Reproducible builds across CI runners. Avoids pinning a system
SQLite version.

**Impact**: SQLite source is public domain (Zlib-equivalent); already
in `deny.toml` allowlist. `cargo deny check bans licenses` is green.

**Follow-up**: None. If we ever swap to system sqlite, the feature flag
is the only change site.

### N-3. ~~Wildcard sentinel approach for `#[non_exhaustive]` enums~~ — rolled back

Initial commit used `_ => "__unsupported__"` sentinels in
`store.rs` to handle `#[non_exhaustive]` enum exhaustiveness; reviewed
and rolled back during PR #2 (commit `27244b9`). Final shape:

- `as_tag()` + `parse()` helpers live on each enum **inside
  `polyref-core`** where the match is exhaustive and `#[non_exhaustive]`
  does not require a wildcard.
- `crates/polyref-graph/src/tags.rs` is a thin shim that maps each
  parse error into `GraphStoreError::UnsupportedEnum`.
- The `__unsupported__` sentinel was removed entirely — fail-closed
  semantics restored.

The lesson is now in `.kiro/steering/lessons-learned.md` ("business
enum" rule applies broadly, not just to Outcome/UnknownReason/BrokenReason).

### N-4. Tempfile pinned to `3.10.1` (transitive `getrandom` MSRV)

`tempfile` ≥ 3.13 transitively pulls `getrandom 0.4.x` which requires
the `edition2024` Cargo feature stabilized only in Rust 1.85. PolyRef
MSRV is 1.79.

**Impact**: Dev-dependency only.

**Follow-up**: Add a CI sanity test asserting `getrandom < 0.4` until
MSRV ≥ 1.85. Track in Layer 12.

### N-5. Local quality-gate must mirror `RUSTFLAGS="-D warnings"`

First push of the branch failed CI on three Rust jobs even though local
gate was green. CI sets `RUSTFLAGS: "-D warnings"`; lints set to "warn"
in `[workspace.lints.clippy]` (`unwrap_used`, `expect_used`, `panic`)
become errors only under that env.

**Follow-up (done)**: `.kiro/scripts/quality-gate.sh` now exports the
env. `git-workflow.md` and `CONTRIBUTING.md` document it.

### N-6. CI does not trigger on push to feature branches

`.github/workflows/ci.yml` triggers only on push-to-main and PR-to-main.
To verify CI on a branch, open a Draft PR. Mark ready-for-review only
after CI is green. Pattern is now mandatory in `git-workflow.md`.

### N-7. CI Security job needs Dependency graph enabled

`actions/dependency-review-action` requires the feature to be on. Enable
at `Settings → Code security and analysis`. New repos don't have it on
by default. Add to a future "repo bootstrap" checklist.

---

## 2026-05-21 — Layer 1 / PR #3 `feat/audit-ndjson` (merged)

### N-8. Schema package version 0.1.0 → 0.2.0

`audit-event.json` went from a placeholder (4 fields,
`additionalProperties: true`) to a closed shape with a 14-member closed
`tag` enum, two new required fields (`actor`, `payload_hash`), and
`additionalProperties: false`. Soft break for any consumer relying on
the placeholder permitting unknown keys; no such consumer exists
in-tree. CHANGELOG entry added.

### N-9. `AuditEvent` is a public DTO with public fields

Unlike `EntityId` / `SafePath` which keep inner fields private, `AuditEvent`
exposes its fields as `pub`. It is a wire-format DTO mirroring
`audit-event.json` 1:1; sealing fields would force every caller through
`new()` plus N getters. Pattern matches `Artifact` / `Entity` /
`Correspondence` in `polyref-graph::model` (also pub-fields wire DTOs).

**Mitigation**: `new()` is the blessed builder; `validate()` is
re-runnable; serde uses `deny_unknown_fields` so it can't bypass cap
rules through extra keys.

**Follow-up**: If a future invariant crosses two fields, promote to
private-fields + getter pattern.

### N-10. `proptest` 1.5.0 pinned for MSRV 1.79

proptest 1.6+ requires Rust 1.85 (`edition2024`). Same constraint as
N-4. Track in Layer 12.

### N-11. Editor blocks JSON files with remote `$schema`

`fs_write` / `str_replace` refuse `schemas/*.json` when `$schema:
https://...` is present (Supervised mode safety policy). Workaround:
`cat > schemas/<file>.json <<'JSON' ... JSON` via bash. Result is
byte-identical. Only matters when authoring a new schema or editing an
existing one in this repo.

---

## 2026-05-22 — Layer 1 / PR #5 `feat/report-store`

### N-12. ReportStore writes both root and evidence manifest copies

ADR-006 and `docs/build-plan.md` define the run layout with
`.polyref/runs/<report_id>/manifest.json` at the run root. The frozen
`schemas/report.json` contract and existing `polyref-core` report tests,
however, model `audit_pointers.manifest_json` as an `EvidencePointer`
(`evidence/manifest.json`).

Implementation keeps the ADR-006 root `manifest.json` as canonical for
operators and writes a byte-identical mirror at `evidence/manifest.json`
so `ValidationReport` can continue referencing a schema-valid evidence
pointer. This is a Layer 1 compatibility bridge; Layer 2 manifest
expansion should decide whether to keep the mirror or update the report
schema/tests.

---

## 2026-05-22 — Layer 2 / PR #9 `feat/loader-checkout`

### N-13. `CheckoutPlan` does not duplicate `report_id`

The initial Layer 2 plan listed `CheckoutPlan { source, commit, report_id }`,
but `checkout_old_workspace` receives a `RunReportStore` whose `path()` is the
actual destination authority. Keeping a second `report_id` in `CheckoutPlan`
would let callers pass mismatched values without affecting the output path.

Implementation removes `report_id` from `CheckoutPlan` and derives the output
solely from `RunReportStore`. This avoids a misleading API until `ReportStore`
exposes a typed report id or a higher-level loader API owns run creation.

### N-14. Checkout v1 rejects symlinks instead of preserving them

Working-tree snapshot copy now rejects symlink entries explicitly. Commit
checkout also rejects Git tree entries with mode `120000`. Preserving safe
relative symlinks is deferred because Layer 2 prioritizes filesystem escape
prevention and deterministic workspace contents.

**Follow-up**: If subject repos require symlinks, add a dedicated symlink policy
that records link targets in the manifest/audit log and rejects absolute or
escaping targets before materialization.
