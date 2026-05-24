# ADR-001: Host language is Rust for the validation core; polyglot plugins over JSON-RPC

## Context
PolyRef-core handles data-intensive graph operations (frontier closure, status assignment, audit-log writes) and must be deterministic, fast, and safe to embed in CI pipelines. Plugins wrap heterogeneous external tools (`oasdiff`, `actionlint`, `sqlfluff`, tree-sitter, TypeScript compiler API) where the ecosystem is split between JS, Python, and native binaries.

## Decision
- Validation core (`polyref-core`, `polyref-graph`, `polyref-frontier`, `polyref-engine`, `polyref-loader`, `polyref-rewriter`, `polyref-report`, `polyref-cli`) is written in Rust (edition 2021, `--deny warnings`, MSRV pinned).
- Plugins (extractors and kind checkers) speak the JSON-RPC SPI from ADR-002 and may be written in any language that can read JSON from stdin and write JSON to stdout.
- Reference plugins may be Rust or Python. Wrapper-heavy integrations default to Python when that ecosystem gives the safest adapter, while parser/checker plugins may use Rust when deterministic typed integration, MSRV-controlled CI, or native parser crates are the better fit.
- The Layer 4 route checker is the Rust example. The first OpenAPI and TypeScript extractors are also Rust plugins so the initial fixture contract, parser behavior, and sandbox/no-network assumptions are locked under the same audited toolchain.

## Consequences

### Positive
- Deterministic memory model and no GC variability, important for the 30-min median latency target.
- Strong types in Rust catch SPI / status / evidence model mismatches at compile time.
- Ecosystem fit: nsjail FFI, content-addressed stores, SQLite, tracing all production-grade in Rust.
- Plugin polyglot story works because all plugin↔host traffic is JSON-RPC over stdio; no shared library ABI.

### Negative
- Polyglot plugin repo. Need shared schemas and codegen (JSON Schema → Rust + Python types via `quicktype`) before adding non-Rust reference plugins.
- Rust learning curve for contributors used to Python when a plugin chooses Rust for determinism or native parser support.
- Cannot use Python's lightweight scripting in core paths.

### Alternatives considered
- **Pure Python core**: rejected. Memoization + sandbox + concurrent plugin pool with deterministic semantics is harder to ship in Python without GIL workarounds; latency budget at risk.
- **Pure TypeScript core**: rejected. Process management + sandbox FFI weaker than Rust; Coq bridge unnatural.
- **Go core**: viable. Rejected because Rust's enum types better model `ValidationStatus / UnknownReason / BrokenReason` and avoid accidental nil paths.
- **OCaml core (matches Coq)**: rejected. Smaller ecosystem for nsjail + SQLite + tracing.

## Status
Accepted

## Date
2026-05-19
