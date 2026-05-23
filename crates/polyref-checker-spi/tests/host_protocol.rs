//! Layer 3 host protocol contract tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use polyref_checker_spi::host::{
    decode_response_line, encode_request_line, PluginHostError, PluginMethod, PluginRequestId,
};
use polyref_checker_spi::limits::Limits;
use serde_json::json;

fn small_limits() -> Limits {
    Limits {
        max_payload_bytes: 96,
        max_json_depth: 64,
        max_id_bytes: 16 * 1024,
        max_path_bytes: 4 * 1024,
        max_deadline_ms: 600_000,
    }
}

#[test]
fn request_line_is_deterministic_single_line_json() {
    let id = PluginRequestId::new("req-1").unwrap();
    let params = json!({"z": 1, "a": 2});

    let first = encode_request_line(PluginMethod::Check, &id, params.clone(), Limits::default()).unwrap();
    let second = encode_request_line(PluginMethod::Check, &id, params, Limits::default()).unwrap();

    assert_eq!(first, second);
    assert!(first.ends_with(b"\n"));
    assert_eq!(first.iter().filter(|byte| **byte == b'\n').count(), 1);
    assert_eq!(
        std::str::from_utf8(&first).unwrap(),
        "{\"jsonrpc\":\"2.0\",\"method\":\"check\",\"id\":\"req-1\",\"params\":{\"a\":2,\"z\":1}}\n"
    );
}

#[test]
fn request_line_rejects_oversized_payload_before_transport() {
    let id = PluginRequestId::new("req-1").unwrap();
    let err = encode_request_line(
        PluginMethod::Extract,
        &id,
        json!({"payload": "x".repeat(200)}),
        small_limits(),
    )
    .unwrap_err();

    assert!(matches!(err, PluginHostError::PayloadTooLarge { .. }));
}

#[test]
fn response_line_rejects_malformed_json() {
    let id = PluginRequestId::new("req-1").unwrap();
    let err = decode_response_line(b"{not-json}\n", &id, Limits::default()).unwrap_err();

    assert!(matches!(err, PluginHostError::Json(_)));
}

#[test]
fn response_line_rejects_mismatched_id() {
    let id = PluginRequestId::new("req-1").unwrap();
    let err = decode_response_line(
        br#"{"jsonrpc":"2.0","id":"other","result":{}}"#,
        &id,
        Limits::default(),
    )
    .unwrap_err();

    assert!(matches!(err, PluginHostError::UnexpectedId { .. }));
}

#[test]
fn response_line_rejects_result_and_error_together() {
    let id = PluginRequestId::new("req-1").unwrap();
    let err = decode_response_line(
        br#"{"jsonrpc":"2.0","id":"req-1","result":{},"error":{"code":-1,"message":"bad"}}"#,
        &id,
        Limits::default(),
    )
    .unwrap_err();

    assert!(matches!(err, PluginHostError::MalformedResponse(_)));
}

#[test]
fn response_line_rejects_lsp_style_framing() {
    let id = PluginRequestId::new("req-1").unwrap();
    let err = decode_response_line(
        b"Content-Length: 2\r\n\r\n{}",
        &id,
        Limits::default(),
    )
    .unwrap_err();

    assert!(matches!(err, PluginHostError::MalformedResponse(_)));
}

#[test]
fn method_parse_rejects_unknown_method() {
    let err = PluginMethod::parse("execute").unwrap_err();

    assert!(matches!(err, PluginHostError::UnsupportedMethod(_)));
}
