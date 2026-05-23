//! OpenAPI route extractor plugin.
//!
//! The extractor is deterministic and intentionally offline-only: it
//! resolves local `#/...` refs required by the Layer 4 route fixture and
//! records remote refs as unsupported features without fetching them.

use polyref_checker_spi::extractor::{
    ExtractRequest, ExtractResult, ExtractedEntity, UnsupportedFeatureNote,
};
use polyref_core::ids::{ArtifactId, EntityId, IdParseError};
use polyref_core::language::Language;
use polyref_core::source_span::{LineCol, SourceSpan, SpanError};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Extractor semantic version surfaced in every `ExtractResult`.
pub const EXTRACTOR_VERSION: &str = "openapi-route-0.1.0";

const HTTP_METHODS: &[&str] = &[
    "delete", "get", "head", "options", "patch", "post", "put", "trace",
];

/// Errors returned by the OpenAPI extractor.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OpenApiExtractError {
    /// The request was not for OpenAPI input.
    #[error("unsupported language: {0}")]
    UnsupportedLanguage(&'static str),
    /// The artifact path could not be resolved safely under the root.
    #[error("unsafe artifact path: {0}")]
    UnsafePath(String),
    /// File-system I/O failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The provided SHA-256 does not match the file bytes.
    #[error("content hash mismatch: expected {expected}, actual {actual}")]
    ContentHashMismatch {
        /// Request hash.
        expected: String,
        /// Actual file hash.
        actual: String,
    },
    /// YAML/JSON parsing failed.
    #[error("parse error: {0}")]
    Parse(String),
    /// Required OpenAPI shape is missing or malformed.
    #[error("invalid OpenAPI document: {0}")]
    InvalidSpec(&'static str),
    /// Duplicate operation ids are ambiguous and fail closed.
    #[error("duplicate operationId: {0}")]
    DuplicateOperationId(String),
    /// ID construction failed.
    #[error("id parse error: {0}")]
    Id(#[from] IdParseError),
    /// Source span construction failed.
    #[error("source span error: {0}")]
    Span(#[from] SpanError),
}

/// Extract one OpenAPI artifact relative to `root`.
///
/// # Errors
///
/// Returns [`OpenApiExtractError`] for unsafe paths, hash mismatch,
/// malformed OpenAPI input, ambiguous operation ids, or invalid emitted ids.
pub fn extract_openapi(
    root: &Path,
    request: &ExtractRequest,
) -> Result<ExtractResult, OpenApiExtractError> {
    if request.language != Language::Openapi {
        return Err(OpenApiExtractError::UnsupportedLanguage(
            request.language.as_tag(),
        ));
    }

    let artifact_path = resolve_artifact_path(root, request.artifact_path.as_str())?;
    let bytes = fs::read(artifact_path)?;
    let actual_hash = sha256_hex(&bytes);
    if actual_hash != request.content_hash {
        return Err(OpenApiExtractError::ContentHashMismatch {
            expected: request.content_hash.clone(),
            actual: actual_hash,
        });
    }

    let document: Value = serde_yaml_ng::from_slice(&bytes)
        .map_err(|err| OpenApiExtractError::Parse(err.to_string()))?;
    let source =
        std::str::from_utf8(&bytes).map_err(|err| OpenApiExtractError::Parse(err.to_string()))?;
    let side = repo_side(request.artifact_path.as_str())?;
    let local_path = local_artifact_path(request.artifact_path.as_str())?;
    let artifact_id = ArtifactId::parse(&format!(
        "artifact:{side}:{local_path}:{}",
        &request.content_hash[..12]
    ))?;

    let routes = extract_routes(&document, source, side, local_path, &artifact_id)?;
    let mut entities = Vec::with_capacity(routes.len());
    let mut local_facts = Vec::with_capacity(routes.len());
    let mut unsupported_features = Vec::new();

    for route in routes {
        entities.push(route.entity);
        local_facts.push(route.fact);
        unsupported_features.extend(route.unsupported_features);
    }

    entities.sort_by(|left, right| left.entity_id.as_str().cmp(right.entity_id.as_str()));
    local_facts.sort_by_key(fact_sort_key);
    unsupported_features.sort_by(|left, right| {
        left.feature
            .cmp(&right.feature)
            .then_with(|| left.note.cmp(&right.note))
    });

    Ok(ExtractResult {
        entities,
        local_facts,
        unsupported_features,
        extractor_version: EXTRACTOR_VERSION.to_owned(),
    })
}

struct RouteExtraction {
    entity: ExtractedEntity,
    fact: Value,
    unsupported_features: Vec<UnsupportedFeatureNote>,
}

fn extract_routes(
    document: &Value,
    source: &str,
    side: &str,
    local_path: &str,
    artifact_id: &ArtifactId,
) -> Result<Vec<RouteExtraction>, OpenApiExtractError> {
    let object = document
        .as_object()
        .ok_or(OpenApiExtractError::InvalidSpec("root must be an object"))?;
    let version = object
        .get("openapi")
        .and_then(Value::as_str)
        .ok_or(OpenApiExtractError::InvalidSpec("missing openapi version"))?;
    if !version.starts_with("3.") {
        return Err(OpenApiExtractError::InvalidSpec(
            "only OpenAPI 3.x is supported",
        ));
    }
    let paths = object
        .get("paths")
        .and_then(Value::as_object)
        .ok_or(OpenApiExtractError::InvalidSpec("missing paths object"))?;

    let mut seen_operation_ids = BTreeSet::new();
    let mut routes = Vec::new();
    for (path, path_item) in sorted_object(paths) {
        let Some(path_item_object) = path_item.as_object() else {
            continue;
        };
        for (method, operation) in sorted_object(path_item_object) {
            if !HTTP_METHODS.contains(&method.as_str()) {
                continue;
            }
            let operation_object =
                operation
                    .as_object()
                    .ok_or(OpenApiExtractError::InvalidSpec(
                        "operation must be an object",
                    ))?;
            let operation_id = operation_object
                .get("operationId")
                .and_then(Value::as_str)
                .ok_or(OpenApiExtractError::InvalidSpec(
                    "operation missing operationId",
                ))?;
            if !seen_operation_ids.insert(operation_id.to_owned()) {
                return Err(OpenApiExtractError::DuplicateOperationId(
                    operation_id.to_owned(),
                ));
            }

            routes.push(extract_route(
                document,
                source,
                side,
                local_path,
                artifact_id,
                path,
                method,
                operation_id,
                operation,
            )?);
        }
    }
    Ok(routes)
}

#[allow(clippy::too_many_arguments)]
fn extract_route(
    document: &Value,
    source: &str,
    side: &str,
    local_path: &str,
    artifact_id: &ArtifactId,
    path: &str,
    method: &str,
    operation_id: &str,
    operation: &Value,
) -> Result<RouteExtraction, OpenApiExtractError> {
    let route_local_path = format!(
        "{local_path}#/paths/{}/{}",
        escape_json_pointer(path),
        method
    );
    let stable_hash = stable_entity_hash(
        side,
        "openapi",
        "route",
        &route_local_path,
        &method.to_uppercase(),
        path,
        operation_id,
    );
    let entity_id = EntityId::parse(&format!(
        "{side}:openapi:route:{route_local_path}:{stable_hash}"
    ))?;
    let span = operation_span(source, artifact_id, path, method)?;
    let request_refs = collect_request_schema_refs(operation, document);
    let response_refs = collect_response_schema_refs(operation, document);

    let mut unsupported_features = Vec::new();
    for remote_ref in request_refs
        .remote
        .iter()
        .chain(response_refs.remote.iter())
    {
        unsupported_features.push(UnsupportedFeatureNote {
            feature: "remote_ref".to_owned(),
            span: span.clone(),
            note: Some(remote_ref.clone()),
        });
    }

    let fact = json!({
        "kind": "route",
        "side": side,
        "artifact_id": artifact_id,
        "entity_id": entity_id,
        "method": method.to_uppercase(),
        "path": path,
        "operation_id": operation_id,
        "request_schema_refs": request_refs.local,
        "response_schema_refs": response_refs.local,
    });

    Ok(RouteExtraction {
        entity: ExtractedEntity {
            entity_id,
            kind: "route".to_owned(),
            local_name: operation_id.to_owned(),
            r#type: Some("route".to_owned()),
            source_span: span,
        },
        fact,
        unsupported_features,
    })
}

#[derive(Default)]
struct RefCollection {
    local: Vec<String>,
    remote: Vec<String>,
}

fn collect_request_schema_refs(operation: &Value, document: &Value) -> RefCollection {
    let mut refs = RefCollection::default();
    if let Some(content) = operation
        .pointer("/requestBody/content")
        .and_then(Value::as_object)
    {
        collect_content_refs(content, document, &mut refs);
    }
    refs.local.sort();
    refs.local.dedup();
    refs.remote.sort();
    refs.remote.dedup();
    refs
}

fn collect_response_schema_refs(operation: &Value, document: &Value) -> RefCollection {
    let mut refs = RefCollection::default();
    if let Some(responses) = operation.get("responses").and_then(Value::as_object) {
        for (_status, response) in sorted_object(responses) {
            if let Some(content) = response.get("content").and_then(Value::as_object) {
                collect_content_refs(content, document, &mut refs);
            }
        }
    }
    refs.local.sort();
    refs.local.dedup();
    refs.remote.sort();
    refs.remote.dedup();
    refs
}

fn collect_content_refs(content: &Map<String, Value>, document: &Value, refs: &mut RefCollection) {
    for (_media_type, media) in sorted_object(content) {
        if let Some(schema) = media.get("schema") {
            collect_schema_refs(schema, document, refs, &mut BTreeSet::new());
        }
    }
}

fn collect_schema_refs(
    schema: &Value,
    document: &Value,
    refs: &mut RefCollection,
    visited_refs: &mut BTreeSet<String>,
) {
    match schema {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                if reference.starts_with("#/components/") {
                    refs.local.push(reference.to_owned());
                    if visited_refs.insert(reference.to_owned()) {
                        if let Some(target) = document.pointer(&reference[1..]) {
                            collect_schema_refs(target, document, refs, visited_refs);
                        }
                    }
                } else {
                    refs.remote.push(reference.to_owned());
                }
            }
            for (key, value) in sorted_object(object) {
                if key != "$ref" {
                    collect_schema_refs(value, document, refs, visited_refs);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_schema_refs(item, document, refs, visited_refs);
            }
        }
        _ => {}
    }
}

fn resolve_artifact_path(root: &Path, safe_path: &str) -> Result<PathBuf, OpenApiExtractError> {
    let root = root.canonicalize()?;
    let path = root.join(safe_path);
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(&root) {
        return Err(OpenApiExtractError::UnsafePath(safe_path.to_owned()));
    }
    Ok(canonical)
}

fn repo_side(path: &str) -> Result<&str, OpenApiExtractError> {
    match path.split('/').next() {
        Some(side @ ("old" | "new")) => Ok(side),
        Some(_) if !path.contains('/') => Ok("old"),
        _ => Err(OpenApiExtractError::InvalidSpec(
            "artifact path repo side must be old or new",
        )),
    }
}

fn local_artifact_path(path: &str) -> Result<&str, OpenApiExtractError> {
    match path.split_once('/') {
        Some(("old" | "new", local)) if !local.is_empty() => Ok(local),
        None if !path.is_empty() => Ok(path),
        _ => Err(OpenApiExtractError::InvalidSpec(
            "artifact path missing local path",
        )),
    }
}

fn operation_span(
    source: &str,
    artifact_id: &ArtifactId,
    path: &str,
    method: &str,
) -> Result<SourceSpan, OpenApiExtractError> {
    let path_line = find_line(source, &format!("{path}:"));
    let method_line = find_line_after(source, &format!("{}:", method), path_line.unwrap_or(1));
    let start_line = method_line.or(path_line).unwrap_or(1);
    let end_line = next_sibling_line(source, start_line).unwrap_or(start_line + 1);
    SourceSpan::try_new(
        artifact_id.clone(),
        LineCol::new(nonzero(start_line), 0),
        LineCol::new(nonzero(end_line), 0),
        None,
    )
    .map_err(Into::into)
}

fn find_line(source: &str, needle: &str) -> Option<u32> {
    source
        .lines()
        .position(|line| line.trim() == needle)
        .and_then(|idx| u32::try_from(idx + 1).ok())
}

fn find_line_after(source: &str, needle: &str, after: u32) -> Option<u32> {
    source
        .lines()
        .enumerate()
        .skip(after as usize)
        .find(|(_idx, line)| line.trim() == needle)
        .and_then(|(idx, _line)| u32::try_from(idx + 1).ok())
}

fn next_sibling_line(source: &str, start_line: u32) -> Option<u32> {
    let lines: Vec<&str> = source.lines().collect();
    let start_index = usize::try_from(start_line.saturating_sub(1)).ok()?;
    let start_indent = lines
        .get(start_index)?
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .count();
    for (idx, line) in lines.iter().enumerate().skip(start_index + 1) {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.chars().take_while(|ch| ch.is_whitespace()).count();
        if indent <= start_indent {
            return u32::try_from(idx + 1).ok();
        }
    }
    u32::try_from(lines.len() + 1).ok()
}

fn nonzero(value: u32) -> NonZeroU32 {
    match NonZeroU32::new(value) {
        Some(value) => value,
        None => NonZeroU32::MIN,
    }
}

fn stable_entity_hash(
    side: &str,
    language: &str,
    kind: &str,
    local_path: &str,
    method: &str,
    path: &str,
    operation_id: &str,
) -> String {
    let mut hasher = Sha256::new();
    let parts = [side, language, kind, local_path, method, path, operation_id];
    for (index, part) in parts.iter().enumerate() {
        hasher.update(part.as_bytes());
        if index + 1 != parts.len() {
            hasher.update([0]);
        }
    }
    format!("{:x}", hasher.finalize())[..12].to_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn escape_json_pointer(path: &str) -> String {
    path.replace('~', "~0").replace('/', "~1")
}

fn sorted_object(object: &Map<String, Value>) -> BTreeMap<&String, &Value> {
    object.iter().collect()
}

fn fact_sort_key(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}
