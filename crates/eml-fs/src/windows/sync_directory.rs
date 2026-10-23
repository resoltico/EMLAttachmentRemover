//! Flush-only endpoint owner; no layout or file creation surface.
use super::{child, creation, handle};
use crate::{BoundDirectory, Identity, SyncReceipt};
use std::{fs::File, io};
use windows_sys::Win32::{Foundation::GENERIC_WRITE, Storage::FileSystem::*};
pub(crate) struct SyncEndpoint {
    file: File,
    identity: Identity,
}
impl SyncEndpoint {
    pub(crate) fn acquire(directory: &BoundDirectory) -> io::Result<Self> {
        directory.check_held()?;
        let resolved = directory.resolved_path()?;
        let file = handle::open(
            &resolved,
            GENERIC_WRITE | child::ANCESTOR_PIN_ACCESS,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )?;
        let identity = handle::directory_identity(&file)?;
        if identity != directory.identity() {
            return Err(io::Error::other("Synchronization endpoint changed"));
        }
        Ok(Self { file, identity })
    }
    pub(crate) fn identity(&self) -> Identity {
        self.identity
    }
    pub(crate) fn sync_namespace(&self) -> SyncReceipt {
        creation::flush(&self.file)
    }
}
