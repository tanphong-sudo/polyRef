//! Read-only graph queries for Layer 5 frontier algorithms.
//!
//! The write-oriented [`crate::GraphStore`] trait stays intentionally narrow.
//! This module exposes deterministic read APIs needed by migration-map building,
//! observation support registration, and affected-frontier closure without
//! leaking SQLite rows or unvalidated ids.

use crate::{error::Result, Artifact, BuildEdge, Correspondence, Entity};
use polyref_core::{
    ids::ArtifactId, ids::EntityId, observation::Observation, observation::SupportRef,
};

/// Observation row returned by the read model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationRecord {
    /// Stable observation id used as the `observation` table primary key.
    pub observation_id: String,
    /// Typed observation payload deserialized from canonical JSON.
    pub observation: Observation,
}

/// Deterministic read-only graph queries used by Layer 5 algorithms.
pub trait GraphReadModel {
    /// List artifacts in stable `artifact_id` order.
    fn list_artifacts(&self) -> Result<Vec<Artifact>>;

    /// List entities in stable `entity_id` order.
    fn list_entities(&self) -> Result<Vec<Entity>>;

    /// List correspondences in stable `corr_id` order.
    fn list_correspondences(&self) -> Result<Vec<Correspondence>>;

    /// List build edges in stable `edge_id` order.
    fn list_build_edges(&self) -> Result<Vec<BuildEdge>>;

    /// List observations in stable `observation_id` order.
    fn list_observations(&self) -> Result<Vec<ObservationRecord>>;

    /// Find all correspondences containing `entity_id`, ordered by `corr_id`.
    fn correspondences_for_entity(&self, entity_id: &EntityId) -> Result<Vec<Correspondence>>;

    /// Find build edges whose source artifact is `artifact_id`, ordered by `edge_id`.
    fn build_edges_from(&self, artifact_id: &ArtifactId) -> Result<Vec<BuildEdge>>;

    /// Find build edges whose destination artifact is `artifact_id`, ordered by `edge_id`.
    fn build_edges_to(&self, artifact_id: &ArtifactId) -> Result<Vec<BuildEdge>>;

    /// Load an observation support set by id.
    fn observation_support(&self, observation_id: &str) -> Result<Vec<SupportRef>>;
}
