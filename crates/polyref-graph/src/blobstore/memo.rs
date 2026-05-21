//! Memoization-key helpers per ADR-006 §"Memoization keys".
//!
//! Two flavours, both produce a [`BlobKey`] (SHA-256 of canonical JSON):
//!
//! - [`extractor_memo_key`] — `H(content_hash, extractor_id,
//!   extractor_version, options_canonical)`. Used by the extractor
//!   pool to skip re-extraction when the input bytes and extractor
//!   identity are unchanged.
//! - [`checker_memo_key`] — `H(plugin_version, contract_id,
//!   sorted endpoint_entity_ids, evidence_inputs_hash, deadline_ms)`.
//!   Used by the kind-checker pool. Endpoint ids are sorted before
//!   hashing so two callers passing the same set in different orders
//!   share a cache slot.
//!
//! # Determinism
//!
//! The internal serialization uses `polyref_core::canonical::canonicalize`
//! so the resulting digest is byte-stable across runs and machines.
//! This is a hard requirement of the replay invariant (NFR5).

use polyref_core::canonical;
use polyref_core::ids::EntityId;
use serde::Serialize;

use super::key::BlobKey;
use super::BlobStoreError;

/// Inputs to [`extractor_memo_key`].
#[derive(Debug, Clone, Serialize)]
struct ExtractorMemoInputs<'a> {
    content_hash: &'a str,
    extractor_id: &'a str,
    extractor_version: &'a str,
    options: &'a serde_json::Value,
}

/// Compute the extractor memoization key per ADR-006.
///
/// # Errors
///
/// Returns [`BlobStoreError::Io`] (wrapping a canonical-JSON error)
/// if the inputs cannot be serialized — should be unreachable for the
/// in-tree input shapes.
pub fn extractor_memo_key(
    content_hash: &BlobKey,
    extractor_id: &str,
    extractor_version: &str,
    options: &serde_json::Value,
) -> Result<BlobKey, BlobStoreError> {
    let hex = content_hash.to_hex();
    let inputs = ExtractorMemoInputs {
        content_hash: &hex,
        extractor_id,
        extractor_version,
        options,
    };
    let value =
        serde_json::to_value(inputs).map_err(|e| BlobStoreError::Io(canonical_io_err(e)))?;
    let canonical_bytes = canonical::canonicalize(&value)
        .map_err(|e| BlobStoreError::Io(canonical_io_err_msg(e.to_string())))?;
    Ok(BlobKey::from_bytes(&canonical_bytes))
}

/// Inputs to [`checker_memo_key`].
#[derive(Debug, Clone, Serialize)]
struct CheckerMemoInputs<'a> {
    plugin_version: &'a str,
    contract_id: &'a str,
    /// `endpoint_ids` sorted by their canonical string form so two
    /// callers passing the same set in different orders share a slot.
    endpoint_ids: Vec<&'a str>,
    evidence_inputs_hash: &'a str,
    deadline_ms: u32,
}

/// Compute the kind-checker memoization key per ADR-006.
///
/// `endpoint_ids` is sorted internally; the spec requires "sorted
/// endpoint_entity_ids" so callers that build the slice in different
/// orders converge on the same key.
///
/// # Errors
///
/// Same surface as [`extractor_memo_key`].
pub fn checker_memo_key(
    plugin_version: &str,
    contract_id: &str,
    endpoint_ids: &[EntityId],
    evidence_inputs_hash: &BlobKey,
    deadline_ms: u32,
) -> Result<BlobKey, BlobStoreError> {
    let mut sorted: Vec<&str> = endpoint_ids.iter().map(EntityId::as_str).collect();
    sorted.sort_unstable();

    let evidence_hex = evidence_inputs_hash.to_hex();
    let inputs = CheckerMemoInputs {
        plugin_version,
        contract_id,
        endpoint_ids: sorted,
        evidence_inputs_hash: &evidence_hex,
        deadline_ms,
    };
    let value =
        serde_json::to_value(inputs).map_err(|e| BlobStoreError::Io(canonical_io_err(e)))?;
    let canonical_bytes = canonical::canonicalize(&value)
        .map_err(|e| BlobStoreError::Io(canonical_io_err_msg(e.to_string())))?;
    Ok(BlobKey::from_bytes(&canonical_bytes))
}

fn canonical_io_err(err: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, err)
}

fn canonical_io_err_msg(msg: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use serde_json::json;

    fn fixture_content_hash() -> BlobKey {
        BlobKey::from_bytes(b"fixture content")
    }

    fn fixture_evidence_hash() -> BlobKey {
        BlobKey::from_bytes(b"fixture evidence")
    }

    fn fixture_endpoint(side: &str, hash: &str) -> EntityId {
        let s = format!("{side}:ts:handler:src/users.ts:{hash}");
        EntityId::parse(&s).unwrap()
    }

    // ── extractor_memo_key ──────────────────────────────────────

    #[test]
    fn extractor_key_is_deterministic() {
        let h = fixture_content_hash();
        let k1 = extractor_memo_key(&h, "extractor-typescript", "1.0.0", &json!({})).unwrap();
        let k2 = extractor_memo_key(&h, "extractor-typescript", "1.0.0", &json!({})).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn extractor_key_changes_on_content_hash() {
        let k1 = extractor_memo_key(
            &BlobKey::from_bytes(b"a"),
            "extractor-typescript",
            "1.0.0",
            &json!({}),
        )
        .unwrap();
        let k2 = extractor_memo_key(
            &BlobKey::from_bytes(b"b"),
            "extractor-typescript",
            "1.0.0",
            &json!({}),
        )
        .unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn extractor_key_changes_on_extractor_version() {
        let h = fixture_content_hash();
        let k1 = extractor_memo_key(&h, "extractor-typescript", "1.0.0", &json!({})).unwrap();
        let k2 = extractor_memo_key(&h, "extractor-typescript", "1.0.1", &json!({})).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn extractor_key_changes_on_options() {
        let h = fixture_content_hash();
        let k1 = extractor_memo_key(
            &h,
            "extractor-typescript",
            "1.0.0",
            &json!({"strict": false}),
        )
        .unwrap();
        let k2 = extractor_memo_key(
            &h,
            "extractor-typescript",
            "1.0.0",
            &json!({"strict": true}),
        )
        .unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn extractor_key_options_are_canonical() {
        // Same logical options in different JSON key order must yield
        // the same key. canonicalize() sorts keys.
        let h = fixture_content_hash();
        let k1 = extractor_memo_key(
            &h,
            "extractor-typescript",
            "1.0.0",
            &json!({"strict": true, "jsx": false}),
        )
        .unwrap();
        let k2 = extractor_memo_key(
            &h,
            "extractor-typescript",
            "1.0.0",
            &json!({"jsx": false, "strict": true}),
        )
        .unwrap();
        assert_eq!(k1, k2);
    }

    // ── checker_memo_key ────────────────────────────────────────

    #[test]
    fn checker_key_is_deterministic() {
        let e = fixture_evidence_hash();
        let endpoints = vec![fixture_endpoint("old", "0123456789ab")];
        let k1 = checker_memo_key("1.0.0", "route", &endpoints, &e, 600_000).unwrap();
        let k2 = checker_memo_key("1.0.0", "route", &endpoints, &e, 600_000).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn checker_key_sorts_endpoints() {
        // Same set of endpoints in different orders → same key.
        let e = fixture_evidence_hash();
        let a = fixture_endpoint("old", "0123456789ab");
        let b = fixture_endpoint("new", "abcdef012345");
        let k1 = checker_memo_key("1.0.0", "route", &[a.clone(), b.clone()], &e, 600_000).unwrap();
        let k2 = checker_memo_key("1.0.0", "route", &[b, a], &e, 600_000).unwrap();
        assert_eq!(k1, k2, "endpoint order must not affect the memo key");
    }

    #[test]
    fn checker_key_changes_on_plugin_version() {
        let e = fixture_evidence_hash();
        let endpoints = vec![fixture_endpoint("old", "0123456789ab")];
        let k1 = checker_memo_key("1.0.0", "route", &endpoints, &e, 600_000).unwrap();
        let k2 = checker_memo_key("1.0.1", "route", &endpoints, &e, 600_000).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn checker_key_changes_on_contract_id() {
        let e = fixture_evidence_hash();
        let endpoints = vec![fixture_endpoint("old", "0123456789ab")];
        let k1 = checker_memo_key("1.0.0", "route", &endpoints, &e, 600_000).unwrap();
        let k2 = checker_memo_key("1.0.0", "schema", &endpoints, &e, 600_000).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn checker_key_changes_on_deadline() {
        let e = fixture_evidence_hash();
        let endpoints = vec![fixture_endpoint("old", "0123456789ab")];
        let k1 = checker_memo_key("1.0.0", "route", &endpoints, &e, 600_000).unwrap();
        let k2 = checker_memo_key("1.0.0", "route", &endpoints, &e, 300_000).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn checker_key_changes_on_evidence_hash() {
        let endpoints = vec![fixture_endpoint("old", "0123456789ab")];
        let k1 = checker_memo_key(
            "1.0.0",
            "route",
            &endpoints,
            &BlobKey::from_bytes(b"evidence-1"),
            600_000,
        )
        .unwrap();
        let k2 = checker_memo_key(
            "1.0.0",
            "route",
            &endpoints,
            &BlobKey::from_bytes(b"evidence-2"),
            600_000,
        )
        .unwrap();
        assert_ne!(k1, k2);
    }
}
