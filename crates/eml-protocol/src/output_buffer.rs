//! Bounded complete-event storage shared by the actual output adapters.
use std::io::{self, Write};
/// One complete event with a checked encoded-byte bound.
pub struct BoundedEventBuffer {
    pub(crate) bytes: Vec<u8>,
    pub(crate) limit: usize,
}
impl Write for BoundedEventBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|length| length > self.limit)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Output event exceeds its encoded byte bound",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl BoundedEventBuffer {
    /// Start an empty event with its format-specific bound.
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
    /// Borrow the complete checked event for delivery.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
