//! Ordinary files retain strict sharing; extra sync access never exposes data writes.
use super::{handle, path::validate_component};
use crate::{BoundFileError, FileSignature, Identity};
use std::{
    fs::File,
    io,
    os::windows::fs::FileExt,
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE},
    Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    },
};

/// Held ordinary file, requested parent identity and observable signature. Source
/// opens are read-only; existing-copy sync opens request write access solely for
/// FlushFileBuffers. No data-write or metadata-setting capability is exposed.
/// No source handle is closed merely to make writable destination binding succeed.
pub struct BoundFile {
    file: File,
    path: PathBuf,
    parent: PathBuf,
    parent_id: Identity,
    initial: FileSignature,
    follow_final: bool,
}
impl BoundFile {
    /// Open an admitted absolute extended DOS/UNC path with read sharing only.
    /// Only explicit selected file links may request following the final link.
    pub fn open(path: &Path, follow_final: bool) -> Result<Self, BoundFileError> {
        Self::open_with_access(path, follow_final, GENERIC_READ)
    }
    /// Open one existing no-follow ordinary file for comparison/resynchronization.
    /// This requires GENERIC_WRITE but never writes data or clears readonly flags.
    /// The workflow must compare read-only first, checked-close that holder, then
    /// revalidate this fresh binding and bytes before requesting synchronization.
    pub fn open_for_sync(path: &Path) -> Result<Self, BoundFileError> {
        Self::open_with_access(path, false, GENERIC_READ | GENERIC_WRITE)
    }
    fn open_with_access(
        path: &Path,
        follow_final: bool,
        access: u32,
    ) -> Result<Self, BoundFileError> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "File has no admitted parent")
        })?;
        validate_component(
            path.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "File has no native component")
            })?,
            32_768,
        )?;
        let (_, parent_id) = handle::directory_observation(parent)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Requested parent is not an ordinary directory",
            )
        })?;
        let file = handle::open(
            path,
            access,
            FILE_SHARE_READ,
            if follow_final {
                0
            } else {
                FILE_FLAG_OPEN_REPARSE_POINT
            },
        )?;
        // Type is checked on the opened handle before any data read.
        let initial = handle::signature(&file)?;
        let bound = Self {
            file,
            path: path.to_owned(),
            parent: parent.to_owned(),
            parent_id,
            initial,
            follow_final,
        };
        bound.check_binding()?;
        Ok(bound)
    }
    /// Request the actual file flush; insufficient rights/failure are receipts,
    /// never silently successful. This alone does not qualify an existing copy.
    pub fn sync_file(&self) -> crate::SyncReceipt {
        super::writable::flush(&self.file)
    }
    /// Consume and explicitly check closure of this primary ordinary file owner.
    /// Never retry a failed close or treat it as a successful ownership transfer.
    pub fn close(self) -> io::Result<()> {
        handle::close(self.file)
    }
    /// Signature observed when this exact ordinary file was opened.
    pub fn initial_signature(&self) -> FileSignature {
        self.initial
    }
    /// Captured physical parent, independently rechecked by check_binding.
    pub fn parent_identity(&self) -> Identity {
        self.parent_id
    }
    /// Current metadata on the still-held source, not a rebaselined signature.
    pub fn current_signature(&self) -> Result<FileSignature, BoundFileError> {
        handle::signature(&self.file).map_err(BoundFileError::against_regular_baseline)
    }
    /// Read a bounded caller buffer from the same held file.
    pub fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        self.file.seek_read(buffer, offset)
    }
    /// Reobserve the current requested parent AND file. A retargeted parent with
    /// another hardlink to the same file is a changed binding, not a snapshot.
    pub fn check_binding(&self) -> Result<(), BoundFileError> {
        let (_, parent_id) =
            handle::directory_observation(&self.parent)?.ok_or(BoundFileError::Changed)?;
        if parent_id != self.parent_id {
            return Err(BoundFileError::Changed);
        }
        let current = handle::open(
            &self.path,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            if self.follow_final {
                0
            } else {
                FILE_FLAG_OPEN_REPARSE_POINT
            },
        )?;
        if handle::signature(&current).map_err(BoundFileError::against_regular_baseline)?
            != self.initial
            || self.current_signature()? != self.initial
        {
            return Err(BoundFileError::Changed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PathContext;
    #[test]
    fn native_file_identity_hardlinks_and_excluded_writer_are_observed() {
        let root = tempfile::tempdir().unwrap();
        let context = PathContext::capture(&std::env::current_dir().unwrap()).unwrap();
        let first = root.path().join("first.eml");
        let alias = root.path().join("alias.eml");
        let other = root.path().join("other.eml");
        std::fs::write(&first, b"synthetic source").unwrap();
        std::fs::hard_link(&first, &alias).unwrap();
        std::fs::write(&other, b"synthetic source").unwrap();
        let source = BoundFile::open(&context.resolve(&first).unwrap(), false).unwrap();
        let alias = BoundFile::open(&context.resolve(&alias).unwrap(), false).unwrap();
        let other = BoundFile::open(&context.resolve(&other).unwrap(), false).unwrap();
        assert_eq!(
            source.initial_signature().identity,
            alias.initial_signature().identity
        );
        assert_ne!(
            source.initial_signature().identity,
            other.initial_signature().identity
        );
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .open(&first)
            .expect_err("Read snapshot allowed write handle");
        assert_eq!(writer.raw_os_error(), Some(32));
        let mut bytes = [0; 32];
        let count = source.read_at(&mut bytes, 0).unwrap();
        assert_eq!(&bytes[..count], b"synthetic source");
        source.check_binding().unwrap();
        drop(source);
        drop(alias);
        drop(other);
        root.close().unwrap();
    }
    #[test]
    fn directories_are_rejected_before_file_read() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("directory");
        std::fs::create_dir(&directory).unwrap();
        let operation = PathContext::capture(&std::env::current_dir().unwrap())
            .unwrap()
            .resolve(&directory)
            .unwrap();
        let failed = match BoundFile::open(&operation, false) {
            Err(error) => error,
            Ok(_) => panic!("Directory was admitted as an ordinary file"),
        };
        // The unchanged source-open flags do not request directory access.
        // An access-denied open is an observation failure, not an observed type.
        assert!(matches!(&failed, BoundFileError::ObservationFailed(_)));
        assert!(failed.native_code().is_some());
        root.close().unwrap();
    }
    #[test]
    fn retargeted_parent_with_same_file_hardlink_is_a_changed_binding() {
        let root = tempfile::tempdir().unwrap();
        let context = PathContext::capture(&std::env::current_dir().unwrap()).unwrap();
        let a = root.path().join("a");
        let b = root.path().join("b");
        let alias = root.path().join("alias");
        for path in [&a, &b, &alias] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(a.join("source.eml"), b"same physical source").unwrap();
        std::fs::hard_link(a.join("source.eml"), b.join("source.eml")).unwrap();
        let reparse = crate::windows::test_reparse::writer(&alias).unwrap();
        crate::windows::test_reparse::set_mount_point(&reparse, &a).unwrap();
        drop(reparse);
        let path = context.resolve(&alias.join("source.eml")).unwrap();
        let source = BoundFile::open(&path, false).unwrap();
        let original_parent = source.parent_identity();
        let reparse = crate::windows::test_reparse::writer(&alias).unwrap();
        crate::windows::test_reparse::set_mount_point(&reparse, &b).unwrap();
        drop(reparse);
        let current = handle::open(
            &path,
            GENERIC_READ,
            FILE_SHARE_READ,
            FILE_FLAG_OPEN_REPARSE_POINT,
        )
        .unwrap();
        assert_eq!(
            handle::signature(&current).unwrap(),
            source.initial_signature()
        );
        let (parent, parent_id) = handle::directory_observation(&context.resolve(&alias).unwrap())
            .unwrap()
            .unwrap();
        assert_ne!(parent_id, original_parent);
        assert!(matches!(
            source.check_binding(),
            Err(BoundFileError::Changed)
        ));
        drop(current);
        drop(parent);
        drop(source);
        std::fs::remove_dir(&alias).unwrap();
        root.close().unwrap();
    }
}

#[cfg(test)]
mod resynchronization_tests {
    use super::*;
    use crate::PathContext;
    use crate::windows::test_actor::RestrictedToken;
    fn root() -> tempfile::TempDir {
        crate::windows::test_fixture::directory()
    }
    #[test]
    fn native_existing_sync_access_keeps_identity_bytes_and_self_validation() {
        let root = root();
        let path = root.path().join("copy.eml");
        let source_path = root.path().join("source.eml");
        std::fs::write(&path, b"owned complete copy").unwrap();
        std::fs::write(&source_path, b"owned source").unwrap();
        let context = PathContext::capture(root.path()).unwrap();
        let operand = context.resolve(&path).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let source = BoundFile::open(&context.resolve(&source_path).unwrap(), false).unwrap();
        let read = BoundFile::open(&operand, false).unwrap();
        let original = read.initial_signature();
        let readonly_flush = read.sync_file();
        assert_eq!(readonly_flush.method, crate::SyncMethod::FlushFileBuffers);
        assert_eq!(readonly_flush.state, crate::SyncState::Failed);
        assert_eq!(readonly_flush.native_code, Some(5));
        let mut bytes = [0; 64];
        assert_eq!(read.read_at(&mut bytes, 0).unwrap(), 19);
        assert_eq!(&bytes[..19], b"owned complete copy");
        read.check_binding().unwrap();
        read.close().unwrap();
        let sync = BoundFile::open_for_sync(&operand).unwrap();
        assert_eq!(sync.initial_signature(), original);
        assert_eq!(sync.read_at(&mut bytes, 0).unwrap(), 19);
        assert_eq!(&bytes[..19], b"owned complete copy");
        sync.check_binding().unwrap();
        let receipt = sync.sync_file();
        assert_eq!(receipt.method, crate::SyncMethod::FlushFileBuffers);
        assert_eq!(receipt.state, crate::SyncState::Succeeded, "{receipt:?}");
        assert_eq!(receipt.native_code, None);
        sync.check_binding().unwrap();
        assert_eq!(sync.current_signature().unwrap(), original);
        let denied = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap_err();
        assert_eq!(denied.raw_os_error(), Some(32));
        sync.close().unwrap();
        source.check_binding().unwrap();
        assert_eq!(source.read_at(&mut bytes, 0).unwrap(), 12);
        assert_eq!(&bytes[..12], b"owned source");
        // The selected source stayed held through the complete existing-file flow.
        source.close().unwrap();
        assert_eq!(std::fs::read(&source_path).unwrap(), b"owned source");
        assert_eq!(std::fs::read(&path).unwrap(), b"owned complete copy");
        drop(actor);
        root.close().unwrap();
    }
    #[test]
    fn readonly_existing_file_is_readable_but_sync_access_is_refused_without_changes() {
        let root = root();
        let path = root.path().join("readonly.eml");
        std::fs::write(&path, b"owned readonly copy").unwrap();
        let context = PathContext::capture(root.path()).unwrap();
        let operand = context.resolve(&path).unwrap();
        let original_permissions = std::fs::metadata(&path).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&path, readonly).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundFile::open(&operand, false).unwrap();
        let mut bytes = [0; 64];
        assert_eq!(read.read_at(&mut bytes, 0).unwrap(), 19);
        assert_eq!(&bytes[..19], b"owned readonly copy");
        read.check_binding().unwrap();
        read.close().unwrap();
        let access_error = match BoundFile::open_for_sync(&operand) {
            Err(BoundFileError::ObservationFailed(error)) => error.raw_os_error(),
            Err(_) => None,
            Ok(file) => {
                file.close().unwrap();
                None
            }
        };
        let unchanged = std::fs::read(&path).unwrap();
        let stayed_readonly = std::fs::metadata(&path).unwrap().permissions().readonly();
        drop(actor);
        // Restore only this owned fixture before outcome assertions.
        std::fs::set_permissions(&path, original_permissions).unwrap();
        assert_eq!(access_error, Some(5));
        assert!(stayed_readonly);
        assert_eq!(unchanged, b"owned readonly copy");
        root.close().unwrap();
    }
    #[test]
    fn held_readonly_file_must_be_checked_closed_before_fresh_sync_access() {
        let root = root();
        let path = root.path().join("copy.eml");
        std::fs::write(&path, b"owned copy").unwrap();
        let context = PathContext::capture(root.path()).unwrap();
        let operand = context.resolve(&path).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundFile::open(&operand, false).unwrap();
        let original = read.initial_signature();
        let denied = match BoundFile::open_for_sync(&operand) {
            Err(BoundFileError::ObservationFailed(error)) => error,
            _ => panic!("Live read-only SHARE_READ owner must exclude write access"),
        };
        assert_eq!(denied.raw_os_error(), Some(32));
        read.check_binding().unwrap();
        read.close().unwrap();
        let sync = BoundFile::open_for_sync(&operand).unwrap();
        assert_eq!(sync.initial_signature(), original);
        sync.check_binding().unwrap();
        sync.close().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"owned copy");
        drop(actor);
        root.close().unwrap();
    }
}
