//! Plugin SPI envelope types and Layer 3 host protocol helpers.
//!
//! Plugins (extractors and kind checkers) speak JSON-RPC 2.0 over stdio
//! per ADR-002. The process pool, cgroup glue, and dispatcher are added
//! incrementally in Layer 3; the protocol module contains deterministic
//! one-line JSON-RPC framing shared by those runtime pieces.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cgroup;
pub mod checker;
pub mod envelope;
pub mod error;
pub mod extractor;
pub mod host;
pub mod limits;
pub mod memo;

pub use cgroup::{IsolationBackend, IsolationError, PluginIsolationProfile, SeccompPolicy};
pub use checker::{CheckRequest, CheckResult, DescribeResult, EndpointArg};
pub use envelope::{JsonRpcEnvelopeError, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
pub use error::SpiError;
pub use extractor::{ExtractRequest, ExtractResult, ExtractedEntity, UnsupportedFeatureNote};
pub use host::{
    decode_response_line, encode_request_line, run_plugin_call, PluginBinary, PluginHostError,
    PluginKind, PluginLaunchConfig, PluginMethod, PluginPool, PluginPoolConfig,
};
pub use limits::{Limits, LimitsError, SafePath, SafePathError};
pub use memo::{PluginMemoKey, PluginMemoStore};
