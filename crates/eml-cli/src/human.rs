//! Human rendering of actual core events; never a GUI protocol.
use eml_core::{Event, EventSink};
use eml_protocol::{BoundedEventBuffer, MAX_LINE_BYTES};
use std::io::{self, Write};
/// Checked human adapter. Wording is for people and never parsed by the app.
pub(crate) struct HumanSink<W> {
    writer: W,
    failed: bool,
    interrupted_items: usize,
}
impl<W: Write> HumanSink<W> {
    /// Start one human stream on the supplied writer.
    pub(crate) fn new(writer: W) -> Self {
        Self {
            writer,
            failed: false,
            interrupted_items: 0,
        }
    }
    // Commit the presentation adjustment only after the whole item is delivered.
    fn deliver(
        &mut self,
        bytes: &[u8],
        facts: Option<&eml_core::vocabulary::OutcomeFacts>,
    ) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        if facts.is_some_and(eml_core::vocabulary::explicitly_interrupted) {
            self.interrupted_items += 1;
        }
        Ok(())
    }
    /// Recover the supplied writer without another output attempt.
    #[cfg(test)]
    pub(crate) fn into_inner(self) -> W {
        self.writer
    }
}
impl<W: Write> EventSink for HumanSink<W> {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Human output stream already failed",
            ));
        }
        let facts = match &event {
            Event::Item(item) => Some(eml_core::vocabulary::OutcomeFacts::from(&item.result)),
            _ => None,
        };
        let result = (|| {
            let mut buffer = BoundedEventBuffer::new(MAX_LINE_BYTES);
            write_human(&mut buffer, event, self.interrupted_items)?;
            self.deliver(buffer.bytes(), facts.as_ref())
        })();
        self.failed = result.is_err();
        result
    }
}
fn write_error(
    writer: &mut impl Write,
    error: &eml_core::OperationFailure,
    indent: &str,
) -> io::Result<()> {
    write!(
        writer,
        "{indent}{}/{}: {}",
        error.code(),
        error.phase(),
        error.detail()
    )?;
    if let Some(cause) = error.native_error() {
        write!(writer, ": {}", cause.kind())?;
    }
    match error.native_diagnostic() {
        Some(eml_core::NativeDiagnostic::OsCode(code)) => write!(writer, " (OS error {code})")?,
        #[cfg(windows)]
        Some(eml_core::NativeDiagnostic::NtStatus { status, win32 }) => {
            write!(writer, " (NTSTATUS {status}; Win32 {win32})")?
        }
        None => {}
    }
    writeln!(writer)?;
    if let Some(hint) = eml_core::vocabulary::recovery_hint(error.code().as_str()) {
        writeln!(writer, "{indent}{hint}")?;
    }
    Ok(())
}
fn write_human(
    writer: &mut impl Write,
    event: Event<'_>,
    interrupted_items: usize,
) -> io::Result<()> {
    match event {
        Event::Start(value) => writeln!(
            writer,
            "{} {} — {}: {}; discovery {}{}",
            env!("CARGO_PKG_NAME"),
            eml_core::product::VERSION_WITH_REVISION.as_str(),
            eml_core::vocabulary::CountKind::KnownItems.label(),
            value.total,
            if value.collection_complete {
                "complete"
            } else {
                "incomplete"
            },
            if value.dry_run { "; dry run" } else { "" }
        ),
        Event::Item(value) => write_item(writer, value),
        Event::Summary(value) => {
            let counts = value.counts();
            writeln!(
                writer,
                "{}",
                eml_core::vocabulary::summary(
                    &counts,
                    value.exit_class(),
                    value.interrupted(),
                    interrupted_items
                )
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
            )?;
            if let Some(error) = value.run_error() {
                write_error(writer, error, "")?;
            }
            Ok(())
        }
    }
}

fn write_item(writer: &mut impl Write, value: &eml_core::ItemEvent<'_>) -> io::Result<()> {
    use eml_core::ItemResult;
    use eml_protocol::TerminalPath;
    let meaning =
        eml_core::vocabulary::classify(&eml_core::vocabulary::OutcomeFacts::from(&value.result));
    let status = meaning.label();
    let metadata = match &value.result {
        ItemResult::Created(_, data) | ItemResult::UpToDate(_, data) => Some(*data),
        ItemResult::Planned(plan) => Some(plan.metadata()),
        ItemResult::Failed(failure) => failure.metadata(),
    };
    write!(
        writer,
        "[{index}] {status}: {source}",
        index = value.index,
        source = TerminalPath::new(value.source.display)
    )?;
    if let Some(path) = value.destination {
        write!(writer, " -> {}", TerminalPath::new(path.display))?;
    }
    writeln!(writer)?;
    if let Some(data) = metadata {
        writeln!(
            writer,
            "  {}: {}; {}: {}; {}: {}",
            eml_core::vocabulary::SOURCE_BYTES_LABEL,
            data.source_bytes,
            eml_core::vocabulary::COPY_BYTES_LABEL,
            data.copy_bytes,
            meaning.removal_mode.parts(),
            data.removed.len()
        )?;
    }
    if let Some(notice) = meaning.related_notice {
        writeln!(writer, "  {notice}")?;
    }
    if let ItemResult::Failed(failure) = &value.result {
        write_error(writer, failure.error(), "  ")?;
        for consequence in &meaning.consequences {
            writeln!(writer, "  {}", consequence.text())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eml_core::{ExitClass, OutputLocation, RunControl, RunMode, RunRequest, RunStart, run};
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../eml-protocol/tests/support/output_writer.rs"
    ));
    fn expected_human_start() -> String {
        format!(
            "emlattachmentremover {} — Known items: 1; discovery complete; dry run\n",
            eml_core::product::VERSION_WITH_REVISION.as_str()
        )
    }
    #[test]
    fn partial_write_and_flush_failures_permanently_stop_human_output() {
        for flush in [false, true] {
            let mut sink = HumanSink::new(Writer {
                remaining: (!flush).then_some(7),
                fail_flush: flush,
                ..Writer::default()
            });
            assert!(sink.emit(Event::Start(&start())).is_err());
            let expected = expected_human_start();
            assert_eq!(
                sink.writer.bytes,
                if flush {
                    expected.as_bytes()
                } else {
                    &expected.as_bytes()[..7]
                }
            );
            assert_eq!(sink.writer.flush_calls, usize::from(flush));

            let calls = (
                sink.writer.write_calls,
                sink.writer.flush_calls,
                sink.writer.bytes.len(),
            );
            assert!(sink.emit(Event::Start(&start())).is_err());
            assert_eq!(
                (
                    sink.writer.write_calls,
                    sink.writer.flush_calls,
                    sink.writer.bytes.len()
                ),
                calls
            );
        }
    }
    #[test]
    fn interruption_adjustment_commits_after_write_and_flush_only() {
        let facts = eml_core::vocabulary::OutcomeFacts {
            result: eml_core::vocabulary::ResultKind::Failed,
            error: Some(eml_core::ErrorCode::Interrupted),
            plan: None,
            publication: eml_core::vocabulary::Publication::NotPublished,
            address: eml_core::AddressState::NotChecked,
            candidate_identity_known: false,
            final_readback: None,
            file_sync: None,
            namespace_sync: eml_core::NamespaceObservation {
                receipt: None,
                required_edges: 0,
                completed_edges: 0,
                failed_edge_index: None,
            },
            cleanup: eml_core::CleanupReceipt {
                state: eml_core::CleanupState::NotNeeded,
                residual_name: None,
                native_code: None,
            },
            existing: None,
            related_resources: false,
        };
        for (remaining, fail_flush) in [(Some(3), false), (None, true), (None, false)] {
            let mut sink = HumanSink::new(Writer {
                remaining,
                fail_flush,
                ..Default::default()
            });
            let delivered = sink.deliver(b"interrupted item\n", Some(&facts)).is_ok();
            assert_eq!(delivered, remaining.is_none() && !fail_flush);
            assert_eq!(sink.interrupted_items, usize::from(delivered));
        }
    }
    #[test]
    fn genuine_failure_events_do_not_adjust_human_or_json_counts() {
        let root = root();
        fs::write(
            root.path().join("a.eml"),
            b"Content-Type: multipart/mixed\r\n\r\nbroken",
        )
        .unwrap();
        let req = request(root.path(), RunMode::DryRun);
        let mut human = HumanSink::new(Vec::new());
        let outcome = run(&req, &RunControl::default(), &mut human).unwrap();
        assert_eq!(outcome.counts().failed(), 1);
        let mut inconsistent = HumanSink::new(Vec::new());
        inconsistent.interrupted_items = 2;
        assert_eq!(
            inconsistent
                .emit(Event::Summary(&outcome))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(inconsistent.writer.is_empty());
        assert!(inconsistent.emit(Event::Start(&start())).is_err());
        assert!(inconsistent.writer.is_empty());
        assert_eq!(human.interrupted_items, 0);
        assert!(
            String::from_utf8(human.into_inner())
                .unwrap()
                .contains("Failed 1; Not processed 0. Exit 1")
        );
        let mut json = eml_protocol::JsonSink::new(Vec::new());
        run(&req, &RunControl::default(), &mut json).unwrap();
        let records: Vec<serde_json::Value> = String::from_utf8(json.into_inner())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records[1]["status"], "failed");
        assert_eq!(records[2]["counts"]["failed"], 1);
        assert_eq!(records[2]["counts"]["not_processed"], 0);
        root.close().unwrap();
    }
    fn root() -> tempfile::TempDir {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/event-tests");
        fs::create_dir_all(&parent).unwrap();
        tempfile::tempdir_in(parent).unwrap()
    }
    fn request(root: &Path, mode: RunMode) -> RunRequest {
        RunRequest {
            paths: vec!["a.eml".into()],
            working_directory: fs::canonicalize(root).unwrap(),
            output: OutputLocation::BesideSource,
            mode,
        }
    }
    #[test]
    fn human_output_escapes_unsafe_filename_units_and_reports_real_failure_effect() {
        let root = root();
        let input = b"Content-Type: text/plain\r\n\r\nbody\r\n";
        fs::write(root.path().join("a.eml"), input).unwrap();
        fs::write(root.path().join("a.no-attachments.eml"), b"different").unwrap();
        let mut req = request(root.path(), RunMode::Create);
        req.paths.push(PathBuf::from("missing\u{2028}\u{1b}.eml"));
        let mut sink = HumanSink::new(Vec::new());
        let outcome = run(&req, &RunControl::default(), &mut sink).unwrap();
        assert_eq!(outcome.exit_class(), ExitClass::Failure);
        let text = String::from_utf8(sink.into_inner()).unwrap();
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\u{2028}'));
        assert!(text.contains("No new copy was published."));
        assert!(
            text.contains(
                &eml_core::vocabulary::summary(
                    &outcome.counts(),
                    outcome.exit_class(),
                    outcome.interrupted(),
                    0
                )
                .unwrap()
            )
        );
        assert_eq!(
            fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
            b"different"
        );
        root.close().unwrap();
    }
}
