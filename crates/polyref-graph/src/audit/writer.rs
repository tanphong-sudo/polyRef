//! Append-only NDJSON writer for [`AuditEvent`]s.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

use thiserror::Error;

use super::event::{AuditEvent, AuditEventError};

/// Append-only NDJSON writer for [`AuditEvent`]s.
///
/// The file is opened with `OpenOptions::append(true)` so concurrent
/// writers from the same process get atomic per-line appends from the
/// kernel (POSIX `O_APPEND`). One `BufWriter` is wrapped around the
/// file to coalesce byte-level writes; [`Self::append`] flushes after
/// every event so a crash mid-run loses at most the partial line of
/// the current event, never a previously-written one.
pub struct AuditWriter {
    inner: BufWriter<File>,
}

/// Failure to write to the audit log.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AuditWriteError {
    /// The supplied event failed schema validation before it could be
    /// serialized. Callers should never produce malformed events but
    /// the writer enforces the cap anyway.
    #[error("audit event invalid: {0}")]
    Invalid(#[from] AuditEventError),

    /// Underlying I/O error.
    #[error("audit io error: {0}")]
    Io(#[from] std::io::Error),

    /// `serde_json` failed to serialize the event. Should be
    /// unreachable for the in-tree wire types but possible if a future
    /// schema change introduces a non-serializable value.
    #[error("audit serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

impl AuditWriter {
    /// Open `path` for append. Creates the file if it does not exist.
    /// Existing content is preserved.
    ///
    /// # Errors
    ///
    /// Returns [`AuditWriteError::Io`] when the file cannot be opened.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AuditWriteError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())?;
        Ok(Self {
            inner: BufWriter::new(file),
        })
    }

    /// Append one event. Validates the event again (defense in depth)
    /// before serializing, writes a single `\n`-terminated JSON line,
    /// and flushes the buffer.
    ///
    /// # Errors
    ///
    /// - [`AuditWriteError::Invalid`] if the event failed schema caps.
    /// - [`AuditWriteError::Json`] if serialization failed.
    /// - [`AuditWriteError::Io`] if the underlying write or flush
    ///   failed.
    pub fn append(&mut self, event: &AuditEvent) -> Result<(), AuditWriteError> {
        event.validate()?;
        let line = serde_json::to_string(event)?;
        // NDJSON: one object per line, LF-terminated. Reject embedded
        // newlines defensively — a buggy upstream that managed to
        // smuggle a literal `\n` into a string field would split a
        // single event across two physical lines and break the reader.
        debug_assert!(
            !line.contains('\n'),
            "serde_json::to_string produced a multiline payload: {line}"
        );
        let mut buf = line.into_bytes();
        buf.push(b'\n');
        self.inner.write_all(&buf)?;
        self.inner.flush()?;
        Ok(())
    }

    /// Flush the underlying buffer. `append` already flushes after
    /// every event; this is only useful when a test wants an explicit
    /// barrier before reopening the file for read.
    ///
    /// # Errors
    ///
    /// Returns [`AuditWriteError::Io`] on flush failure.
    pub fn flush(&mut self) -> Result<(), AuditWriteError> {
        self.inner.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::super::event::AuditEventTag;
    use super::*;
    use std::io::Read;

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
    fn writer_creates_file_and_appends_one_line_per_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        {
            let mut w = AuditWriter::open(&path).unwrap();
            w.append(&sample_event(AuditEventTag::RepoLoaded, b'a'))
                .unwrap();
            w.append(&sample_event(AuditEventTag::ExtractorInvoked, b'b'))
                .unwrap();
        }

        let mut buf = String::new();
        File::open(&path).unwrap().read_to_string(&mut buf).unwrap();
        let lines: Vec<_> = buf.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"repo_loaded\""));
        assert!(lines[1].contains("\"extractor_invoked\""));
        // Each line ends with LF.
        assert!(buf.ends_with('\n'));
    }

    #[test]
    fn writer_preserves_existing_content_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");

        {
            let mut w = AuditWriter::open(&path).unwrap();
            w.append(&sample_event(AuditEventTag::RepoLoaded, b'a'))
                .unwrap();
        }
        // Reopen and append more.
        {
            let mut w = AuditWriter::open(&path).unwrap();
            w.append(&sample_event(AuditEventTag::ReportFinalized, b'c'))
                .unwrap();
        }

        let mut buf = String::new();
        File::open(&path).unwrap().read_to_string(&mut buf).unwrap();
        let lines: Vec<_> = buf.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"repo_loaded\""));
        assert!(lines[1].contains("\"report_finalized\""));
    }

    #[test]
    fn writer_rejects_malformed_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");
        let mut w = AuditWriter::open(path).unwrap();

        // Hand-construct an invalid event by skipping the builder.
        let mut bad = sample_event(AuditEventTag::RepoLoaded, b'a');
        bad.payload_hash = "not-hex".into();
        let err = w.append(&bad).unwrap_err();
        assert!(
            matches!(
                err,
                AuditWriteError::Invalid(AuditEventError::BadPayloadHash)
            ),
            "expected BadPayloadHash, got {err:?}"
        );
    }

    #[test]
    fn writer_flush_after_each_event_is_durable() {
        // Even without explicit `flush()` and without dropping the
        // writer, a previously-appended event must already be visible
        // on disk (write_through guarantee).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.ndjson");
        let mut w = AuditWriter::open(&path).unwrap();
        w.append(&sample_event(AuditEventTag::RepoLoaded, b'a'))
            .unwrap();

        // Concurrent reader-style check: re-open the same file, count
        // bytes — must be > 0 because append() flushed.
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 0, "append() must flush durable bytes");
    }
}
