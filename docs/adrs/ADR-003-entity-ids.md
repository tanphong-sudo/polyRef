# ADR-003: Entity IDs are content-derived, language-tagged, and span-stable

## Context
Entity IDs must be:
- stable across re-extractions of the same artifact content,
- distinguishable across `R` and `R'` (since μ rewrites old → new),
- capable of carrying ambiguity (multiple candidate matches for a route),
- canonical for cache keys and audit logs.

gpt.md left this as an open decision.

## Decision

```
EntityId := <repo_side> ":" <language> ":" <kind> ":" <local_path> ":" <stable_hash>

repo_side    := "old" | "new"
language     := "ts" | "py" | "java" | "openapi" | "jsonschema" | "sql" | "yaml" | "dockerfile" | "json" | "build"
              // "build" covers package manifests + build scripts (package.json, pyproject.toml,
              // pom.xml, build.gradle, Bazel BUILD, Makefile, CMakeLists.txt, lockfiles).
              // It is the language tag for the "build_file" ArtifactKind in 01-architecture §1.4.
kind         := the EntityKind discriminant in lowercase ("route", "handler", "schema", "table", ...)
local_path   := slash-joined logical path, e.g. "src/users.py#create_user"
stable_hash  := first 12 hex chars of SHA-256(canonical(local_facts_payload))
```

Rules:
- `local_path` uses POSIX separators and a `#` to separate file from in-file anchor; in-file anchors use `:` to separate scopes (e.g. `User#methods:setName`).
- `canonical(local_facts_payload)` is the JSON canonicalization of the entity's `{kind, local_name, type, source_span}` minus byte offsets (line/column kept). This makes the id robust to whitespace-only edits but sensitive to type/name changes.
- For generated artifacts the id includes the generator command digest so a regenerated client gets a new id.

`MigrationMap` is `Map<EntityId(old:...), EntityId(new:...)>`. The **type-respecting check enforces matching `kind` segments only — not `language`**. Paper §3.3 Definition 5 defines `type-respecting ⟺ type(n) = type(μ(n))`, where `type: N → T` is the *local kind* (handler, schema field, SQL table, …) per §3.1 Definition 1. Cross-language migrations (TS handler ↔ JS handler when both extract to the same `handler` kind, OpenAPI YAML ↔ JSON-Schema JSON for the same schema-field kind, generated client toolchain swap) are first-class and must pass. Comparing `language` segments would over-reject the very cases the paper is designed to validate.

### Ambiguity handling
When multiple candidate entities match a single endpoint slot, the graph stores an `AmbiguityRecord` listing the candidate `EntityId`s with confidence scores. Frontier validator surfaces it as `Unknown(reason=AmbiguousEndpoint)` until evidence narrows it down or a developer annotation pins one.

### ID for build edges
```
EdgeId := "edge:" <kind> ":" <source_artifact_id> "->" <target_artifact_id> ":" <hash(generator)>
```

## Consequences

### Positive
- Deterministic. Cache keys + audit pointers are reproducible.
- Type-respecting check is a single string-segment comparison (`kind` only) and matches paper §3.3 Definition 5 exactly.
- Ambiguity is first-class, not silently dropped.
- Span-stable across cosmetic edits.

### Negative
- Renames change the id. That is correct for our purposes (a rename is an entity migration), but cross-id continuity must come from `MigrationMap`, not id equality.
- Hash is short (12 hex chars). Collision risk negligible at expected entity counts (< 10⁶ per run); spot-checked in tests.

### Alternatives considered
- **UUIDs**: rejected. Not deterministic across runs.
- **Fully qualified syntactic path only**: rejected. Refactoring (renames) destroys the id without the hash component capturing edit-tolerance.
- **Tree-sitter node hashes only**: rejected. Too brittle to whitespace.

## Status
Accepted

## Date
2026-05-19
