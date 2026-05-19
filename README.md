# PolyRef

Semantics-preserving multilingual refactoring via typed program correspondences.

PolyRef is a **validator** for polyglot repository refactorings. It takes a candidate edit from an IDE, static template, or LLM, computes the affected typed-correspondence frontier, and reports whether the change is `accepted`, `broken`, or `unknown` — with auditable evidence for every decision.

## Quick start

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/verify-schemas.sh
```

## Repository layout

```
.
├── Cargo.toml              Rust workspace root
├── crates/                 Rust crates (core types, plugin SPI, ...)
├── schemas/                JSON Schemas — cross-language source of truth
├── scripts/                Schema validation, drift checks
├── docs/                   Architecture, ADRs, build plan, verification
├── .github/workflows/      CI (schemas, Rust MSRV matrix, cargo-deny)
├── CONTRIBUTING.md         How to contribute
└── LICENSE                 Apache-2.0
```

## Documentation

| Document | Content |
|----------|---------|
| [`docs/overview.md`](docs/overview.md) | What PolyRef is and is not |
| [`docs/architecture.md`](docs/architecture.md) | Components, data flow, security, KPIs |
| [`docs/build-plan.md`](docs/build-plan.md) | Dependency-ordered implementation layers |
| [`docs/verification.md`](docs/verification.md) | Gate matrix, test strategy, SLOs |
| [`docs/adrs/`](docs/adrs/) | Architecture Decision Records (ADR-001 … ADR-010) |

## License

Apache-2.0
