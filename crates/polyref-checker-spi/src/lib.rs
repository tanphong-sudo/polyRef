//! Plugin SPI envelope types — host side.
//!
//! Plugins (extractors and kind checkers) speak JSON-RPC 2.0 over stdio
//! per ADR-002. This crate provides the wire types only; the plugin
//! process pool, cgroup glue, and dispatcher live in Slice 3 and are
//! out of scope here.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod checker;
pub mod envelope;
pub mod error;
pub mod extractor;
pub mod limits;

pub use checker::{CheckRequest, CheckResult, DescribeResult, EndpointArg};
pub use envelope::{JsonRpcRequest, JsonRpcResponse, JsonRpcError, JsonRpcEnvelopeError};
pub use error::SpiError;
pub use extractor::{ExtractRequest, ExtractResult, ExtractedEntity, UnsupportedFeatureNote};
pub use limits::{Limits, LimitsError, SafePath, SafePathError};
