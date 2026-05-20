//! Property-based tests for the NDJSON audit log.
//!
//! Per `docs/verification.md` Layer 1, the strongest lock against
//! silent regressions is a property test. The properties below pin:
//!
//! - **Round-trip identity**: any sequence of valid events written
//!   through `AuditWriter` must read back through `AuditReader`
//!   element-by-element identical. Locks against a future change that
//!   "improves" line termination, JSON indentation, or evidence-pointer
//!   ordering.
//! - **Replay-friendly stream**: re-opening the file mid-run preserves
//!   prior content; this is the contract that lets a CI replay verifier
//!   tail an in-flight log.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use polyref_graph::{AuditEvent, AuditEventTag, AuditReader, AuditWriter};
use proptest::prelude::*;

fn arb_tag() -> impl Strategy<Value = AuditEventTag> {
    prop_oneof![
        Just(AuditEventTag::ArtifactClassified),
        Just(AuditEventTag::CheckerInvoked),
        Just(AuditEventTag::CheckerResult),
        Just(AuditEventTag::CorrespondenceCreated),
        Just(AuditEventTag::EntityEmitted),
        Just(AuditEventTag::ExtractorInvoked),
        Just(AuditEventTag::FrontierComputed),
        Just(AuditEventTag::FrontierItemStatusAssigned),
        Just(AuditEventTag::MigrationMapBuilt),
        Just(AuditEventTag::ObligationEmitted),
        Just(AuditEventTag::ObservationRewritten),
        Just(AuditEventTag::ObservationStatusAssigned),
        Just(AuditEventTag::ReportFinalized),
        Just(AuditEventTag::RepoLoaded),
    ]
}

fn arb_hex64() -> impl Strategy<Value = String> {
    // 64 chars from [0-9a-f]. Each char position is independent so
    // proptest shrinks freely.
    proptest::collection::vec(0u8..16, 64).prop_map(|nibbles| {
        nibbles
            .into_iter()
            .map(|n| {
                let c = if n < 10 { b'0' + n } else { b'a' + (n - 10) };
                c as char
            })
            .collect()
    })
}

fn arb_short_string(max: usize) -> impl Strategy<Value = String> {
    // Restrict to printable ASCII so JSON encoding is straightforward
    // and shrinker output stays readable.
    proptest::collection::vec(b'!'..=b'~', 1..=max)
        .prop_map(|bytes| bytes.into_iter().map(|b| b as char).collect::<String>())
}

fn arb_event() -> impl Strategy<Value = AuditEvent> {
    (
        arb_tag(),
        arb_hex64(),
        arb_short_string(64), // report_id (capped at 256, keep small)
        arb_short_string(32), // stage (capped at 64)
        arb_short_string(64), // actor (capped at 256)
    )
        .prop_map(|(tag, hash, report_id, stage, actor)| {
            AuditEvent::new(
                "2026-05-21T10:00:00Z",
                report_id,
                stage,
                tag,
                actor,
                hash,
                vec![],
            )
            .unwrap()
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        ..ProptestConfig::default()
    })]

    /// For any 1..=20 valid events, write-then-read returns the exact
    /// same Vec<AuditEvent>.
    #[test]
    fn prop_audit_round_trip_is_identity(events in prop::collection::vec(arb_event(), 1..=20)) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        {
            let mut w = AuditWriter::open(&path).unwrap();
            for e in &events {
                w.append(e).unwrap();
            }
        }

        let read_back: Vec<AuditEvent> = AuditReader::open(&path)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        prop_assert_eq!(events, read_back);
    }

    /// Append-then-reopen-then-append yields exactly the union of both
    /// batches in declared order.
    #[test]
    fn prop_audit_reopen_preserves_history(
        first  in prop::collection::vec(arb_event(), 1..=10),
        second in prop::collection::vec(arb_event(), 1..=10),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        {
            let mut w = AuditWriter::open(&path).unwrap();
            for e in &first {
                w.append(e).unwrap();
            }
        }
        {
            let mut w = AuditWriter::open(&path).unwrap();
            for e in &second {
                w.append(e).unwrap();
            }
        }

        let read_back: Vec<AuditEvent> = AuditReader::open(&path)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        let mut expected = first;
        expected.extend(second);
        prop_assert_eq!(expected, read_back);
    }
}

#[test]
fn audit_round_trip_1000_events() {
    // The acceptance gate calls for "write 1000 events → read back →
    // assert equality". Property tests cover small sequences with
    // shrinking; this single deterministic test exercises the
    // throughput case so a regression in BufWriter sizing is caught
    // even when proptest shrinks down to a 1-event minimum.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.ndjson");

    let mut originals = Vec::with_capacity(1000);
    {
        let mut w = AuditWriter::open(&path).unwrap();
        for i in 0..1000_u32 {
            let hash: String = std::iter::repeat_with(|| {
                let n = (i % 16) as u8;
                if n < 10 {
                    (b'0' + n) as char
                } else {
                    (b'a' + (n - 10)) as char
                }
            })
            .take(64)
            .collect();
            let e = AuditEvent::new(
                "2026-05-21T10:00:00Z",
                format!("run-{i:04}"),
                "extraction",
                AuditEventTag::ExtractorInvoked,
                "polyref-loader",
                hash,
                vec![],
            )
            .unwrap();
            w.append(&e).unwrap();
            originals.push(e);
        }
    }

    let read_back: Vec<AuditEvent> = AuditReader::open(&path)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(originals.len(), read_back.len());
    assert_eq!(originals, read_back);
}
