//! Native operation receipts shared by the supported filesystem backends.
use super::{CleanupReceipt, CreationFailure};
use std::io;

/// Exact requested synchronization method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMethod {
    /// Portable fsync.
    Fsync,
    /// fsync and Darwin F_FULLFSYNC both completed.
    FsyncFullfsync,
    /// fsync completed; only the stronger Darwin request was unsupported.
    FsyncFullfsyncUnsupported,
    /// Windows FlushFileBuffers completed on the held object.
    FlushFileBuffers,
}
/// Native request result, never inferred from file existence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    /// The specified method completed.
    Succeeded,
    /// A required request is unsupported.
    Unsupported,
    /// A required request failed.
    Failed,
}
/// Receipt for one exact held-object synchronization request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncReceipt {
    /// Method actually requested.
    pub method: SyncMethod,
    /// Result actually observed.
    pub state: SyncState,
    /// Native error retained for failure or limited-method evidence.
    pub native_code: Option<i64>,
}
impl SyncReceipt {
    /// Whether the complete receipt consistently records successful synchronization.
    /// This fact alone grants no publication or existing-copy qualification.
    pub fn succeeded(self) -> bool {
        if self.state != SyncState::Succeeded {
            return false;
        }
        match self.method {
            // This method names Darwin's stronger request. Its ABI error values
            // are EINVAL, ENOTSUP and EOPNOTSUPP even when read on another OS.
            SyncMethod::FsyncFullfsyncUnsupported => {
                matches!(self.native_code, Some(22 | 45 | 102))
            }
            _ => self.native_code.is_none(),
        }
    }
}

/// Native operation in progress when staging failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageOperation {
    /// Create and validate owned staging resources before bytes. POSIX resources
    /// are owner-private; Windows resources retain native destination inheritance.
    Create,
    /// Write, reread or validate candidate storage, including checks after flush.
    Write,
    /// Request candidate file synchronization.
    Sync,
}
/// Staging failed before publication; preserves its cause, flush and owned cleanup.
#[derive(Debug)]
pub struct StageFailure {
    /// Native operation, so callers need not parse error prose.
    pub operation: StageOperation,
    /// Primary native/storage invariant failure.
    pub error: io::Error,
    /// File synchronization, if reached.
    pub file_sync: Option<SyncReceipt>,
    /// Actual attempt to remove this operation's owned resources.
    pub cleanup: CleanupReceipt,
}
impl std::fmt::Display for StageFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}; cleanup {:?}", self.error, self.cleanup.state)
    }
}
impl std::error::Error for StageFailure {}
impl From<CreationFailure> for StageFailure {
    fn from(error: CreationFailure) -> Self {
        Self {
            operation: StageOperation::Create,
            error: error.error,
            file_sync: None,
            cleanup: error.cleanup,
        }
    }
}

/// Observation of this candidate's no-replace publication attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationState {
    /// No publication edge succeeded.
    NotPublished,
    /// The native no-replace edge succeeded or its identity was reconciled.
    Published,
    /// An ambiguous native error could not be reconciled.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successful_receipts_require_method_specific_native_facts() {
        for method in [
            SyncMethod::Fsync,
            SyncMethod::FsyncFullfsync,
            SyncMethod::FlushFileBuffers,
        ] {
            for code in [None, Some(0), Some(5), Some(22), Some(45), Some(102)] {
                for state in [
                    SyncState::Succeeded,
                    SyncState::Unsupported,
                    SyncState::Failed,
                ] {
                    assert_eq!(
                        SyncReceipt {
                            method,
                            state,
                            native_code: code
                        }
                        .succeeded(),
                        state == SyncState::Succeeded && code.is_none(),
                        "{method:?} {state:?} {code:?}"
                    );
                }
            }
        }
        for code in [
            None,
            Some(0),
            Some(5),
            Some(22),
            Some(45),
            Some(95),
            Some(102),
        ] {
            for state in [
                SyncState::Succeeded,
                SyncState::Unsupported,
                SyncState::Failed,
            ] {
                assert_eq!(
                    SyncReceipt {
                        method: SyncMethod::FsyncFullfsyncUnsupported,
                        state,
                        native_code: code
                    }
                    .succeeded(),
                    state == SyncState::Succeeded && matches!(code, Some(22 | 45 | 102)),
                    "{state:?} {code:?}"
                );
            }
        }
    }
}
