# 01 — Overview

## Purpose

PolyRef validates whether a candidate multilingual refactoring preserves the observable behaviour of a repository. Validation is per-observation; the candidate decision is the meet over visible observations.

## What PolyRef does

- Loads an old repository `R` and applies a candidate `ρ = (Δ, μ)` inside a sandbox to obtain `R' = apply(R, ρ)`.
- Extracts entities and local facts from artifacts in nine families (see [Artifact families](#artifact-families)).
- Builds a typed correspondence graph `(A, N, L, C, Build, O, owner, type)` per paper Definition 1.
- Computes the affected frontier `∂ρ(o)` for each observation as the least closure of paper Definition 7.
- Runs versioned kind checkers and assigns each frontier item one of `Pres / Migrated / Broken / Unknown` per Algorithm A2.
- Emits an auditable JSON + Markdown report. An `Accepted` candidate decision is impossible while `missing_endpoint_unknown == true` (fail-closed).

## What PolyRef is not

- Not a refactoring generator. Candidate edits come from IDEs, static templates, or LLMs and are untrusted input.
- Not a whole-language semantics model. Local language soundness is a checker assumption.
- Not a guarantee on every observation kind. Unsupported observations return `Unknown` and never `Accepted`.

## Paper anchors

| Concept | Paper reference |
| --- | --- |
| Repository graph | Definition 1 |
| Correspondence kind | Definition 2 + Table 3 |
| Refactoring `ρ` and migration map `μ` | Definition 5 |
| Observation semantics + support `supp(o)` | Definition 6 |
| Affected frontier `∂ρ(o)` | Definition 7 |
| Validation status set | Definition 8 |
| Accepted frontier | Definition 9 |
| Algorithm A1 / A2 | Figures 5 + 6 |
| Observation migration `μ(o)` | Definition 11 |
| Build closure | Lemma 2 + Assumption 3 |
| Coq theorems | Section 4.5 + Table 13 |

## Artifact families

Nine closed `ArtifactKind` members. The list combines paper §3.1 inline list with the build-file family stratified in Table 5.

| Kind | Examples |
| --- | --- |
| `source_file` | `*.ts`, `*.py`, `*.java` |
| `schema` | OpenAPI, JSON Schema, Avro, Protobuf |
| `query` | `*.sql`, ORM models |
| `config` | `*.env`, `application.yaml` |
| `workflow` | GitHub Actions, Jenkinsfile |
| `dockerfile` | `Dockerfile`, `Containerfile` |
| `build_file` | `package.json`, `pyproject.toml`, `pom.xml`, `Bazel BUILD`, `Makefile`, lockfiles |
| `generated` | Codegen output, regenerated SDKs |
| `test` | Test files |

`build_file` is the artifact family on which `Build ⊆ A × A` lives; build closure (Lemma 2) is grounded here.

## Status set and fail-closed convention

`Outcome = Pres | Migrated | Broken(BrokenReason) | Unknown(UnknownReason)`. Reasons are payloads of the negative variants only. The candidate decision rule:

```
candidate_decision =
  Accepted iff every visible observation is Accepted
  Broken   iff any  visible observation is Broken
  Unknown  otherwise
```

Held-out and evaluation-only observations never participate in the candidate decision. They are scored only after the report is produced.

## Trusted base

Trusted (audited via versioned logs, not proven by Coq):
- Per-language extractors and kind checkers.
- Local language tools (parsers, type checkers, schema differs, linters, build-system frontends, SMT/lint plugins).
- The sandbox.

Not trusted (always re-validated):
- Candidate proposers (IDE actions, static templates, LLM patches).
- Plugin processes — sandboxed and crash-isolated; failures map to `Unknown(PluginFailure | CheckerTimeout)`.
