//! Operation-specific native authority. Observation alone cannot create entries.
use crate::{BoundDirectory, Identity, SyncReceipt};
use std::io;

/// Complete endpoint access for existing namespace synchronization, with no creation API.
pub struct SyncDirectory {
    #[cfg(unix)]
    directory: BoundDirectory,
    #[cfg(windows)]
    directory: crate::windows::sync_directory::SyncEndpoint,
}
impl SyncDirectory {
    /// Acquire only the actual endpoint's native synchronization access.
    pub fn acquire(directory: &BoundDirectory) -> io::Result<Self> {
        directory.check_held()?;
        #[cfg(unix)]
        {
            Ok(Self {
                directory: directory.clone(),
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                directory: crate::windows::sync_directory::SyncEndpoint::acquire(directory)?,
            })
        }
    }
    /// Identity of the exact held endpoint.
    pub fn identity(&self) -> Identity {
        self.directory.identity()
    }
    /// Actual native synchronization receipt, independent of namespace creation policy.
    pub fn sync_namespace(&self) -> SyncReceipt {
        self.directory.sync_namespace()
    }
}

#[cfg(unix)]
/// Current immediate mutation parent; every native edge rechecks admission.
pub struct CreationDirectory {
    directory: BoundDirectory,
}
#[cfg(unix)]
impl CreationDirectory {
    /// Qualify this held immediate parent for creation. Untouched ancestors are excluded.
    pub fn acquire(directory: &BoundDirectory) -> io::Result<Self> {
        directory.admit_name_domain()?;
        directory.admit_namespace()?;
        Ok(Self {
            directory: directory.clone(),
        })
    }
    /// Read-only held parent facts; borrowing this does not convey creation authority.
    pub fn observed(&self) -> &BoundDirectory {
        &self.directory
    }
    /// Actual held parent identity.
    pub fn identity(&self) -> Identity {
        self.directory.identity()
    }
    /// Exclusive private child creation with admission rechecked at the edge.
    pub fn create_child(
        &self,
        name: &std::ffi::OsStr,
    ) -> Result<crate::CreatedDirectory, crate::CreationFailure> {
        self.directory.create_private_child(name)
    }
    /// Stage supplied immutable verified bytes using the admitted immediate parent.
    pub fn stage_bytes(
        &self,
        bytes: &[u8],
        checkpoint: impl FnMut() -> io::Result<()>,
    ) -> Result<crate::ReadyCandidate, crate::StageFailure> {
        self.directory.stage_bytes(bytes, checkpoint)
    }
    /// Execute the same staging engine with finite nonshipping boundary observations.
    #[cfg(feature = "native-fixtures")]
    pub fn stage_bytes_observed(
        &self,
        bytes: &[u8],
        checkpoint: impl FnMut() -> io::Result<()>,
        observer: impl FnMut(crate::StoragePhase),
    ) -> Result<crate::ReadyCandidate, crate::StageFailure> {
        self.directory
            .stage_bytes_observed(bytes, checkpoint, observer)
    }
    /// Synchronize the exact held parent's namespace.
    pub fn sync_namespace(&self) -> SyncReceipt {
        self.directory.sync_namespace()
    }
}
