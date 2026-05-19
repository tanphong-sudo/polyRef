//! Canonical JSON per RFC 8785 (JSON Canonicalization Scheme).
//!
//! Implementation is self-contained (no external dep). The algorithm:
//! 1. Object keys sorted lexicographically by UTF-16 code units (same
//!    as sorting by Unicode codepoint for BMP; RFC 8785 §3.2.3).
//! 2. No whitespace between tokens.
//! 3. Numbers serialized per ECMAScript `Number.toString()` rules
//!    (RFC 8785 §3.2.2.3). We rely on `serde_json` which already
//!    produces compliant output for finite f64 values.
//! 4. Strings use minimal escaping per RFC 8785 §3.2.2.2.
//!
//! Hard caps (per F-6):
//! - Max payload size: 16 MiB.
//! - Max nesting depth: 64.
//! - Non-finite numbers (NaN, Infinity) are rejected.

use thiserror::Error;

/// Hard cap on canonical-JSON payload size (bytes).
pub const PAYLOAD_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Hard cap on JSON nesting depth.
pub const PAYLOAD_MAX_DEPTH: usize = 64;

/// Errors emitted by [`canonicalize`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CanonicalError {
    /// Input exceeded [`PAYLOAD_MAX_BYTES`].
    #[error("payload exceeds {} bytes", PAYLOAD_MAX_BYTES)]
    Oversize,
    /// Input nesting exceeds [`PAYLOAD_MAX_DEPTH`].
    #[error("payload exceeds depth {}", PAYLOAD_MAX_DEPTH)]
    TooDeep,
    /// Input contains a NaN or infinity.
    #[error("payload contains non-finite number")]
    NonFinite,
    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialize(String),
}

/// Canonicalize a JSON value per RFC 8785.
///
/// Returns the canonical byte representation. The output is
/// deterministic: same logical value → same bytes, regardless of
/// original key order or whitespace.
pub fn canonicalize(value: &serde_json::Value) -> Result<Vec<u8>, CanonicalError> {
    // Check depth and non-finite numbers first.
    check_value(value, 0)?;

    let mut buf = Vec::with_capacity(256);
    write_value(value, &mut buf)?;

    if buf.len() > PAYLOAD_MAX_BYTES {
        return Err(CanonicalError::Oversize);
    }

    Ok(buf)
}

/// Recursively check depth and non-finite numbers.
fn check_value(value: &serde_json::Value, depth: usize) -> Result<(), CanonicalError> {
    if depth > PAYLOAD_MAX_DEPTH {
        return Err(CanonicalError::TooDeep);
    }
    match value {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.is_nan() || f.is_infinite() {
                    return Err(CanonicalError::NonFinite);
                }
            }
            Ok(())
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                check_value(item, depth + 1)?;
            }
            Ok(())
        }
        serde_json::Value::Object(obj) => {
            for (_, v) in obj {
                check_value(v, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Write a value in canonical form to the buffer.
fn write_value(value: &serde_json::Value, buf: &mut Vec<u8>) -> Result<(), CanonicalError> {
    match value {
        serde_json::Value::Null => {
            buf.extend_from_slice(b"null");
        }
        serde_json::Value::Bool(b) => {
            if *b {
                buf.extend_from_slice(b"true");
            } else {
                buf.extend_from_slice(b"false");
            }
        }
        serde_json::Value::Number(n) => {
            // serde_json's Display for Number is ECMAScript-compliant for
            // finite values (no leading zeros, no trailing zeros after
            // decimal, uses exponential notation where appropriate).
            let s = n.to_string();
            buf.extend_from_slice(s.as_bytes());
        }
        serde_json::Value::String(s) => {
            write_string(s, buf);
        }
        serde_json::Value::Array(arr) => {
            buf.push(b'[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                write_value(item, buf)?;
            }
            buf.push(b']');
        }
        serde_json::Value::Object(obj) => {
            // RFC 8785 §3.2.3: sort keys by UTF-16 code unit values.
            // For strings that are valid UTF-8 (which JSON requires),
            // sorting by UTF-16 code units is equivalent to sorting by
            // Unicode codepoints for BMP characters. For supplementary
            // characters, we need to compare by UTF-16 encoding.
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort_by(|a, b| cmp_utf16(a, b));

            buf.push(b'{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                write_string(key, buf);
                buf.push(b':');
                write_value(&obj[*key], buf)?;
            }
            buf.push(b'}');
        }
    }
    Ok(())
}

/// Write a JSON string with minimal escaping per RFC 8785 §3.2.2.2.
fn write_string(s: &str, buf: &mut Vec<u8>) {
    buf.push(b'"');
    for ch in s.chars() {
        match ch {
            '"' => buf.extend_from_slice(b"\\\""),
            '\\' => buf.extend_from_slice(b"\\\\"),
            '\u{0008}' => buf.extend_from_slice(b"\\b"),
            '\u{000C}' => buf.extend_from_slice(b"\\f"),
            '\n' => buf.extend_from_slice(b"\\n"),
            '\r' => buf.extend_from_slice(b"\\r"),
            '\t' => buf.extend_from_slice(b"\\t"),
            c if c < '\u{0020}' => {
                // Other control chars: \u00XX
                let n = c as u32;
                buf.extend_from_slice(format!("\\u{n:04x}").as_bytes());
            }
            c => {
                let mut utf8_buf = [0u8; 4];
                buf.extend_from_slice(c.encode_utf8(&mut utf8_buf).as_bytes());
            }
        }
    }
    buf.push(b'"');
}

/// Compare two strings by UTF-16 code unit values (RFC 8785 §3.2.3).
fn cmp_utf16(a: &str, b: &str) -> std::cmp::Ordering {
    let a_units: Vec<u16> = a.encode_utf16().collect();
    let b_units: Vec<u16> = b.encode_utf16().collect();
    a_units.cmp(&b_units)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_keys() {
        let val = json!({"b": 2, "a": 1});
        let out = canonicalize(&val).unwrap();
        assert_eq!(std::str::from_utf8(&out).unwrap(), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn canonical_json_is_stable_under_key_reorder() {
        let v1 = json!({"z": 1, "a": 2, "m": 3});
        let v2 = json!({"a": 2, "m": 3, "z": 1});
        assert_eq!(canonicalize(&v1).unwrap(), canonicalize(&v2).unwrap());
    }

    #[test]
    fn canonical_json_no_whitespace() {
        let val = json!({"key": [1, 2, 3]});
        let out = canonicalize(&val).unwrap();
        let s = std::str::from_utf8(&out).unwrap();
        assert!(!s.contains(' '));
        assert_eq!(s, r#"{"key":[1,2,3]}"#);
    }

    #[test]
    fn canonical_json_rejects_oversize_payload() {
        // Build a value that serializes to > 16 MiB
        let big = "x".repeat(PAYLOAD_MAX_BYTES + 1);
        let val = serde_json::Value::String(big);
        let result = canonicalize(&val);
        assert_eq!(result, Err(CanonicalError::Oversize));
    }

    #[test]
    fn canonical_json_rejects_too_deep() {
        // Build nested arrays deeper than PAYLOAD_MAX_DEPTH
        let mut val = json!(null);
        for _ in 0..=PAYLOAD_MAX_DEPTH + 1 {
            val = json!([val]);
        }
        let result = canonicalize(&val);
        assert_eq!(result, Err(CanonicalError::TooDeep));
    }

    #[test]
    fn canonical_json_handles_strings_with_escapes() {
        let val = json!("hello\nworld\t\"quoted\"");
        let out = canonicalize(&val).unwrap();
        assert_eq!(
            std::str::from_utf8(&out).unwrap(),
            r#""hello\nworld\t\"quoted\"""#
        );
    }

    #[test]
    fn canonical_json_handles_null_bool() {
        assert_eq!(
            std::str::from_utf8(&canonicalize(&json!(null)).unwrap()).unwrap(),
            "null"
        );
        assert_eq!(
            std::str::from_utf8(&canonicalize(&json!(true)).unwrap()).unwrap(),
            "true"
        );
        assert_eq!(
            std::str::from_utf8(&canonicalize(&json!(false)).unwrap()).unwrap(),
            "false"
        );
    }

    #[test]
    fn canonical_json_numbers() {
        // Integer
        assert_eq!(
            std::str::from_utf8(&canonicalize(&json!(42)).unwrap()).unwrap(),
            "42"
        );
        // Negative
        assert_eq!(
            std::str::from_utf8(&canonicalize(&json!(-1)).unwrap()).unwrap(),
            "-1"
        );
    }

    #[test]
    fn canonical_json_empty_containers() {
        assert_eq!(
            std::str::from_utf8(&canonicalize(&json!({})).unwrap()).unwrap(),
            "{}"
        );
        assert_eq!(
            std::str::from_utf8(&canonicalize(&json!([])).unwrap()).unwrap(),
            "[]"
        );
    }
}
