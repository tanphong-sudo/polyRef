//! Typed obligation IR for Layer 6 (paper Table 15 `GenerateObligations`).
//!
//! `generate_obligations` converts a Layer 5 `required(o)` frontier into the
//! typed obligations A2 (L6-02) will discharge. Per Table 15, for each frontier
//! item `x` the engine emits:
//!
//! - a **correspondence** (compat) obligation for a correspondence item, or a
//!   **build** obligation for a build-edge item — the base obligation every
//!   frontier item carries;
//! - a **migration** obligation when `x`'s endpoints are rewritten by `μ`;
//! - a **local** obligation for edited endpoint entities (an endpoint owned by
//!   an edited artifact in `Δ`);
//! - an **observation-support** obligation when the selected observation uses
//!   `x` (i.e. `x ∈ supp(o)`).
//!
//! Coverage gaps surfaced by Layer 5 (`FrontierResult::diagnostics`) are carried
//! as `precheck_unknowns` rather than dropped: A2 must turn each into an
//! `Unknown` before any accepting rule can fire (fail-closed).
//!
//! Determinism: every container is a `BTreeMap`/`BTreeSet` or a vector built by
//! iterating sorted keys, so generation is byte-stable regardless of frontier
//! entry order.

use std::collections::{BTreeMap, BTreeSet};

use polyref_core::{
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, EntityId},
    observation::SupportRef,
};
use polyref_frontier::{FrontierInput, FrontierItem, FrontierResult};
use polyref_graph::GraphReadModel;

use crate::error::Result;

/// The five obligation classes from paper Table 15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ObligationKind {
    /// Local preservation of an edited endpoint entity.
    Local,
    /// Kind-specific compatibility of a correspondence (`compat_k`).
    Correspondence,
    /// Migration of endpoints rewritten by `μ` (`migrate_k`).
    Migration,
    /// Reachability/validity of a build edge (`Art × Art`).
    Build,
    /// The frontier item is used by the selected observation (`x ∈ supp(o)`).
    ObservationSupport,
}

impl ObligationKind {
    /// Stable snake-case tag; lets consumers avoid a wildcard `_` arm.
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            ObligationKind::Local => "local",
            ObligationKind::Correspondence => "correspondence",
            ObligationKind::Migration => "migration",
            ObligationKind::Build => "build",
            ObligationKind::ObservationSupport => "observation_support",
        }
    }
}

/// One typed obligation attached to a frontier item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Obligation {
    /// Frontier item this obligation constrains.
    pub item: FrontierItem,
    /// Obligation class.
    pub kind: ObligationKind,
    /// Correspondence kind for a correspondence/migration obligation; `None`
    /// for build edges (artifact→artifact, no correspondence kind).
    pub corr_kind: Option<CorrespondenceKind>,
}

impl Obligation {
    /// Deterministic sort key: `(item, kind)`. `corr_kind` is a derived payload
    /// of the item, so it never participates in ordering (and
    /// `CorrespondenceKind` is intentionally not `Ord`).
    fn sort_key(&self) -> (&FrontierItem, ObligationKind) {
        (&self.item, self.kind)
    }
}

/// A pre-check Unknown carried over from Layer 5 coverage diagnostics. A2 must
/// turn each of these into an `Unknown` outcome for its item before any
/// accepting rule fires.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrecheckUnknown {
    /// Observation the gap belongs to.
    pub observation_id: String,
    /// Sanitized item / support id that lacks typed support.
    pub item: String,
}

/// Deterministic obligation set for one observation's frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierObligationSet {
    /// Observation these obligations belong to.
    pub observation_id: String,
    /// Sorted, deduped obligations.
    pub obligations: Vec<Obligation>,
    /// Sorted, deduped pre-check Unknowns from Layer 5 coverage diagnostics.
    pub precheck_unknowns: Vec<PrecheckUnknown>,
}

/// Generate the typed obligation set for one observation's frontier.
///
/// # Errors
///
/// Returns [`crate::error::EngineError::GraphRead`] if the graph read model
/// cannot load the rows needed to classify endpoints.
pub fn generate_obligations<G>(
    graph: &G,
    input: &FrontierInput,
    frontier: &FrontierResult,
) -> Result<FrontierObligationSet>
where
    G: GraphReadModel,
{
    let index = GraphIndex::load(graph)?;
    let support_items = support_items(&input.support);
    let dom_mu: BTreeSet<EntityId> = input
        .migration_map
        .iter()
        .map(|(old, _)| old.clone())
        .collect();

    // Keyed by (item, kind) for dedup + deterministic order; CorrespondenceKind
    // is a payload, not part of the key, so it stays out of ordering.
    let mut obligations = BTreeMap::<(FrontierItem, ObligationKind), Obligation>::new();
    let mut insert = |ob: Obligation| {
        obligations.insert((ob.item.clone(), ob.kind), ob);
    };

    for entry in &frontier.entries {
        match &entry.item {
            FrontierItem::Correspondence(corr_id) => {
                let corr_kind = index.corr_kind.get(corr_id).copied();
                // Base: correspondence compatibility.
                insert(Obligation {
                    item: entry.item.clone(),
                    kind: ObligationKind::Correspondence,
                    corr_kind,
                });
                let endpoints = index.corr_endpoints.get(corr_id);
                // Migration: any endpoint rewritten by μ.
                if let Some(endpoints) = endpoints {
                    if endpoints.iter().any(|e| dom_mu.contains(e)) {
                        insert(Obligation {
                            item: entry.item.clone(),
                            kind: ObligationKind::Migration,
                            corr_kind,
                        });
                    }
                    // Local: any endpoint owned by an edited artifact.
                    if endpoints
                        .iter()
                        .any(|e| index.endpoint_in_edited(e, &input.edited_artifacts))
                    {
                        insert(Obligation {
                            item: entry.item.clone(),
                            kind: ObligationKind::Local,
                            corr_kind,
                        });
                    }
                }
            }
            FrontierItem::BuildEdge(_) => {
                // Base: build-edge reachability/validity. No correspondence kind.
                insert(Obligation {
                    item: entry.item.clone(),
                    kind: ObligationKind::Build,
                    corr_kind: None,
                });
            }
        }

        // Observation-support: the selected observation uses this item.
        if support_items.contains(&entry.item) {
            let corr_kind = match &entry.item {
                FrontierItem::Correspondence(corr_id) => index.corr_kind.get(corr_id).copied(),
                FrontierItem::BuildEdge(_) => None,
            };
            insert(Obligation {
                item: entry.item.clone(),
                kind: ObligationKind::ObservationSupport,
                corr_kind,
            });
        }
    }

    let precheck_unknowns = frontier
        .diagnostics
        .iter()
        .map(|d| PrecheckUnknown {
            observation_id: d.observation_id.clone(),
            item: d.item.clone(),
        })
        .collect::<BTreeSet<_>>();

    let mut obligations: Vec<Obligation> = obligations.into_values().collect();
    obligations.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    Ok(FrontierObligationSet {
        observation_id: input.observation_id.clone(),
        obligations,
        precheck_unknowns: precheck_unknowns.into_iter().collect(),
    })
}

fn support_items(support: &[SupportRef]) -> BTreeSet<FrontierItem> {
    support
        .iter()
        .filter_map(|s| match s {
            SupportRef::Corr(id) => Some(FrontierItem::Correspondence(id.clone())),
            SupportRef::Edge(id) => Some(FrontierItem::BuildEdge(id.clone())),
            _ => None,
        })
        .collect()
}

/// Read-side projection of the graph rows obligation generation needs:
/// correspondence kinds, correspondence endpoints, and entity→artifact ownership.
struct GraphIndex {
    corr_kind: BTreeMap<polyref_core::ids::CorrId, CorrespondenceKind>,
    corr_endpoints: BTreeMap<polyref_core::ids::CorrId, Vec<EntityId>>,
    entity_artifact: BTreeMap<EntityId, ArtifactId>,
}

impl GraphIndex {
    fn load<G>(graph: &G) -> Result<Self>
    where
        G: GraphReadModel,
    {
        let mut corr_kind = BTreeMap::new();
        let mut corr_endpoints = BTreeMap::new();
        for corr in graph.list_correspondences()? {
            corr_kind.insert(corr.corr_id.clone(), corr.kind);
            corr_endpoints.insert(corr.corr_id, corr.endpoints);
        }
        let mut entity_artifact = BTreeMap::new();
        for entity in graph.list_entities()? {
            entity_artifact.insert(entity.entity_id, entity.artifact_id);
        }
        Ok(Self {
            corr_kind,
            corr_endpoints,
            entity_artifact,
        })
    }

    fn endpoint_in_edited(&self, endpoint: &EntityId, edited: &BTreeSet<ArtifactId>) -> bool {
        self.entity_artifact
            .get(endpoint)
            .is_some_and(|artifact| edited.contains(artifact))
    }
}
