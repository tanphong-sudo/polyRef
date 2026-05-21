#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use polyref_core::evidence::EvidencePointer;
use polyref_core::report::{
    ReportAuditPointers, ReportCandidate, ReportConfigs, ReportParts, ReportRepoRef, ReportRepos,
};
use polyref_core::ValidationReport;
use polyref_graph::{AuditEvent, AuditEventTag, AuditReader, ReportStore, RunManifest};

fn sample_report(report_id: &str) -> ValidationReport {
    ValidationReport::assemble(ReportParts {
        report_id: report_id.to_owned(),
        repos: ReportRepos {
            old: ReportRepoRef {
                repo_id: "repo-old".to_owned(),
                commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            },
            new: ReportRepoRef {
                repo_id: "repo-new".to_owned(),
                commit: "89abcdef0123456789abcdef0123456789abcdef".to_owned(),
            },
        },
        candidate: ReportCandidate {
            candidate_id: "candidate-1".to_owned(),
            source: "manual".to_owned(),
            patch_hash: "a".repeat(64),
        },
        configs: ReportConfigs {
            extractor_versions: BTreeMap::new(),
            checker_versions: BTreeMap::new(),
        },
        observations: vec![],
        missing_endpoint_unknown: false,
        audit_pointers: ReportAuditPointers {
            audit_ndjson: "evidence/audit.ndjson".to_owned(),
            manifest_json: "evidence/manifest.json".to_owned(),
        },
    })
    .unwrap()
}

fn sample_event(report_id: &str) -> AuditEvent {
    AuditEvent::new(
        "2026-05-22T00:00:00Z".to_owned(),
        report_id.to_owned(),
        "report".to_owned(),
        AuditEventTag::ReportFinalized,
        "polyref-report-store".to_owned(),
        "b".repeat(64),
        vec![],
    )
    .unwrap()
}

#[test]
fn creates_run_layout_and_round_trips_report_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReportStore::open(dir.path()).unwrap();
    let run = store.create_run("report-1").unwrap();
    let report = sample_report("report-1");
    let manifest = RunManifest::new("report-1");

    run.write_report_json(&report).unwrap();
    run.write_report_markdown("# Report\n\naccepted\n").unwrap();
    run.write_manifest_json(&manifest).unwrap();
    run.write_evidence(
        &EvidencePointer::parse("evidence/logs/checker.log").unwrap(),
        b"checker output",
    )
    .unwrap();
    run.append_audit_event(&sample_event("report-1")).unwrap();

    assert!(run.path().join("report.json").is_file());
    assert!(run.path().join("report.md").is_file());
    assert!(run.path().join("audit.ndjson").is_file());
    assert!(run.path().join("manifest.json").is_file());
    assert!(run.path().join("evidence").is_dir());
    assert!(run.path().join("evidence/manifest.json").is_file());
    assert!(run.path().join("evidence/logs/checker.log").is_file());

    let read_report = run.read_report_json().unwrap();
    assert_eq!(read_report, report);
    assert_eq!(
        std::fs::read_to_string(run.path().join("report.md")).unwrap(),
        "# Report\n\naccepted\n"
    );
    assert_eq!(
        std::fs::read(run.path().join("manifest.json")).unwrap(),
        std::fs::read(run.path().join("evidence/manifest.json")).unwrap()
    );

    let events = AuditReader::open(run.path().join("audit.ndjson"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events, vec![sample_event("report-1")]);
}

#[test]
fn rejects_report_ids_that_escape_run_root() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReportStore::open(dir.path()).unwrap();

    for report_id in ["", "../x", "nested/x", "nested\\x", "/absolute"] {
        assert!(
            store.create_run(report_id).is_err(),
            "report_id must be rejected: {report_id:?}"
        );
    }
}

#[test]
fn evidence_pointer_cannot_escape_evidence_directory() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReportStore::open(dir.path()).unwrap();
    let run = store.create_run("report-1").unwrap();
    let pointer = EvidencePointer::parse("evidence/log.txt").unwrap();

    run.write_evidence(&pointer, b"log").unwrap();

    assert_eq!(
        std::fs::read(run.path().join("evidence/log.txt")).unwrap(),
        b"log"
    );
    assert!(!run.path().join("log.txt").exists());
}
