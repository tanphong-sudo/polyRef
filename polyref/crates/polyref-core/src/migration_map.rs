//! Migration map type.
//!
//! Per paper §3.3 Definition 5, `μ : N ⇀ N′` is *type-respecting* iff
//! `type(n) = type(μ(n))` where `type` is the entity's *local kind*.
//! The check therefore compares the `kind` segment of the EntityId
//! only — **not** the `language` segment. Cross-language migrations
//! (TS handler ↔ JS handler, OpenAPI YAML ↔ JSON-Schema JSON for the
//! same schema-field kind, generated client toolchain swap) are
//! first-class and must succeed.

use crate::ids::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Errors `MigrationMap::try_new` may emit.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MigrationMapError {
    /// At least one rewrite has mismatched `kind` segments.
    #[error("mapping {old} -> {new} is not type-respecting (kind segment mismatch)")]
    KindMismatch {
        /// The old entity id.
        old: String,
        /// The proposed new entity id.
        new: String,
    },
    /// Two proposers emitted concrete, conflicting targets for the
    /// same source entity.
    #[error("conflict on {old}: {first} vs {second}")]
    Conflict {
        /// Old entity id.
        old: String,
        /// First proposed target.
        first: String,
        /// Second proposed target.
        second: String,
    },
}

/// Rewrite of a part of an observation (e.g. an HTTP path segment).
/// Slice 1 placeholder; concrete fields land in the rewriter slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObsPartRewrite {
    /// Free-form record describing the part being rewritten.
    pub kind: String,
    /// Old value.
    pub old: serde_json::Value,
    /// New value.
    pub new: serde_json::Value,
}

/// Recorded migration-map conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationConflict {
    /// Old entity id.
    pub old: EntityId,
    /// First proposed target.
    pub first: EntityId,
    /// Second proposed target.
    pub second: EntityId,
}

/// Migration map data type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationMap {
    entity_rewrites: BTreeMap<EntityId, EntityId>,
    observation_part_rewrites: Vec<ObsPartRewrite>,
    conflicts: Vec<MigrationConflict>,
    type_respecting: bool,
}

impl MigrationMap {
    /// Build a `MigrationMap` and enforce the type-respecting predicate
    /// from paper Definition 5 (kind-segment match only).
    pub fn try_new(
        _entity_rewrites: BTreeMap<EntityId, EntityId>,
        _observation_part_rewrites: Vec<ObsPartRewrite>,
        _conflicts: Vec<MigrationConflict>,
    ) -> Result<Self, MigrationMapError> {
        todo!(
            "§E-1 migration_map_rejects_kind_mismatch + \
             migration_map_allows_language_mismatch_when_kinds_match"
        )
    }

    /// Lookup.
    #[must_use]
    pub fn get(&self, k: &EntityId) -> Option<&EntityId> {
        self.entity_rewrites.get(k)
    }

    /// Iterate over rewrites in deterministic order.
    pub fn iter(&self) -> impl Iterator<Item = (&EntityId, &EntityId)> {
        self.entity_rewrites.iter()
    }

    /// Whether the map satisfies the type-respecting predicate.
    #[must_use]
    pub fn is_type_respecting(&self) -> bool {
        self.type_respecting
    }

    /// Recorded conflicts (concrete BROKEN candidates).
    #[must_use]
    pub fn conflicts(&self) -> &[MigrationConflict] {
        &self.conflicts
    }

    /// Observation-part rewrites.
    #[must_use]
    pub fn observation_part_rewrites(&self) -> &[ObsPartRewrite] {
        &self.observation_part_rewrites
    }
}
