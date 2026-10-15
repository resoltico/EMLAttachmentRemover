//! Native operation receipts shared by the supported filesystem backends.
use crate::{CleanupReceipt, CreationFailure};
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

/// Native operation in progress when staging failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageOperation {
    /// Create and validate private owned resources before bytes.
    Create,
    /// Write or reread candidate storage.
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
