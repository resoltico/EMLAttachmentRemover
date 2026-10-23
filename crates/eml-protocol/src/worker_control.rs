//! Private worker control is independent of protocol-1 outcome events.
/// Handler/transport readiness, emitted before parent request feeding.
pub const READY: u8 = b'R';
/// First core Stop flag observed; only then can the parent send the second INT.
pub const FIRST_INTERRUPT: u8 = b'F';
/// Exact private worker identity includes compiled CPU/OS, so a translated or
/// wrong-target helper cannot satisfy the desktop product's version probe.
pub fn identity_line(version: &str) -> String {
    crate::desktop::identity_line(version, &eml_core::product::BUILD_NUMBER.to_string())
}
/// Invalid, duplicated, overlong or out-of-order private control bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlStreamError;
/// A two-byte finite stream. It never retains a queue or infers signal delivery.
#[derive(Default)]
pub struct WorkerControlStream {
    ready: bool,
    first: bool,
    failed: bool,
    eof: bool,
}
impl WorkerControlStream {
    /// Consume R followed by at most one F; any invalid byte permanently fails.
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), ControlStreamError> {
        if self.failed || self.eof {
            self.failed = true;
            return Err(ControlStreamError);
        }
        for byte in bytes {
            if !self.ready && *byte == READY {
                self.ready = true;
            } else if self.ready && !self.first && *byte == FIRST_INTERRUPT {
                self.first = true;
            } else {
                self.failed = true;
                return Err(ControlStreamError);
            }
        }
        Ok(())
    }
    /// Observe EOF separately from actual process reap and other pipe completion.
    pub fn finish(&mut self) -> Result<(), ControlStreamError> {
        self.eof = true;
        if self.failed || !self.ready {
            self.failed = true;
            Err(ControlStreamError)
        } else {
            Ok(())
        }
    }
    /// Real readiness byte was accepted.
    pub fn ready(&self) -> bool {
        self.ready
    }
    /// Real first-interrupt byte was accepted, not inferred from a successful kill.
    pub fn first_interrupt(&self) -> bool {
        self.first
    }
}
