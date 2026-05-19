# PolyRef — Documentation

PolyRef is a validator for multilingual (polyglot) refactorings. It does not propose edits. It takes a candidate refactoring `ρ = (Δ, μ)` against a repository `R`, computes the affected typed-correspondence frontier `∂ρ(o)` per observation, and emits an auditable report whose accepted rows are protected by a fail-closed contract.

This directory is the project's design and engineering reference. Read in order:

1. [`overview.md`](overview.md) — what PolyRef is and is not, paper anchors, system contract.
2. [`architecture.md`](architecture.md) — components, data flow, persistence, concurrency, security.
3. [`build-plan.md`](build-plan.md) — dependency-ordered implementation layers (the Slice plan).
4. [`verification.md`](verification.md) — gate matrix, test categories, KPI/SLO targets.
5. [`slice-1-core-ir.md`](slice-1-core-ir.md) — current implementation slice: types, schemas, plugin SPI envelopes.
6. [`adrs/`](adrs/) — Architecture Decision Records (ADR-001 … ADR-010).

Source code lives in [`../polyref/`](../polyref/). The cross-language JSON Schema package is at [`../polyref/schemas/`](../polyref/schemas/) and is the source of truth for every wire-format claim in these docs.
