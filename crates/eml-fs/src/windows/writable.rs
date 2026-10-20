//! Contained file creation and unowned mirror binding; cleanup authority stays explicit.
use super::{child, containment, handle};
use crate::{
    BoundDirectory, CleanupReceipt, CleanupState, CreationFailure, EntryKind, Identity,
    NtStatusError, SyncMethod, SyncReceipt, SyncState,
    staging::{NAME_ATTEMPTS, random_name},
};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE, STATUS_OBJECT_NAME_NOT_FOUND},
    Storage::FileSystem::*,
};

/// A held ordinary directory with creation/flush access, but no address qualification by itself.
pub struct WritableDirectory(BoundDirectory);
const ROOT_WRITE_ACCESS: u32 =
    GENERIC_WRITE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
const ROOT_WRITE_SHARING: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;
const ROOT_WRITE_FLAGS: u32 = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
struct ParentBinding {
    file: File,
    identity: Identity,
}
/// Ordinary mirror bound after a successful Boolean directory-creation call.
/// The later open does not prove ownership, so this value grants no deletion authority.
pub struct DirectoryCreation {
    directory: WritableDirectory,
    parent: ParentBinding,
    guard_cleanup: CleanupReceipt,
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
    // SAFETY: live borrowed handle. No buffer/ownership is transferred.
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
fn remove(
    file: File,
    parent: ParentBinding,
    name: OsString,
    expected: Option<Identity>,
) -> CleanupReceipt {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: only a known owned file (or an explicitly owned test fixture) with
    // DELETE access. The aligned generated buffer lives through the call.
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
    let closed = handle::close(file);
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
        Ok(identity) if expected == Some(identity) => receipt(
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
    /// Actual native component limit; no fallback on query failure.
    pub fn name_limit(&self) -> io::Result<usize> {
        self.0.name_limit()
    }
    /// No-follow metadata only; a missing name is not permission to create.
    pub fn observe_child_if_present(
        &self,
        name: &OsStr,
    ) -> io::Result<Option<crate::EntryObservation>> {
        self.0.observe_child_if_present(name)
    }
    /// Retain exact object lifetime across candidate release without data access.
    pub fn pin_child(&self, name: &OsStr) -> io::Result<super::EntryPin> {
        self.0.pin_child(name)
    }
    /// Open an existing ordinary writable child with its earlier full identity.
    pub fn open_existing_child(&self, name: &OsStr, expected: Identity) -> io::Result<Self> {
        Self::open_under(&self.0, name, expected)
    }
    pub(super) fn open_under(
        parent: &BoundDirectory,
        name: &OsStr,
        expected: Identity,
    ) -> io::Result<Self> {
        if handle::directory_identity(&parent.file)? != parent.identity {
            return Err(containment::changed());
        }
        let file = child::writable_directory(&parent.file, name)?;
        let identity = handle::directory_identity(&file)?;
        if identity != expected {
            return Err(containment::changed());
        }
        Ok(Self(BoundDirectory { file, identity }))
    }
    /// Reobserve the held ordinary/full-ID fact; not continuous metadata immutability.
    pub fn check_held(&self) -> io::Result<()> {
        if handle::directory_identity(&self.0.file)? == self.0.identity {
            Ok(())
        } else {
            Err(containment::changed())
        }
    }
    /// Bind an already-resolved no-follow directory for mutation; ancestry is separate.
    pub fn bind_resolved(path: &Path, expected: Identity) -> io::Result<Self> {
        let file = handle::open(
            path,
            ROOT_WRITE_ACCESS,
            ROOT_WRITE_SHARING,
            ROOT_WRITE_FLAGS,
        )?;
        let identity = handle::directory_identity(&file)?;
        if identity != expected {
            return Err(containment::changed());
        }
        Ok(Self(BoundDirectory { file, identity }))
    }
    /// Full identity of the held directory.
    pub fn identity(&self) -> Identity {
        self.0.identity()
    }
    /// Actual namespace synchronization through the held writable handle.
    pub fn sync_namespace(&self) -> SyncReceipt {
        flush(&self.0.file)
    }
    /// Create/bind one mirror while an owned zero-byte entry protects its parent.
    /// This operation observes creation, but cannot prove opened-mirror ownership.
    pub fn create_child(&self, name: &OsStr) -> Result<DirectoryCreation, CreationFailure> {
        super::path::validate_component(name, u16::MAX as usize / 2).map_err(no_creation)?;
        let (guard, guard_identity) = self.occupancy_guard()?;
        let mut may_have_created = false;
        let result = (|| -> io::Result<(Self, ParentBinding)> {
            let captured = containment::capture_parent(&self.0.file, self.identity())?;
            let mut units = containment::append(&captured, name)?;
            units.push(0);
            let parent = ParentBinding {
                file: self.0.file.try_clone()?,
                identity: self.identity(),
            };
            #[cfg(any(test, feature = "native-fixtures"))]
            super::creation_observer::notify(
                super::creation_observer::CreationBoundary::BeforeMirrorCreate,
                &self.0.file,
                name,
                None,
            );
            // SAFETY: terminated bounded native path. Null security attributes
            // select inheritance. The contained guard remains alive throughout
            // create/open/qualification, keeping this pinned parent nonempty.
            may_have_created = true;
            if unsafe { CreateDirectoryW(units.as_ptr(), std::ptr::null()) } == 0 {
                let error = io::Error::last_os_error();
                if crate::name_collision(&error) {
                    may_have_created = false;
                }
                return Err(error);
            }
            #[cfg(any(test, feature = "native-fixtures"))]
            super::creation_observer::notify(
                super::creation_observer::CreationBoundary::MirrorCreated,
                &self.0.file,
                name,
                None,
            );
            let path = std::path::PathBuf::from(OsString::from_wide(&units[..units.len() - 1]));
            let file = handle::open(
                &path,
                ROOT_WRITE_ACCESS,
                ROOT_WRITE_SHARING,
                ROOT_WRITE_FLAGS,
            )?;
            #[cfg(any(test, feature = "native-fixtures"))]
            super::creation_observer::notify(
                super::creation_observer::CreationBoundary::MirrorOpened,
                &self.0.file,
                name,
                Some(&file),
            );
            containment::verify(
                &self.0.file,
                self.identity(),
                &captured,
                &file,
                name,
                EntryKind::Directory,
            )?;
            let identity = handle::directory_identity(&file)?;
            Ok((Self(BoundDirectory { file, identity }), parent))
        })();
        let cleanup = guard.cleanup(Some(guard_identity));
        let guard_complete =
            cleanup.state == CleanupState::Complete && cleanup.native_code.is_none();
        match result {
            Ok((directory, parent)) if guard_complete => Ok(DirectoryCreation {
                directory,
                parent,
                guard_cleanup: cleanup,
            }),
            Ok((directory, _parent)) => {
                let close = handle::close(directory.0.file).err();
                Err(CreationFailure {
                    error: io::Error::other("Owned occupancy guard cleanup is unverified"),
                    cleanup: receipt(
                        CleanupState::Unknown,
                        None,
                        cleanup
                            .native_code
                            .or_else(|| close.as_ref().and_then(native_code)),
                    ),
                })
            }
            Err(error) => Err(CreationFailure {
                error,
                cleanup: if may_have_created {
                    receipt(CleanupState::Unknown, None, cleanup.native_code)
                } else {
                    cleanup
                },
            }),
        }
    }
    fn occupancy_guard(&self) -> Result<(Creation, Identity), CreationFailure> {
        let mut last = None;
        for _ in 0..NAME_ATTEMPTS {
            let guard_name = random_name().map_err(no_creation)?;
            match self.create_file(&guard_name) {
                Ok(value) => return Ok(value),
                Err(error) if error.is_name_collision() => last = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last.expect("positive bounded guard attempts record a collision"))
    }
    pub(super) fn create_file(
        &self,
        name: &OsStr,
    ) -> Result<(Creation, Identity), CreationFailure> {
        super::path::validate_component(name, u16::MAX as usize / 2).map_err(no_creation)?;
        let captured =
            containment::capture_parent(&self.0.file, self.identity()).map_err(no_creation)?;
        let mut units = containment::append(&captured, name).map_err(no_creation)?;
        units.push(0);
        let parent = ParentBinding {
            file: self.0.file.try_clone().map_err(no_creation)?,
            identity: self.identity(),
        };
        #[cfg(any(test, feature = "native-fixtures"))]
        super::creation_observer::notify(
            super::creation_observer::CreationBoundary::BeforeFileCreate,
            &self.0.file,
            name,
            None,
        );
        // SAFETY: one bounded terminated native path, fixed CREATE_NEW, no
        // sharing/inherited handle. Success returns the exact newly created file.
        let raw = unsafe {
            CreateFileW(
                units.as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
                0,
                std::ptr::null(),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(no_creation(io::Error::last_os_error()));
        }
        if raw.is_null() {
            return Err(CreationFailure {
                error: io::Error::other("Successful exclusive create supplied no handle"),
                cleanup: receipt(CleanupState::Unknown, None, None),
            });
        }
        // SAFETY: successful exclusive CreateFileW transferred one valid handle.
        let file = File::from(unsafe { OwnedHandle::from_raw_handle(raw) });
        let mut owned = Creation {
            file,
            parent,
            name: name.to_owned(),
            contained: false,
        };
        #[cfg(any(test, feature = "native-fixtures"))]
        owned.observe(super::creation_observer::CreationBoundary::FileOpened);
        let mut identity = None;
        let result = (|| {
            let signature =
                handle::signature(&owned.file).map_err(crate::BoundFileError::into_io_error)?;
            identity = Some(signature.identity);
            if signature.size != 0 {
                return Err(io::Error::other("New candidate is not initially empty"));
            }
            containment::verify(
                &self.0.file,
                self.identity(),
                &captured,
                &owned.file,
                name,
                EntryKind::File,
            )?;
            owned.contained = true;
            Ok(signature.identity)
        })();
        match result {
            Ok(identity) => Ok((owned, identity)),
            Err(error) => Err(CreationFailure {
                error,
                cleanup: owned.cleanup(identity),
            }),
        }
    }
}
/// An exact CREATE_NEW owner. Only this type grants staged-file deletion authority.
pub(super) struct Creation {
    pub(super) file: File,
    parent: ParentBinding,
    name: OsString,
    contained: bool,
}
impl Creation {
    #[cfg(any(test, feature = "native-fixtures"))]
    pub(super) fn observe(&self, boundary: super::creation_observer::CreationBoundary) {
        super::creation_observer::notify(boundary, &self.parent.file, &self.name, Some(&self.file));
    }
    pub(super) fn component_limit(&self) -> io::Result<usize> {
        handle::component_limit(&self.parent.file)
    }
    pub(super) fn parent_path(&self) -> io::Result<Vec<u16>> {
        containment::capture_parent(&self.parent.file, self.parent.identity)
    }
    pub(super) fn release(self) -> io::Result<()> {
        handle::close(self.file)
    }
    pub(super) fn validate_file(&self, identity: Identity, length: u64) -> io::Result<()> {
        if handle::directory_identity(&self.parent.file)? != self.parent.identity {
            return Err(containment::changed());
        }
        let current =
            handle::signature(&self.file).map_err(crate::BoundFileError::into_io_error)?;
        if current.identity != identity || current.size != length {
            return Err(io::Error::other(
                "Owned candidate identity, type or length changed",
            ));
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn name(&self) -> &OsStr {
        &self.name
    }
    pub(super) fn cleanup(self, identity: Option<Identity>) -> CleanupReceipt {
        let mut cleanup = remove(self.file, self.parent, self.name, identity);
        if !self.contained {
            // Absence in the intended parent does not prove removal of an
            // owned file whose actual containment was never established.
            cleanup.state = CleanupState::Unknown;
            cleanup.residual_name = None;
        }
        cleanup
    }
}
impl DirectoryCreation {
    /// Borrow the ordinary writable binding; this grants no deletion authority.
    pub fn directory(&self) -> &WritableDirectory {
        &self.directory
    }
    /// Actual owned guard removal/release observation before this binding returned.
    pub fn guard_cleanup(&self) -> &CleanupReceipt {
        &self.guard_cleanup
    }
    /// Synchronize the containing parent, independently of the child and ownership.
    pub fn sync_parent_namespace(&self) -> SyncReceipt {
        flush(&self.parent.file)
    }
    /// Keep the intended output directory, closing the retained observation parent.
    pub fn persist(self) -> WritableDirectory {
        self.directory
    }
    /// Release without deletion. Creation/open did not establish directory ownership.
    /// No location, emptiness or residual component is guessed.
    pub fn release_unverified(self) -> CleanupReceipt {
        let child = handle::close(self.directory.0.file);
        let parent = handle::close(self.parent.file);
        receipt(
            CleanupState::Unknown,
            None,
            child
                .err()
                .or_else(|| parent.err())
                .as_ref()
                .and_then(native_code),
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
    fn owned_fixture_nonempty_disposition_reports_actual_error_without_recursing() {
        let fixture = fixture();
        let actor = RestrictedToken::enter().unwrap();
        let name = OsStr::new("owned-directory");
        std::fs::create_dir(fixture.path().join(name)).unwrap();
        std::fs::write(
            fixture.path().join(name).join("marker"),
            b"keep this owned fixture subtree",
        )
        .unwrap();
        let parent = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let file = handle::open(
            &operation(&fixture.path().join(name)),
            ROOT_WRITE_ACCESS | DELETE,
            ROOT_WRITE_SHARING,
            ROOT_WRITE_FLAGS,
        )
        .unwrap();
        let identity = handle::identity(&file).unwrap();
        // The harness owns this entire private fixture and runs no namespace
        // competitor. This tests the native primitive, not mirror ownership.
        let cleanup = remove(
            file,
            ParentBinding {
                file: parent.file.try_clone().unwrap(),
                identity: parent.identity(),
            },
            name.to_owned(),
            Some(identity),
        );
        assert_eq!(cleanup.state, CleanupState::Residual);
        assert_eq!(cleanup.native_code, Some(145));
        assert_eq!(cleanup.residual_name.as_deref(), Some(name));
        assert_eq!(
            std::fs::read(fixture.path().join(name).join("marker")).unwrap(),
            b"keep this owned fixture subtree"
        );
        drop(parent);
        drop(actor);
        fixture.close().unwrap();
    }

    #[test]
    fn native_mirror_binding_flushes_and_releases_without_unproved_deletion() {
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
        assert_eq!(created.guard_cleanup().state, CleanupState::Complete);
        let cleanup = created.release_unverified();
        assert_eq!(cleanup.state, CleanupState::Unknown, "{cleanup:?}");
        assert!(cleanup.residual_name.is_none());
        assert!(moved.join("created").try_exists().unwrap());
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
        assert_eq!(linked.release_unverified().state, CleanupState::Unknown);
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
        assert_eq!(conflict.cleanup.state, CleanupState::Complete);
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
    fn unowned_nonempty_mirror_release_preserves_entries_without_deletion() {
        let fixture = fixture();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let writer =
            WritableDirectory::bind_resolved(&operation(fixture.path()), read.identity()).unwrap();
        let parent = writer.create_child(OsStr::new("parent")).unwrap();
        let expected = parent.directory().identity();
        let leaf = parent.directory().create_child(OsStr::new("leaf")).unwrap();
        let leaf_id = leaf.directory().identity();
        let residual = parent.release_unverified();
        assert_eq!(residual.state, CleanupState::Unknown, "{residual:?}");
        assert!(residual.residual_name.is_none());
        assert!(residual.native_code.is_none());
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
        assert_eq!(leaf.release_unverified().state, CleanupState::Unknown);
        // Unowned binding release deliberately retained these entries. Harness teardown later
        // removes its entire synthetic fixture after every handle/actor stops.
        assert!(fixture.path().join("parent").try_exists().unwrap());
        drop(writer);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
}
