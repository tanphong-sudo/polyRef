#![allow(clippy::unwrap_used)]

use polyref_graph::ReportStore;
use polyref_loader::manifest::{ManifestRepoRef, RunManifest, SandboxBackend, SandboxProfile};

#[test]
fn minimal_manifest_uses_schema_version_0_1_0() {
    let manifest = RunManifest::new("report-1");

    assert_eq!(manifest.report_id, "report-1");
    assert_eq!(manifest.schema_version, "0.1.0");
}

#[test]
fn expanded_manifest_round_trips() {
    let mut manifest = RunManifest::new("report-1");
    manifest.old_repo = Some(ManifestRepoRef {
        repo_id: "repo-old".to_owned(),
        commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        workspace_path: "workspace/old".to_owned(),
    });
    manifest.new_repo = Some(ManifestRepoRef {
        repo_id: "repo-new".to_owned(),
        commit: "89abcdef0123456789abcdef0123456789abcdef".to_owned(),
        workspace_path: "workspace/new".to_owned(),
    });
    manifest.patch_hash = Some("a".repeat(64));
    manifest.patch_path = Some("candidate.patch".to_owned());
    manifest.sandbox = Some(SandboxProfile {
        backend: SandboxBackend::Docker,
        profile_id: "no-network-default".to_owned(),
        network_allowed: false,
        env_keys: vec!["PATH".to_owned()],
        cpu_seconds: Some(60),
        memory_bytes: Some(1_073_741_824),
        tmpfs_bytes: Some(268_435_456),
    });
    manifest.created_at = Some("2026-05-22T00:00:00Z".to_owned());
    manifest
        .tool_versions
        .insert("polyref-loader".to_owned(), "0.1.0".to_owned());

    let json = serde_json::to_string(&manifest).unwrap();
    let back: RunManifest = serde_json::from_str(&json).unwrap();

    assert_eq!(back, manifest);
}

#[test]
fn unknown_manifest_fields_remain_accepted() {
    let json = r#"{
      "report_id":"report-1",
      "schema_version":"0.1.0",
      "future_field":{"kept":true}
    }"#;

    let manifest: RunManifest = serde_json::from_str(json).unwrap();

    assert_eq!(manifest.report_id, "report-1");
}

#[test]
fn writer_preserves_root_and_evidence_manifest_copies() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReportStore::open(dir.path()).unwrap();
    let run = store.create_run("report-1").unwrap();
    let manifest = RunManifest::new("report-1");

    manifest.write_to_report_store(&run).unwrap();

    let root = std::fs::read(run.path().join("manifest.json")).unwrap();
    let mirror = std::fs::read(run.path().join("evidence/manifest.json")).unwrap();
    assert_eq!(root, mirror);

    let back: RunManifest = serde_json::from_slice(&root).unwrap();
    assert_eq!(back, manifest);
}
