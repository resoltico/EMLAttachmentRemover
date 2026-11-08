//! Human rendering of core outcomes; never a GUI protocol.
use eml_core::{
    Event, EventSink,
    vocabulary::{BatchTotals, Outcome, OutcomeFacts, RunFailure, format_bytes},
};
use eml_protocol::{BoundedEventBuffer, MAX_LINE_BYTES};
use std::io::{self, Write};
type HumanItem = (Outcome, usize, Option<(u64, u64)>);
pub(crate) struct HumanSink<W> {
    writer: W,
    failed: bool,
    interrupted_items: usize,
    items: Vec<HumanItem>,
    checking: bool,
}
impl<W: Write> HumanSink<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self {
            writer,
            failed: false,
            interrupted_items: 0,
            items: Vec::new(),
            checking: false,
        }
    }
    fn deliver(&mut self, bytes: &[u8], facts: Option<&OutcomeFacts>) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        if facts.is_some_and(eml_core::vocabulary::explicitly_interrupted) {
            self.interrupted_items += 1;
        }
        Ok(())
    }
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
            Event::Item(item) => Some(OutcomeFacts::from(&item.result)),
            _ => None,
        };
        let result = (|| {
            let mut buffer = BoundedEventBuffer::new(MAX_LINE_BYTES);
            write_human(
                &mut buffer,
                &event,
                self.interrupted_items,
                &self.items,
                self.checking,
            )?;
            self.deliver(buffer.bytes(), facts.as_ref())?;
            match event {
                Event::Start(start) => self.checking = start.dry_run,
                Event::Item(item) => {
                    let data = metadata(&item.result);
                    let parts = data.map_or(0, |d| d.removed.len());
                    self.items.push((
                        Outcome::from(facts.as_ref().unwrap(), parts),
                        parts,
                        data.map(|d| (d.source_bytes, d.copy_bytes)),
                    ));
                }
                Event::Summary(_) => {}
            }
            Ok(())
        })();
        self.failed = result.is_err();
        result
    }
}
fn write_error(writer: &mut impl Write, error: &eml_core::OperationFailure) -> io::Result<()> {
    writeln!(
        writer,
        "                 [{} during {}: {}]",
        error.code(),
        error.phase(),
        error.detail()
    )
}
fn metadata<'a>(result: &'a eml_core::ItemResult<'a>) -> Option<eml_core::MessageMetadata<'a>> {
    use eml_core::ItemResult;
    match result {
        ItemResult::Created(_, data) | ItemResult::UpToDate(_, data) => Some(*data),
        ItemResult::Planned(plan) => Some(plan.metadata()),
        ItemResult::Failed(failure) => failure.metadata(),
    }
}
fn write_human(
    writer: &mut impl Write,
    event: &Event<'_>,
    interrupted_items: usize,
    items: &[HumanItem],
    checking: bool,
) -> io::Result<()> {
    match event {
        Event::Start(value) => writeln!(
            writer,
            "{} {} — {} emails{}{}",
            env!("CARGO_PKG_NAME"),
            eml_core::product::VERSION_WITH_REVISION.as_str(),
            value.total,
            if value.dry_run {
                " · check only, nothing is written"
            } else {
                ""
            },
            if value.collection_complete {
                ""
            } else {
                " · some selected paths could not be read"
            }
        ),
        Event::Item(value) => write_item(writer, value),
        Event::Summary(value) => {
            let counts = value.counts();
            counts
                .failed()
                .checked_sub(interrupted_items)
                .and_then(|_| counts.not_processed().checked_add(interrupted_items))
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid interrupted item counts",
                    )
                })?;
            let mut totals = BatchTotals::from(items.iter().copied());
            *totals.counts.entry(Outcome::NotProcessed).or_default() += counts.not_processed();
            writeln!(
                writer,
                "{}. Exit {}{}",
                totals.summary(checking),
                value.exit_class().code(),
                if value.interrupted() {
                    "; interrupted"
                } else {
                    ""
                }
            )?;
            if let Some(error) = value.run_error().filter(|e| {
                e.code() != eml_core::ErrorCode::InputAdmissionFailed || items.is_empty()
            }) {
                writeln!(writer, "{}", RunFailure(error.code()).explanation())?;
                write_error(writer, error)?;
            }
            Ok(())
        }
    }
}
fn write_item(writer: &mut impl Write, value: &eml_core::ItemEvent<'_>) -> io::Result<()> {
    use eml_protocol::TerminalPath;
    let facts = OutcomeFacts::from(&value.result);
    let data = metadata(&value.result);
    let outcome = Outcome::from(&facts, data.map_or(0, |d| d.removed.len()));
    write!(
        writer,
        "{:<24} {}",
        outcome.label(),
        TerminalPath::new(value.source.display)
    )?;
    if let Some(path) = value.destination {
        write!(writer, " → {}", TerminalPath::new(path.display))?;
    }
    if let Some(data) = data {
        write!(
            writer,
            " · {} {} · {} → {}",
            data.removed.len(),
            if data.removed.len() == 1 {
                "attachment"
            } else {
                "attachments"
            },
            format_bytes(data.source_bytes),
            format_bytes(data.copy_bytes)
        )?;
    }
    writeln!(writer)?;
    if let eml_core::ItemResult::Failed(failure) = &value.result {
        writeln!(writer, "                 {}", outcome.explanation())?;
        write_error(writer, failure.error())?;
        for consequence in eml_core::vocabulary::classify(&facts).consequences {
            writeln!(writer, "                 {}", consequence.text())?;
        }
    }
    if let Some(notice) = eml_core::vocabulary::classify(&facts).related_notice {
        writeln!(writer, "                 {notice}")?;
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
            "emlattachmentremover {} — 1 emails · check only, nothing is written\n",
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
                .contains("1 can’t be copied · 0 not processed")
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
            text.contains("1 can’t be copied · 1 not processed"),
            "{text}"
        );
        assert_eq!(
            fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
            b"different"
        );
        root.close().unwrap();
    }
}
