// Shared rejecting writer for independent CLI-human and JSON delivery controls.
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
