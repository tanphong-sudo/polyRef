# ADR-004: Admissibility of dynamic traces and generated-file policy

## Context
Two related gpt.md missing decisions: (3) when dynamic traces become typed evidence, and (8) what counts as adequate generated-file evidence (regeneration vs source maps vs checksum).

## Decision

### Dynamic-trace policy
A dynamic trace (route hit log, request log, generated-client replay) becomes admissible typed evidence iff **all** of:
1. The trace was produced under a sandboxed replay (ADR-009) with the same checkout commit and isolated environment.
2. Each row in the trace carries a typed endpoint reference (resolved to an `EntityId`) and source provenance.
3. The trace producer is registered in the checker contract's `requiredEvidenceFields` for that kind.
4. Replay is reproducible: re-running the producer on the same input yields a trace with the same endpoint set (set equality, ignoring order/timing).

Anything failing (1)–(4) → still raw input, **not** typed evidence; affected frontier items remain `Unknown(reason=DynamicEvidenceUnverified)`.

`v1 scope`: dynamic traces are advisory in v1 except for the `route` kind, where a sandboxed HTTP probe replay is admitted. Other kinds keep dynamic traces as documentation only.

### Generated-file policy
A build edge `source → generated_target` validates as `Pres` or `Migrated` only when:
1. The generator command is declared in a tracked manifest (`package.json` script, `pyproject.toml`, Bazel `BUILD`, Makefile target, GitHub Actions step).
2. **At least two** of the following hold (defense in depth):
   - Source map: a generator-emitted source map links generated symbols back to source spans.
   - Re-execution: re-running the generator inside the sandbox yields the same target file (byte-identical or up to known-deterministic post-processing).
   - Checksum manifest: the repo carries a checksum manifest with the recorded source-target pair, signed by CI.
3. The generated artifact's `EntityId`s carry the generator command digest (per ADR-003).

If only one of source map / re-execution / checksum is present → `Unknown(reason=GeneratedEvidenceWeak)`.
If none are present → `Unknown(reason=GeneratedEvidenceMissing)`.
If re-execution fails or differs from the committed file → `Broken(reason=GeneratorMismatch)`.

### Pres vs Migrated for a build edge
The generated-file policy above decides when an edge is eligible for `Pres`/`Migrated`
versus `Unknown`/`Broken`. Which of `Pres` or `Migrated` it gets depends on whether the
edge's **identity** changed, not on whether the artifact's bytes changed.

`μ` is defined on entities (Definition 5); a build edge is `artifact → artifact`. Lift μ
to artifacts through `owner` (`lift_μ`): an endpoint artifact `a` is *rewritten*
(`lift_μ(a) = a' ≠ a`) only when
- some entity `n` with `owner(n) = a` is in `dom(μ)` and `μ(n)` is owned by a different
  artifact / resolves to a different path (the owning artifact moves or is renamed), or
- `a` is a generated artifact whose source-spec entity is in `dom(μ)` (the generator
  input migrated, so the output path is rewritten accordingly).

Otherwise `lift_μ(a) = a` (identity). Then:
- **Migrated** — at least one endpoint is rewritten by `lift_μ`, the lifted edge is
  well-defined, and `migrate_build` holds for it.
- **Pres** — neither endpoint is rewritten (`lift_μ` is identity on both) and
  `compat_build` holds in `R'`.

Editing an artifact's **content** while keeping its path is **not** a rewrite: the build
edge keeps its endpoint identities, so it stays `Pres` (re-check `compat_build`) rather
than `Migrated`. Only a path/identity change via `lift_μ` produces `Migrated`. This keeps
the distinction aligned with Definition 8 ("`Pres` means … without changing endpoint
identities").

### Cyclic generators
If the generator graph has a cycle (e.g. OpenAPI → client → OpenAPI), the build edge's `Pres/Migrated` status is forfeit; status is `Unknown(reason=CyclicGenerator)`. Bounded fragment excludes cyclic generators per paper.

## Consequences

### Positive
- Closes ambiguity in gpt.md without weakening fail-closed.
- The two-of-three rule is auditable and graceful: source maps alone (common in TS) or re-execution alone (common in Python) plus a manifest crosses the bar.
- v1 scope keeps the dynamic-trace surface small; expansion documented for v2.

### Negative
- Two-of-three is conservative: some generators with strong source maps but no checksum will land in `Unknown`. Acceptable; that is what `Unknown` is for.
- Re-execution requires the generator to be runnable inside the sandbox; some commercial codegen tools may not be.

### Alternatives considered
- **Trust source maps only**: rejected. TS source maps are easy to forge; would weaken the contract.
- **Trust re-execution only**: rejected. Many generators are nondeterministic (timestamps, ordering); breaks reproducibility.
- **Trust checksum manifests only**: rejected. Requires repo discipline most projects do not have.
- **Always require all three**: rejected. Excessive false-Unknown rate; fails RQ5 practicality target.

## Status
Accepted

## Date
2026-05-19
