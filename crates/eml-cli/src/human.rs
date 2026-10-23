//! Human rendering of actual core events; never a GUI protocol.
use eml_core::{Event, EventSink};
use eml_protocol::{BoundedEventBuffer, MAX_LINE_BYTES};
use std::io::{self, Write};
/// Checked human adapter. Wording is for people and never parsed by the app.
pub(crate) struct HumanSink<W> {
    writer: W,
    failed: bool,
}
impl<W: Write> HumanSink<W> {
    /// Start one human stream on the supplied writer.
    pub(crate) fn new(writer: W) -> Self {
        Self {
            writer,
            failed: false,
        }
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
        let result = (|| {
            let mut buffer = BoundedEventBuffer::new(MAX_LINE_BYTES);
            write_human(&mut buffer, event)?;
            self.writer.write_all(buffer.bytes())?;
            self.writer.flush()
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
    if let Some(hint) = eml_core::meaning::recovery_hint(error.code().as_str()) {
        writeln!(writer, "{indent}{hint}")?;
    }
    Ok(())
}
fn write_human(writer: &mut impl Write, event: Event<'_>) -> io::Result<()> {
    match event {
        Event::Start(value) => writeln!(
            writer,
            "{} {} — known items: {}; discovery {}{}",
            env!("CARGO_PKG_NAME"),
            eml_core::product::VERSION_WITH_BUILD.as_str(),
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
                "Created {}; up to date {}; planned {}; failed {}; not processed {}. Exit {}{}",
                counts.created(),
                counts.up_to_date(),
                counts.planned(),
                counts.failed(),
                counts.not_processed(),
                value.exit_class().code(),
                if value.interrupted() {
                    "; interrupted"
                } else {
                    ""
                }
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
        eml_core::meaning::classify(&eml_core::meaning::OutcomeFacts::from(&value.result));
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
            "  Source bytes: {}; prepared copy bytes: {}; {}: {}",
            data.source_bytes,
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
    }
    for consequence in meaning.consequences {
        writeln!(writer, "  {}", consequence.text())?;
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
        "/../../tests/support/output_writer.rs"
    ));
    fn expected_human_start() -> String {
        format!(
            "remove-eml-attachments {} — known items: 1; discovery complete; dry run\n",
            eml_core::product::VERSION_WITH_BUILD.as_str()
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
        fs::write(
            root.path()
                .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml"),
            b"different",
        )
        .unwrap();
        let mut req = request(root.path(), RunMode::Create);
        req.paths.push(PathBuf::from("missing\u{2028}\u{1b}.eml"));
        let mut sink = HumanSink::new(Vec::new());
        let outcome = run(&req, &RunControl::default(), &mut sink).unwrap();
        assert_eq!(outcome.exit_class(), ExitClass::Failure);
        let text = String::from_utf8(sink.into_inner()).unwrap();
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\u{2028}'));
        assert!(text.contains("No new copy was published."));
        assert!(text.contains("not processed 1"));
        assert_eq!(
            fs::read(
                root.path()
                    .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
            )
            .unwrap(),
            b"different"
        );
        root.close().unwrap();
    }
}
