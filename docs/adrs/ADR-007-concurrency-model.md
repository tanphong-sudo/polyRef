# ADR-007: Concurrency model is async-task-per-observation, bounded plugin pool, no shared mutable graph

## Context
Median 43-minute validation budget pushes us to parallelize. But the data flow has dependencies: extraction must finish before graph; graph before frontier; frontier before obligations; obligations before checker fan-out. We need a model that gets parallel speed without race conditions on the graph.

## Decision

### Stages and parallelism
| Stage                  | Pattern                                         |
| ---------------------- | ----------------------------------------------- |
| Extraction             | Parallel per-artifact, bounded by `min(num_cpus, 8)` worker tasks. |
| Graph build            | Single-writer ingest into the SQLite GraphStore (one writer task), feeding from the extractor results queue. |
| Migration-map build    | Single task. Cheap. |
| Observation registry   | Single task. Cheap. |
| Frontier computation   | Parallel **across observations**; serial within each observation (least-closure traversal). |
| Obligation generation  | Parallel **across frontier items**. |
| Kind-checker fan-out   | Async tasks fed through a per-kind bounded queue; each kind has a small pool of long-lived plugin processes. |
| Observation rewriter   | Parallel **across observations**. |
| Status assignment      | Per (observation, item); merged in deterministic order at the end. |
| Report emission        | Single task. |

### Plugin pool
Each kind plugin runs as N long-lived processes (default N=2; CPU-bound checkers like schema-diff get N=4; configurable per plugin). The host maintains a per-kind work queue and dispatches `check` requests to the next free process. A plugin process that crashes is replaced; the request that crashed it returns `Unknown(PluginFailure)`. A plugin process is recycled (gracefully restarted) every M requests (default 200) to bound memory growth.

### Determinism in the face of parallelism
- All async tasks emit results into channels keyed by `(observation_id, item_id)`.
- A reducer collects results and assigns statuses **in lexicographic order of `(observation_id, item_id)`**, not in completion order. Result: the same input produces the same report bytes.
- Audit-log writes go through a single sink task; sink writes events in a deterministic order keyed by `(stage, observation_id, item_id, sub_step)`.

### Cancellation
A user-issued cancel propagates through a `tokio::CancellationToken`. In-flight plugin calls are killed; the report is written with whatever statuses completed plus `Unknown(reason=CheckerTimeout)` for the rest. The candidate decision becomes `Unknown` (never silently `Accepted`).

## Consequences

### Positive
- Linear speedup proportional to `num_observations × num_frontier_items`, the dominant loops.
- Plugin pool keeps process-startup cost amortized.
- Deterministic ordering at the reducer means parallelism does not break replayability.

### Negative
- Plugin pools mean memory cost scales with `Σ_kinds(N_k × resident_size)`. Bounded; profiled in CI.
- SQLite single-writer is a serialization point during graph build. Mitigated by extractor outputs being bulk-loaded in batches.

### Alternatives considered
- **One thread per checker call**: rejected. Process-spawn cost dominates at the per-call level.
- **Shared in-memory graph + parallel writers**: rejected. Race-condition risk and worse repeat-run determinism.
- **Erlang-style actor system**: overkill for a single-host CLI/library.

## Status
Accepted

## Date
2026-05-19
