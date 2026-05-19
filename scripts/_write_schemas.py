"""Write the Slice 1 JSON Schema package.

Run: python3 scripts/_write_schemas.py

This is a one-shot generator used by the skeleton author because the
authoring environment refused direct creation of JSON Schema files. It is
NOT part of the build; do not rely on it after Slice 1.
"""
from __future__ import annotations

import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCHEMAS = ROOT / "schemas"


def write(rel: str, doc: dict) -> None:
    path = SCHEMAS / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    print(f"  wrote {path.relative_to(ROOT)}")


def main() -> int:
    # ----------------------------------------------------------- _meta
    write("_meta/version.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/_meta/version.json",
        "title": "Schema package version",
        "description": "Frozen semver tag for the polyref JSON Schema package. Bump on any added/removed enum variant or required field per ADR-006.",
        "type": "object",
        "required": ["schema_version"],
        "additionalProperties": False,
        "properties": {
            "schema_version": {
                "type": "string",
                "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$",
                "const": "0.1.0",
            }
        },
    })

    # ----------------------------------------------------------- ids
    entity_id_pattern = (
        "^(old|new):"
        "(build|dockerfile|java|json|jsonschema|openapi|py|sql|ts|yaml):"
        "[a-z_]+:"
        "[A-Za-z0-9._/#:\\-]+:"
        "[0-9a-f]{12}$"
    )
    write("ids/entity-id.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/ids/entity-id.json",
        "title": "EntityId",
        "description": (
            "EntityId grammar per ADR-003. Layout: "
            "<repo_side>:<language>:<kind>:<local_path>:<stable_hash>. "
            "stable_hash is first 12 hex chars of SHA-256 over the canonicalized "
            "local-facts payload. Type-respecting check on MigrationMap "
            "compares the kind segment ONLY (not language); cross-language "
            "migrations are first-class per paper Definition 5."
        ),
        "type": "string",
        "minLength": 1,
        "maxLength": 16384,
        "pattern": entity_id_pattern,
    })
    write("ids/artifact-id.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/ids/artifact-id.json",
        "title": "ArtifactId",
        "type": "string",
        "minLength": 1,
        "maxLength": 8192,
        "pattern": "^artifact:(old|new):[A-Za-z0-9._/\\-]{1,4096}:[0-9a-f]{12}$",
    })
    write("ids/correspondence-id.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/ids/correspondence-id.json",
        "title": "CorrId",
        "type": "string",
        "minLength": 1,
        "maxLength": 4096,
        "pattern": "^corr:[a-z_]+:[0-9a-f]{16,64}$",
    })
    write("ids/edge-id.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/ids/edge-id.json",
        "title": "EdgeId",
        "type": "string",
        "minLength": 1,
        "maxLength": 4096,
        "pattern": "^edge:[a-z_]+:[0-9a-f]{16,64}$",
    })

    # ----------------------------------------------------------- source span
    write("source-span.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/source-span.json",
        "title": "SourceSpan",
        "description": (
            "Half-open span. start.line >= 1 (NonZeroU32 in Rust). col is "
            "0-indexed UTF-8 byte column. utf16_cols is optional for editor "
            "interop. start <= end (lexicographic on (line, col))."
        ),
        "type": "object",
        "required": ["artifact", "start", "end"],
        "additionalProperties": False,
        "properties": {
            "artifact": {"$ref": "polyref://schemas/ids/artifact-id.json"},
            "start": {"$ref": "#/$defs/lineCol"},
            "end": {"$ref": "#/$defs/lineCol"},
            "utf16_cols": {
                "type": "array",
                "items": {"type": "integer", "minimum": 0},
                "minItems": 2,
                "maxItems": 2,
            },
        },
        "$defs": {
            "lineCol": {
                "type": "object",
                "required": ["line", "col"],
                "additionalProperties": False,
                "properties": {
                    "line": {"type": "integer", "minimum": 1},
                    "col": {"type": "integer", "minimum": 0},
                },
            }
        },
    })

    # ----------------------------------------------------------- artifact / language / corr kinds
    write("artifact-kind.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/artifact-kind.json",
        "title": "ArtifactKind",
        "description": "Closed set of nine artifact families per architecture §1.4.",
        "type": "string",
        "enum": [
            "build_file",
            "config",
            "dockerfile",
            "generated",
            "query",
            "schema",
            "source_file",
            "test",
            "workflow",
        ],
    })
    write("language.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/language.json",
        "title": "Language",
        "description": (
            "Language tag carried by EntityId. The literal 'build' covers "
            "package manifests and build scripts (package.json, pyproject.toml, "
            "pom.xml, build.gradle, Bazel BUILD, Makefile, CMakeLists.txt, "
            "lockfiles). Per ADR-003."
        ),
        "type": "string",
        "enum": [
            "build",
            "dockerfile",
            "java",
            "json",
            "jsonschema",
            "openapi",
            "py",
            "sql",
            "ts",
            "yaml",
        ],
    })
    write("correspondence-kind.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/correspondence-kind.json",
        "title": "CorrespondenceKind",
        "description": (
            "Closed set of correspondence kinds per paper Table 3. The paper "
            "lists 'call' separately from 'route' in §3.2 ('the initial kind "
            "set is deliberately modest: call, route, schema, serialization, "
            "configuration, build, query/table, event, test-oracle, and "
            "workflow') so we keep both. 11 entries; revisit if Slice 2 "
            "review prefers to merge call into route."
        ),
        "type": "string",
        "enum": [
            "build_codegen",
            "call",
            "configuration",
            "event",
            "generated_client",
            "query_table",
            "route",
            "schema",
            "serialization",
            "test_oracle",
            "workflow",
        ],
    })

    # ----------------------------------------------------------- status / reasons
    write("validation-status.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/validation-status.json",
        "title": "ValidationStatus",
        "description": "Closed set of frontier-item statuses per paper Definition 8.",
        "type": "string",
        "enum": ["broken", "migrated", "pres", "unknown"],
    })
    write("unknown-reason.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/unknown-reason.json",
        "title": "UnknownReason",
        "description": "Closed set per ADR-005. Lexicographic ascending order is the public contract (hard blocker F-2).",
        "type": "string",
        "enum": [
            "ambiguous_endpoint",
            "checker_timeout",
            "cyclic_generator",
            "dynamic_evidence_unverified",
            "dynamic_string",
            "generated_evidence_missing",
            "generated_evidence_weak",
            "migration_map_ambiguous",
            "missing_endpoint",
            "no_accepting_rule_applied",
            "observation_rewrite_undefined",
            "opaque_build_cache",
            "plugin_failure",
            "reflection",
            "unsupported_extractor",
            "unsupported_framework",
        ],
    })
    write("broken-reason.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/broken-reason.json",
        "title": "BrokenReason",
        "description": "Closed set per ADR-005. Lexicographic ascending order is the public contract (hard blocker F-2).",
        "type": "string",
        "enum": [
            "build_target_unreachable",
            "event_payload_incompatible",
            "generated_client_stale",
            "generator_mismatch",
            "handler_binding_mismatch",
            "local_checker_failure",
            "migration_map_conflict",
            "query_table_missing",
            "required_field_drift",
            "route_path_refuted",
            "schema_incompatible",
            "workflow_packages_old_target",
        ],
    })

    # ----------------------------------------------------------- evidence
    write("evidence-pointer.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/evidence-pointer.json",
        "title": "EvidencePointer",
        "description": (
            "Relative POSIX path under the report's evidence/ directory. "
            "Recommended by hard blocker F-7; final regex frozen by Slice 1 "
            "ADR addendum."
        ),
        "type": "string",
        "minLength": 9,
        "maxLength": 4096,
        "pattern": "^evidence/[A-Za-z0-9_./\\-]{1,512}$",
    })
    write("evidence.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/evidence.json",
        "title": "Evidence",
        "description": (
            "Outcome carries reason as a tag-payload. Pres and Migrated MUST "
            "NOT carry an unknown_reason or broken_reason. The fail-closed "
            "invariant is enforced at the type level by polyref-core."
        ),
        "type": "object",
        "required": ["outcome", "predicate", "checker_version", "rule_version"],
        "additionalProperties": False,
        "properties": {
            "outcome": {
                "oneOf": [
                    {"type": "object", "required": ["tag"], "properties": {
                        "tag": {"const": "pres"}
                    }, "additionalProperties": False},
                    {"type": "object", "required": ["tag"], "properties": {
                        "tag": {"const": "migrated"}
                    }, "additionalProperties": False},
                    {"type": "object", "required": ["tag", "reason"], "properties": {
                        "tag": {"const": "broken"},
                        "reason": {"$ref": "polyref://schemas/broken-reason.json"}
                    }, "additionalProperties": False},
                    {"type": "object", "required": ["tag", "reason"], "properties": {
                        "tag": {"const": "unknown"},
                        "reason": {"$ref": "polyref://schemas/unknown-reason.json"}
                    }, "additionalProperties": False}
                ]
            },
            "predicate": {"type": "string", "minLength": 1, "maxLength": 256},
            "spans": {
                "type": "array",
                "items": {"$ref": "polyref://schemas/source-span.json"},
                "default": []
            },
            "pointers": {
                "type": "array",
                "items": {"$ref": "polyref://schemas/evidence-pointer.json"},
                "default": []
            },
            "checker_version": {"type": "string", "minLength": 1, "maxLength": 64},
            "rule_version": {"type": "string", "minLength": 1, "maxLength": 64}
        }
    })

    # ----------------------------------------------------------- migration map
    write("migration-map.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/migration-map.json",
        "title": "MigrationMap",
        "description": (
            "Per paper Definition 5, type-respecting iff type(n) = type(μ(n)) "
            "where type is the local kind. Cross-language migrations are "
            "allowed when kinds match."
        ),
        "type": "object",
        "required": ["entity_rewrites", "observation_part_rewrites", "conflicts", "type_respecting"],
        "additionalProperties": False,
        "properties": {
            "entity_rewrites": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["old", "new"],
                    "additionalProperties": False,
                    "properties": {
                        "old": {"$ref": "polyref://schemas/ids/entity-id.json"},
                        "new": {"$ref": "polyref://schemas/ids/entity-id.json"}
                    }
                }
            },
            "observation_part_rewrites": {
                "type": "array",
                "items": {"type": "object"}
            },
            "conflicts": {
                "type": "array",
                "items": {"type": "object"}
            },
            "type_respecting": {"type": "boolean"}
        }
    })

    # ----------------------------------------------------------- observations
    write("observation/visibility.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/observation/visibility.json",
        "title": "Visibility",
        "description": "Per ADR-010.",
        "type": "string",
        "enum": ["evaluation_only", "held_out", "visible"]
    })
    write("observation/_kind.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/observation/_kind.json",
        "title": "ObservationKind discriminator",
        "type": "string",
        "enum": [
            "api_call",
            "build_target",
            "schema_validation",
            "test_invocation",
            "workflow_run"
        ]
    })

    obs_common = {
        "visibility": {"$ref": "polyref://schemas/observation/visibility.json"},
        "support": {
            "type": "array",
            "items": {
                "oneOf": [
                    {"$ref": "polyref://schemas/ids/correspondence-id.json"},
                    {"$ref": "polyref://schemas/ids/edge-id.json"}
                ]
            }
        },
        "defined_semantics": {"type": "boolean"}
    }
    common_required = ["visibility", "support", "defined_semantics"]

    write("observation/api-call.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/observation/api-call.json",
        "title": "ApiCallObservation",
        "type": "object",
        "required": ["method", "path"] + common_required,
        "additionalProperties": False,
        "properties": dict({
            "method": {"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]},
            "path": {"type": "string", "minLength": 1, "maxLength": 4096},
            "request_schema_id": {"$ref": "polyref://schemas/ids/entity-id.json"},
            "response_schema_id": {"$ref": "polyref://schemas/ids/entity-id.json"},
            "client_id": {"$ref": "polyref://schemas/ids/entity-id.json"}
        }, **obs_common)
    })
    write("observation/test-invocation.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/observation/test-invocation.json",
        "title": "TestInvocationObservation",
        "type": "object",
        "required": ["test_id"] + common_required,
        "additionalProperties": False,
        "properties": dict({
            "test_id": {"$ref": "polyref://schemas/ids/entity-id.json"},
            "public_entrypoint": {"$ref": "polyref://schemas/ids/entity-id.json"}
        }, **obs_common)
    })
    write("observation/build-target.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/observation/build-target.json",
        "title": "BuildTargetObservation",
        "type": "object",
        "required": ["target_name"] + common_required,
        "additionalProperties": False,
        "properties": dict({
            "target_name": {"type": "string", "minLength": 1, "maxLength": 4096},
            "generator_command": {"type": "string", "maxLength": 4096},
            "expected_artifact_path": {"type": "string", "maxLength": 4096}
        }, **obs_common)
    })
    write("observation/workflow-run.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/observation/workflow-run.json",
        "title": "WorkflowRunObservation",
        "type": "object",
        "required": ["workflow_id"] + common_required,
        "additionalProperties": False,
        "properties": dict({
            "workflow_id": {"$ref": "polyref://schemas/ids/entity-id.json"},
            "packaged_target_name": {"type": "string", "maxLength": 4096},
            "env_keys": {"type": "array", "items": {"type": "string", "maxLength": 256}}
        }, **obs_common)
    })
    write("observation/schema-validation.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/observation/schema-validation.json",
        "title": "SchemaValidationObservation",
        "type": "object",
        "required": ["schema_id"] + common_required,
        "additionalProperties": False,
        "properties": dict({
            "schema_id": {"$ref": "polyref://schemas/ids/entity-id.json"},
            "sample_payload_ref": {"$ref": "polyref://schemas/evidence-pointer.json"},
            "expected_outcome": {"type": "string", "enum": ["valid", "invalid"]}
        }, **obs_common)
    })

    # ----------------------------------------------------------- SPI
    write("checker-spi/describe.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/checker-spi/describe.json",
        "title": "KindChecker.describe response",
        "type": "object",
        "required": [
            "contract_id", "kind_id", "endpoint_signature", "required_evidence_fields",
            "compat_rule_id", "migrate_rule_id", "plugin_version", "default_timeout_ms",
            "supported_unknown_reasons", "supported_broken_reasons"
        ],
        "additionalProperties": False,
        "properties": {
            "contract_id": {"type": "string", "maxLength": 256},
            "kind_id": {"$ref": "polyref://schemas/correspondence-kind.json"},
            "endpoint_signature": {"type": "array", "items": {"type": "string"}},
            "required_evidence_fields": {"type": "array", "items": {"type": "string"}},
            "compat_rule_id": {"type": "string", "maxLength": 256},
            "migrate_rule_id": {"type": "string", "maxLength": 256},
            "plugin_version": {"type": "string", "maxLength": 64},
            "default_timeout_ms": {"type": "integer", "minimum": 1, "maximum": 600000},
            "supported_unknown_reasons": {
                "type": "array",
                "items": {"$ref": "polyref://schemas/unknown-reason.json"}
            },
            "supported_broken_reasons": {
                "type": "array",
                "items": {"$ref": "polyref://schemas/broken-reason.json"}
            }
        }
    })
    write("checker-spi/check.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/checker-spi/check.json",
        "title": "KindChecker.check request and response",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "request": {
                "type": "object",
                "required": [
                    "contract_id", "kind", "endpoints",
                    "old_repo_root", "new_repo_root",
                    "deadline_ms", "log_dir"
                ],
                "additionalProperties": False,
                "properties": {
                    "contract_id": {"type": "string", "maxLength": 256},
                    "kind": {"$ref": "polyref://schemas/correspondence-kind.json"},
                    "endpoints": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["entity_id", "type"],
                            "additionalProperties": False,
                            "properties": {
                                "entity_id": {"$ref": "polyref://schemas/ids/entity-id.json"},
                                "type": {"type": "string", "maxLength": 256}
                            }
                        }
                    },
                    "old_repo_root": {"type": "string", "maxLength": 4096},
                    "new_repo_root": {"type": "string", "maxLength": 4096},
                    "migration_map_excerpt": {"type": "object"},
                    "observation_excerpt": {"type": "object"},
                    "deadline_ms": {"type": "integer", "minimum": 1, "maximum": 600000},
                    "log_dir": {"type": "string", "maxLength": 4096}
                }
            },
            "response": {"$ref": "polyref://schemas/evidence.json"}
        }
    })

    write("extractor-spi/extract.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/extractor-spi/extract.json",
        "title": "Extractor.extract request and response",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "request": {
                "type": "object",
                "required": ["artifact_path", "content_hash", "language", "deadline_ms", "log_dir"],
                "additionalProperties": False,
                "properties": {
                    "artifact_path": {"type": "string", "maxLength": 4096},
                    "content_hash": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                    "language": {"$ref": "polyref://schemas/language.json"},
                    "options": {"type": "object"},
                    "deadline_ms": {"type": "integer", "minimum": 1, "maximum": 600000},
                    "log_dir": {"type": "string", "maxLength": 4096}
                }
            },
            "response": {
                "type": "object",
                "required": ["entities", "local_facts", "unsupported_features", "extractor_version"],
                "additionalProperties": False,
                "properties": {
                    "entities": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["entity_id", "kind", "local_name", "source_span"],
                            "additionalProperties": False,
                            "properties": {
                                "entity_id": {"$ref": "polyref://schemas/ids/entity-id.json"},
                                "kind": {"type": "string", "maxLength": 64},
                                "local_name": {"type": "string", "maxLength": 1024},
                                "type": {"type": "string", "maxLength": 256},
                                "source_span": {"$ref": "polyref://schemas/source-span.json"}
                            }
                        }
                    },
                    "local_facts": {"type": "array", "items": {"type": "object"}},
                    "unsupported_features": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["feature", "span"],
                            "additionalProperties": False,
                            "properties": {
                                "feature": {"type": "string", "maxLength": 256},
                                "span": {"$ref": "polyref://schemas/source-span.json"},
                                "note": {"type": "string", "maxLength": 1024}
                            }
                        }
                    },
                    "extractor_version": {"type": "string", "maxLength": 64}
                }
            }
        }
    })

    # ----------------------------------------------------------- report
    write("report.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/report.json",
        "title": "ValidationReport",
        "description": (
            "Frozen report contract per ADR-006. The fail-closed invariant "
            "(Accepted ⇒ missing_endpoint_unknown == false) is enforced at "
            "type-level by polyref-core::report::ValidationReport::assemble."
        ),
        "type": "object",
        "required": [
            "schema_version", "report_id", "candidate", "repos",
            "configs", "observations", "candidate_decision",
            "missing_endpoint_unknown", "audit_pointers"
        ],
        "additionalProperties": False,
        "properties": {
            "schema_version": {"type": "string", "const": "0.1.0"},
            "report_id": {"type": "string", "minLength": 1, "maxLength": 256},
            "candidate": {
                "type": "object",
                "required": ["candidate_id", "source", "patch_hash"],
                "additionalProperties": False,
                "properties": {
                    "candidate_id": {"type": "string", "maxLength": 256},
                    "source": {"type": "string", "enum": ["llm", "ide", "template", "manual"]},
                    "patch_hash": {"type": "string", "pattern": "^[0-9a-f]{64}$"}
                }
            },
            "repos": {
                "type": "object",
                "required": ["old", "new"],
                "additionalProperties": False,
                "properties": {
                    "old": {"$ref": "#/$defs/repoRef"},
                    "new": {"$ref": "#/$defs/repoRef"}
                }
            },
            "configs": {
                "type": "object",
                "required": ["extractor_versions", "checker_versions"],
                "additionalProperties": False,
                "properties": {
                    "extractor_versions": {"type": "object", "additionalProperties": {"type": "string"}},
                    "checker_versions": {"type": "object", "additionalProperties": {"type": "string"}}
                }
            },
            "observations": {
                "type": "array",
                "items": {"$ref": "#/$defs/observationRow"}
            },
            "candidate_decision": {"type": "string", "enum": ["accepted", "broken", "unknown"]},
            "missing_endpoint_unknown": {"type": "boolean"},
            "audit_pointers": {
                "type": "object",
                "required": ["audit_ndjson", "manifest_json"],
                "additionalProperties": False,
                "properties": {
                    "audit_ndjson": {"$ref": "polyref://schemas/evidence-pointer.json"},
                    "manifest_json": {"$ref": "polyref://schemas/evidence-pointer.json"}
                }
            }
        },
        "$defs": {
            "repoRef": {
                "type": "object",
                "required": ["repo_id", "commit"],
                "additionalProperties": False,
                "properties": {
                    "repo_id": {"type": "string", "maxLength": 256},
                    "commit": {"type": "string", "pattern": "^[0-9a-f]{40,64}$"}
                }
            },
            "observationRow": {
                "type": "object",
                "required": [
                    "observation_id", "obs_kind", "visibility",
                    "frontier_size", "items", "status"
                ],
                "additionalProperties": False,
                "properties": {
                    "observation_id": {"type": "string", "maxLength": 256},
                    "obs_kind": {"$ref": "polyref://schemas/observation/_kind.json"},
                    "visibility": {"$ref": "polyref://schemas/observation/visibility.json"},
                    "frontier_size": {"type": "integer", "minimum": 0},
                    "items": {
                        "type": "array",
                        "items": {"$ref": "polyref://schemas/evidence.json"}
                    },
                    "observation_rewrite": {
                        "type": "object",
                        "additionalProperties": True
                    },
                    "status": {"type": "string", "enum": ["accepted", "broken", "unknown"]}
                }
            }
        }
    })

    # ----------------------------------------------------------- audit-event + manifest placeholders
    write("audit-event.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/audit-event.json",
        "title": "AuditEvent",
        "description": "Placeholder for Slice 1; expand in Slice 2 with the closed event-tag list per ADR-006.",
        "type": "object",
        "required": ["ts", "report_id", "stage", "tag"],
        "additionalProperties": True,
        "properties": {
            "ts": {"type": "string", "format": "date-time"},
            "report_id": {"type": "string", "maxLength": 256},
            "stage": {"type": "string", "maxLength": 64},
            "tag": {"type": "string", "maxLength": 64}
        }
    })
    write("manifest.json", {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "polyref://schemas/manifest.json",
        "title": "RunManifest",
        "description": "Placeholder for Slice 1; expanded in Slice 2 by polyref-loader.",
        "type": "object",
        "required": ["report_id", "schema_version"],
        "additionalProperties": True,
        "properties": {
            "report_id": {"type": "string", "maxLength": 256},
            "schema_version": {"type": "string", "const": "0.1.0"}
        }
    })

    print("schemas written.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
