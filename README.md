# PolyRef

Semantics-preserving multilingual refactoring via typed program correspondences.

PolyRef is a **validator** — it does not propose edits. Given a candidate refactoring `ρ = (Δ, μ)` and a repository `R`, it computes the affected typed-correspondence frontier and emits an auditable report: `accepted`, `broken`, or `unknown` per observation.

## Quick start

```bash
cd polyref
cargo build --workspace
cargo test --workspace
bash scripts/verify-schemas.sh
```

## Repository layout

```
polyref/          Rust workspace (core types, plugin SPI, schemas)
docs/             Architecture, ADRs, build plan, verification
```

## Status

**Slice 1** — core IR skeleton. Type stubs with `todo!()` bodies; tests in TDD red state (`#[ignore]`). See [`docs/slice-1-core-ir.md`](docs/slice-1-core-ir.md) for the implementation spec.

## Documentation

| Document | Content |
|----------|---------|
| [`docs/overview.md`](docs/overview.md) | What PolyRef is and is not |
| [`docs/architecture.md`](docs/architecture.md) | Components, data flow, security, KPIs |
| [`docs/build-plan.md`](docs/build-plan.md) | Dependency-ordered implementation layers |
| [`docs/verification.md`](docs/verification.md) | Gate matrix, test strategy, SLOs |
| [`docs/slice-1-core-ir.md`](docs/slice-1-core-ir.md) | Current slice spec |
| [`docs/adrs/`](docs/adrs/) | Architecture Decision Records (ADR-001 … ADR-010) |

## License

Apache-2.0 (see [`polyref/LICENSE`](polyref/LICENSE))
