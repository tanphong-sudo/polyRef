//! Route compatibility and migration checker plugin.

use polyref_checker_spi::checker::{CheckRequest, DescribeResult};
use polyref_core::correspondence_kind::CorrespondenceKind;
use polyref_core::evidence::{Evidence, PredicateId, Version};
use polyref_core::ids::EntityId;
use polyref_core::status::{BrokenReason, UnknownReason};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;

const CONTRACT_ID: &str = "polyref.route.checker.v1";
const COMPAT_RULE_ID: &str = "route.compat.v1";
const MIGRATE_RULE_ID: &str = "route.migrate.v1";
const PLUGIN_VERSION: &str = "0.1.0";
const DEFAULT_TIMEOUT_MS: u32 = 5_000;

/// Errors for malformed route-checker requests.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RouteCheckError {
    /// Request field failed structural validation.
    #[error("invalid route check request: {0}")]
    InvalidRequest(String),
}

type Result<T> = std::result::Result<T, RouteCheckError>;

/// Describe the route checker SPI contract.
#[must_use]
pub fn describe_route_checker() -> DescribeResult {
    DescribeResult {
        contract_id: CONTRACT_ID.to_owned(),
        kind_id: CorrespondenceKind::Route,
        endpoint_signature: vec!["route".to_owned(), "handler".to_owned()],
        required_evidence_fields: vec![
            "migration_map_excerpt.entity_rewrites".to_owned(),
            "observation_excerpt.old".to_owned(),
            "observation_excerpt.new".to_owned(),
        ],
        compat_rule_id: COMPAT_RULE_ID.to_owned(),
        migrate_rule_id: MIGRATE_RULE_ID.to_owned(),
        plugin_version: PLUGIN_VERSION.to_owned(),
        default_timeout_ms: DEFAULT_TIMEOUT_MS,
        supported_unknown_reasons: vec![
            UnknownReason::DynamicString,
            UnknownReason::MigrationMapAmbiguous,
            UnknownReason::MissingEndpoint,
            UnknownReason::UnsupportedExtractor,
        ],
        supported_broken_reasons: vec![
            BrokenReason::HandlerBindingMismatch,
            BrokenReason::RoutePathRefuted,
        ],
    }
}

/// Check one route correspondence obligation.
///
/// # Errors
///
/// Returns [`RouteCheckError`] when the request is structurally malformed.
pub fn check_route(request: &CheckRequest) -> Result<Evidence> {
    validate_request_shape(request)?;
    if has_unsupported_feature(&request.observation_excerpt, "dynamic_route") {
        return Ok(unknown(UnknownReason::DynamicString));
    }
    if has_any_unsupported_feature(&request.observation_excerpt) {
        return Ok(unknown(UnknownReason::UnsupportedExtractor));
    }

    let observation = parse_observation(&request.observation_excerpt)?;
    if observation.old == observation.new {
        return Ok(Evidence::ok_pres(
            PredicateId::new(COMPAT_RULE_ID),
            Vec::new(),
            Vec::new(),
            Version::new(PLUGIN_VERSION),
            Version::new(COMPAT_RULE_ID),
        ));
    }

    if !observation
        .old
        .method
        .eq_ignore_ascii_case(&observation.new.method)
    {
        return Ok(broken(BrokenReason::RoutePathRefuted));
    }

    let rewrites = match parse_rewrites(&request.migration_map_excerpt) {
        Ok(value) => value,
        Err(RewriteProblem::Missing) => return Ok(unknown(UnknownReason::MissingEndpoint)),
        Err(RewriteProblem::Ambiguous) => {
            return Ok(unknown(UnknownReason::MigrationMapAmbiguous));
        }
    };

    let old_route = &request.endpoints[0].entity_id;
    let old_handler = &request.endpoints[1].entity_id;
    let Some(new_route) = rewrites.get(old_route.as_str()) else {
        return Ok(unknown(UnknownReason::MissingEndpoint));
    };
    let Some(new_handler) = rewrites.get(old_handler.as_str()) else {
        return Ok(unknown(UnknownReason::MissingEndpoint));
    };

    if new_route.kind() != "route" || new_route.repo_side() != "new" {
        return Ok(broken(BrokenReason::RoutePathRefuted));
    }
    if new_handler.kind() != "handler" || new_handler.repo_side() != "new" {
        return Ok(broken(BrokenReason::HandlerBindingMismatch));
    }
    if !entity_matches_observation(new_route, &observation.new, "route") {
        return Ok(broken(BrokenReason::RoutePathRefuted));
    }
    if !entity_matches_observation(new_handler, &observation.new, "handler") {
        return Ok(broken(BrokenReason::HandlerBindingMismatch));
    }

    Ok(Evidence::ok_migrated(
        PredicateId::new(MIGRATE_RULE_ID),
        Vec::new(),
        Vec::new(),
        Version::new(PLUGIN_VERSION),
        Version::new(MIGRATE_RULE_ID),
    ))
}

fn validate_request_shape(request: &CheckRequest) -> Result<()> {
    if request.contract_id != CONTRACT_ID {
        return Err(RouteCheckError::InvalidRequest(
            "contract_id mismatch".to_owned(),
        ));
    }
    if request.kind != CorrespondenceKind::Route {
        return Err(RouteCheckError::InvalidRequest(
            "kind must be route".to_owned(),
        ));
    }
    if request.deadline_ms == 0 {
        return Err(RouteCheckError::InvalidRequest(
            "deadline_ms must be positive".to_owned(),
        ));
    }
    if request.endpoints.len() != 2 {
        return Err(RouteCheckError::InvalidRequest(
            "route checker expects 2 endpoints".to_owned(),
        ));
    }
    let route = &request.endpoints[0];
    let handler = &request.endpoints[1];
    if route.r#type != "route"
        || route.entity_id.kind() != "route"
        || route.entity_id.repo_side() != "old"
    {
        return Err(RouteCheckError::InvalidRequest(
            "endpoint 0 must be old route".to_owned(),
        ));
    }
    if handler.r#type != "handler"
        || handler.entity_id.kind() != "handler"
        || handler.entity_id.repo_side() != "old"
    {
        return Err(RouteCheckError::InvalidRequest(
            "endpoint 1 must be old handler".to_owned(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RouteSide {
    method: String,
    path: String,
    operation_id: String,
    handler: String,
}

#[derive(Debug, Deserialize)]
struct ObservationExcerpt {
    kind: String,
    old: RouteSide,
    new: RouteSide,
}

fn parse_observation(value: &Value) -> Result<ObservationExcerpt> {
    let observation: ObservationExcerpt = serde_json::from_value(value.clone())
        .map_err(|err| RouteCheckError::InvalidRequest(format!("observation_excerpt: {err}")))?;
    if observation.kind != "route" {
        return Err(RouteCheckError::InvalidRequest(
            "observation kind must be route".to_owned(),
        ));
    }
    for field in [
        &observation.old.method,
        &observation.old.path,
        &observation.old.operation_id,
        &observation.old.handler,
        &observation.new.method,
        &observation.new.path,
        &observation.new.operation_id,
        &observation.new.handler,
    ] {
        if field.is_empty() {
            return Err(RouteCheckError::InvalidRequest(
                "route observation fields must be non-empty".to_owned(),
            ));
        }
    }
    Ok(observation)
}

#[derive(Debug, Deserialize)]
struct RewriteWire {
    kind: String,
    old: EntityId,
    new: EntityId,
}

#[derive(Debug, Deserialize)]
struct MigrationMapWire {
    #[serde(default)]
    conflicts: Vec<Value>,
    #[serde(default)]
    entity_rewrites: Vec<RewriteWire>,
}

enum RewriteProblem {
    Missing,
    Ambiguous,
}

fn parse_rewrites(
    value: &Value,
) -> std::result::Result<BTreeMap<String, EntityId>, RewriteProblem> {
    let wire: MigrationMapWire =
        serde_json::from_value(value.clone()).map_err(|_| RewriteProblem::Missing)?;
    if !wire.conflicts.is_empty() {
        return Err(RewriteProblem::Ambiguous);
    }
    let mut rewrites = BTreeMap::new();
    for rewrite in wire.entity_rewrites {
        if rewrite.kind != rewrite.old.kind()
            || rewrite.kind != rewrite.new.kind()
            || rewrite.old.kind() != rewrite.new.kind()
        {
            return Err(RewriteProblem::Ambiguous);
        }
        if rewrite.old.repo_side() != "old" || rewrite.new.repo_side() != "new" {
            return Err(RewriteProblem::Ambiguous);
        }
        if rewrites
            .insert(rewrite.old.as_str().to_owned(), rewrite.new)
            .is_some()
        {
            return Err(RewriteProblem::Ambiguous);
        }
    }
    Ok(rewrites)
}

fn entity_matches_observation(entity: &EntityId, route: &RouteSide, expected_kind: &str) -> bool {
    match expected_kind {
        "route" => {
            let method = route.method.to_ascii_lowercase();
            entity.local_path().contains(&route.path.replace('/', "~1"))
                && entity.local_path().ends_with(&format!("/{method}"))
        }
        "handler" => entity
            .local_path()
            .ends_with(&format!("#{}", route.handler)),
        _ => false,
    }
}

fn has_unsupported_feature(value: &Value, feature: &str) -> bool {
    value
        .get("unsupported_features")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("feature").and_then(Value::as_str) == Some(feature))
        })
}

fn has_any_unsupported_feature(value: &Value) -> bool {
    value
        .get("unsupported_features")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
}

fn unknown(reason: UnknownReason) -> Evidence {
    Evidence::unknown(
        reason,
        PredicateId::new(MIGRATE_RULE_ID),
        Vec::new(),
        Vec::new(),
        Version::new(PLUGIN_VERSION),
        Version::new(MIGRATE_RULE_ID),
    )
}

fn broken(reason: BrokenReason) -> Evidence {
    Evidence::broken(
        reason,
        PredicateId::new(MIGRATE_RULE_ID),
        Vec::new(),
        Vec::new(),
        Version::new(PLUGIN_VERSION),
        Version::new(MIGRATE_RULE_ID),
    )
}
