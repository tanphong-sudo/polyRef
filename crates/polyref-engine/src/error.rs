//! Engine error type.
//!
//! Layer 6 reads the graph through [`polyref_graph::GraphReadModel`], so the
//! only fallible dependency in obligation generation is the graph read. We wrap
//! it in a typed engine error rather than leaking the graph-store error directly,
//! keeping the engine's public surface independent of the persistence layer.

use thiserror::Error;

/// Failure modes of the Layer 6 engine.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The graph read model could not load rows needed to generate obligations.
    #[error("graph read failed: {0}")]
    GraphRead(#[from] polyref_graph::GraphStoreError),
}

/// Convenience result type for engine operations.
pub type Result<T> = std::result::Result<T, EngineError>;
