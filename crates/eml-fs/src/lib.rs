//! Narrow held-identity filesystem primitives. Business copy policy belongs in eml-core.
#![deny(unsafe_code)]
#[cfg(unix)]
mod file;
#[cfg(unix)]
mod namespace;
#[cfg(unix)]
pub use file::BoundFile;
mod observation;
pub use observation::{BoundFileError, EntryKind, EntryObservation, FileSignature};
mod receipts;
#[cfg(any(unix, windows))]
mod staging;
pub use receipts::{
    PublicationState, StageFailure, StageOperation, SyncMethod, SyncReceipt, SyncState,
};
#[cfg(unix)]
mod inventory;
#[cfg(unix)]
pub use inventory::DirectoryEntries;
#[cfg(unix)]
mod candidate;
#[cfg(unix)]
pub use candidate::{PublicationCandidate, PublishAttempt, ReadyCandidate};
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{BoundDirectory, CreatedDirectory};
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Only the documented Darwin opaque ACL ABI, ownership and descriptor calls.
mod darwin_acl;
#[cfg(windows)]
#[allow(unsafe_code)] // Generated Windows ABI only; native buffers/handles remain owned.
mod windows;
#[cfg(all(windows, any(test, feature = "native-fixtures")))]
pub use windows::creation_observer;
#[cfg(windows)]
pub use windows::path::{PathContext, validate_component};
#[cfg(windows)]
pub use windows::{
    BoundDirectory, BoundFile, ContainmentFailure, DirectoryCreation, DirectoryEntries, EntryPin,
    NtStatusError, PinnedRoot, PrivateDirectory, PublicationCandidate, PublishAttempt,
    ReadyCandidate, RootBindingError, WritableDirectory, create_private_directory,
    observe_selected, rename_artifact_noreplace,
};

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
    /// No owned staging resource requires removal.
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
impl CreationFailure {
    /// A definite native collision with no unresolved owned resource.
    /// Other statuses, translated ErrorKind values and uncertain cleanup cannot
    /// authorize a retry or acceptance of another creator's entry.
    pub fn is_name_collision(&self) -> bool {
        let cleaned = self.cleanup.state == CleanupState::NotNeeded
            || (cfg!(windows) && self.cleanup.state == CleanupState::Complete);
        if !cleaned || self.cleanup.native_code.is_some() {
            return false;
        }
        name_collision(&self.error)
    }
}
pub(crate) fn name_collision(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(rustix::io::Errno::EXIST.raw_os_error())
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS};
        matches!(
            error.raw_os_error().map(|code| code as u32),
            Some(ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS)
        )
    }
}
pub(crate) fn unsupported_publication(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        [
            rustix::io::Errno::NOTSUP,
            rustix::io::Errno::OPNOTSUPP,
            rustix::io::Errno::NOSYS,
        ]
        .iter()
        .any(|code| error.raw_os_error() == Some(code.raw_os_error()))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            ERROR_CALL_NOT_IMPLEMENTED, ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED,
        };
        matches!(
            error.raw_os_error().map(|code| code as u32),
            Some(ERROR_CALL_NOT_IMPLEMENTED | ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED)
        )
    }
}
impl std::fmt::Display for CreationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}; cleanup {:?}", self.error, self.cleanup.state)
    }
}
impl std::error::Error for CreationFailure {}
