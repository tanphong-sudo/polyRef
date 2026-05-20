# Contributing to PolyRef

## Prerequisites

- Rust toolchain ≥ 1.79 (install via [rustup](https://rustup.rs/))
- Python ≥ 3.10 (for schema validation and plugin development)
- `cargo-deny` (`cargo install --locked cargo-deny`)

## Branch strategy

| Branch | Purpose |
|--------|---------|
| `main` | Stable — all CI gates pass. Never push directly. |
| `feat/<layer>-<description>` | Feature work per build-plan layer. |
| `fix/<description>` | Bug fixes. |
| `docs/<description>` | Documentation-only changes. |
| `chore/<description>` | Tooling, CI, dependency updates. |

Always branch from `main`. Push with `-u` for new branches.

## Commit format

```
<type>(<scope>): <short description>

<optional body explaining why>
```

**Types**: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`

**Scopes**: `polyref-core`, `polyref-checker-spi`, `polyref-graph`, `polyref-loader`, `polyref-frontier`, `polyref-engine`, `polyref-rewriter`, `polyref-report`, `polyref-cli`, `schemas`, `coq`, `eval`, `ci`

Examples:
```
feat(polyref-core): implement EntityId parser with NFC validation
fix(polyref-core): reject bidi overrides in local_path segment
test(polyref-core): add property test prop_entity_id_roundtrip
docs(schemas): bump schema version to 0.2.0
ci: add MSRV matrix to CI workflow
```

## Development workflow

1. Read the layer spec in [`docs/build-plan.md`](docs/build-plan.md)
2. Create a feature branch: `git checkout -b feat/layer0-ids`
3. Write failing tests first (TDD RED)
4. Implement minimum to pass (GREEN)
5. Refactor while tests stay green
6. Run verification (see below)
7. Commit with conventional format
8. Open a PR against `main`

## Verification before commit

All four must pass:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Schema validation:

```bash
bash scripts/verify-schemas.sh
```

## Pull request process

1. PR title: concise, under 70 characters
2. PR body must include:
   - Summary of changes
   - Which layer this belongs to
   - What was tested
3. All CI checks must pass
4. At least one approval required

## Code style

- `unsafe` is **forbidden** workspace-wide
- `cargo clippy` warnings are errors
- Missing docs on public items produce warnings
- See [`docs/adrs/`](docs/adrs/) for architectural decisions — do not contradict them without a new ADR

## Testing requirements

- Every public function has a unit test
- Property-based tests (`proptest`) for invariants
- Coverage target: 80%+ on `polyref-core` and `polyref-checker-spi`
- See [`docs/verification.md`](docs/verification.md) for the full gate matrix

## Key invariants (never violate)

1. **Fail-closed**: No accepted row with `missing_endpoint_unknown = true`
2. **A2 ordering**: Algorithm A2 step order is load-bearing (test-locked)
3. **Type-respecting μ**: Migration map compares `kind` segment only, NOT `language`
4. **Outcome sum type**: `Pres`/`Migrated` can never carry a reason field
5. **ID validation**: All IDs parsed via `parse()` — no bypass
6. **Deterministic**: Same inputs → byte-identical report

## Adding a new correspondence kind or reason variant

1. Add the variant to the JSON Schema in `polyref/schemas/`
2. Bump `schemas/_meta/version.json`
3. Add entry to `schemas/CHANGELOG.md`
4. Add the Rust variant to the corresponding enum
5. Run `bash scripts/schema-bindings-check.sh` to verify no drift
6. Update tests

## Questions?

Read the docs in order: [`docs/overview.md`](docs/overview.md) → [`docs/architecture.md`](docs/architecture.md) → [`docs/build-plan.md`](docs/build-plan.md).
