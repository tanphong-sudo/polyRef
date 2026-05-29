//! Streaming NDJSON reader for [`AuditEvent`]s.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use thiserror::Error;

use super::event::{AuditEvent, AuditEventError};

/// Hard cap on a single audit-line byte length. Defends against a
/// malicious or runaway producer that writes a giant line; the cap is
/// 1 MiB which is two orders of magnitude above the largest event we
/// realistically expect (the typical event is a few hundred bytes).
pub const AUDIT_LINE_MAX_BYTES: usize = 1024 * 1024;

/// Streaming NDJSON reader for [`AuditEvent`]s.
///
/// Lines are read incrementally via `BufRead::fill_buf` with an
/// explicit per-line byte cap ([`AUDIT_LINE_MAX_BYTES`]), which keeps
/// memory bounded regardless of file size. Each line is also validated
/// through [`AuditEvent::validate`] so a corrupted or out-of-bounds
/// field is rejected at read time, not silently trusted.
pub struct AuditReader<R: BufRead> {
    inner: R,
    line_no: usize,
    /// Reusable scratch buffer for `read_line` to avoid per-line
    /// allocation churn.
    scratch: String,
}

/// Failure to read or parse an audit-log line.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AuditReadError {
    /// Underlying I/O error.
    #[error("audit read io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parse failed on the given physical line (1-indexed).
    #[error("audit line {line_no}: malformed JSON: {source}")]
    BadJson {
        /// Physical line number (1-indexed) where parsing failed.
        line_no: usize,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// JSON parsed but failed schema validation (length cap, bad
    /// payload_hash, etc.).
    #[error("audit line {line_no}: schema validation failed: {source}")]
    Invalid {
        /// Physical line number.
        line_no: usize,
        /// Underlying validation error.
        #[source]
        source: AuditEventError,
    },

    /// A single line exceeded [`AUDIT_LINE_MAX_BYTES`]. Likely a
    /// corrupted log or a producer that smuggled a literal `\n` into a
    /// payload.
    #[error("audit line {line_no}: line exceeds {AUDIT_LINE_MAX_BYTES} bytes")]
    LineTooLong {
        /// Physical line number.
        line_no: usize,
    },
}

impl<R: BufRead> AuditReader<R> {
    /// Wrap an existing buffered reader (e.g. one from a custom
    /// source or an in-memory fixture). Tests use this directly;
    /// production callers prefer [`Self::open`].
    #[must_use]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            line_no: 0,
            scratch: String::new(),
        }
    }
}

impl AuditReader<BufReader<File>> {
    /// Open `path` for reading. The file must exist.
    ///
    /// # Errors
    ///
    /// Returns [`AuditReadError::Io`] if the file cannot be opened.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AuditReadError> {
        let file = File::open(path.as_ref())?;
        Ok(Self::new(BufReader::new(file)))
    }
}

impl<R: BufRead> Iterator for AuditReader<R> {
    type Item = Result<AuditEvent, AuditReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.scratch.clear();
            self.line_no += 1;

            // Manual length-capped read: read byte-by-byte through
            // BufRead::fill_buf so a giant line cannot exhaust memory.
            let mut total = 0_usize;
            let saw_eof = loop {
                let buf = match self.inner.fill_buf() {
                    Ok(b) => b,
                    Err(e) => return Some(Err(AuditReadError::Io(e))),
                };
                if buf.is_empty() {
                    break true; // EOF
                }
                let nl = buf.iter().position(|&b| b == b'\n');
                let take = nl.map_or(buf.len(), |i| i + 1);
                if total + take > AUDIT_LINE_MAX_BYTES {
                    let needs_drain = nl.is_none();
                    self.inner.consume(take);
                    if needs_drain {
                        if let Err(error) = self.drain_to_next_line() {
                            return Some(Err(AuditReadError::Io(error)));
                        }
                    }
                    return Some(Err(AuditReadError::LineTooLong {
                        line_no: self.line_no,
                    }));
                }
                // Reject non-UTF-8 lines as malformed JSON. JSON is
                // UTF-8 so this is the right semantic level.
                let chunk = match std::str::from_utf8(&buf[..take]) {
                    Ok(s) => s,
                    Err(_) => {
                        // Consume the rest of the line so the iterator
                        // can advance past the corruption.
                        let needs_drain = nl.is_none();
                        self.inner.consume(take);
                        if needs_drain {
                            if let Err(error) = self.drain_to_next_line() {
                                return Some(Err(AuditReadError::Io(error)));
                            }
                        }
                        return Some(Err(
                            self.synth_bad_json("line is not valid UTF-8".to_owned())
                        ));
                    }
                };
                self.scratch.push_str(chunk);
                self.inner.consume(take);
                total += take;
                if nl.is_some() {
                    break false;
                }
            };

            // Trim trailing LF / CRLF before parsing.
            let trimmed = self.scratch.trim_end_matches('\n').trim_end_matches('\r');

            if trimmed.is_empty() {
                if saw_eof {
                    return None;
                }
                // Skip blank lines silently — robust to a producer
                // that writes an extra `\n` somewhere; doesn't change
                // the typed event stream.
                continue;
            }

            return Some(self.parse_line(trimmed));
        }
    }
}

impl<R: BufRead> AuditReader<R> {
    fn drain_to_next_line(&mut self) -> std::io::Result<()> {
        loop {
            let buf = self.inner.fill_buf()?;
            if buf.is_empty() {
                return Ok(());
            }
            let nl = buf.iter().position(|&b| b == b'\n');
            let take = nl.map_or(buf.len(), |i| i + 1);
            self.inner.consume(take);
            if nl.is_some() {
                return Ok(());
            }
        }
    }

    fn parse_line(&self, line: &str) -> Result<AuditEvent, AuditReadError> {
        let event: AuditEvent =
            serde_json::from_str(line).map_err(|e| AuditReadError::BadJson {
                line_no: self.line_no,
                source: e,
            })?;
        event.validate().map_err(|e| AuditReadError::Invalid {
            line_no: self.line_no,
            source: e,
        })?;
        Ok(event)
    }

    fn synth_bad_json(&self, msg: String) -> AuditReadError {
        AuditReadError::BadJson {
            line_no: self.line_no,
            source: serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                msg,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::super::event::AuditEventTag;
    use super::super::writer::AuditWriter;
    use super::*;
    use std::io::Cursor;

    fn h(byte: u8) -> String {
        std::iter::repeat(byte as char).take(64).collect()
    }

    fn sample_event(tag: AuditEventTag, hash_byte: u8) -> AuditEvent {
        AuditEvent::new(
            "2026-05-21T10:00:00Z",
            "run-001",
            "extraction",
            tag,
            "polyref-loader",
            h(hash_byte),
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn reader_streams_each_line_to_an_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        let mut w = AuditWriter::open(&path).unwrap();
        for tag in [
            AuditEventTag::RepoLoaded,
            AuditEventTag::ExtractorInvoked,
            AuditEventTag::ReportFinalized,
        ] {
            w.append(&sample_event(tag, b'a')).unwrap();
        }
        w.flush().unwrap();
        drop(w);

        let r = AuditReader::open(&path).unwrap();
        let events: Result<Vec<_>, _> = r.collect();
        let events = events.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].tag, AuditEventTag::RepoLoaded);
        assert_eq!(events[1].tag, AuditEventTag::ExtractorInvoked);
        assert_eq!(events[2].tag, AuditEventTag::ReportFinalized);
    }

    #[test]
    fn reader_skips_blank_lines() {
        let mut payload = String::new();
        payload.push_str("\n\n");
        payload.push_str(
            &serde_json::to_string(&sample_event(AuditEventTag::RepoLoaded, b'a')).unwrap(),
        );
        payload.push_str("\n\n");

        let r = AuditReader::new(Cursor::new(payload));
        let events: Result<Vec<_>, _> = r.collect();
        assert_eq!(events.unwrap().len(), 1);
    }

    #[test]
    fn reader_returns_typed_error_on_malformed_json() {
        let payload = b"{not json at all}\n";
        let mut r = AuditReader::new(Cursor::new(payload.as_ref()));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(err, AuditReadError::BadJson { line_no: 1, .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn reader_returns_typed_error_on_schema_violation() {
        // Valid JSON but payload_hash is too short → fails validate().
        let mut bad = sample_event(AuditEventTag::RepoLoaded, b'a');
        bad.payload_hash = "deadbeef".into();
        let line = serde_json::to_string(&bad).unwrap();
        let payload = format!("{line}\n");

        let mut r = AuditReader::new(Cursor::new(payload));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(
                err,
                AuditReadError::Invalid {
                    line_no: 1,
                    source: AuditEventError::BadPayloadHash
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn reader_caps_line_length() {
        // Build a line larger than the cap. Use repeated valid JSON
        // bytes so we exercise the cap path, not the UTF-8 path.
        let big = "x".repeat(AUDIT_LINE_MAX_BYTES + 16);
        let payload = format!("{big}\n");

        let mut r = AuditReader::new(Cursor::new(payload));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(err, AuditReadError::LineTooLong { line_no: 1 }),
            "got {err:?}"
        );
    }

    #[test]
    fn reader_recovers_after_overlong_line() {
        let big = "x".repeat(AUDIT_LINE_MAX_BYTES + 16);
        let valid = serde_json::to_string(&sample_event(AuditEventTag::RepoLoaded, b'a')).unwrap();
        let payload = format!("{big}\n{valid}\n");

        let mut r = AuditReader::new(Cursor::new(payload));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(err, AuditReadError::LineTooLong { line_no: 1 }),
            "got {err:?}"
        );

        let event = r.next().unwrap().unwrap();
        assert_eq!(event.tag, AuditEventTag::RepoLoaded);
        assert!(r.next().is_none());
    }

    #[test]
    fn reader_recovers_after_multichunk_non_utf8_line() {
        let valid = serde_json::to_string(&sample_event(AuditEventTag::RepoLoaded, b'a')).unwrap();
        let mut payload = vec![b'a'; 32];
        payload[3] = 0xff;
        payload.extend_from_slice(b"\n");
        payload.extend_from_slice(valid.as_bytes());
        payload.extend_from_slice(b"\n");
        let cursor = Cursor::new(payload);
        let mut r = AuditReader::new(BufReader::with_capacity(8, cursor));

        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(err, AuditReadError::BadJson { line_no: 1, .. }),
            "got {err:?}"
        );

        let event = r.next().unwrap().unwrap();
        assert_eq!(event.tag, AuditEventTag::RepoLoaded);
        assert!(r.next().is_none());
    }

    #[test]
    fn reader_returns_typed_error_on_unknown_tag() {
        // Hand-construct a JSON line with a tag that isn't in the
        // closed enum — closed-enum policy rejects it.
        let payload = r#"{"ts":"2026-05-21T10:00:00Z","report_id":"r","stage":"x","tag":"not_a_tag","actor":"a","payload_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
"#;
        let mut r = AuditReader::new(Cursor::new(payload));
        let err = r.next().unwrap().unwrap_err();
        assert!(
            matches!(err, AuditReadError::BadJson { line_no: 1, .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn reader_round_trip_after_writer_yields_byte_equal_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        let originals: Vec<AuditEvent> = [
            (AuditEventTag::RepoLoaded, b'a'),
            (AuditEventTag::ArtifactClassified, b'b'),
            (AuditEventTag::ExtractorInvoked, b'c'),
            (AuditEventTag::EntityEmitted, b'd'),
            (AuditEventTag::CorrespondenceCreated, b'e'),
            (AuditEventTag::MigrationMapBuilt, b'f'),
            (AuditEventTag::FrontierComputed, b'0'),
            (AuditEventTag::ObligationEmitted, b'1'),
            (AuditEventTag::CheckerInvoked, b'2'),
            (AuditEventTag::CheckerResult, b'3'),
            (AuditEventTag::ObservationRewritten, b'4'),
            (AuditEventTag::FrontierItemStatusAssigned, b'5'),
            (AuditEventTag::ObservationStatusAssigned, b'6'),
            (AuditEventTag::ReportFinalized, b'7'),
        ]
        .into_iter()
        .map(|(t, h)| sample_event(t, h))
        .collect();

        {
            let mut w = AuditWriter::open(&path).unwrap();
            for e in &originals {
                w.append(e).unwrap();
            }
        }

        let read_back: Vec<AuditEvent> = AuditReader::open(&path)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(originals, read_back);
    }
}
