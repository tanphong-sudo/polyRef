//! JSON-RPC stdio adapter for the OpenAPI extractor plugin.

use polyref_checker_spi::envelope::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use polyref_checker_spi::extractor::ExtractRequest;
use std::io::{self, BufRead, Write};

fn main() {
    if let Err(err) = run() {
        let _ = writeln!(io::stderr(), "{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let response = handle_line(&line);
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle_line(line: &str) -> JsonRpcResponse {
    match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(request) => handle_request(request),
        Err(err) => JsonRpcResponse {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::Value::Null,
            result: None,
            error: Some(JsonRpcError {
                code: -32700,
                message: "parse error".to_owned(),
                data: Some(serde_json::json!({ "detail": err.to_string() })),
            }),
        },
    }
}

fn handle_request(request: JsonRpcRequest) -> JsonRpcResponse {
    if request.jsonrpc != "2.0" {
        return error_response(request.id, -32600, "invalid jsonrpc version", None);
    }
    if request.method != "extract" {
        return error_response(request.id, -32601, "method not found", None);
    }
    let extract_request = match serde_json::from_value::<ExtractRequest>(request.params) {
        Ok(value) => value,
        Err(err) => {
            return error_response(
                request.id,
                -32602,
                "invalid extract params",
                Some(serde_json::json!({ "detail": err.to_string() })),
            )
        }
    };
    match polyref_extractor_openapi::extract_openapi(std::path::Path::new("."), &extract_request) {
        Ok(result) => match serde_json::to_value(result) {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_owned(),
                id: request.id,
                result: Some(value),
                error: None,
            },
            Err(err) => error_response(
                request.id,
                -32603,
                "internal serialization error",
                Some(serde_json::json!({ "detail": err.to_string() })),
            ),
        },
        Err(err) => error_response(
            request.id,
            -32000,
            "extract failed",
            Some(serde_json::json!({ "detail": err.to_string() })),
        ),
    }
}

fn error_response(
    id: serde_json::Value,
    code: i32,
    message: &str,
    data: Option<serde_json::Value>,
) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_owned(),
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_owned(),
            data,
        }),
    }
}
