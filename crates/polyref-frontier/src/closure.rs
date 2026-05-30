//! Affected-frontier closure (paper Definition 7).
//!
//! See the crate-level documentation in [`crate`] for the high-level
//! contract. This module owns the deterministic implementation: the
//! input/output DTOs, the in-memory graph indexes, the touch /
//! reachability traversals, and the reason-aggregation logic.

use polyref_core::{
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    migration_map::MigrationMap,
    observation::SupportRef,
};
use polyref_graph::{BuildEdge, Correspondence, GraphReadModel, GraphStoreError};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Convenience result type for frontier computation.
pub type Result<T> = std::result::Result<T, GraphStoreError>;

/// Input to paper Definition 7 affected-frontier closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierInput {
    /// Edited artifact set `Δ`.
    pub edited_artifacts: BTreeSet<ArtifactId>,
    /// Migration map `μ` connecting old and new entities.
    pub migration_map: MigrationMap,
    /// Observation id whose support set is being closed over.
    pub observation_id: String,
    /// Observation support `supp(o)` supplied by the registry/read model.
    pub support: Vec<SupportRef>,
}

/// One item in an affected frontier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FrontierItem {
    /// Correspondence id in `C`.
    Correspondence(CorrId),
    /// Build edge id in `Build`.
    BuildEdge(EdgeId),
}

/// Why a frontier item was included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum FrontierReason {
    /// A correspondence endpoint intersects `touchρ`.
    TouchedEndpoint,
    /// A build edge starts from an edited artifact in `Δ`.
    EditedArtifactBuild,
    /// A build edge starts from a generated/dependent artifact reached earlier.
    GeneratedArtifactBuild,
    /// The item is in `supp(o)` and reachable from `touchρ`.
    ReachableSupport,
}

/// Sorted frontier item plus deterministic inclusion reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierEntry {
    /// Frontier item.
    pub item: FrontierItem,
    /// Sorted, deduped reasons for inclusion.
    pub reasons: BTreeSet<FrontierReason>,
}

/// Frontier diagnostic category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum FrontierDiagnosticKind {
    /// `supp(o)` references a correspondence or build edge absent from graph rows.
    MissingSupport,
    /// A graph row references an artifact/entity endpoint absent from graph rows.
    MissingGraphEndpoint,
}

/// One deterministic frontier diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierDiagnostic {
    /// Diagnostic category.
    pub kind: FrontierDiagnosticKind,
    /// Observation being computed.
    pub observation_id: String,
    /// Affected support item or graph id.
    pub item: String,
}

/// Result of affected-frontier closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierResult {
    /// Sorted frontier entries.
    pub entries: Vec<FrontierEntry>,
    /// Sorted fail-closed diagnostics.
    pub diagnostics: Vec<FrontierDiagnostic>,
}

/// Compute the deterministic affected frontier for one observation.
///
/// # Errors
///
/// Returns a graph-store error if the read model cannot load graph rows.
pub fn compute_frontier<G>(graph: &G, input: &FrontierInput) -> Result<FrontierResult>
where
    G: GraphReadModel,
{
    let indexes = GraphIndexes::load(graph)?;
    let support = normalize_support(&input.support);
    let support_items = support_items(&support);
    let mut diagnostics = validate_support(&indexes, input, &support);
    diagnostics.extend(indexes.integrity_diagnostics(&input.observation_id));

    let touch = compute_touch(&indexes, input);
    let reachable = reachable_items(&indexes, &touch);
    let mut reasons = BTreeMap::<FrontierItem, BTreeSet<FrontierReason>>::new();

    seed_touched_correspondences(&indexes, &touch, &support_items, &mut reasons);
    seed_build_closure(&indexes, input, &support_items, &mut reasons);
    seed_reachable_support(&support, &reachable, &mut reasons);

    let entries = reasons
        .into_iter()
        .map(|(item, reasons)| FrontierEntry { item, reasons })
        .collect();
    diagnostics.sort_by(|left, right| {
        (left.observation_id.as_str(), left.kind, left.item.as_str()).cmp(&(
            right.observation_id.as_str(),
            right.kind,
            right.item.as_str(),
        ))
    });
    diagnostics.dedup_by(|left, right| {
        left.kind == right.kind
            && left.observation_id == right.observation_id
            && left.item == right.item
    });

    Ok(FrontierResult {
        entries,
        diagnostics,
    })
}

#[derive(Debug)]
struct GraphIndexes {
    artifact_entities: BTreeMap<ArtifactId, BTreeSet<EntityId>>,
    entity_artifact: BTreeMap<EntityId, ArtifactId>,
    entity_corrs: BTreeMap<EntityId, BTreeSet<CorrId>>,
    corr_endpoints: BTreeMap<CorrId, BTreeSet<EntityId>>,
    build_edges: BTreeMap<EdgeId, BuildEdge>,
    build_out: BTreeMap<ArtifactId, BTreeSet<EdgeId>>,
    build_in: BTreeMap<ArtifactId, BTreeSet<EdgeId>>,
    missing_refs: BTreeSet<String>,
}

impl GraphIndexes {
    fn load<G>(graph: &G) -> Result<Self>
    where
        G: GraphReadModel,
    {
        let artifacts = graph
            .list_artifacts()?
            .into_iter()
            .map(|artifact| artifact.artifact_id)
            .collect::<BTreeSet<_>>();
        let entities = graph.list_entities()?;
        let correspondences = graph.list_correspondences()?;
        let build_edges = graph.list_build_edges()?;

        let mut artifact_entities = BTreeMap::<ArtifactId, BTreeSet<EntityId>>::new();
        let mut entity_artifact = BTreeMap::<EntityId, ArtifactId>::new();
        let mut missing_refs = BTreeSet::<String>::new();
        for entity in entities {
            if artifacts.contains(&entity.artifact_id) {
                artifact_entities
                    .entry(entity.artifact_id.clone())
                    .or_default()
                    .insert(entity.entity_id.clone());
                entity_artifact.insert(entity.entity_id, entity.artifact_id);
            } else {
                missing_refs.insert(format!(
                    "entity:{}->{}",
                    entity.entity_id.as_str(),
                    entity.artifact_id.as_str()
                ));
            }
        }

        let mut entity_corrs = BTreeMap::<EntityId, BTreeSet<CorrId>>::new();
        let mut corr_endpoints = BTreeMap::<CorrId, BTreeSet<EntityId>>::new();
        for corr in correspondences {
            index_correspondence(
                &mut entity_corrs,
                &mut corr_endpoints,
                &mut missing_refs,
                &entity_artifact,
                corr,
            );
        }

        let mut build_edge_map = BTreeMap::<EdgeId, BuildEdge>::new();
        let mut build_out = BTreeMap::<ArtifactId, BTreeSet<EdgeId>>::new();
        let mut build_in = BTreeMap::<ArtifactId, BTreeSet<EdgeId>>::new();
        for edge in build_edges {
            if !artifacts.contains(&edge.src_artifact) {
                missing_refs.insert(format!(
                    "edge:{}->{}",
                    edge.edge_id.as_str(),
                    edge.src_artifact.as_str()
                ));
                continue;
            }
            if !artifacts.contains(&edge.dst_artifact) {
                missing_refs.insert(format!(
                    "edge:{}->{}",
                    edge.edge_id.as_str(),
                    edge.dst_artifact.as_str()
                ));
                continue;
            }
            build_out
                .entry(edge.src_artifact.clone())
                .or_default()
                .insert(edge.edge_id.clone());
            build_in
                .entry(edge.dst_artifact.clone())
                .or_default()
                .insert(edge.edge_id.clone());
            build_edge_map.insert(edge.edge_id.clone(), edge);
        }

        Ok(Self {
            artifact_entities,
            entity_artifact,
            entity_corrs,
            corr_endpoints,
            build_edges: build_edge_map,
            build_out,
            build_in,
            missing_refs,
        })
    }

    fn integrity_diagnostics(&self, observation_id: &str) -> Vec<FrontierDiagnostic> {
        self.missing_refs
            .iter()
            .map(|item| FrontierDiagnostic {
                kind: FrontierDiagnosticKind::MissingGraphEndpoint,
                observation_id: observation_id.to_owned(),
                item: item.clone(),
            })
            .collect()
    }
}

fn index_correspondence(
    entity_corrs: &mut BTreeMap<EntityId, BTreeSet<CorrId>>,
    corr_endpoints: &mut BTreeMap<CorrId, BTreeSet<EntityId>>,
    missing_refs: &mut BTreeSet<String>,
    entity_artifact: &BTreeMap<EntityId, ArtifactId>,
    corr: Correspondence,
) {
    let mut endpoints = BTreeSet::<EntityId>::new();
    for endpoint in corr.endpoints {
        if entity_artifact.contains_key(&endpoint) {
            entity_corrs
                .entry(endpoint.clone())
                .or_default()
                .insert(corr.corr_id.clone());
            endpoints.insert(endpoint);
        } else {
            missing_refs.insert(format!(
                "corr:{}->{}",
                corr.corr_id.as_str(),
                endpoint.as_str()
            ));
        }
    }
    corr_endpoints.insert(corr.corr_id, endpoints);
}

fn normalize_support(support: &[SupportRef]) -> Vec<SupportRef> {
    let mut keyed = BTreeMap::<String, SupportRef>::new();
    for support_ref in support {
        match support_ref {
            SupportRef::Corr(id) => {
                keyed.insert(id.as_str().to_owned(), SupportRef::Corr(id.clone()));
            }
            SupportRef::Edge(id) => {
                keyed.insert(id.as_str().to_owned(), SupportRef::Edge(id.clone()));
            }
            _ => {}
        }
    }
    keyed.into_values().collect()
}

fn validate_support(
    indexes: &GraphIndexes,
    input: &FrontierInput,
    support: &[SupportRef],
) -> Vec<FrontierDiagnostic> {
    support
        .iter()
        .filter_map(|support_ref| match support_ref {
            SupportRef::Corr(id) if !indexes.corr_endpoints.contains_key(id) => Some(id.as_str()),
            SupportRef::Edge(id) if !indexes.build_edges.contains_key(id) => Some(id.as_str()),
            SupportRef::Corr(_) | SupportRef::Edge(_) => None,
            _ => None,
        })
        .map(|item| FrontierDiagnostic {
            kind: FrontierDiagnosticKind::MissingSupport,
            observation_id: input.observation_id.clone(),
            item: item.to_owned(),
        })
        .collect()
}

fn support_items(support: &[SupportRef]) -> BTreeSet<FrontierItem> {
    support
        .iter()
        .filter_map(|support_ref| match support_ref {
            SupportRef::Corr(id) => Some(FrontierItem::Correspondence(id.clone())),
            SupportRef::Edge(id) => Some(FrontierItem::BuildEdge(id.clone())),
            _ => None,
        })
        .collect()
}

fn compute_touch(indexes: &GraphIndexes, input: &FrontierInput) -> BTreeSet<EntityId> {
    let mut touch = BTreeSet::<EntityId>::new();
    for artifact in &input.edited_artifacts {
        if let Some(entities) = indexes.artifact_entities.get(artifact) {
            touch.extend(entities.iter().cloned());
        }
    }
    for (old, new) in input.migration_map.iter() {
        if indexes.entity_artifact.contains_key(old) {
            touch.insert(old.clone());
        }
        if indexes.entity_artifact.contains_key(new) {
            touch.insert(new.clone());
        }
    }
    touch
}

fn seed_touched_correspondences(
    indexes: &GraphIndexes,
    touch: &BTreeSet<EntityId>,
    support_items: &BTreeSet<FrontierItem>,
    reasons: &mut BTreeMap<FrontierItem, BTreeSet<FrontierReason>>,
) {
    for entity in touch {
        if let Some(corrs) = indexes.entity_corrs.get(entity) {
            for corr in corrs {
                let item = FrontierItem::Correspondence(corr.clone());
                if !support_items.contains(&item) {
                    continue;
                }
                insert_reason(reasons, item, FrontierReason::TouchedEndpoint);
            }
        }
    }
}

fn seed_build_closure(
    indexes: &GraphIndexes,
    input: &FrontierInput,
    support_items: &BTreeSet<FrontierItem>,
    reasons: &mut BTreeMap<FrontierItem, BTreeSet<FrontierReason>>,
) {
    // required(o): a build edge is required when it is forward-reachable from an edited
    // artifact AND lies on a path to a supp(o) element. The intermediate codegen edges
    // that reach a supp(o) element are required even when they are not themselves in
    // supp(o) (paper Definition 7 clause 2 + the build-closure lemma, which inducts
    // along those edges). `reaches_supp` is the set of artifacts from which a supp(o)
    // anchor is reachable through build edges.
    let reaches_supp = artifacts_reaching_support(indexes, support_items);

    let mut reached_artifacts = BTreeSet::<ArtifactId>::new();
    let mut worklist = VecDeque::<ArtifactId>::new();
    for artifact in &input.edited_artifacts {
        if reached_artifacts.insert(artifact.clone()) {
            worklist.push_back(artifact.clone());
        }
    }

    while let Some(artifact) = worklist.pop_front() {
        let Some(edges) = indexes.build_out.get(&artifact) else {
            continue;
        };
        for edge_id in edges {
            let Some(edge) = indexes.build_edges.get(edge_id) else {
                continue;
            };
            let reason = if input.edited_artifacts.contains(&artifact) {
                FrontierReason::EditedArtifactBuild
            } else {
                FrontierReason::GeneratedArtifactBuild
            };
            let item = FrontierItem::BuildEdge(edge_id.clone());
            // In required(o) iff it is a supp(o) build edge directly, or it is an
            // intermediate edge whose destination can still reach a supp(o) element.
            if support_items.contains(&item) || reaches_supp.contains(&edge.dst_artifact) {
                insert_reason(reasons, item, reason);
            }
            if reached_artifacts.insert(edge.dst_artifact.clone()) {
                worklist.push_back(edge.dst_artifact.clone());
            }
        }
    }
}

/// Artifacts from which a supp(o) element is reachable through build edges
/// (reflexive). An *anchor* is the source artifact of a supp(o) build edge or the
/// owner artifact of a supp(o) correspondence endpoint; any artifact that can reach
/// an anchor by following `build_out` is included. Used to keep the build closure
/// `o`-relative: only intermediate edges on a path to supp(o) enter `required(o)`.
fn artifacts_reaching_support(
    indexes: &GraphIndexes,
    support_items: &BTreeSet<FrontierItem>,
) -> BTreeSet<ArtifactId> {
    let mut reaches = BTreeSet::<ArtifactId>::new();
    let mut worklist = VecDeque::<ArtifactId>::new();
    for item in support_items {
        match item {
            FrontierItem::BuildEdge(edge_id) => {
                if let Some(edge) = indexes.build_edges.get(edge_id) {
                    if reaches.insert(edge.src_artifact.clone()) {
                        worklist.push_back(edge.src_artifact.clone());
                    }
                }
            }
            FrontierItem::Correspondence(corr_id) => {
                if let Some(endpoints) = indexes.corr_endpoints.get(corr_id) {
                    for endpoint in endpoints {
                        if let Some(artifact) = indexes.entity_artifact.get(endpoint) {
                            if reaches.insert(artifact.clone()) {
                                worklist.push_back(artifact.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    // Reverse BFS over `build_in`: a predecessor artifact reaches an anchor if it has
    // a build edge into an artifact already known to reach an anchor.
    while let Some(artifact) = worklist.pop_front() {
        let Some(in_edges) = indexes.build_in.get(&artifact) else {
            continue;
        };
        for edge_id in in_edges {
            if let Some(edge) = indexes.build_edges.get(edge_id) {
                if reaches.insert(edge.src_artifact.clone()) {
                    worklist.push_back(edge.src_artifact.clone());
                }
            }
        }
    }
    reaches
}

#[derive(Debug, Default)]
struct ReachableState {
    corrs: BTreeSet<CorrId>,
    edges: BTreeSet<EdgeId>,
}

fn reachable_items(indexes: &GraphIndexes, touch: &BTreeSet<EntityId>) -> ReachableState {
    let mut reached_entities = BTreeSet::<EntityId>::new();
    let mut reached_artifacts = BTreeSet::<ArtifactId>::new();
    let mut reached_corrs = BTreeSet::<CorrId>::new();
    let mut reached_edges = BTreeSet::<EdgeId>::new();
    let mut entity_work = VecDeque::<EntityId>::new();
    let mut artifact_work = VecDeque::<ArtifactId>::new();

    for entity in touch {
        if reached_entities.insert(entity.clone()) {
            entity_work.push_back(entity.clone());
        }
    }

    loop {
        while let Some(entity) = entity_work.pop_front() {
            if let Some(artifact) = indexes.entity_artifact.get(&entity) {
                if reached_artifacts.insert(artifact.clone()) {
                    artifact_work.push_back(artifact.clone());
                }
            }
            if let Some(corrs) = indexes.entity_corrs.get(&entity) {
                for corr in corrs {
                    if reached_corrs.insert(corr.clone()) {
                        if let Some(endpoints) = indexes.corr_endpoints.get(corr) {
                            for endpoint in endpoints {
                                if reached_entities.insert(endpoint.clone()) {
                                    entity_work.push_back(endpoint.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut added = false;
        while let Some(artifact) = artifact_work.pop_front() {
            if let Some(entities) = indexes.artifact_entities.get(&artifact) {
                for entity in entities {
                    if reached_entities.insert(entity.clone()) {
                        entity_work.push_back(entity.clone());
                        added = true;
                    }
                }
            }
            enqueue_build_neighbors(
                indexes,
                &artifact,
                &mut reached_edges,
                &mut reached_artifacts,
                &mut artifact_work,
                &mut added,
            );
        }

        if !added && entity_work.is_empty() && artifact_work.is_empty() {
            break;
        }
    }

    ReachableState {
        corrs: reached_corrs,
        edges: reached_edges,
    }
}

fn enqueue_build_neighbors(
    indexes: &GraphIndexes,
    artifact: &ArtifactId,
    reached_edges: &mut BTreeSet<EdgeId>,
    reached_artifacts: &mut BTreeSet<ArtifactId>,
    artifact_work: &mut VecDeque<ArtifactId>,
    added: &mut bool,
) {
    for edge_id in indexes
        .build_out
        .get(artifact)
        .into_iter()
        .flatten()
        .chain(indexes.build_in.get(artifact).into_iter().flatten())
    {
        if let Some(edge) = indexes.build_edges.get(edge_id) {
            reached_edges.insert(edge_id.clone());
            for next in [&edge.src_artifact, &edge.dst_artifact] {
                if reached_artifacts.insert(next.clone()) {
                    artifact_work.push_back(next.clone());
                    *added = true;
                }
            }
        }
    }
}

fn seed_reachable_support(
    support: &[SupportRef],
    reachable: &ReachableState,
    reasons: &mut BTreeMap<FrontierItem, BTreeSet<FrontierReason>>,
) {
    for support_ref in support {
        match support_ref {
            SupportRef::Corr(id) if reachable.corrs.contains(id) => insert_reason(
                reasons,
                FrontierItem::Correspondence(id.clone()),
                FrontierReason::ReachableSupport,
            ),
            SupportRef::Edge(id) if reachable.edges.contains(id) => insert_reason(
                reasons,
                FrontierItem::BuildEdge(id.clone()),
                FrontierReason::ReachableSupport,
            ),
            SupportRef::Corr(_) | SupportRef::Edge(_) => {}
            _ => {}
        }
    }
}

fn insert_reason(
    reasons: &mut BTreeMap<FrontierItem, BTreeSet<FrontierReason>>,
    item: FrontierItem,
    reason: FrontierReason,
) {
    reasons.entry(item).or_default().insert(reason);
}
