# ADR-008: Decision granularity is per-observation, candidate is meet-of-observations; SLOs codified

## Context
gpt.md missing decision (6): per-observation vs per-repository vs both. Also no SLOs in gpt.md. Production-grade prototype must ship measurable targets.

## Decision

### Granularity
- The atomic decision is **per observation**: `accepted | broken | unknown`.
- Per observation, acceptance quantifies over `required(o)` — the o-relative frontier plus the intermediate build/codegen edges on paths to `supp(o)` (see architecture.md, "Affected frontier and required(o)"), not over a bare `supp(o)` slice. `o` is `Accepted` iff every item in `required(o)` is `Pres`/`Migrated` and its required local/build/observation obligations validate.
- The **candidate decision** is the meet over the candidate's selected observations:
  - `Accepted` iff every selected observation is `Accepted`.
  - `Broken` iff any selected observation is `Broken`.
  - `Unknown` otherwise.
- The **task-level rate** in the evaluation harness counts whole candidates; this lives in the harness, not the engine.

### Visible vs held-out vs evaluation-only observations
- `visible` — used by proposal methods, build/test baselines, and PolyRef.
- `held_out` — only consulted **after** PolyRef returns; used to compute false-accept and false-reject rates.
- `evaluation_only` — never used by any method; reserved for the empirical harness's seeded oracles.

The candidate decision is computed using **only `visible` observations**. Held-out and evaluation-only observations are reported separately and never affect acceptance.

### SLOs (Service-level objectives)
| SLO | Target | Measurement |
| --- | --- | --- |
| Median end-to-end validation latency | ≤ 30 min | per-task wallclock from `repo_loaded` to `report_finalized` |
| p95 validation latency | ≤ 60 min | same |
| Median extraction latency | ≤ 5 min | sum of `extractor_invoked → entity_emitted` per artifact |
| Median frontier computation | ≤ 30 s | per observation |
| Median per-checker call | ≤ 10 s | `checker_invoked → checker_result` |
| p95 per-checker call | ≤ 60 s | same |
| Plugin failure rate | ≤ 1% | `checker_result.outcome == Unknown(PluginFailure)` / total |
| Cache hit rate (warm) | ≥ 0.5 | memoization hits / extractor + checker calls |
| Replayability | 100% | CI test re-runs report from cache; bytes match |
| Fail-closed invariant violations | 0 | accepted rows with `missing_endpoint_unknown=true` |

### Quality targets (paper-aligned)
| Quality | Target | Measurement |
| --- | --- | --- |
| FA-rate (LLM+PolyRef) on the 180-task corpus | ≤ 0.12 | held-out + seeded oracle |
| Extraction precision (audit) | ≥ 0.83 | stratified two-reviewer audit |
| Extraction recall (audit) | ≥ 0.79 | same |
| Bounded-fragment coverage | ≥ 0.85 | of the 180 tasks |

### Error budget
- Latency error budget: 1 quarter of breaches in the SLO window before triggering perf work.
- Quality error budget: any FA-rate breach > 0.12 over 4 consecutive runs triggers a root-cause review.

## Consequences

### Positive
- Crisp granularity: report consumers know exactly what an `Accepted` claim covers.
- SLOs are testable in CI: synthetic 30-task suite measures latency and quality every nightly.
- Held-out observations are protected from leakage by definition.

### Negative
- Per-observation granularity means a candidate touching many observations has many checks. Mitigated by per-observation parallelism (ADR-007) and memoization (ADR-006).

### Alternatives considered
- **Per-repository decision only**: rejected. Loses information; report consumers want to know *which* observation failed.
- **Per-frontier-item decision only**: rejected. Hides whether the failure matters for any actual observation.

## Status
Accepted

## Date
2026-05-19
