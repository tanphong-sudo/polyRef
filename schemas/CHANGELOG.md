# PolyRef Schema Package Changelog

The schemas under `schemas/` are the source of truth for the cross-language
contract between the Rust host (`polyref-core`, `polyref-checker-spi`) and
the polyglot plugins (`plugins/extractor-*`, `plugins/checker-*`).

Adding a new enum variant or a new required field is a **minor** version
bump. Removing a variant or making an optional field required is a
**major** version bump and breaks plugin compatibility.

## 0.1.0 — Layer 0 skeleton

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
- `audit-event.json` *(placeholder; expand in Layer 1)*
- `manifest.json` *(placeholder; expand in Layer 1)*

Reason enum order is **lexicographic ascending of variant names** per
ADR-005.

## 0.2.0 — Layer 1 audit log

`audit-event.json` no longer a placeholder. Closed `tag` enum (14
members per ADR-006) plus required `actor` and `payload_hash` fields
for replay verification:

- `artifact_classified`
- `checker_invoked`
- `checker_result`
- `correspondence_created`
- `entity_emitted`
- `extractor_invoked`
- `frontier_computed`
- `frontier_item_status_assigned`
- `migration_map_built`
- `obligation_emitted`
- `observation_rewritten`
- `observation_status_assigned`
- `replay_completed`
- `repo_loaded`
- `report_finalized`
- `sandbox_denied`
- `sandbox_started`

Tag enum order is **lexicographic ascending of variant names** per
ADR-005 (hard blocker F-2).

`additionalProperties` is now `false` on the AuditEvent envelope; this
is a soft break for any consumer that was relying on the placeholder
permitting unknown keys, but no such consumer exists in-tree (the audit
log was a placeholder).

`payload_hash` is `^[a-f0-9]{64}$` (lowercase hex SHA-256). Per ADR-006
the chain-of-hashes is reconstructable from cache; the actual payload
lives off-line so leakage of held-out observation fields cannot happen
through the audit log.
