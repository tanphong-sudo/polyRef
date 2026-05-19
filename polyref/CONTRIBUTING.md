# Contributing — Slice 1

Slice 1 follows the TDD discipline from `../claude/05-handoff-1-core-ir.md`
section §E.

## Workflow per type

1. Read the type sketch in §C of the handoff.
2. Read the test list in §E for that type.
3. Write the test (RED).
4. Run `cargo test --workspace`.
5. Implement the minimum to make the test pass (GREEN).
6. Refactor, keep tests green.
7. Add property tests once the example tests pass.

## Slice boundaries

Do not add code that belongs to a later slice. The taboo list lives in
`../claude/05-handoff-1-core-ir.md` section §I.

## Lints

- `unsafe_code` is forbidden in every crate.
- `clippy::all` and `clippy::pedantic` are enabled.
- Run `cargo clippy --workspace --all-targets -- -D warnings` before any
  commit.

## Property-based tests

Property tests use `proptest`. Running them produces randomized inputs;
when your CI workflow runs them, surface that fact to the user.
