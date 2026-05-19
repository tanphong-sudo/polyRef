# PolyRef — Slice 1 Skeleton

Implementation skeleton for the PolyRef validator described in
`../claude/`. This directory contains only what Slice 1 of the build plan
in `../claude/03-build-plan.md` defines: schemas, Rust workspace skeleton,
core crate boundaries, test layout, CI/tooling files, and minimal
compiling type stubs.

**Out of scope for this slice**: extractor plugins, kind-checker plugins,
plugin process host, sandbox, graph builder, frontier computation,
validation engine, observation rewriters, CLI, Coq, empirical harness.

## Layout

```
polyref/
├── Cargo.toml              # workspace root
├── rust-toolchain.toml     # MSRV pinned to 1.79
├── .cargo/config.toml      # build settings
├── deny.toml               # cargo-deny supply-chain policy
├── schemas/                # JSON Schemas — cross-language source of truth
├── crates/
│   ├── polyref-core/       # ids, status, evidence, report types (no I/O)
│   └── polyref-checker-spi/# JSON-RPC envelope + extract/check payloads
├── scripts/                # schema validation + bindings drift check
└── .github/workflows/      # CI
```

## Status

This is a Slice 1 skeleton. Most function bodies are `todo!()` stubs.
Tests are present but `#[ignore]`-marked; they are the TDD red state for
the implementer to turn green per `../claude/05-handoff-1-core-ir.md`
section §E.

## Reading order

1. `../claude/05-handoff-1-core-ir.md` — the contract this skeleton must
   satisfy.
2. `../claude/01-architecture.md` — overall architecture.
3. `../claude/02-adrs/` — closed decisions.

## Hard blockers (from §G.1) still open

- F-5 RFC 8785 implementation source (in-house vs `serde_jcs`).
- F-7 EvidencePointer canonical form (regex frozen yet?).
- F-8 generated bindings strategy (codegen vs hand-written + drift check).

These must be closed before Layer 1 of `../claude/03-build-plan.md` can
ship its first Green build.

## Verifying the skeleton

```bash
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/verify-schemas.sh
```

The skeleton author's local machine did not have a Rust toolchain
installed at the time of authoring, so the workspace has not been
`cargo check`-ed locally. Run the commands above to confirm.
