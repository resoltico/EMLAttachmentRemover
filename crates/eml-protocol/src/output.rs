//! Checked whole-event delivery. A failed sink never starts a replacement stream.
use eml_core::{Event, EventSink};
use std::io::{self, Write};

/// Maximum encoded protocol line, including its terminating LF byte.
pub const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;
/// Strict upper bound on encoded events retained/delivered for one run.
pub const MAX_RUN_BYTES: usize = 256 * 1024 * 1024;

use crate::BoundedEventBuffer as LimitedBuffer;
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

#[cfg(test)]
mod tests {
    use super::*;
    use eml_core::RunStart;
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/support/output_writer.rs"
    ));
    fn expected_json_start() -> String {
        format!(
            "{{\"type\":\"start\",\"protocol\":1,\"program\":\"remove-eml-attachments\",\"version\":\"{}\",\"dry_run\":true,\"total\":1,\"collection_complete\":true}}\n",
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
    fn partial_write_and_flush_failures_permanently_stop_json_output() {
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
