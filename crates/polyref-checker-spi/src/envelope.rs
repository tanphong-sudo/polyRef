//! JSON-RPC 2.0 envelope types.
//!
//! Plugins receive one JSON object per line on stdin and reply with one
//! JSON object on stdout. LSP-style framing is rejected for v1
//! (ADR-002); a single line is simpler to size-cap and audit.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// `"2.0"`.
    pub jsonrpc: String,
    /// Method name (`extract`, `describe`, `check`).
    pub method: String,
    /// Request id; never null in our SPI.
    pub id: serde_json::Value,
    /// Method-specific parameters.
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// `"2.0"`.
    pub jsonrpc: String,
    /// Echoed request id.
    pub id: serde_json::Value,
    /// Successful result; mutually exclusive with `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error payload; mutually exclusive with `result`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Numeric error code per JSON-RPC.
    pub code: i32,
    /// Human-readable message.
    pub message: String,
    /// Optional structured error data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Errors validating an envelope.
#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum JsonRpcEnvelopeError {
    /// `jsonrpc` field absent or wrong.
    #[error("invalid jsonrpc version")]
    BadVersion,
    /// Request lacks an id (we never use notifications).
    #[error("request lacks id")]
    NoId,
    /// Method name unknown.
    #[error("unknown method")]
    UnknownMethod,
    /// Payload exceeded the wire size cap.
    #[error("oversize payload")]
    Oversize,
    /// Body was not a JSON object.
    #[error("body must be a JSON object")]
    NotAnObject,
}
