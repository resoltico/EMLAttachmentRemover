//! Shared fixture for independent human and JSON delivery tests.
use eml_core::RunStart;
use std::io::{self, Write};

#[derive(Default)]
pub(super) struct Writer {
    pub(super) bytes: Vec<u8>,
    pub(super) write_calls: usize,
    pub(super) flush_calls: usize,
    pub(super) remaining: Option<usize>,
    pub(super) fail_flush: bool,
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
pub(super) fn start() -> RunStart {
    RunStart {
        run_error: None,
        output_unavailable: false,
        dry_run: true,
        total: 1,
        collection_complete: true,
    }
}
