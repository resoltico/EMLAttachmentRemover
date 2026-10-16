//! Owned empty directories. No candidate bytes, ACL rewriting or publication.
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
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
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
fn flush(file: &File) -> SyncReceipt {
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
    // SAFETY: positively created owned directory handle with DELETE access;
    // generated fixed buffer remains live through the synchronous call. Native
    // directory deletion enforces emptiness; this never recursively removes data.
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
    let observed =
        child::observe(&parent.file, &name).and_then(|file| handle::entry_observation(&file));
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
        Ok(entry) if expected.is_some_and(|identity| identity == entry.identity) => receipt(
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
    /// Bind an admitted selected-root expression for modification. The caller
    /// supplies its earlier full identity; a different target is refused before creation.
    pub fn open_selected(path: &Path, expected: Identity) -> io::Result<Self> {
        // This private reopen call site is known to hold a CreateFileW-selected
        // root. It is not exposed as conversion of arbitrary NT child handles.
        let root = BoundDirectory::open(path)?;
        Self::reopen_selected_root(&root, expected)
    }
    fn reopen_selected_root(root: &BoundDirectory, expected: Identity) -> io::Result<Self> {
        if root.identity() != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Selected directory identity changed",
            ));
        }
        // SAFETY: live CreateFileW-selected ordinary directory. Fixed explicit
        // access/share/flags; no path, inheritance, template or privilege change.
        let raw = unsafe {
            ReOpenFile(
                root.file.as_raw_handle(),
                ROOT_WRITE_ACCESS,
                ROOT_WRITE_SHARING,
                ROOT_WRITE_FLAGS,
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            let error = io::Error::last_os_error();
            #[cfg(test)]
            eprintln!("Writable-parent native ReOpenFile failed before metadata: {error:?}");
            return Err(error);
        }
        if raw.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Writable reopen supplied no handle",
            ));
        }
        // SAFETY: successful ReOpenFile transferred one uniquely owned handle.
        let file = File::from(unsafe { OwnedHandle::from_raw_handle(raw) });
        #[cfg(test)]
        eprintln!("Writable-parent native ReOpenFile returned an owned handle");
        let observed = handle::directory_identity(&file);
        #[cfg(test)]
        eprintln!("Writable-parent post-ReOpenFile directory metadata: {observed:?}");
        let identity = observed?;
        if identity != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Writable directory identity changed",
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
        validate_component(name, u16::MAX as usize / 2).map_err(no_creation)?;
        let mut units: Vec<u16> = name.encode_wide().collect();
        let length = u16::try_from(units.len() * 2).map_err(|_| {
            no_creation(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Native directory name is too long",
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
        // IRP_MJ_CREATE completion. Fixed CREATE/DIRECTORY/NONALERT options,
        // null descriptor for native inheritance, no caller-controlled flags.
        let result = unsafe {
            NtCreateFile(
                &mut raw,
                FILE_GENERIC_WRITE
                    | FILE_LIST_DIRECTORY
                    | FILE_READ_ATTRIBUTES
                    | FILE_TRAVERSE
                    | SYNCHRONIZE
                    | DELETE,
                &attributes,
                &mut iosb,
                std::ptr::null(),
                FILE_ATTRIBUTE_DIRECTORY,
                FILE_SHARE_READ,
                FILE_CREATE,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
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
                    "Successful directory create supplied no handle",
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
                    "Native disposition did not prove new directory ownership",
                ),
                cleanup: receipt(CleanupState::Unknown, None, cleanup_code),
            });
        }
        // Exact FILE_CREATED now proves ownership. Even a failed ID observation
        // retains handle-only empty cleanup; never delete a fresh pathname.
        let identity = match handle::directory_identity(&file) {
            Ok(identity) => identity,
            Err(error) => {
                return Err(CreationFailure {
                    error,
                    cleanup: remove(file, parent, name.to_owned(), None),
                });
            }
        };
        Ok(CreatedDirectory {
            directory: Self(BoundDirectory { file, identity }),
            parent,
            name: name.to_owned(),
        })
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
    use super::super::test_actor::{RestrictedToken, information, require_unimpersonated_thread};
    use super::*;

    /// Prepare only a freshly owned test directory so the verified runner UserSID
    /// has real creation/flush/cleanup access without depending on checkout ACLs.
    /// This is fixture setup, not product ACL policy or a mutation of an owner folder.
    fn grant_current_user_directory(path: &std::path::Path) -> io::Result<()> {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::*, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
                SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, TokenUser,
            },
            Storage::FileSystem::FILE_ALL_ACCESS,
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        require_unimpersonated_thread()?;
        let mut raw = ptr::null_mut();
        // SAFETY: live process pseudo-handle and writable output slot; query only.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful call transferred one owned process-token handle.
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let info = information(&token, TokenUser)?;
        if info.bytes < size_of::<TOKEN_USER>() {
            return Err(io::Error::other("Short fixture token-user information"));
        }
        // SAFETY: generated native header is present/aligned; its SID points into
        // the successful native token information kept alive through ACL construction.
        let sid = unsafe { (*(info.words.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        if sid.is_null() {
            return Err(io::Error::other("Missing fixture UserSID"));
        }
        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: sid.cast(),
                ..Default::default()
            },
        };
        let mut acl = ptr::null_mut();
        // SAFETY: one fully initialized generated entry with live SID; the system
        // returns a separately allocated ACL, and no prior ACL pointer is borrowed.
        let built = unsafe { SetEntriesInAclW(1, &entry, ptr::null(), &mut acl) };
        if built != 0 {
            return Err(io::Error::from_raw_os_error(built as i32));
        }
        if acl.is_null() {
            return Err(io::Error::other("Fixture ACL builder supplied no ACL"));
        }
        let mut native: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: terminated fresh owned fixture path; live constructed ACL. Only
        // this synthetic directory's DACL changes, before any restricted actor enters.
        let applied = unsafe {
            SetNamedSecurityInfoW(
                native.as_mut_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            )
        };
        // SAFETY: the successful ACL builder allocated this exact pointer once.
        let released = unsafe { LocalFree(acl.cast()) };
        if applied != 0 {
            return Err(io::Error::from_raw_os_error(applied as i32));
        }
        if !released.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        grant_current_user_directory(root.path()).unwrap();
        root
    }
    fn operation(path: &Path) -> std::path::PathBuf {
        crate::PathContext::capture()
            .unwrap()
            .resolve(path)
            .unwrap()
    }
    fn permission_control(path: &Path, expected: Identity) -> io::Result<Identity> {
        // Independent native API with the EXACT same access/share/flags. This
        // OPEN_EXISTING diagnostic creates nothing and is never a fallback.
        let opened = handle::open(
            &operation(path),
            ROOT_WRITE_ACCESS,
            ROOT_WRITE_SHARING,
            ROOT_WRITE_FLAGS,
        );
        eprintln!(
            "Direct CreateFileW writable permission control open: {:?}",
            opened.as_ref().map(|_| ())
        );
        let file = opened?;
        let observed = handle::directory_identity(&file);
        eprintln!("Direct CreateFileW writable permission control metadata: {observed:?}");
        let identity = observed?;
        assert_eq!(identity, expected);
        // The diagnostic handle closes before the tested acquisition, so it
        // cannot manufacture a self-sharing failure in ReOpenFile.
        Ok(identity)
    }
    #[test]
    fn native_empty_directory_creation_flush_and_checked_cleanup_use_the_held_moved_root() {
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
        // This private helper's input was explicitly selected by CreateFileW.
        // Movement precedes the strict writable pin, not while claiming exclusion.
        let permission = permission_control(&moved, expected_parent);
        eprintln!("Held-moved-root permission control result: {permission:?}");
        let writer = WritableDirectory::reopen_selected_root(&read, expected_parent).unwrap();
        assert_eq!(permission.unwrap(), expected_parent);
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
        std::fs::write(
            fixture.path().join("collision/marker"),
            b"synthetic unchanged",
        )
        .unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let other = read.observe_child(OsStr::new("other")).unwrap();
        assert!(
            WritableDirectory::open_selected(&operation(fixture.path()), other.identity).is_err()
        );
        let permission = permission_control(fixture.path(), read.identity());
        eprintln!("Collision fixture permission control result: {permission:?}");
        let writer =
            WritableDirectory::open_selected(&operation(fixture.path()), read.identity()).unwrap();
        assert_eq!(permission.unwrap(), read.identity());
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
                OsString::from("other")
            ])
        );
        drop(writer);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
    #[test]
    fn native_nonempty_directory_cleanup_retains_the_owned_entry_and_never_recurses() {
        let fixture = fixture();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let permission = permission_control(fixture.path(), read.identity());
        eprintln!("Cleanup fixture permission control result: {permission:?}");
        let writer =
            WritableDirectory::open_selected(&operation(fixture.path()), read.identity()).unwrap();
        assert_eq!(permission.unwrap(), read.identity());
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
