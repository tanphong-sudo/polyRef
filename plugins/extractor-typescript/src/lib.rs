//! TypeScript route handler extractor plugin.
//!
//! The extractor is deterministic and offline-only. It uses tree-sitter
//! for syntax validation and source spans, and extracts the documented
//! route metadata shape used by the Layer 4 fixture.

use polyref_checker_spi::extractor::{
    ExtractRequest, ExtractResult, ExtractedEntity, UnsupportedFeatureNote,
};
use polyref_core::ids::{ArtifactId, EntityId, IdParseError};
use polyref_core::language::Language;
use polyref_core::source_span::{LineCol, SourceSpan, SpanError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tree_sitter::{Node, Parser, Point, Tree};

/// Extractor semantic version surfaced in every `ExtractResult`.
pub const EXTRACTOR_VERSION: &str = "typescript-route-0.1.0";

/// Errors returned by the TypeScript extractor.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TypeScriptExtractError {
    /// The request was not for TypeScript input.
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
    /// Source was not valid UTF-8.
    #[error("parse error: {0}")]
    Parse(String),
    /// Required TypeScript shape is missing or malformed.
    #[error("invalid TypeScript route metadata: {0}")]
    InvalidRoute(&'static str),
    /// Duplicate route metadata is ambiguous and fails closed.
    #[error("duplicate route metadata")]
    DuplicateRouteMetadata,
    /// Duplicate exported handler names are ambiguous and fail closed.
    #[error("duplicate exported handler: {0}")]
    DuplicateHandler(String),
    /// ID construction failed.
    #[error("id parse error: {0}")]
    Id(#[from] IdParseError),
    /// Source span construction failed.
    #[error("source span error: {0}")]
    Span(#[from] SpanError),
}

/// Extract one TypeScript artifact relative to `root`.
///
/// # Errors
///
/// Returns [`TypeScriptExtractError`] for unsafe paths, hash mismatch,
/// malformed TypeScript input, ambiguous route metadata, or invalid ids.
pub fn extract_typescript(
    root: &Path,
    request: &ExtractRequest,
) -> Result<ExtractResult, TypeScriptExtractError> {
    if request.language != Language::Ts {
        return Err(TypeScriptExtractError::UnsupportedLanguage(
            request.language.as_tag(),
        ));
    }

    let artifact_path = resolve_artifact_path(root, request.artifact_path.as_str())?;
    let bytes = fs::read(artifact_path)?;
    let actual_hash = sha256_hex(&bytes);
    if actual_hash != request.content_hash {
        return Err(TypeScriptExtractError::ContentHashMismatch {
            expected: request.content_hash.clone(),
            actual: actual_hash,
        });
    }
    let source = std::str::from_utf8(&bytes)
        .map_err(|err| TypeScriptExtractError::Parse(err.to_string()))?;
    let mut parser = Parser::new();
    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT;
    parser
        .set_language(&language.into())
        .map_err(|err| TypeScriptExtractError::Parse(err.to_string()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| TypeScriptExtractError::Parse("parser returned no tree".to_owned()))?;
    if tree.root_node().has_error() {
        return Err(TypeScriptExtractError::Parse(
            "tree-sitter reported syntax error".to_owned(),
        ));
    }

    let side = repo_side(request.artifact_path.as_str())?;
    let local_path = local_artifact_path(request.artifact_path.as_str())?;
    let artifact_id = ArtifactId::parse(&format!(
        "artifact:{side}:{local_path}:{}",
        &request.content_hash[..12]
    ))?;
    let route_candidates = route_metadata_candidates(&tree, source)?;
    if route_candidates.len() > 1 {
        return Err(TypeScriptExtractError::DuplicateRouteMetadata);
    }
    let exported_handlers = exported_handlers(&tree, source)?;

    let mut entities = Vec::new();
    let mut local_facts = Vec::new();
    let mut unsupported_features = Vec::new();

    if let Some(route) = route_candidates.into_iter().next() {
        if route.is_dynamic {
            if let Some(handler_name) = &route.handler {
                let handler = exported_handlers.get(handler_name.as_str()).ok_or(
                    TypeScriptExtractError::InvalidRoute("route handler export not found"),
                )?;
                let entity_id = handler_entity_id(side, local_path, handler_name)?;
                entities.push(ExtractedEntity {
                    entity_id,
                    kind: "handler".to_owned(),
                    local_name: handler_name.clone(),
                    r#type: Some("handler".to_owned()),
                    source_span: source_span(&artifact_id, handler.node)?,
                });
            }
            unsupported_features.push(UnsupportedFeatureNote {
                feature: "dynamic_route".to_owned(),
                span: source_span(&artifact_id, route.node)?,
                note: Some(
                    "route metadata contains a non-literal method, path, operationId, or handler"
                        .to_owned(),
                ),
            });
        } else {
            let handler_name =
                route
                    .handler
                    .as_deref()
                    .ok_or(TypeScriptExtractError::InvalidRoute(
                        "route metadata missing handler",
                    ))?;
            let handler =
                exported_handlers
                    .get(handler_name)
                    .ok_or(TypeScriptExtractError::InvalidRoute(
                        "route handler export not found",
                    ))?;
            let entity_id = handler_entity_id(side, local_path, handler_name)?;
            entities.push(ExtractedEntity {
                entity_id: entity_id.clone(),
                kind: "handler".to_owned(),
                local_name: handler_name.to_owned(),
                r#type: Some("handler".to_owned()),
                source_span: source_span(&artifact_id, handler.node)?,
            });
            local_facts.push(json!({
                "kind": "route_metadata",
                "side": side,
                "artifact_id": artifact_id,
                "handler_entity_id": entity_id,
                "method": route.method,
                "path": route.path,
                "operation_id": route.operation_id,
                "handler_export": handler_name,
                "local_path": local_path,
            }));
        }
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

#[derive(Debug, Clone)]
struct RouteMetadata<'tree> {
    method: String,
    path: String,
    operation_id: String,
    handler: Option<String>,
    is_dynamic: bool,
    node: Node<'tree>,
}

#[derive(Debug, Clone, Copy)]
struct HandlerExport<'tree> {
    node: Node<'tree>,
}

fn route_metadata_candidates<'tree>(
    tree: &'tree Tree,
    source: &str,
) -> Result<Vec<RouteMetadata<'tree>>, TypeScriptExtractError> {
    let mut candidates = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "variable_declarator" {
            if let Some(candidate) = route_metadata_from_declarator(node, source)? {
                candidates.push(candidate);
            }
        }
        push_children(node, &mut stack);
    }
    candidates.sort_by_key(|route| (route.node.start_byte(), route.node.end_byte()));
    Ok(candidates)
}

fn route_metadata_from_declarator<'tree>(
    node: Node<'tree>,
    source: &str,
) -> Result<Option<RouteMetadata<'tree>>, TypeScriptExtractError> {
    let name = node
        .child_by_field_name("name")
        .and_then(|child| node_text(child, source));
    if !matches!(name.as_deref(), Some(name) if name.starts_with("route")) {
        return Ok(None);
    }
    let value = node
        .child_by_field_name("value")
        .ok_or(TypeScriptExtractError::InvalidRoute(
            "route metadata missing value",
        ))?;
    let object = match value.kind() {
        "object" => value,
        "as_expression" => value
            .child(0)
            .filter(|child| child.kind() == "object")
            .ok_or(TypeScriptExtractError::InvalidRoute(
                "route metadata as-expression missing object",
            ))?,
        _ => return Ok(None),
    };
    let properties = object_properties(object, source);
    if !properties
        .keys()
        .any(|key| ["method", "path", "operationId", "handler"].contains(&key.as_str()))
    {
        return Ok(None);
    }
    let mut is_dynamic = false;
    let method = literal_property(&properties, "method", source, &mut is_dynamic);
    let path = literal_property(&properties, "path", source, &mut is_dynamic);
    let operation_id = literal_property(&properties, "operationId", source, &mut is_dynamic);
    let handler = literal_property(&properties, "handler", source, &mut is_dynamic);
    let has_missing_required_literal =
        method.is_none() || path.is_none() || operation_id.is_none() || handler.is_none();
    Ok(Some(RouteMetadata {
        method: method.unwrap_or_default().to_ascii_uppercase(),
        path: path.unwrap_or_default(),
        operation_id: operation_id.unwrap_or_default(),
        handler,
        is_dynamic: is_dynamic || has_missing_required_literal,
        node,
    }))
}

fn object_properties<'tree>(object: Node<'tree>, source: &str) -> BTreeMap<String, Node<'tree>> {
    let mut properties = BTreeMap::new();
    let mut cursor = object.walk();
    for child in object.named_children(&mut cursor) {
        if child.kind() == "pair" {
            if let Some(key_node) = child.child_by_field_name("key") {
                if let Some(key) = property_key(key_node, source) {
                    if let Some(value) = child.child_by_field_name("value") {
                        properties.insert(key, value);
                    }
                }
            }
        }
    }
    properties
}

fn property_key(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "property_identifier" | "identifier" => node_text(node, source),
        "string" => string_literal_value(node, source),
        _ => None,
    }
}

fn literal_property(
    properties: &BTreeMap<String, Node<'_>>,
    key: &str,
    source: &str,
    is_dynamic: &mut bool,
) -> Option<String> {
    let value = *properties.get(key)?;
    match value.kind() {
        "string" => string_literal_value(value, source),
        _ => {
            *is_dynamic = true;
            None
        }
    }
}

fn exported_handlers<'tree>(
    tree: &'tree Tree,
    source: &str,
) -> Result<BTreeMap<String, HandlerExport<'tree>>, TypeScriptExtractError> {
    let mut handlers = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "export_statement" {
            if let Some(function_node) = find_descendant_kind(node, "function_declaration") {
                if let Some(name_node) = function_node.child_by_field_name("name") {
                    if let Some(name) = node_text(name_node, source) {
                        if !seen.insert(name.clone()) {
                            return Err(TypeScriptExtractError::DuplicateHandler(name));
                        }
                        handlers.insert(
                            name,
                            HandlerExport {
                                node: function_node,
                            },
                        );
                    }
                }
            }
        }
        push_children(node, &mut stack);
    }
    Ok(handlers)
}

fn find_descendant_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == kind {
            return Some(current);
        }
        push_children(current, &mut stack);
    }
    None
}

fn push_children<'tree>(node: Node<'tree>, stack: &mut Vec<Node<'tree>>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        stack.push(child);
    }
}

fn node_text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes())
        .ok()
        .map(ToOwned::to_owned)
}

fn string_literal_value(node: Node<'_>, source: &str) -> Option<String> {
    let raw = node_text(node, source)?;
    raw.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            raw.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .map(ToOwned::to_owned)
}

fn handler_entity_id(
    side: &str,
    local_path: &str,
    handler: &str,
) -> Result<EntityId, TypeScriptExtractError> {
    let handler_local_path = format!("{local_path}#{handler}");
    let stable_hash = stable_handler_hash(side, &handler_local_path, handler);
    Ok(EntityId::parse(&format!(
        "{side}:ts:handler:{handler_local_path}:{stable_hash}"
    ))?)
}

fn stable_handler_hash(side: &str, local_path: &str, handler: &str) -> String {
    let mut hasher = Sha256::new();
    let parts = [side, "ts", "handler", local_path, handler];
    for (index, part) in parts.iter().enumerate() {
        hasher.update(part.as_bytes());
        if index + 1 != parts.len() {
            hasher.update([0]);
        }
    }
    format!("{:x}", hasher.finalize())[..12].to_owned()
}

fn source_span(
    artifact_id: &ArtifactId,
    node: Node<'_>,
) -> Result<SourceSpan, TypeScriptExtractError> {
    SourceSpan::try_new(
        artifact_id.clone(),
        point_to_line_col(node.start_position()),
        point_to_line_col(node.end_position()),
        None,
    )
    .map_err(Into::into)
}

fn point_to_line_col(point: Point) -> LineCol {
    let line = u32::try_from(point.row + 1).unwrap_or(u32::MAX);
    let col = u32::try_from(point.column).unwrap_or(u32::MAX);
    LineCol::new(nonzero(line), col)
}

fn resolve_artifact_path(root: &Path, safe_path: &str) -> Result<PathBuf, TypeScriptExtractError> {
    let root = root.canonicalize()?;
    let path = root.join(safe_path);
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(&root) {
        return Err(TypeScriptExtractError::UnsafePath(safe_path.to_owned()));
    }
    Ok(canonical)
}

fn repo_side(path: &str) -> Result<&str, TypeScriptExtractError> {
    match path.split('/').next() {
        Some(side @ ("old" | "new")) => Ok(side),
        Some(_) if !path.contains('/') || path.starts_with("src/") => Ok("old"),
        _ => Err(TypeScriptExtractError::InvalidRoute(
            "artifact path repo side must be old or new",
        )),
    }
}

fn local_artifact_path(path: &str) -> Result<&str, TypeScriptExtractError> {
    match path.split_once('/') {
        Some(("old" | "new", local)) if !local.is_empty() => Ok(local),
        None if !path.is_empty() => Ok(path),
        Some(("src", _)) => Ok(path),
        _ => Err(TypeScriptExtractError::InvalidRoute(
            "artifact path missing local path",
        )),
    }
}

fn nonzero(value: u32) -> NonZeroU32 {
    match NonZeroU32::new(value) {
        Some(value) => value,
        None => NonZeroU32::MIN,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn fact_sort_key(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}
