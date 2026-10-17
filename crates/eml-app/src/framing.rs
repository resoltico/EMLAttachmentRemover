//! Streaming LF framing with irreversible failure and no retained frame queue.
use eml_protocol::{MAX_LINE_BYTES, MAX_RUN_BYTES};

/// A framing or consumer failure; preceding accepted observations stay valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// A line, including its required LF, exceeds the allowed size.
    LineLimit,
    /// The total stream reaches or exceeds the strict run bound.
    RunLimit,
    /// An empty LF record cannot be a protocol event.
    EmptyRecord,
    /// The stream has already failed and cannot resume acceptance.
    Failed,
}
impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Protocol framing: {self:?}")
    }
}
impl std::error::Error for FrameError {}

/// Calls the consumer for each complete record before accepting the next one.
/// An EOF tail is discarded explicitly; it is never decoded as a complete event.
#[derive(Default)]
pub struct FrameSplitter {
    pending: Vec<u8>,
    received: usize,
    failed: bool,
}
impl FrameSplitter {
    /// Accept a bounded chunk; consumer failure irreversibly closes this stream.
    pub fn append<E: From<FrameError>>(
        &mut self,
        bytes: &[u8],
        mut consume: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.failed {
            return Err(FrameError::Failed.into());
        }
        let result = self.accept(bytes, &mut consume);
        if result.is_err() {
            self.failed = true;
            self.pending.clear();
            self.pending.shrink_to_fit();
        }
        result
    }
    fn accept<E: From<FrameError>>(
        &mut self,
        bytes: &[u8],
        consume: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        if bytes.len() >= MAX_RUN_BYTES.saturating_sub(self.received) {
            return Err(FrameError::RunLimit.into());
        }
        self.received += bytes.len();
        let mut rest = bytes;
        while !rest.is_empty() {
            let newline = rest.iter().position(|byte| *byte == b'\n');
            let count = newline.unwrap_or(rest.len());
            if count >= MAX_LINE_BYTES.saturating_sub(self.pending.len()) {
                return Err(FrameError::LineLimit.into());
            }
            self.pending.extend_from_slice(&rest[..count]);
            if let Some(index) = newline {
                if self.pending.is_empty() {
                    return Err(FrameError::EmptyRecord.into());
                }
                consume(&self.pending)?;
                self.pending.clear();
                rest = &rest[index + 1..];
            } else {
                break;
            }
        }
        Ok(())
    }
    /// Close acceptance, discard incomplete EOF bytes and report their presence.
    pub fn finish(&mut self) -> bool {
        let partial = !self.pending.is_empty();
        self.failed = true;
        self.pending.clear();
        self.pending.shrink_to_fit();
        partial
    }
}
