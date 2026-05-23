//! Layer 3 plugin-host protocol helpers.
//!
//! This module owns ADR-002 one-line JSON-RPC framing and validation. It does
//! not spawn plugin processes; process supervision is layered on top so protocol
//! tests can stay deterministic and backend-neutral.

use crate::envelope::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::limits::Limits;
use serde_json::Value;
use thiserror::Error;

/// JSON-RPC methods supported by the PolyRef plugin SPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PluginMethod {
    /// Extract entities/facts from one artifact.
    Extract,
    /// Describe a kind-checker contract.
    Describe,
    /// Check one typed correspondence obligation.
    Check,
}

/// Non-null JSON-RPC request id used by the host.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginRequestId(String);

/// Protocol-layer host errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PluginHostError {
    /// Payload exceeded the configured byte cap.
    #[error("plugin payload exceeds {limit} bytes: {actual}")]
    PayloadTooLarge {
        /// Configured byte limit.
        limit: usize,
        /// Actual byte length.
        actual: usize,
    },
    /// JSON parse or serialization failed.
    #[error("plugin protocol json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Response was structurally invalid.
    #[error("malformed plugin response: {0}")]
    MalformedResponse(String),
    /// Response id did not match the request id.
    #[error("unexpected plugin response id: expected {expected}, actual {actual}")]
    UnexpectedId {
        /// Expected request id.
        expected: String,
        /// Actual response id.
        actual: String,
    },
    /// Method is not in the v1 SPI method set.
    #[error("unsupported plugin method: {0}")]
    UnsupportedMethod(String),
    /// Request id is empty or too large.
    #[error("invalid plugin request id: {0}")]
    InvalidRequestId(String),
}

impl PluginMethod {
    /// Return the JSON-RPC method string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extract => "extract",
            Self::Describe => "describe",
            Self::Check => "check",
        }
    }

    /// Parse a JSON-RPC method string.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::UnsupportedMethod`] for non-SPI methods.
    pub fn parse(input: &str) -> Result<Self, PluginHostError> {
        match input {
            "extract" => Ok(Self::Extract),
            "describe" => Ok(Self::Describe),
            "check" => Ok(Self::Check),
            other => Err(PluginHostError::UnsupportedMethod(other.to_owned())),
        }
    }
}

impl PluginRequestId {
    /// Create a request id.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::InvalidRequestId`] when the id is empty or
    /// exceeds [`Limits::max_id_bytes`].
    pub fn new(input: impl Into<String>) -> Result<Self, PluginHostError> {
        Self::with_limits(input, Limits::default())
    }

    /// Create a request id using explicit limits.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::InvalidRequestId`] when the id is empty or
    /// exceeds [`Limits::max_id_bytes`].
    pub fn with_limits(input: impl Into<String>, limits: Limits) -> Result<Self, PluginHostError> {
        let input = input.into();
        if input.is_empty() {
            return Err(PluginHostError::InvalidRequestId("empty".to_owned()));
        }
        if input.len() > limits.max_id_bytes {
            return Err(PluginHostError::InvalidRequestId("too long".to_owned()));
        }
        Ok(Self(input))
    }

    /// Return the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn as_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

/// Encode a JSON-RPC request as one canonical transport line.
///
/// The returned bytes contain exactly one trailing newline. The cached request
/// bytes for memoization should use [`encode_request_payload`] instead.
///
/// # Errors
///
/// Returns [`PluginHostError`] for serialization failure or size-cap overflow.
pub fn encode_request_line(
    method: PluginMethod,
    id: &PluginRequestId,
    params: Value,
    limits: Limits,
) -> Result<Vec<u8>, PluginHostError> {
    let mut payload = encode_request_payload(method, id, params, limits)?;
    payload.push(b'\n');
    Ok(payload)
}

/// Encode a JSON-RPC request payload without transport newline.
///
/// # Errors
///
/// Returns [`PluginHostError`] for serialization failure or size-cap overflow.
pub fn encode_request_payload(
    method: PluginMethod,
    id: &PluginRequestId,
    params: Value,
    limits: Limits,
) -> Result<Vec<u8>, PluginHostError> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        method: method.as_str().to_owned(),
        id: id.as_value(),
        params,
    };
    let payload = serde_json::to_vec(&request)?;
    enforce_payload_limit(payload.len(), limits.max_payload_bytes)?;
    Ok(payload)
}

/// Decode and validate one JSON-RPC response line.
///
/// # Errors
///
/// Returns [`PluginHostError`] for malformed framing, JSON, size-cap overflow,
/// id mismatch, or result/error shape violations.
pub fn decode_response_line(
    line: &[u8],
    expected_id: &PluginRequestId,
    limits: Limits,
) -> Result<JsonRpcResponse, PluginHostError> {
    reject_lsp_framing(line)?;
    let payload = trim_one_line_ending(line)?;
    enforce_payload_limit(payload.len(), limits.max_payload_bytes)?;
    let response: JsonRpcResponse = serde_json::from_slice(payload)?;
    validate_response(&response, expected_id)?;
    Ok(response)
}

fn validate_response(
    response: &JsonRpcResponse,
    expected_id: &PluginRequestId,
) -> Result<(), PluginHostError> {
    if response.jsonrpc != "2.0" {
        return Err(PluginHostError::MalformedResponse(
            "invalid jsonrpc version".to_owned(),
        ));
    }
    if response.id != expected_id.as_value() {
        return Err(PluginHostError::UnexpectedId {
            expected: expected_id.as_str().to_owned(),
            actual: response.id.to_string(),
        });
    }
    match (&response.result, &response.error) {
        (Some(_), None) | (None, Some(_)) => Ok(()),
        (Some(_), Some(_)) => Err(PluginHostError::MalformedResponse(
            "response contains both result and error".to_owned(),
        )),
        (None, None) => Err(PluginHostError::MalformedResponse(
            "response contains neither result nor error".to_owned(),
        )),
    }
}

fn reject_lsp_framing(line: &[u8]) -> Result<(), PluginHostError> {
    if line.starts_with(b"Content-Length:") || line.starts_with(b"content-length:") {
        return Err(PluginHostError::MalformedResponse(
            "LSP-style framing is not supported".to_owned(),
        ));
    }
    Ok(())
}

fn trim_one_line_ending(line: &[u8]) -> Result<&[u8], PluginHostError> {
    let trimmed = match line.strip_suffix(b"\n") {
        Some(without_lf) => without_lf.strip_suffix(b"\r").unwrap_or(without_lf),
        None => line,
    };
    if trimmed.contains(&b'\n') || trimmed.contains(&b'\r') {
        return Err(PluginHostError::MalformedResponse(
            "response must be a single JSON line".to_owned(),
        ));
    }
    if trimmed.is_empty() {
        return Err(PluginHostError::MalformedResponse(
            "empty response".to_owned(),
        ));
    }
    Ok(trimmed)
}

fn enforce_payload_limit(actual: usize, limit: usize) -> Result<(), PluginHostError> {
    if actual > limit {
        return Err(PluginHostError::PayloadTooLarge { limit, actual });
    }
    Ok(())
}

#[allow(dead_code)]
fn _json_rpc_error_is_public_contract(_: JsonRpcError) {}
