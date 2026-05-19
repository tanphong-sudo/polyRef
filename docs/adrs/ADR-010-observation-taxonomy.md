# ADR-010: Observation inventory taxonomy and discovery rules

## Context
gpt.md missing decision (10): which observations are user-declared, auto-discovered, visible, held-out, evaluation-only. The paper distinguishes these but gives no ingestion contract.

## Decision

### Two orthogonal axes
**Provenance** — how the observation entered the run:
- `user_declared` — written in `polyref.toml` or passed by `--observation`.
- `auto_discovered` — extracted by an `ObservationProbe` (a plugin variant that reads test files, OpenAPI examples, workflow targets).
- `seeded` — synthesized by the empirical harness for evaluation.

**Visibility** — when in the pipeline the observation may be consulted (ADR-008):
- `visible` — proposal + validation may read the observation.
- `held_out` — only the post-acceptance evaluator may read it.
- `evaluation_only` — never inspected by any method; oracle for the harness.

Every observation carries both axes. They are independent: a `user_declared` observation can be `held_out`.

### Observation kinds (closed set v1)
- `api_call` — typed fields: `method, path, query, request_schema_id, response_schema_id, client_id`.
- `test_invocation` — typed fields: `test_id, public_entrypoint_entity_id, expected_structured_value`.
- `build_target` — typed fields: `target_name, generator_command, expected_artifact_path`.
- `workflow_run` — typed fields: `workflow_id, packaged_target_name, env_keys`.
- `schema_validation` — typed fields: `schema_id, sample_payload_ref, expected_outcome`.

Each kind has a typed-field schema in `schemas/observation/<kind>.json`. Adding a new kind is a minor version bump.

### Auto-discovery rules
For each artifact, the matching `ObservationProbe` plugin emits zero or more candidate observations. The host applies these rules:
1. **De-duplicate** by `(kind, typed_fields canonical hash)`.
2. **Reject** observations whose typed fields cannot be resolved to entity ids in the graph (these become `defined_semantics=false`; see paper §3 partial semantics).
3. **Tag** all auto-discovered observations as `visible` by default. The user can promote any to `held_out` via config.

### Held-out leakage prevention
- The `held_out` set is loaded from a separate file (`held_out.toml`) and stored under `.polyref/held_out/<run_id>.json` with restricted permissions during the run.
- The validation engine receives only the `visible` set. The `held_out` set is presented to the post-acceptance evaluator over a different file descriptor; engine code that reads it triggers a CI-enforced lint failure.
- Audit log records which set each observation came from but never logs the typed fields of held-out observations until after the candidate decision.

### Observation `supp(o)` computation
At observation-registration time, the registry computes `supp(o)` by:
1. Resolving each typed-field reference to the corresponding `EntityId` or `EdgeId`.
2. Adding all `Correspondence` and `BuildEdge` records incident to those entities.
3. Recording any unresolved positions as a `coverage_risk` flag on the observation; if any are unresolved at validation time, the observation rewrite is undefined and the observation status is `Unknown(ObservationRewriteUndefined)`.

### Default observation set per kind of repo
- Web service: api_call (auto from OpenAPI examples), test_invocation (auto from test runners), workflow_run (auto from `.github/workflows/*`), build_target (auto from CI manifest), schema_validation (auto from JSON schemas).
- Library: build_target, test_invocation. (No api_call by default.)

Operators can override entirely via `polyref.toml`.

## Consequences

### Positive
- Clean separation of "what is observed" from "who can see it when".
- Auto-discovery covers the common case; explicit declaration available for edge cases.
- Held-out leakage prevented by structural separation, not just policy.

### Negative
- Probes are extra plugins to maintain. Mitigated by sharing tree-sitter parsers with extractors.
- Default sets need tuning per repo type; documented in operator guide.

### Alternatives considered
- **Single visibility-only axis**: rejected. Loses provenance, which the harness needs.
- **All observations user-declared**: rejected. Friction kills adoption.
- **Auto-discover only**: rejected. Some observations only the operator knows (e.g. a smoke test on a private endpoint).

## Status
Accepted

## Date
2026-05-19
