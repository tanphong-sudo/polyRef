//! Observation registry for Layer 5 frontier construction.
//!
//! The registry turns declarative observation specs into typed
//! [`polyref_core::observation::Observation`] rows, validates support refs
//! against the graph, and persists deterministic observations through the
//! existing [`crate::GraphStore`] API.

use crate::error::Result;
use crate::{GraphReadModel, GraphStore};
use polyref_core::ids::{CorrId, EdgeId, EntityId};
use polyref_core::observation::{
    ApiCallObs, HttpMethod, ObsHeader, Observation, SupportRef, TestObs, Visibility,
};
use std::collections::BTreeSet;

/// Declarative observation spec accepted by the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationRegistrationSpec {
    /// Stable observation id, e.g. `obs:api:create-user-visible`.
    pub observation_id: String,
    /// Visibility partition for leakage-safe validation.
    pub visibility: Visibility,
    /// Typed observation fields.
    pub kind: ObservationSpecKind,
    /// Declared support refs; only `corr:*` and `edge:*` refs are valid.
    pub support: Vec<SupportRef>,
    /// Unsupported evidence tags such as dynamic route path evidence.
    pub unsupported_evidence: Vec<String>,
}

/// Typed observation spec variants needed by the Layer 5 fixture path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObservationSpecKind {
    /// HTTP API call observation.
    ApiCall {
        /// HTTP method.
        method: HttpMethod,
        /// Request path or pattern.
        path: String,
        /// Optional request schema entity.
        request_schema_id: Option<EntityId>,
        /// Optional response schema entity.
        response_schema_id: Option<EntityId>,
        /// Optional generated-client method entity.
        client_id: Option<EntityId>,
    },
    /// Test invocation observation.
    TestInvocation {
        /// Test entity.
        test_id: EntityId,
        /// Optional public entrypoint targeted by the test.
        public_entrypoint: Option<EntityId>,
    },
}

/// Registry diagnostic category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ObservationRegistryDiagnosticKind {
    /// A support ref did not resolve to an existing graph row.
    MissingSupport,
    /// Duplicate support ref was deduped before persistence.
    DuplicateSupport,
    /// Unsupported or dynamic evidence makes observation semantics undefined.
    UnsupportedEvidence,
    /// Observation id failed the local registry id policy.
    InvalidObservationId,
}

/// One deterministic registry diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationRegistryDiagnostic {
    /// Diagnostic category.
    pub kind: ObservationRegistryDiagnosticKind,
    /// Observation id associated with the diagnostic.
    pub observation_id: String,
    /// Optional support/evidence item associated with the diagnostic.
    pub item: Option<String>,
}

/// Result of registering a batch of observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationRegistryResult {
    /// Number of observations persisted.
    pub registered_count: usize,
    /// Diagnostics emitted while registering the batch.
    pub diagnostics: Vec<ObservationRegistryDiagnostic>,
}

/// Register typed observations and persist them through the graph store.
///
/// Missing support or unsupported evidence produces `defined_semantics=false`
/// observations rather than silently accepting vacuous semantics.
pub fn register_observations<G, I>(graph: &G, specs: I) -> Result<ObservationRegistryResult>
where
    G: GraphStore + GraphReadModel,
    I: IntoIterator<Item = ObservationRegistrationSpec>,
{
    let existing_corrs = graph
        .list_correspondences()?
        .into_iter()
        .map(|corr| corr.corr_id)
        .collect::<BTreeSet<_>>();
    let existing_edges = graph
        .list_build_edges()?
        .into_iter()
        .map(|edge| edge.edge_id)
        .collect::<BTreeSet<_>>();

    let mut registered_count = 0;
    let mut diagnostics = Vec::new();

    for spec in specs {
        if !valid_observation_id(&spec.observation_id) {
            diagnostics.push(ObservationRegistryDiagnostic {
                kind: ObservationRegistryDiagnosticKind::InvalidObservationId,
                observation_id: spec.observation_id,
                item: None,
            });
            continue;
        }

        let (support, mut support_diagnostics) = resolve_support(
            &spec.observation_id,
            &spec.support,
            &existing_corrs,
            &existing_edges,
        );
        let mut defined_semantics = support_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind != ObservationRegistryDiagnosticKind::MissingSupport);

        for item in &spec.unsupported_evidence {
            diagnostics.push(ObservationRegistryDiagnostic {
                kind: ObservationRegistryDiagnosticKind::UnsupportedEvidence,
                observation_id: spec.observation_id.clone(),
                item: Some(item.clone()),
            });
            defined_semantics = false;
        }
        diagnostics.append(&mut support_diagnostics);

        let observation = build_observation(&spec, support, defined_semantics);
        graph.save_observation(&spec.observation_id, &observation)?;
        registered_count += 1;
    }

    diagnostics.sort_by(|left, right| {
        (
            left.observation_id.as_str(),
            left.kind,
            left.item.as_deref().unwrap_or(""),
        )
            .cmp(&(
                right.observation_id.as_str(),
                right.kind,
                right.item.as_deref().unwrap_or(""),
            ))
    });

    Ok(ObservationRegistryResult {
        registered_count,
        diagnostics,
    })
}

fn resolve_support(
    observation_id: &str,
    support: &[SupportRef],
    existing_corrs: &BTreeSet<CorrId>,
    existing_edges: &BTreeSet<EdgeId>,
) -> (Vec<SupportRef>, Vec<ObservationRegistryDiagnostic>) {
    let mut seen = BTreeSet::<String>::new();
    let mut resolved = Vec::<SupportRef>::new();
    let mut diagnostics = Vec::<ObservationRegistryDiagnostic>::new();

    for support_ref in support {
        let key = support_key(support_ref);
        if !seen.insert(key.clone()) {
            diagnostics.push(ObservationRegistryDiagnostic {
                kind: ObservationRegistryDiagnosticKind::DuplicateSupport,
                observation_id: observation_id.to_owned(),
                item: Some(key),
            });
            continue;
        }
        if support_exists(support_ref, existing_corrs, existing_edges) {
            resolved.push(support_ref.clone());
        } else {
            diagnostics.push(ObservationRegistryDiagnostic {
                kind: ObservationRegistryDiagnosticKind::MissingSupport,
                observation_id: observation_id.to_owned(),
                item: Some(key),
            });
        }
    }

    resolved.sort_by_key(support_key);
    (resolved, diagnostics)
}

fn support_exists(
    support: &SupportRef,
    existing_corrs: &BTreeSet<CorrId>,
    existing_edges: &BTreeSet<EdgeId>,
) -> bool {
    match support {
        SupportRef::Corr(id) => existing_corrs.contains(id),
        SupportRef::Edge(id) => existing_edges.contains(id),
        _ => false,
    }
}

fn build_observation(
    spec: &ObservationRegistrationSpec,
    support: Vec<SupportRef>,
    defined_semantics: bool,
) -> Observation {
    let header = ObsHeader {
        visibility: spec.visibility,
        support,
        defined_semantics,
    };
    match &spec.kind {
        ObservationSpecKind::ApiCall {
            method,
            path,
            request_schema_id,
            response_schema_id,
            client_id,
        } => Observation::ApiCall(ApiCallObs {
            method: *method,
            path: path.clone(),
            request_schema_id: request_schema_id.clone(),
            response_schema_id: response_schema_id.clone(),
            client_id: client_id.clone(),
            header,
        }),
        ObservationSpecKind::TestInvocation {
            test_id,
            public_entrypoint,
        } => Observation::TestInvocation(TestObs {
            test_id: test_id.clone(),
            public_entrypoint: public_entrypoint.clone(),
            header,
        }),
    }
}

fn valid_observation_id(id: &str) -> bool {
    !id.is_empty()
        && id.starts_with("obs:")
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains("..")
        && id
            .chars()
            .all(|ch| !ch.is_control() && ch != '\0' && ch != '\u{202e}')
}

fn support_key(support: &SupportRef) -> String {
    match support {
        SupportRef::Corr(id) => id.as_str().to_owned(),
        SupportRef::Edge(id) => id.as_str().to_owned(),
        _ => "unsupported".to_owned(),
    }
}
