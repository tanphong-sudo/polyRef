# 05 — Layer 0: Core IR + Schemas + Plugin SPI

Layer 0 ships the type substrate every later layer imports unchanged: schemas, core types, plugin SPI envelopes, test layout, CI/tooling. Two crates only — `polyref-core` and `polyref-checker-spi` — and 28 JSON Schemas. No runtime I/O in either crate.

## Files

```
polyref/
├── Cargo.toml  rust-toolchain.toml  .cargo/config.toml
├── deny.toml  LICENSE  README.md  CONTRIBUTING.md
├── .gitignore  .gitattributes  .github/workflows/ci.yml
├── schemas/                          (28 JSON Schemas — source of truth)
│   ├── _meta/version.json
│   ├── ids/{entity, artifact, correspondence, edge}-id.json
│   ├── source-span.json  artifact-kind.json  language.json
│   ├── correspondence-kind.json
│   ├── validation-status.json  unknown-reason.json  broken-reason.json
│   ├── evidence.json  evidence-pointer.json  migration-map.json
│   ├── observation/{_kind, visibility, api-call, test-invocation,
│   │                build-target, workflow-run, schema-validation}.json
│   ├── checker-spi/{describe, check}.json
│   ├── extractor-spi/extract.json
│   ├── report.json  audit-event.json  manifest.json
│   └── CHANGELOG.md
├── crates/
│   ├── polyref-core/
│   │   ├── Cargo.toml
│   │   └── src/{lib, ids, source_span, artifact_kind, language,
│   │            correspondence_kind, status, evidence, migration_map,
│   │            canonical, error, report, observation/{mod, visibility}}.rs
│   │   └── tests/red_tests.rs
│   └── polyref-checker-spi/
│       ├── Cargo.toml
│       └── src/{lib, envelope, limits, error, extractor, checker}.rs
│       └── tests/red_tests.rs
└── scripts/{verify-schemas.sh, schema-bindings-check.sh}
```

## Type contracts

### IDs

```rust
pub struct EntityId(String);
pub struct ArtifactId(String);
pub struct CorrId(String);
pub struct EdgeId(String);
```

Each id is a newtype with a private inner string. Construction is via `parse(&str) -> Result<Self, IdParseError>` only; serde routes through `parse`.

`EntityId` grammar (ADR-003):

```
<repo_side>:<language>:<kind>:<local_path>:<stable_hash>
repo_side  := "old" | "new"
language   := one of language.json
kind       := lowercase EntityKind discriminant
local_path := POSIX path with optional `#` anchor
stable_hash:= 12 hex chars of SHA-256 over canonicalized local-facts payload
```

Type-respecting check on `MigrationMap` compares the **kind** segment only — not language. This is the paper Definition 5 contract; cross-language migrations (TS ↔ JS handler, OpenAPI YAML ↔ JSON-Schema JSON, generated client toolchain swap) are first-class.

### SourceSpan

```rust
pub struct SourceSpan { /* private fields */ }
impl SourceSpan {
    pub fn try_new(artifact: ArtifactId, start: LineCol, end: LineCol,
                   utf16_cols: Option<(u32, u32)>) -> Result<Self, SpanError>;
}
pub struct LineCol { pub line: NonZeroU32, pub col: u32 }
```

`try_new` rejects inverted ranges and inverted UTF-16 column pairs. `NonZeroU32` rules out line 0 statically. Serde routes through `try_new`.

### Status — invariant-locking sum

```rust
pub enum Outcome {
    Pres,
    Migrated,
    Broken  { reason: BrokenReason  },
    Unknown { reason: UnknownReason },
}
```

Reasons are payloads of `Broken` and `Unknown` only. Code that tries to attach a reason to `Pres` or `Migrated` does not compile. `UnknownReason` and `BrokenReason` are closed enums in JSON Schema (per schema version) and `#[non_exhaustive]` Rust enums (for defensive matching across crate boundaries). Adding a variant requires a schema minor bump.

### Evidence

```rust
pub struct Evidence { /* private */ }
impl Evidence {
    pub fn ok_pres(predicate, spans, pointers, checker_version, rule_version) -> Self;
    pub fn ok_migrated(predicate, spans, pointers, checker_version, rule_version) -> Self;
    pub fn broken (reason, predicate, spans, pointers, checker_version, rule_version) -> Self;
    pub fn unknown(reason, predicate, spans, pointers, checker_version, rule_version) -> Self;
    pub fn outcome(&self) -> &Outcome;
}
pub struct EvidencePointer(/* validated path under evidence/ */);
```

`EvidencePointer::parse` accepts only relative POSIX paths matching `^evidence/[A-Za-z0-9_./-]{1,512}$`. Anything else returns `EvidencePointerError`.

### MigrationMap

```rust
pub struct MigrationMap { /* private */ }
impl MigrationMap {
    pub fn try_new(rewrites: BTreeMap<EntityId, EntityId>,
                   obs_part: Vec<ObsPartRewrite>,
                   conflicts: Vec<MigrationConflict>) -> Result<Self, MigrationMapError>;
    pub fn get(&self, k: &EntityId) -> Option<&EntityId>;
    pub fn iter(&self) -> impl Iterator<Item = (&EntityId, &EntityId)>;
    pub fn is_type_respecting(&self) -> bool;
}
```

`try_new` rejects rewrites whose **kind** segments differ. The `language` segment is ignored, so cross-language rewrites are accepted when the local kinds match.

### ValidationReport

```rust
pub struct ValidationReport { /* private */ }
impl ValidationReport {
    pub const SCHEMA_VERSION: &'static str = "0.1.0";
    pub fn assemble(parts: ReportParts) -> Result<Self, ReportInvariantError>;
    pub fn candidate_decision(&self) -> CandidateDecision;
    pub fn missing_endpoint_unknown(&self) -> bool;
    pub fn observations(&self) -> &[ObservationRow];
}
pub enum CandidateDecision { Accepted, Broken, Unknown }
pub enum ReportInvariantError {
    MissingEndpointUnknownInAccepted,
    NonAcceptingItemInAcceptedObservation,
    EvidencePointerOutsideEvidenceDir,
    InvalidIdSyntax,
}
```

`assemble` is the only constructor. It rejects assembly when the fail-closed invariant would be violated, when an evidence pointer escaped the `evidence/` subtree, when a referenced id has invalid syntax, or when an `Accepted` observation contains a non-accepting item. Cross-graph reference resolution (verifying an `EntityId` exists in a `Repository`) is not in this layer; it lives in `polyref-graph`.

### Plugin SPI envelopes

`polyref-checker-spi` exposes the wire types only. The plugin process pool and dispatcher are Layer 3.

```rust
pub struct ExtractRequest  { artifact_path: SafePath, content_hash: String,
                             language: Language, options: serde_json::Value,
                             deadline_ms: u32, log_dir: SafePath }
pub struct ExtractResult   { entities, local_facts, unsupported_features,
                             extractor_version }

pub struct DescribeResult  { contract_id, kind_id, endpoint_signature,
                             required_evidence_fields, compat_rule_id,
                             migrate_rule_id, plugin_version, default_timeout_ms,
                             supported_unknown_reasons, supported_broken_reasons }

pub struct CheckRequest    { contract_id, kind, endpoints,
                             old_repo_root: SafePath, new_repo_root: SafePath,
                             migration_map_excerpt, observation_excerpt,
                             deadline_ms, log_dir: SafePath }
pub type   CheckResult     = Evidence;
```

`SafePath` wraps `String` and is **always interpreted relative to a sandbox or run root**, never host-absolute. Its parser rejects absolute paths, parent-traversal segments, NUL, control codepoints, bidi overrides, and zero-width characters.

## Hard caps (defense in depth)

| Cap | Default | Site |
| --- | --- | --- |
| Wire payload size | 16 MiB | `polyref-checker-spi::limits::Limits` |
| JSON nesting depth | 64 | same |
| EntityId length | 16 KiB | `polyref-core::ids::ID_MAX_LEN` |
| SafePath length | 4 KiB | `polyref-checker-spi::limits::Limits` |
| Per-call deadline | 600 s | `polyref-checker-spi::limits::Limits` |
| Canonical-JSON payload | 16 MiB | `polyref-core::canonical::PAYLOAD_MAX_BYTES` |

## TDD red-state checklist

The implementer turns each `#[ignore]`-marked test green by implementing the corresponding parser, builder, or invariant. The list below is exhaustive for Layer 0.

**Schemas**
- `schemas_validate_under_draft_2020_12`
- `enum_files_have_no_open_extension_points`
- `unknown_reason_enum_is_source_of_truth`
- `broken_reason_enum_is_source_of_truth`
- `artifact_kind_has_exactly_nine_members`
- `correspondence_kind_has_expected_members`
- `report_schema_requires_missing_endpoint_unknown_field`

**`polyref-core::ids` (mirror per id type)**
- `entity_id_parse_accepts_canonical_form`
- `entity_id_parse_rejects_empty`
- `entity_id_parse_rejects_no_separator`
- `entity_id_parse_rejects_invalid_repo_side`
- `entity_id_parse_rejects_invalid_language`
- `entity_id_parse_rejects_invalid_kind`
- `entity_id_parse_rejects_path_with_null_byte`
- `entity_id_parse_rejects_path_with_parent_traversal`
- `entity_id_parse_rejects_oversize_input`
- `entity_id_parse_rejects_control_chars`
- `entity_id_parse_rejects_bidi_overrides`
- `entity_id_parse_rejects_zero_width_chars`
- `entity_id_parse_normalises_to_nfc`
- `entity_id_serde_roundtrip_is_identity`
- `entity_id_serde_does_not_bypass_parse`
- `entity_id_segments_are_extractable`
- `entity_id_eq_is_segment_aware`
- Property: `prop_entity_id_roundtrip`
- Property: `prop_entity_id_no_bypass`

**`polyref-core::source_span`**
- `source_span_rejects_inverted_range`
- `source_span_rejects_zero_line` *(redundant with `NonZeroU32`; keep as compile-fail trybuild)*
- `source_span_utf16_columns_optional`
- `source_span_serde_roundtrip`

**`polyref-core::status` and `evidence`**
- `outcome_pres_has_no_reason_field` *(trybuild compile-fail)*
- `outcome_broken_carries_reason`
- `outcome_unknown_carries_reason`
- `evidence_ok_pres_has_outcome_pres` *(already green)*
- `evidence_broken_has_outcome_broken_with_reason` *(already green)*
- `evidence_unknown_has_outcome_unknown_with_reason` *(already green)*
- `evidence_pointer_rejects_path_outside_evidence_dir`
- `evidence_pointer_rejects_absolute_path`
- `evidence_pointer_rejects_parent_traversal`
- Property: `prop_outcome_idempotent_serde`
- Property: `prop_evidence_no_reason_for_pres_or_migrated`

**`polyref-core::migration_map`**
- `migration_map_rejects_kind_mismatch`
- `migration_map_allows_language_mismatch_when_kinds_match`
- `migration_map_records_conflicts`
- `migration_map_iter_is_deterministic`
- `migration_map_serde_roundtrip`

**`polyref-core::observation`**
- Per kind: typed-field requirement
- `visibility_default_is_visible`
- `observation_with_unresolved_support_has_defined_semantics_false`

**`polyref-core::canonical`**
- `canonical_json_matches_rfc_8785_test_vectors`
- `canonical_json_is_stable_under_key_reorder`
- `canonical_json_rejects_nan_and_infinity`
- `canonical_json_rejects_oversize_payload`

**`polyref-core::report`**
- `report_assemble_rejects_accepted_with_missing_endpoint_unknown`
- `report_assemble_rejects_pointer_outside_evidence_dir`
- `report_assemble_rejects_invalid_id_syntax`
- `report_serde_roundtrip_is_identity`
- `report_canonical_json_is_byte_stable_across_runs`
- Property: `prop_no_accepted_with_missing_endpoint_unknown`
- Property: `prop_candidate_decision_is_meet`

**`polyref-checker-spi`**
- `jsonrpc_envelope_round_trip`
- `jsonrpc_envelope_rejects_request_without_id`
- `jsonrpc_envelope_rejects_oversize_payload`
- `jsonrpc_envelope_rejects_non_object_payload`
- `jsonrpc_envelope_rejects_unknown_method`
- `deadline_ms_clamped_to_max`
- `payload_size_cap_enforced`
- `safe_path_rejects_absolute`
- `safe_path_rejects_parent_traversal`
- `safe_path_rejects_empty`
- `safe_path_rejects_nul`
- `safe_path_accepts_canonical_relative_path`
- `extract_request_rejects_unsafe_path`
- `extract_request_rejects_unknown_language`
- `extract_result_rejects_entity_with_invalid_id`
- `check_request_rejects_endpoint_with_mismatched_type`
- `check_result_rejects_outcome_pres_with_unknown_reason_set`
- `describe_result_supported_reasons_subset_of_canonical_enums`

## Definition of done

1. `cargo build --workspace --all-targets` is clean.
2. `cargo test --workspace` is green.
3. `cargo clippy --workspace --all-targets -- -D warnings` is clean.
4. `cargo fmt --all -- --check` is clean.
5. `cargo deny check` is green.
6. `bash scripts/verify-schemas.sh` reports 28 schemas validate under Draft 2020-12.
7. Coverage on `polyref-core` and `polyref-checker-spi` is ≥ 80 % on lines and branches.
8. Compile-fail `trybuild` tests pass.
9. `prop_no_accepted_with_missing_endpoint_unknown` passes a 4096-case CI run.

## Open hard blockers (must close before Layer 1)

- **F-2** Final lex-sorted reason enum order — defaulted in `schemas/CHANGELOG.md`; confirm with project owner.
- **F-5** RFC 8785 implementation source: write our own (audited) vs depend on `serde_jcs`.
- **F-6** Confirm or override default hard caps (16 MiB payload, 64 JSON depth, 16 KiB id, 4 KiB path, 600 s deadline).
- **F-7** Final EvidencePointer regex: skeleton ships `^evidence/[A-Za-z0-9_./-]{1,512}$`.
- **F-8** Generated bindings strategy: hand-written + drift-check (current default) vs codegen.

Soft blockers (close before layer review): MSRV (currently 1.79), final license text (currently Apache-2.0 placeholder), `trybuild` budget, proptest case budget, `Visibility` immutability (currently immutable per ADR-010).
