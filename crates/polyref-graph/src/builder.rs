//! Build GraphStore rows from extractor plugin output.
//!
//! Layer 4 keeps this builder deliberately narrow: it ingests the
//! documented OpenAPI route facts and TypeScript route metadata facts,
//! persists artifacts/entities, and derives side-local route
//! correspondences. It does not compute Layer 5 frontiers or validate
//! route migrations.

use crate::error::GraphStoreError;
use crate::model::{Artifact, Correspondence, Entity, RepoSide};
use crate::store::GraphStore;
use polyref_checker_spi::extractor::{ExtractResult, UnsupportedFeatureNote};
use polyref_core::artifact_kind::ArtifactKind;
use polyref_core::correspondence_kind::CorrespondenceKind;
use polyref_core::ids::{ArtifactId, CorrId, EntityId, IdParseError};
use polyref_core::language::{Language, LanguageParseError};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

/// Artifact metadata paired with one extractor result.
#[derive(Debug, Clone)]
pub struct ExtractorArtifactInput {
    /// Stable artifact id.
    pub artifact_id: ArtifactId,
    /// Repository side.
    pub repo_side: RepoSide,
    /// Artifact family.
    pub kind: ArtifactKind,
    /// Artifact language/format.
    pub language: Language,
    /// Repo-relative POSIX path.
    pub local_path: String,
    /// 12-hex content hash segment.
    pub content_hash: String,
}

/// One extractor output and its owning artifact metadata.
#[derive(Debug, Clone)]
pub struct ExtractorOutputBundle {
    /// Owning artifact metadata.
    pub metadata: ExtractorArtifactInput,
    /// Plugin extraction result.
    pub result: ExtractResult,
}

/// Rows produced and persisted by [`build_extractor_graph`].
#[derive(Debug, Clone)]
pub struct GraphBuildResult {
    /// Persisted artifact rows.
    pub artifacts: Vec<Artifact>,
    /// Persisted entity rows.
    pub entities: Vec<Entity>,
    /// Persisted correspondence rows.
    pub correspondences: Vec<Correspondence>,
    /// Unsupported notes preserved from extractor output.
    pub unsupported_features: Vec<UnsupportedFeatureNote>,
}

/// Errors emitted by the extractor graph builder.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GraphBuilderError {
    /// GraphStore operation failed.
    #[error("graph store: {0}")]
    Store(#[from] GraphStoreError),
    /// ID parsing or validation failed.
    #[error("id parse error: {0}")]
    Id(#[from] IdParseError),
    /// Entity id language segment is not known.
    #[error("language parse error: {0}")]
    Language(#[from] LanguageParseError),
    /// Extractor output does not match its artifact metadata.
    #[error("invalid extractor output: {0}")]
    InvalidOutput(String),
    /// Local fact row is malformed or unsupported.
    #[error("invalid local fact: {0}")]
    InvalidFact(String),
    /// Two facts with the same semantic key disagree.
    #[error("conflicting duplicate local fact: {0}")]
    ConflictingFact(String),
}

/// Convenience result type for graph-builder operations.
pub type Result<T> = std::result::Result<T, GraphBuilderError>;

/// Normalize extractor output into persisted graph rows.
///
/// # Errors
///
/// Returns [`GraphBuilderError`] when extractor output is malformed,
/// contradicts artifact metadata, or cannot be persisted.
pub fn build_extractor_graph(
    store: &dyn GraphStore,
    bundles: &[ExtractorOutputBundle],
) -> Result<GraphBuildResult> {
    let mut artifacts = Vec::new();
    let mut entities = Vec::new();
    let mut unsupported_features = Vec::new();
    let mut route_facts = BTreeMap::new();
    let mut metadata_facts = BTreeMap::new();

    for bundle in bundles {
        let artifact = artifact_from_input(&bundle.metadata)?;
        store.save_artifact(&artifact)?;
        artifacts.push(artifact.clone());

        for extracted in &bundle.result.entities {
            let entity = entity_from_output(&artifact, extracted)?;
            store.save_entity(&entity)?;
            entities.push(entity);
        }

        unsupported_features.extend(bundle.result.unsupported_features.iter().cloned());
        for fact in &bundle.result.local_facts {
            match fact.get("kind").and_then(serde_json::Value::as_str) {
                Some("route") => {
                    insert_route_fact(&mut route_facts, parse_route_fact(fact, &artifact)?)?
                }
                Some("route_metadata") => insert_metadata_fact(
                    &mut metadata_facts,
                    parse_metadata_fact(fact, &artifact)?,
                )?,
                Some(kind) => {
                    return Err(GraphBuilderError::InvalidFact(format!(
                        "unsupported kind {kind}"
                    )))
                }
                None => return Err(GraphBuilderError::InvalidFact("missing kind".to_owned())),
            }
        }
    }

    let mut correspondences = derive_route_correspondences(&route_facts, &metadata_facts)?;
    correspondences.sort_by(|left, right| left.corr_id.as_str().cmp(right.corr_id.as_str()));
    for correspondence in &correspondences {
        store.save_correspondence(correspondence)?;
    }

    artifacts.sort_by(|left, right| left.artifact_id.as_str().cmp(right.artifact_id.as_str()));
    entities.sort_by(|left, right| left.entity_id.as_str().cmp(right.entity_id.as_str()));
    unsupported_features.sort_by(|left, right| {
        left.feature
            .cmp(&right.feature)
            .then_with(|| left.note.cmp(&right.note))
    });

    Ok(GraphBuildResult {
        artifacts,
        entities,
        correspondences,
        unsupported_features,
    })
}

fn artifact_from_input(input: &ExtractorArtifactInput) -> Result<Artifact> {
    if input.content_hash.len() != 12
        || !input
            .content_hash
            .chars()
            .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase())
    {
        return Err(GraphBuilderError::InvalidOutput(
            "artifact content_hash must be 12 lowercase hex chars".to_owned(),
        ));
    }
    Ok(Artifact {
        artifact_id: input.artifact_id.clone(),
        repo_side: input.repo_side,
        kind: input.kind,
        language: input.language,
        local_path: input.local_path.clone(),
        content_hash: input.content_hash.clone(),
    })
}

fn entity_from_output(
    artifact: &Artifact,
    extracted: &polyref_checker_spi::extractor::ExtractedEntity,
) -> Result<Entity> {
    let entity_id = &extracted.entity_id;
    let repo_side = parse_repo_side(entity_id.repo_side())?;
    let language = Language::parse(entity_id.language())?;
    if repo_side != artifact.repo_side {
        return Err(GraphBuilderError::InvalidOutput(format!(
            "entity {} repo side does not match artifact {}",
            entity_id.as_str(),
            artifact.artifact_id.as_str()
        )));
    }
    if language != artifact.language {
        return Err(GraphBuilderError::InvalidOutput(format!(
            "entity {} language does not match artifact {}",
            entity_id.as_str(),
            artifact.artifact_id.as_str()
        )));
    }
    if entity_id.kind() != extracted.kind {
        return Err(GraphBuilderError::InvalidOutput(format!(
            "entity {} kind does not match extracted kind {}",
            entity_id.as_str(),
            extracted.kind
        )));
    }
    if extracted.source_span.artifact() != &artifact.artifact_id {
        return Err(GraphBuilderError::InvalidOutput(format!(
            "entity {} source span artifact does not match {}",
            entity_id.as_str(),
            artifact.artifact_id.as_str()
        )));
    }
    Ok(Entity {
        entity_id: entity_id.clone(),
        artifact_id: artifact.artifact_id.clone(),
        repo_side,
        language,
        kind: entity_id.kind().to_owned(),
        local_path: entity_id.local_path().to_owned(),
        stable_hash: entity_id.stable_hash().to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RouteKey {
    side: RepoSideKey,
    method: String,
    path: String,
    operation_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RepoSideKey {
    Old,
    New,
}

impl From<RepoSide> for RepoSideKey {
    fn from(value: RepoSide) -> Self {
        match value {
            RepoSide::Old => Self::Old,
            RepoSide::New => Self::New,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteFact {
    key: RouteKey,
    entity_id: EntityId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataFact {
    key: RouteKey,
    handler_entity_id: EntityId,
}

#[derive(Deserialize)]
struct RouteFactWire {
    side: String,
    artifact_id: ArtifactId,
    entity_id: EntityId,
    method: String,
    path: String,
    operation_id: String,
}

#[derive(Deserialize)]
struct MetadataFactWire {
    side: String,
    artifact_id: ArtifactId,
    handler_entity_id: EntityId,
    method: String,
    path: String,
    operation_id: String,
}

fn parse_route_fact(value: &serde_json::Value, artifact: &Artifact) -> Result<RouteFact> {
    let wire: RouteFactWire = serde_json::from_value(value.clone())
        .map_err(|err| GraphBuilderError::InvalidFact(format!("route fact shape: {err}")))?;
    validate_fact_artifact(&wire.artifact_id, artifact)?;
    if wire.entity_id.kind() != "route" {
        return Err(GraphBuilderError::InvalidFact(format!(
            "route fact entity {} is not kind route",
            wire.entity_id.as_str()
        )));
    }
    let key = route_key(
        &wire.side,
        &wire.method,
        &wire.path,
        &wire.operation_id,
        artifact,
    )?;
    Ok(RouteFact {
        key,
        entity_id: wire.entity_id,
    })
}

fn parse_metadata_fact(value: &serde_json::Value, artifact: &Artifact) -> Result<MetadataFact> {
    let wire: MetadataFactWire = serde_json::from_value(value.clone()).map_err(|err| {
        GraphBuilderError::InvalidFact(format!("route metadata fact shape: {err}"))
    })?;
    validate_fact_artifact(&wire.artifact_id, artifact)?;
    if wire.handler_entity_id.kind() != "handler" {
        return Err(GraphBuilderError::InvalidFact(format!(
            "route metadata handler {} is not kind handler",
            wire.handler_entity_id.as_str()
        )));
    }
    let key = route_key(
        &wire.side,
        &wire.method,
        &wire.path,
        &wire.operation_id,
        artifact,
    )?;
    Ok(MetadataFact {
        key,
        handler_entity_id: wire.handler_entity_id,
    })
}

fn validate_fact_artifact(fact_artifact: &ArtifactId, artifact: &Artifact) -> Result<()> {
    if fact_artifact != &artifact.artifact_id {
        return Err(GraphBuilderError::InvalidFact(format!(
            "fact artifact {} does not match bundle artifact {}",
            fact_artifact.as_str(),
            artifact.artifact_id.as_str()
        )));
    }
    Ok(())
}

fn route_key(
    side: &str,
    method: &str,
    path: &str,
    operation_id: &str,
    artifact: &Artifact,
) -> Result<RouteKey> {
    let fact_side = parse_repo_side(side)?;
    if fact_side != artifact.repo_side {
        return Err(GraphBuilderError::InvalidFact(format!(
            "fact side {side} does not match artifact side {}",
            artifact.repo_side.as_str()
        )));
    }
    if method.is_empty() || path.is_empty() || operation_id.is_empty() {
        return Err(GraphBuilderError::InvalidFact(
            "route key fields must be non-empty".to_owned(),
        ));
    }
    Ok(RouteKey {
        side: fact_side.into(),
        method: method.to_ascii_uppercase(),
        path: path.to_owned(),
        operation_id: operation_id.to_owned(),
    })
}

fn insert_route_fact(map: &mut BTreeMap<RouteKey, RouteFact>, fact: RouteFact) -> Result<()> {
    match map.get(&fact.key) {
        Some(existing) if existing == &fact => Ok(()),
        Some(_) => Err(GraphBuilderError::ConflictingFact(format!(
            "route {} {} {}",
            fact.key.method, fact.key.path, fact.key.operation_id
        ))),
        None => {
            map.insert(fact.key.clone(), fact);
            Ok(())
        }
    }
}

fn insert_metadata_fact(
    map: &mut BTreeMap<RouteKey, MetadataFact>,
    fact: MetadataFact,
) -> Result<()> {
    match map.get(&fact.key) {
        Some(existing) if existing == &fact => Ok(()),
        Some(_) => Err(GraphBuilderError::ConflictingFact(format!(
            "route_metadata {} {} {}",
            fact.key.method, fact.key.path, fact.key.operation_id
        ))),
        None => {
            map.insert(fact.key.clone(), fact);
            Ok(())
        }
    }
}

fn derive_route_correspondences(
    route_facts: &BTreeMap<RouteKey, RouteFact>,
    metadata_facts: &BTreeMap<RouteKey, MetadataFact>,
) -> Result<Vec<Correspondence>> {
    let mut correspondences = Vec::new();
    for (key, route) in route_facts {
        if let Some(metadata) = metadata_facts.get(key) {
            let endpoints = vec![route.entity_id.clone(), metadata.handler_entity_id.clone()];
            correspondences.push(Correspondence {
                corr_id: route_corr_id(&endpoints)?,
                kind: CorrespondenceKind::Route,
                rule_version: Some("layer4-route-fact-v1".to_owned()),
                endpoints,
            });
        }
    }
    Ok(correspondences)
}

fn route_corr_id(endpoints: &[EntityId]) -> Result<CorrId> {
    let mut hasher = Sha256::new();
    hasher.update(CorrespondenceKind::Route.as_tag().as_bytes());
    for endpoint in endpoints {
        hasher.update([0]);
        hasher.update(endpoint.as_str().as_bytes());
    }
    Ok(CorrId::parse(&format!(
        "corr:{}:{:x}",
        CorrespondenceKind::Route.as_tag(),
        hasher.finalize()
    ))?)
}

fn parse_repo_side(side: &str) -> Result<RepoSide> {
    match side {
        "old" => Ok(RepoSide::Old),
        "new" => Ok(RepoSide::New),
        other => Err(GraphBuilderError::InvalidOutput(format!(
            "invalid repo side {other}"
        ))),
    }
}
