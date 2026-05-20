# Contributing to PolyRef

## Prerequisites

- Rust ≥ 1.79 ([rustup](https://rustup.rs/))
- Python ≥ 3.10 (schema validation, plugins)
- `cargo-deny` (`cargo install --locked cargo-deny`)

## Workflow (GitHub Flow)

1. Branch from `main`: `git checkout -b feat/<module>-<description>`
2. Write tests first (TDD RED → GREEN → REFACTOR)
3. Run quality gate (see below)
4. Commit with conventional format
5. Push: `git push -u origin feat/<module>-<description>`
6. Open PR → CI must pass → squash merge into `main`

## Branch naming

```
feat/<module>-<description>   # feature work
fix/<description>             # bug fix
docs/<description>            # docs only
chore/<description>           # CI, tooling, deps
```

Name by module/task, not by layer. Keep branches short-lived.

## Commit format

```
<type>(<scope>): <description under 70 chars>
```

Types: `feat` `fix` `refactor` `docs` `test` `chore` `perf` `ci`

Scopes: `polyref-core`, `polyref-checker-spi`, `polyref-graph`, `polyref-loader`, `polyref-frontier`, `polyref-engine`, `polyref-rewriter`, `polyref-report`, `polyref-cli`, `schemas`, `coq`, `eval`, `ci`

## Quality gate (before every commit)

CI sets `RUSTFLAGS: "-D warnings"`. Run the gate with the same env so
warnings can't pass locally and fail CI:

```bash
export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check bans licenses
bash scripts/verify-schemas.sh
bash scripts/schema-bindings-check.sh
```

All must pass. Do not push if any fails. Wrapper: `.kiro/scripts/quality-gate.sh`.

## Pull requests

- **CI must be green on the head commit before opening or marking ready-for-review.**
  After every push: `gh run list --branch <branch> --limit 1`. Inspect failures with
  `gh run view <run-id> --log-failed`. Don't open the PR until status is `success`.
- Title: conventional-commit style, under 70 chars
- Body: what changed, why, which layer, what was tested
- Draft PR for WIP; mark ready-for-review when CI green
- **Squash merge** into `main` (default)
- Delete branch after merge

## CI pipeline

PR triggers: schemas validation → Rust build/test/clippy (MSRV matrix) → cargo-deny → schema drift check → security (dependency review).

## Key invariants (never violate)

1. **Fail-closed**: No accepted row with `missing_endpoint_unknown = true`
2. **A2 ordering**: Algorithm A2 step order is load-bearing (test-locked)
3. **Type-respecting μ**: Migration map compares `kind` segment only, NOT `language`
4. **Outcome sum type**: `Pres`/`Migrated` can never carry a reason field
5. **ID validation**: All IDs parsed via `parse()` — no bypass
6. **Deterministic**: Same inputs → byte-identical report

## Adding a new enum variant

1. Add to JSON Schema in `schemas/`
2. Bump `schemas/_meta/version.json`
3. Add to `schemas/CHANGELOG.md`
4. Add Rust variant to the corresponding enum
5. Run `bash scripts/schema-bindings-check.sh`
6. Update tests

## Code style

- `unsafe` forbidden workspace-wide
- `cargo clippy` warnings are errors
- No `From<String>` on ID types
- No wildcard `_` match on business enums
- No `.unwrap()` in production code

## Docs

[`docs/overview.md`](docs/overview.md) → [`docs/architecture.md`](docs/architecture.md) → [`docs/build-plan.md`](docs/build-plan.md) → [`docs/verification.md`](docs/verification.md)
