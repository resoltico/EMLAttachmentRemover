//! Narrow held-identity filesystem primitives. Business copy policy belongs in eml-core.
#![deny(unsafe_code)]
#[cfg(unix)]
mod file;
#[cfg(unix)]
mod namespace;
#[cfg(unix)]
pub use file::{BoundFile, FileSignature};
#[cfg(unix)]
mod inventory;
#[cfg(unix)]
pub use inventory::{DirectoryEntries, EntryKind, EntryObservation};
#[cfg(unix)]
mod candidate;
#[cfg(unix)]
pub use candidate::{
    PublicationCandidate, PublicationState, PublishAttempt, ReadyCandidate, StageFailure,
    SyncMethod, SyncReceipt, SyncState,
};
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{BoundDirectory, CreatedDirectory};
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Only the documented Darwin opaque ACL ABI, ownership and descriptor calls.
mod darwin_acl;

/// Native object identity; never a pathname hash or lossy display value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Identity {
    /// POSIX device/inode pair.
    Posix {
        /// Native device identifier.
        device: u64,
        /// Native inode identifier.
        inode: u64,
    },
    /// Windows volume serial and 128-bit file identifier.
    Windows {
        /// Native volume serial.
        volume: u64,
        /// Native FILE_ID_128 bytes.
        file: [u8; 16],
    },
}

/// Observation of cleanup of this operation's owned resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupReceipt {
    /// Whether cleanup was unnecessary, completed, residual or unconfirmed.
    pub state: CleanupState,
    /// Known native single component, never an inferred absolute residual path.
    pub residual_name: Option<std::ffi::OsString>,
    /// Native cleanup failure, when an OS operation supplied it.
    pub native_code: Option<i64>,
}
/// Cleanup evidence states; unknown is not success.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupState {
    /// No owned resource was created.
    NotNeeded,
    /// The owned resource was actually removed.
    Complete,
    /// A known owned resource remains.
    Residual,
    /// Removal or current external name could not be established.
    Unknown,
}
/// Creation failure retaining the primary cause and actual owned cleanup observation.
#[derive(Debug)]
pub struct CreationFailure {
    /// Primary native or invariant failure.
    pub error: std::io::Error,
    /// Cleanup actually attempted, never silently assumed from Drop.
    pub cleanup: CleanupReceipt,
}
impl std::fmt::Display for CreationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}; cleanup {:?}", self.error, self.cleanup.state)
    }
}
impl std::error::Error for CreationFailure {}
