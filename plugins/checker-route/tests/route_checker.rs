#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_checker_route::{check_route, describe_route_checker};
use polyref_checker_spi::checker::{CheckRequest, EndpointArg};
use polyref_checker_spi::envelope::JsonRpcResponse;
use polyref_checker_spi::limits::SafePath;
use polyref_core::correspondence_kind::CorrespondenceKind;
use polyref_core::ids::EntityId;
use polyref_core::status::{BrokenReason, Outcome, UnknownReason};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const FIXTURE_ROOT: &str = "../../fixtures/layer4/users-route-migration";

#[derive(Debug, Deserialize)]
struct ExpectedFixture {
    entities: ExpectedEntities,
    migration_map_excerpt: Value,
    observation_excerpt: Value,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntities {
    old_handler: ExpectedEntity,
    old_route: ExpectedEntity,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntity {
    id: String,
}

#[test]
fn describe_contract_is_stable_and_schema_serializable() {
    let description = describe_route_checker();

    assert_eq!(description.contract_id, "polyref.route.checker.v1");
    assert_eq!(description.kind_id, CorrespondenceKind::Route);
    assert_eq!(description.endpoint_signature, vec!["route", "handler"]);
    assert_eq!(description.compat_rule_id, "route.compat.v1");
    assert_eq!(description.migrate_rule_id, "route.migrate.v1");
    assert_eq!(description.plugin_version, "0.1.0");
    assert!(description.default_timeout_ms > 0);
    assert!(description
        .supported_unknown_reasons
        .contains(&UnknownReason::MissingEndpoint));
    assert!(description
        .supported_unknown_reasons
        .contains(&UnknownReason::MigrationMapAmbiguous));
    assert!(description
        .supported_broken_reasons
        .contains(&BrokenReason::RoutePathRefuted));
    assert!(description
        .supported_broken_reasons
        .contains(&BrokenReason::HandlerBindingMismatch));

    let encoded = serde_json::to_value(description).unwrap();
    assert_eq!(encoded["kind_id"], json!("route"));
}

#[test]
fn canonical_fixture_returns_migrated() {
    let request = canonical_request();

    let evidence = check_route(&request).unwrap();

    assert_eq!(evidence.outcome(), &Outcome::Migrated);
    assert_eq!(evidence.predicate().as_str(), "route.migrate.v1");
    assert!(evidence.spans().is_empty());
    assert!(evidence.pointers().is_empty());
}

#[test]
fn unchanged_route_returns_preserved() {
    let fixture = expected_fixture();
    let mut request = canonical_request();
    let old = fixture.observation_excerpt["old"].clone();
    request.observation_excerpt = json!({
        "kind": "route",
        "old": old,
        "new": old,
    });
    request.migration_map_excerpt = json!({
        "conflicts": [],
        "entity_rewrites": [],
        "observation_part_rewrites": []
    });

    let evidence = check_route(&request).unwrap();

    assert_eq!(evidence.outcome(), &Outcome::Pres);
    assert_eq!(evidence.predicate().as_str(), "route.compat.v1");
}

#[test]
fn method_mismatch_returns_route_path_refuted() {
    let mut request = canonical_request();
    request.observation_excerpt["new"]["method"] = json!("GET");

    let evidence = check_route(&request).unwrap();

    assert_eq!(
        evidence.outcome(),
        &Outcome::Broken {
            reason: BrokenReason::RoutePathRefuted
        }
    );
}

#[test]
fn handler_mismatch_returns_handler_binding_mismatch() {
    let mut request = canonical_request();
    request.observation_excerpt["new"]["handler"] = json!("createOtherUser");

    let evidence = check_route(&request).unwrap();

    assert_eq!(
        evidence.outcome(),
        &Outcome::Broken {
            reason: BrokenReason::HandlerBindingMismatch
        }
    );
}

#[test]
fn missing_rewrite_returns_missing_endpoint_unknown() {
    let mut request = canonical_request();
    request.migration_map_excerpt["entity_rewrites"] = json!([]);

    let evidence = check_route(&request).unwrap();

    assert_eq!(
        evidence.outcome(),
        &Outcome::Unknown {
            reason: UnknownReason::MissingEndpoint
        }
    );
}

#[test]
fn ambiguous_migration_returns_ambiguous_unknown() {
    let mut request = canonical_request();
    let duplicate = request.migration_map_excerpt["entity_rewrites"][0].clone();
    request.migration_map_excerpt["entity_rewrites"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);

    let evidence = check_route(&request).unwrap();

    assert_eq!(
        evidence.outcome(),
        &Outcome::Unknown {
            reason: UnknownReason::MigrationMapAmbiguous
        }
    );
}

#[test]
fn dynamic_route_observation_returns_dynamic_string_unknown() {
    let mut request = canonical_request();
    request.observation_excerpt["unsupported_features"] = json!([{
        "feature": "dynamic_route"
    }]);

    let evidence = check_route(&request).unwrap();

    assert_eq!(
        evidence.outcome(),
        &Outcome::Unknown {
            reason: UnknownReason::DynamicString
        }
    );
}

#[test]
fn malformed_params_and_unknown_methods_are_json_rpc_errors() {
    let parse_response = run_plugin(json!({
        "jsonrpc": "2.0",
        "id": "bad-params",
        "method": "check",
        "params": { "not": "a check request" }
    }));
    assert_eq!(parse_response.error.unwrap().code, -32602);

    let method_response = run_plugin(json!({
        "jsonrpc": "2.0",
        "id": "bad-method",
        "method": "extract",
        "params": {}
    }));
    assert_eq!(method_response.error.unwrap().code, -32601);
}

fn canonical_request() -> CheckRequest {
    let fixture = expected_fixture();
    CheckRequest {
        contract_id: "polyref.route.checker.v1".to_owned(),
        kind: CorrespondenceKind::Route,
        endpoints: vec![
            EndpointArg {
                entity_id: EntityId::parse(&fixture.entities.old_route.id).unwrap(),
                r#type: "route".to_owned(),
            },
            EndpointArg {
                entity_id: EntityId::parse(&fixture.entities.old_handler.id).unwrap(),
                r#type: "handler".to_owned(),
            },
        ],
        old_repo_root: SafePath::parse("old").unwrap(),
        new_repo_root: SafePath::parse("new").unwrap(),
        migration_map_excerpt: fixture.migration_map_excerpt,
        observation_excerpt: fixture.observation_excerpt,
        deadline_ms: 5_000,
        log_dir: SafePath::parse("logs/checker-route").unwrap(),
    }
}

fn expected_fixture() -> ExpectedFixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join("expected.json");
    let contents = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&contents).unwrap()
}

fn run_plugin(request: Value) -> JsonRpcResponse {
    let binary = std::env::var("CARGO_BIN_EXE_polyref-checker-route")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let mut path = std::env::current_exe().unwrap();
            path.pop();
            if path.file_name().is_some_and(|name| name == "deps") {
                path.pop();
            }
            path.push("polyref-checker-route");
            path
        });
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        serde_json::to_writer(&mut *stdin, &request).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    serde_json::from_slice::<JsonRpcResponse>(&output.stdout).unwrap()
}
