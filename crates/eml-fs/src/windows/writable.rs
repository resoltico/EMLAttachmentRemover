//! Fixed creation ownership and cleanup; directory and candidate storage stay distinct.
use super::{child, handle, path::validate_component, status};
use crate::{
    BoundDirectory, CleanupReceipt, CleanupState, CreationFailure, Identity, NtStatusError,
    SyncMethod, SyncReceipt, SyncState,
};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE,
            FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{
            CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE, OBJ_DONT_REPARSE,
            STATUS_OBJECT_NAME_COLLISION, STATUS_OBJECT_NAME_NOT_FOUND, UNICODE_STRING,
        },
        Storage::FileSystem::*,
        System::{IO::IO_STATUS_BLOCK, WindowsProgramming::FILE_CREATED},
    },
};

/// A selected ordinary directory with explicit creation and flush access.
/// Physical binding does not itself qualify a requested address or publication.
pub struct WritableDirectory(BoundDirectory);
const ROOT_WRITE_ACCESS: u32 =
    GENERIC_WRITE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
const ROOT_WRITE_SHARING: u32 = FILE_SHARE_READ;
const ROOT_WRITE_FLAGS: u32 = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
struct ParentBinding {
    file: File,
    identity: Identity,
}
/// A positively created directory retained for explicit empty cleanup or persistence.
/// Drop closes handles and never recursively removes a pathname.
pub struct CreatedDirectory {
    directory: WritableDirectory,
    parent: ParentBinding,
    name: OsString,
}
fn receipt(
    state: CleanupState,
    name: Option<OsString>,
    native_code: Option<i64>,
) -> CleanupReceipt {
    CleanupReceipt {
        state,
        residual_name: name,
        native_code,
    }
}
fn no_creation(error: io::Error) -> CreationFailure {
    CreationFailure {
        error,
        cleanup: receipt(CleanupState::NotNeeded, None, None),
    }
}
fn native_code(error: &io::Error) -> Option<i64> {
    error.raw_os_error().map(i64::from).or_else(|| {
        error
            .get_ref()
            .and_then(|cause| cause.downcast_ref::<NtStatusError>())
            .map(|cause| i64::from(cause.nt_status()))
    })
}
pub(super) fn flush(file: &File) -> SyncReceipt {
    // SAFETY: live held file opened with the mapped GENERIC_WRITE access set.
    if unsafe { FlushFileBuffers(file.as_raw_handle()) } != 0 {
        return SyncReceipt {
            method: SyncMethod::FlushFileBuffers,
            state: SyncState::Succeeded,
            native_code: None,
        };
    }
    let error = io::Error::last_os_error();
    SyncReceipt {
        method: SyncMethod::FlushFileBuffers,
        state: if matches!(error.raw_os_error(), Some(1 | 50)) {
            SyncState::Unsupported
        } else {
            SyncState::Failed
        },
        native_code: native_code(&error),
    }
}
fn close(file: File) -> io::Result<()> {
    let raw = file.into_raw_handle();
    // SAFETY: exactly one owned valid file handle transferred out of File.
    // No second close is guessed after a failed native close.
    if unsafe { CloseHandle(raw) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn remove(
    file: File,
    parent: ParentBinding,
    name: OsString,
    expected: Option<Identity>,
) -> CleanupReceipt {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: positively created owned file/directory handle with DELETE access;
    // generated fixed buffer remains live through the synchronous call. Native
    // directory deletion enforces emptiness; a file caller may remove its owned
    // nonempty file. Neither operation recursively removes directory contents.
    let marked = if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    };
    // Once disposition was requested, query no further child metadata through
    // this handle. Close explicitly, then observe only through its held parent.
    let closed = close(file);
    let first = marked.as_ref().err().or_else(|| closed.as_ref().err());
    match handle::directory_identity(&parent.file) {
        Ok(identity) if identity == parent.identity => {}
        Ok(_) => return receipt(CleanupState::Unknown, None, first.and_then(native_code)),
        Err(error) => {
            return receipt(
                CleanupState::Unknown,
                None,
                first.or(Some(&error)).and_then(native_code),
            );
        }
    }
    let observed = child::observe(&parent.file, &name).and_then(|file| handle::identity(&file));
    match observed {
        Err(error)
            if marked.is_ok()
                && closed.is_ok()
                && error
                    .get_ref()
                    .and_then(|cause| cause.downcast_ref::<NtStatusError>())
                    .is_some_and(|cause| cause.nt_status() == STATUS_OBJECT_NAME_NOT_FOUND) =>
        {
            receipt(CleanupState::Complete, None, None)
        }
        Ok(identity) if expected.is_some_and(|owned| owned == identity) => receipt(
            CleanupState::Residual,
            Some(name),
            first.and_then(native_code),
        ),
        Err(error) => receipt(
            CleanupState::Unknown,
            None,
            first.or(Some(&error)).and_then(native_code),
        ),
        Ok(_) => receipt(CleanupState::Unknown, None, first.and_then(native_code)),
    }
}
impl WritableDirectory {
    /// Bind the caller's already-resolved ordinary root expression for modification.
    /// This does not resolve links, certify ancestry or qualify the requested address.
    /// The earlier admitted full identity must match before any child creation.
    pub fn bind_resolved(path: &Path, expected: Identity) -> io::Result<Self> {
        let file = handle::open(
            path,
            ROOT_WRITE_ACCESS,
            ROOT_WRITE_SHARING,
            ROOT_WRITE_FLAGS,
        )?;
        let identity = handle::directory_identity(&file)?;
        if identity != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Writable directory differs from the admitted root",
            ));
        }
        Ok(Self(BoundDirectory { file, identity }))
    }
    /// Usable full native identity of this held directory.
    pub fn identity(&self) -> Identity {
        self.0.identity()
    }
    /// Request actual namespace synchronization through the write-capable handle.
    pub fn sync_namespace(&self) -> SyncReceipt {
        flush(&self.0.file)
    }
    /// Exclusively create one empty inherited-ACL directory under this held parent.
    /// This primitive does not qualify hostile-parent confinement or publication.
    pub fn create_child(&self, name: &OsStr) -> Result<CreatedDirectory, CreationFailure> {
        let owned = self.create_entry(name, CreateKind::Directory)?;
        let identity = match handle::directory_identity(&owned.file) {
            Ok(identity) => identity,
            Err(error) => {
                return Err(CreationFailure {
                    error,
                    cleanup: owned.cleanup(None),
                });
            }
        };
        Ok(CreatedDirectory {
            directory: Self(BoundDirectory {
                file: owned.file,
                identity,
            }),
            parent: owned.parent,
            name: owned.name,
        })
    }
    pub(super) fn create_file(
        &self,
        name: &OsStr,
    ) -> Result<(Creation, Identity), CreationFailure> {
        let owned = self.create_entry(name, CreateKind::File)?;
        let initial = handle::signature(&owned.file).map_err(crate::BoundFileError::into_io_error);
        let identity = match initial {
            Ok(signature) if signature.size == 0 => signature.identity,
            Ok(signature) => {
                return Err(CreationFailure {
                    error: io::Error::new(
                        io::ErrorKind::InvalidData,
                        "New candidate is not initially empty",
                    ),
                    cleanup: owned.cleanup(Some(signature.identity)),
                });
            }
            Err(error) => {
                return Err(CreationFailure {
                    error,
                    cleanup: owned.cleanup(None),
                });
            }
        };
        if let Err(error) = owned.validate_file(identity, 0) {
            return Err(CreationFailure {
                error,
                cleanup: owned.cleanup(Some(identity)),
            });
        }
        Ok((owned, identity))
    }
    fn create_entry(&self, name: &OsStr, kind: CreateKind) -> Result<Creation, CreationFailure> {
        validate_component(name, u16::MAX as usize / 2).map_err(no_creation)?;
        let mut units: Vec<u16> = name.encode_wide().collect();
        let length = u16::try_from(units.len() * 2).map_err(|_| {
            no_creation(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Native staging component is too long",
            ))
        })?;
        if handle::directory_identity(&self.0.file).map_err(no_creation)? != self.identity() {
            return Err(no_creation(io::Error::new(
                io::ErrorKind::InvalidData,
                "Creation parent identity changed",
            )));
        }
        // This duplicate is retained only for metadata/flush/cleanup. There is
        // no enumeration API claiming a second independent file-object cursor.
        let parent = ParentBinding {
            file: self.0.file.try_clone().map_err(no_creation)?,
            identity: self.identity(),
        };
        let native_name = UNICODE_STRING {
            Length: length,
            MaximumLength: length,
            Buffer: units.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: self.0.file.as_raw_handle(),
            ObjectName: &native_name,
            Attributes: OBJ_DONT_REPARSE,
            ..Default::default()
        };
        let mut raw = std::ptr::null_mut();
        let mut iosb = IO_STATUS_BLOCK::default();
        // SAFETY: one bounded validated relative component and live held parent;
        // generated aligned buffers/name remain live through synchronous
        // IRP_MJ_CREATE completion. Fixed CREATE/type/NONALERT options,
        // null descriptor for native inheritance, no caller-controlled flags.
        let (access, file_attributes, sharing, options) = match kind {
            CreateKind::Directory => (
                FILE_GENERIC_WRITE
                    | FILE_LIST_DIRECTORY
                    | FILE_READ_ATTRIBUTES
                    | FILE_TRAVERSE
                    | SYNCHRONIZE
                    | DELETE,
                FILE_ATTRIBUTE_DIRECTORY,
                FILE_SHARE_READ,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            ),
            CreateKind::File => (
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
                FILE_ATTRIBUTE_NORMAL,
                0,
                FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            ),
        };
        let result = unsafe {
            NtCreateFile(
                &mut raw,
                access,
                &attributes,
                &mut iosb,
                std::ptr::null(),
                file_attributes,
                sharing,
                FILE_CREATE,
                options,
                std::ptr::null(),
                0,
            )
        };
        if result < 0 {
            return Err(CreationFailure {
                error: status::error(result),
                cleanup: receipt(
                    if result == STATUS_OBJECT_NAME_COLLISION {
                        CleanupState::NotNeeded
                    } else {
                        CleanupState::Unknown
                    },
                    None,
                    None,
                ),
            });
        }
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(CreationFailure {
                error: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Successful create supplied no handle",
                ),
                cleanup: receipt(CleanupState::Unknown, None, None),
            });
        }
        // SAFETY: NT_SUCCESS transfers exactly one valid handle. This alone does
        // not establish authority to delete a newly created namespace object.
        let file = File::from(unsafe { OwnedHandle::from_raw_handle(raw) });
        if iosb.Information != FILE_CREATED as usize {
            let cleanup_code = close(file).err().as_ref().and_then(native_code);
            return Err(CreationFailure {
                error: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Native disposition did not prove new object ownership",
                ),
                cleanup: receipt(CleanupState::Unknown, None, cleanup_code),
            });
        }
        Ok(Creation {
            file,
            parent,
            name: name.to_owned(),
        })
    }
}
enum CreateKind {
    Directory,
    File,
}
/// Private creation owner, issued only after native FILE_CREATED proof.
pub(super) struct Creation {
    pub(super) file: File,
    parent: ParentBinding,
    name: OsString,
}
impl Creation {
    pub(super) fn validate_file(&self, identity: Identity, length: u64) -> io::Result<()> {
        if handle::directory_identity(&self.parent.file)? != self.parent.identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Candidate parent identity changed",
            ));
        }
        let current =
            handle::signature(&self.file).map_err(crate::BoundFileError::into_io_error)?;
        let named = child::observe(&self.parent.file, &self.name)?;
        let named = handle::signature(&named).map_err(crate::BoundFileError::into_io_error)?;
        if current.identity != identity
            || current.size != length
            || named.identity != identity
            || named.size != length
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Owned candidate identity, entry or length changed",
            ));
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn name(&self) -> &OsStr {
        &self.name
    }
    pub(super) fn cleanup(self, identity: Option<Identity>) -> CleanupReceipt {
        remove(self.file, self.parent, self.name, identity)
    }
}
impl CreatedDirectory {
    /// Borrow the created directory's existing write capability; do not reopen it.
    pub fn directory(&self) -> &WritableDirectory {
        &self.directory
    }
    /// Synchronize the containing directory independently of child synchronization.
    pub fn sync_parent_namespace(&self) -> SyncReceipt {
        flush(&self.parent.file)
    }
    /// Keep an intended output directory, closing only the retained parent handle.
    pub fn persist(self) -> WritableDirectory {
        self.directory
    }
    /// Attempt only owned empty-handle removal, retaining observed cleanup truth.
    pub fn remove_empty(self) -> CleanupReceipt {
        let identity = self.directory.identity();
        remove(
            self.directory.0.file,
            self.parent,
            self.name,
            Some(identity),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        test_actor::RestrictedToken,
        test_fixture::{directory as fixture, operation},
    };
    use super::*;

    #[test]
    fn native_empty_directory_create_flush_cleanup_requires_current_root_and_earlier_identity() {
        let fixture = fixture();
        let selected = fixture.path().join("selected");
        let moved = fixture.path().join("moved");
        std::fs::create_dir(&selected).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(&selected)).unwrap();
        let expected_parent = read.identity();
        drop(actor);
        std::fs::rename(&selected, &moved).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        // A vanished old expression is refused. The current operand is an
        // explicit separate fixture request, never a product discovery/fallback.
        assert!(WritableDirectory::bind_resolved(&operation(&selected), expected_parent).is_err());
        let writer = WritableDirectory::bind_resolved(&operation(&moved), expected_parent).unwrap();
        assert_eq!(writer.identity(), expected_parent);
        let created = writer.create_child(OsStr::new("created")).unwrap();
        let identity = created.directory().identity();
        assert_eq!(
            read.observe_child(OsStr::new("created")).unwrap().identity,
            identity
        );
        assert!(moved.join("created").try_exists().unwrap());
        assert!(!selected.try_exists().unwrap());
        let child_sync = created.directory().sync_namespace();
        let parent_sync = created.sync_parent_namespace();
        assert_eq!(child_sync.state, SyncState::Succeeded, "{child_sync:?}");
        assert_eq!(parent_sync.state, SyncState::Succeeded, "{parent_sync:?}");
        assert_eq!(child_sync.method, SyncMethod::FlushFileBuffers);
        let cleanup = created.remove_empty();
        assert_eq!(cleanup.state, CleanupState::Complete, "{cleanup:?}");
        assert!(cleanup.residual_name.is_none());
        assert!(!moved.join("created").try_exists().unwrap());
        assert_eq!(writer.sync_namespace().state, SyncState::Succeeded);
        drop(writer);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
    #[test]
    fn native_directory_collision_and_invalid_components_do_not_modify_existing_entries() {
        let fixture = fixture();
        std::fs::create_dir(fixture.path().join("collision")).unwrap();
        std::fs::create_dir(fixture.path().join("other")).unwrap();
        let alias = fixture.path().join("selected-link");
        std::fs::create_dir(&alias).unwrap();
        let link = super::super::test_reparse::writer(&alias).unwrap();
        super::super::test_reparse::set_mount_point(&link, &fixture.path().join("other")).unwrap();
        drop(link);
        std::fs::write(
            fixture.path().join("collision/marker"),
            b"synthetic unchanged",
        )
        .unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let other = read.observe_child(OsStr::new("other")).unwrap();
        assert!(
            WritableDirectory::bind_resolved(&operation(fixture.path()), other.identity).is_err()
        );
        let selected_link = BoundDirectory::open(&operation(&alias)).unwrap();
        assert_eq!(selected_link.identity(), other.identity);
        assert!(
            WritableDirectory::bind_resolved(&operation(&alias), selected_link.identity()).is_err()
        );
        // The target operand is independently known from fixture setup. This
        // does not simulate GetFinalPathName or full selected-root admission.
        let resolved = WritableDirectory::bind_resolved(
            &operation(&fixture.path().join("other")),
            selected_link.identity(),
        )
        .unwrap();
        let linked = resolved
            .create_child(OsStr::new("from-resolved-target"))
            .unwrap();
        assert_eq!(linked.remove_empty().state, CleanupState::Complete);
        drop(resolved);
        drop(selected_link);
        let writer =
            WritableDirectory::bind_resolved(&operation(fixture.path()), read.identity()).unwrap();
        let existing = read.observe_child(OsStr::new("collision")).unwrap();
        let conflict = writer
            .create_child(OsStr::new("collision"))
            .err()
            .expect("collision accepted");
        assert_eq!(conflict.error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(conflict.cleanup.state, CleanupState::NotNeeded);
        assert_eq!(
            read.observe_child(OsStr::new("collision")).unwrap(),
            existing
        );
        assert_eq!(
            std::fs::read(fixture.path().join("collision/marker")).unwrap(),
            b"synthetic unchanged"
        );
        for name in ["", ".", "..", "a/b", r"a\b", "a:b", "NUL", "a\0b"] {
            let refused = writer
                .create_child(OsStr::new(name))
                .err()
                .expect("invalid component accepted");
            assert_eq!(refused.error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(refused.cleanup.state, CleanupState::NotNeeded);
        }
        let actual: std::collections::BTreeSet<_> = std::fs::read_dir(fixture.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            actual,
            std::collections::BTreeSet::from([
                OsString::from("collision"),
                OsString::from("other"),
                OsString::from("selected-link"),
            ])
        );
        drop(writer);
        drop(read);
        drop(actor);
        std::fs::remove_dir(alias).unwrap();
        fixture.close().unwrap();
    }
    #[test]
    fn native_nonempty_directory_cleanup_retains_the_owned_entry_and_never_recurses() {
        let fixture = fixture();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let writer =
            WritableDirectory::bind_resolved(&operation(fixture.path()), read.identity()).unwrap();
        let parent = writer.create_child(OsStr::new("parent")).unwrap();
        let expected = parent.directory().identity();
        let leaf = parent.directory().create_child(OsStr::new("leaf")).unwrap();
        let leaf_id = leaf.directory().identity();
        let residual = parent.remove_empty();
        assert_eq!(residual.state, CleanupState::Residual, "{residual:?}");
        assert_eq!(
            residual.residual_name.as_deref(),
            Some(OsStr::new("parent"))
        );
        assert_eq!(
            residual.native_code,
            Some(145),
            "nonempty refusal must not be an unrelated permission failure"
        );
        assert_eq!(
            read.observe_child(OsStr::new("parent")).unwrap().identity,
            expected
        );
        let retained = read.open_child(OsStr::new("parent")).unwrap();
        assert_eq!(
            retained.observe_child(OsStr::new("leaf")).unwrap().identity,
            leaf_id
        );
        drop(retained);
        assert_eq!(leaf.remove_empty().state, CleanupState::Complete);
        // Product cleanup deliberately retained parent. Harness teardown later
        // removes its entire synthetic fixture after every handle/actor stops.
        assert!(fixture.path().join("parent").try_exists().unwrap());
        drop(writer);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
}
