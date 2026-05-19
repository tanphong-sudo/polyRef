# ADR-005: Hyperedge ambiguity, canonical Unknown/Broken reasons, and tie-break order

## Context
gpt.md left two related issues open: (4) ambiguous hyperedges without combinatorial blowup, and (5) what wins when one item has both missing-evidence (→ Unknown) and partial concrete failure (→ Broken).

## Decision

### 1. Canonical reason taxonomy

`UnknownReason` (closed enum):
- `MissingEndpoint` — at least one endpoint slot has no candidate entity.
- `AmbiguousEndpoint` — multiple candidates remain after evidence collection.
- `UnsupportedExtractor` — extractor produced an `unsupported_features` note for the artifact.
- `UnsupportedFramework` — framework convention not modeled by any kind.
- `DynamicString` — endpoint built from a dynamic string at runtime.
- `Reflection` — endpoint resolved by reflection / metaprogramming.
- `GeneratedEvidenceMissing` — no source map, no re-execution, no checksum (ADR-004).
- `GeneratedEvidenceWeak` — only one of the three pillars in ADR-004.
- `CyclicGenerator` — generator graph has a cycle.
- `OpaqueBuildCache` — build target produced by a non-introspectable cache.
- `MigrationMapAmbiguous` — μ has multiple candidate targets for a shared entity.
- `ObservationRewriteUndefined` — μ(o) cannot be defined for a required position.
- `CheckerTimeout` — plugin exceeded `deadline_ms`.
- `PluginFailure` — plugin process crashed or returned malformed output.
- `DynamicEvidenceUnverified` — dynamic trace did not pass ADR-004 admission.
- `NoAcceptingRuleApplied` — fallback path in Algorithm A2 step 5.

`BrokenReason` (closed enum):
- `RoutePathRefuted`
- `HandlerBindingMismatch`
- `SchemaIncompatible`
- `RequiredFieldDrift`
- `GeneratedClientStale`
- `GeneratorMismatch`
- `WorkflowPackagesOldTarget`
- `BuildTargetUnreachable`
- `QueryTableMissing`
- `EventPayloadIncompatible`
- `MigrationMapConflict`
- `LocalCheckerFailure`

These are stable identifiers. Adding a new reason is a minor version bump for the host; removing is a breaking change.

### 2. Hyperedge ambiguity representation

For an `n`-ary correspondence, ambiguity is represented as **per-endpoint candidate sets with confidences**, never as a Cartesian-product list of full hyperedges:

```rust
struct AmbiguityRecord {
  per_endpoint_candidates: Vec<Vec<(EntityId, f32 /* confidence */)>>, // length n
  resolution_evidence: Vec<EvidencePointer>,
}
```

Frontier validator collapses this lazily:
1. If any endpoint has zero candidates → `Unknown(MissingEndpoint)`.
2. If every endpoint has exactly one candidate → resolved; treat as a normal correspondence.
3. Else → `Unknown(AmbiguousEndpoint)`. The validator does **not** enumerate all combinations.

A developer annotation, dynamic trace (ADR-004), or higher-priority extractor pass can prune the candidate list. After pruning, validator re-runs on that single correspondence.

Consequence: graph size stays linear in entity count; no factorial blowup.

### 3. Tie-break: Broken vs Unknown on the same item

Algorithm A2 already orders: unsupported/timeout/missing → Unknown **before** running the predicate. So the only tie-break case is when **multiple checkers** vote on the same frontier item (e.g. a route correspondence checked by both the route checker and a focused API-diff helper).

Rule:
- If **any** required checker returns Broken with concrete evidence → status is Broken (BrokenReason from that checker).
- Else if **any** required checker returns Unknown → status is Unknown (UnknownReason from the highest-priority checker; priority = order in `requiredEvidence`).
- Else → Pres or Migrated per Algorithm A2.

Rationale: a concrete refutation is more informative than missing evidence and must dominate. This matches the paper's "Broken records a failed kind-specific predicate; Unknown records missing extractor coverage, local-checker support, or build information."

### 4. Final candidate decision granularity

Per-observation status is the primitive. The candidate-level decision is the **meet** over selected observations:
```
candidate_decision =
  Accepted   iff every selected observation is Accepted
  Broken     iff any selected observation is Broken
  Unknown    otherwise
```

This is the only granularity the report shows. Per-repository or per-task aggregations live in the empirical harness, not the core engine.

## Consequences

### Positive
- Closed reason enums make reports machine-comparable and metric-able.
- Hyperedge ambiguity stays tractable.
- Broken-dominates-Unknown matches paper intent and gives users actionable failure messages.

### Negative
- Adding a new framework may require new `UnknownReason` variants; gated by minor version bumps.

### Alternatives considered
- **Free-form reason strings**: rejected. Breaks dashboards, reduces report comparability.
- **Enumerate all hyperedge combinations**: rejected. Combinatorial.
- **Unknown dominates Broken**: rejected. Hides actionable concrete failures.

## Status
Accepted

## Date
2026-05-19
