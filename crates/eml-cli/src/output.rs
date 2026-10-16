//! Checked whole-event delivery. A failed sink never starts a replacement stream.
use eml_core::{Event, EventSink};
use std::io::{self, Write};

/// Maximum encoded protocol line, including its terminating LF byte.
pub const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;
/// Strict upper bound on encoded events retained/delivered for one run.
pub const MAX_RUN_BYTES: usize = 256 * 1024 * 1024;

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|length| length > self.limit)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Protocol line exceeds its encoded byte bound",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
/// JSON Lines adapter to borrowed Core events. It makes no filesystem, outcome
/// or count decisions; even an undelivered summary is a checked sink failure.
pub struct JsonSink<W> {
    writer: W,
    attempted_bytes: usize,
    failed: bool,
}
impl<W: Write> JsonSink<W> {
    /// Start one new protocol stream on an explicitly supplied checked writer.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            attempted_bytes: 0,
            failed: false,
        }
    }
    /// Recover the supplied writer without another write/flush attempt.
    pub fn into_inner(self) -> W {
        self.writer
    }
}
impl<W: Write> EventSink for JsonSink<W> {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Protocol stream already failed",
            ));
        }
        let result = (|| {
            let mut buffer = LimitedBuffer {
                bytes: Vec::new(),
                limit: MAX_LINE_BYTES - 1,
            };
            crate::event::write_json(&mut buffer, event).map_err(io::Error::other)?;
            buffer.bytes.push(b'\n');
            let total = self
                .attempted_bytes
                .checked_add(buffer.bytes.len())
                .filter(|total| *total < MAX_RUN_BYTES)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Protocol run exceeds its encoded byte bound",
                    )
                })?;
            self.attempted_bytes = total;
            self.writer.write_all(&buffer.bytes)?;
            self.writer.flush()
        })();
        self.failed = result.is_err();
        result
    }
}

/// Checked human adapter. Wording is for people and never parsed by the app.
pub struct HumanSink<W> {
    writer: W,
    failed: bool,
}
impl<W: Write> HumanSink<W> {
    /// Start one human stream on the supplied writer.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            failed: false,
        }
    }
    /// Recover the supplied writer without another output attempt.
    pub fn into_inner(self) -> W {
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
            let mut buffer = LimitedBuffer {
                bytes: Vec::new(),
                limit: MAX_LINE_BYTES,
            };
            write_human(&mut buffer, event)?;
            self.writer.write_all(&buffer.bytes)?;
            self.writer.flush()
        })();
        self.failed = result.is_err();
        result
    }
}
fn write_human(writer: &mut impl Write, event: Event<'_>) -> io::Result<()> {
    use crate::TerminalPath;
    use eml_core::{AddressState, FailureEffect, ItemResult, PlanAction};
    match event {
        Event::Start(value) => writeln!(
            writer,
            "{} {} — {} known item(s); discovery {}{}",
            crate::event::PROGRAM,
            crate::event::VERSION,
            value.total,
            if value.collection_complete {
                "complete"
            } else {
                "incomplete"
            },
            if value.dry_run { "; dry run" } else { "" }
        ),
        Event::Item(value) => {
            let (status, metadata) = match &value.result {
                ItemResult::Created(_, data) => ("Created", Some(*data)),
                ItemResult::UpToDate(_, data) => ("Up to date", Some(*data)),
                ItemResult::Planned(plan) => (
                    match plan.action() {
                        PlanAction::Create => "Planned create",
                        PlanAction::VerifyExisting => "Planned existing-copy verification",
                    },
                    Some(plan.metadata()),
                ),
                ItemResult::Failed(failure) => ("Failed", failure.metadata()),
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
                    "  {} source bytes; {} copy bytes; {} removed part(s)",
                    data.source_bytes,
                    data.copy_bytes,
                    data.removed.len()
                )?;
            }
            if let ItemResult::Failed(failure) = &value.result {
                writeln!(
                    writer,
                    "  {}/{}: {}",
                    failure.error().code(),
                    failure.error().phase(),
                    failure.error().detail()
                )?;
                match failure.effect() {
                    FailureEffect::Published(copy) => writeln!(
                        writer,
                        "  A complete copy was published; {}.",
                        match copy.address() {
                            AddressState::Verified => "its requested address was verified",
                            AddressState::Changed => "its requested address changed",
                            _ => "its requested address could not be verified",
                        }
                    )?,
                    FailureEffect::Unknown(_) => writeln!(
                        writer,
                        "  Publication could not be confirmed; a complete copy might exist."
                    )?,
                    FailureEffect::NotPublished { .. } => {
                        writeln!(writer, "  This item did not publish a new copy.")?
                    }
                }
                if let Some(existing) = failure.existing()
                    && existing.bytes_equal == Some(true)
                {
                    writeln!(
                        writer,
                        "  Existing bytes matched; synchronization/address qualification did not complete."
                    )?;
                }
            }
            Ok(())
        }
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
                writeln!(
                    writer,
                    "{}/{}: {}",
                    error.code(),
                    error.phase(),
                    error.detail()
                )?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eml_core::RunStart;
    #[derive(Default)]
    struct Writer {
        bytes: Vec<u8>,
        write_calls: usize,
        flush_calls: usize,
        remaining: Option<usize>,
        fail_flush: bool,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.write_calls += 1;
            let count = if let Some(left) = &mut self.remaining {
                if *left == 0 {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                let count = bytes.len().min(*left);
                *left -= count;
                count
            } else {
                bytes.len()
            };
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flush_calls += 1;
            if self.fail_flush {
                Err(io::ErrorKind::BrokenPipe.into())
            } else {
                Ok(())
            }
        }
    }
    fn start() -> RunStart {
        RunStart {
            dry_run: true,
            total: 1,
            collection_complete: true,
        }
    }
    fn expected_json_start() -> String {
        format!(
            "{{\"type\":\"start\",\"protocol\":1,\"program\":\"remove-eml-attachments\",\"version\":\"{}\",\"dry_run\":true,\"total\":1,\"collection_complete\":true}}\n",
            env!("CARGO_PKG_VERSION")
        )
    }
    fn expected_human_start() -> String {
        format!(
            "remove-eml-attachments {} — 1 known item(s); discovery complete; dry run\n",
            env!("CARGO_PKG_VERSION")
        )
    }
    #[test]
    fn hand_authored_start_has_one_compact_lf_and_checked_flush() {
        let mut sink = JsonSink::new(Writer::default());
        sink.emit(Event::Start(&start())).unwrap();
        let writer = sink.into_inner();
        let expected = format!(
            "{{\"type\":\"start\",\"protocol\":1,\"program\":\"remove-eml-attachments\",\"version\":\"{}\",\"dry_run\":true,\"total\":1,\"collection_complete\":true}}\n",
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(writer.bytes, expected.as_bytes());
        assert_eq!(writer.flush_calls, 1);
    }
    #[test]
    fn partial_write_and_flush_failures_permanently_stop_both_adapters() {
        for flush in [false, true] {
            let mut sink = JsonSink::new(Writer {
                remaining: (!flush).then_some(7),
                fail_flush: flush,
                ..Writer::default()
            });
            assert!(sink.emit(Event::Start(&start())).is_err());
            let expected = expected_json_start();
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
    fn exact_line_bound_reserves_lf_and_one_more_byte_is_rejected() {
        assert_eq!(MAX_LINE_BYTES, 67_108_864);
        let mut buffer = LimitedBuffer {
            bytes: Vec::new(),
            limit: MAX_LINE_BYTES - 1,
        };
        let chunk = vec![b'x'; 65_536];
        while buffer.bytes.len() + chunk.len() < MAX_LINE_BYTES {
            buffer.write_all(&chunk).unwrap();
        }
        let remainder = MAX_LINE_BYTES - 1 - buffer.bytes.len();
        buffer.write_all(&chunk[..remainder]).unwrap();
        assert_eq!(buffer.bytes.len() + 1, MAX_LINE_BYTES);
        assert_eq!(
            buffer.write(b"x").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(buffer.bytes.len(), MAX_LINE_BYTES - 1);
    }
    #[test]
    fn run_bound_failure_happens_before_output_and_does_not_restart() {
        let mut sink = JsonSink::new(Writer::default());
        sink.attempted_bytes = MAX_RUN_BYTES - 1;
        assert!(sink.emit(Event::Start(&start())).is_err());
        assert!(sink.writer.bytes.is_empty());
        assert_eq!(sink.writer.flush_calls, 0);
        assert!(sink.failed);
        assert!(sink.emit(Event::Start(&start())).is_err());
        assert!(sink.writer.bytes.is_empty());
        assert_eq!((sink.writer.write_calls, sink.writer.flush_calls), (0, 0));
    }
}
