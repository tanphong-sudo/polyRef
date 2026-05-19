# PolyRef Schema Package Changelog

The schemas under `schemas/` are the source of truth for the cross-language
contract between the Rust host (`polyref-core`, `polyref-checker-spi`) and
the polyglot plugins (`plugins/extractor-*`, `plugins/checker-*`).

Adding a new enum variant or a new required field is a **minor** version
bump. Removing a variant or making an optional field required is a
**major** version bump and breaks plugin compatibility.

## 0.1.0 — Slice 1 skeleton

Initial schema set:

- `_meta/version.json`
- `ids/{entity-id, artifact-id, correspondence-id, edge-id}.json`
- `source-span.json`
- `artifact-kind.json` (9 members per architecture §1.4)
- `language.json` (per ADR-003 incl. `build`)
- `correspondence-kind.json` (10 members per paper Table 3)
- `validation-status.json`
- `unknown-reason.json`
- `broken-reason.json`
- `evidence.json`
- `evidence-pointer.json`
- `migration-map.json`
- `observation/{_kind, api-call, test-invocation, build-target,
  workflow-run, schema-validation, visibility}.json`
- `checker-spi/{describe, check}.json`
- `extractor-spi/extract.json`
- `report.json`
- `audit-event.json` *(placeholder; expand in Slice 2)*
- `manifest.json` *(placeholder; expand in Slice 2)*

Reason enum order is **lexicographic ascending of variant names** per
hard blocker F-2 in `../claude/05-handoff-1-core-ir.md`.
