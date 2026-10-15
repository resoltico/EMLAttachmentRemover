//! Safe rustix descriptors and native namespace facts, without MIME/naming decisions.
use crate::{CleanupReceipt, CleanupState, CreationFailure, Identity};
use rustix::fs::{self, Mode, OFlags};
use std::{
    ffi::{OsStr, OsString},
    io,
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    path::{Path, PathBuf},
    sync::Arc,
};

/// A held ordinary directory with its native requested address and admission principal.
#[derive(Clone)]
pub struct BoundDirectory {
    fd: Arc<OwnedFd>,
    path: PathBuf,
    identity: Identity,
    uid: u32,
}
pub(crate) fn component(name: &OsStr) -> io::Result<()> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes == b"."
        || bytes == b".."
        || bytes.contains(&0)
        || bytes.contains(&b'/')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "A bound operation requires a native single component",
        ));
    }
    Ok(())
}
#[allow(clippy::unnecessary_cast)] // Darwin dev_t is signed32; Linux dev_t is unsigned64.
pub(crate) fn identity(stat: &fs::Stat) -> io::Result<Identity> {
    usable_identity(stat.st_dev as u64, stat.st_ino)
}
fn usable_identity(device: u64, inode: u64) -> io::Result<Identity> {
    if inode == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "The filesystem supplied no usable object identity",
        ));
    }
    Ok(Identity::Posix { device, inode })
}
fn no_creation(error: io::Error) -> CreationFailure {
    CreationFailure {
        error,
        cleanup: CleanupReceipt {
            state: CleanupState::NotNeeded,
            residual_name: None,
            native_code: None,
        },
    }
}
fn unknown_creation(error: io::Error) -> CreationFailure {
    CreationFailure {
        error,
        cleanup: CleanupReceipt {
            state: CleanupState::Unknown,
            residual_name: None,
            native_code: None,
        },
    }
}
fn remove_known_empty(parent: &BoundDirectory, name: &OsStr, expected: Identity) -> CleanupReceipt {
    let failure = |state, name, error: Option<io::Error>| CleanupReceipt {
        state,
        residual_name: name,
        native_code: error.and_then(|error| error.raw_os_error()).map(i64::from),
    };
    if let Err(error) = parent.admit_namespace() {
        return failure(CleanupState::Unknown, None, Some(error));
    }
    let entry = match fs::statat(parent, name, fs::AtFlags::SYMLINK_NOFOLLOW) {
        Ok(entry) => entry,
        Err(error) => return failure(CleanupState::Unknown, None, Some(error.into())),
    };
    if identity(&entry).ok() != Some(expected)
        || fs::FileType::from_raw_mode(entry.st_mode) != fs::FileType::Directory
    {
        return failure(CleanupState::Unknown, None, None);
    }
    match fs::unlinkat(parent, name, fs::AtFlags::REMOVEDIR) {
        Ok(()) => CleanupReceipt {
            state: CleanupState::Complete,
            residual_name: None,
            native_code: None,
        },
        Err(error) => {
            // A failed return alone cannot prove whether removal occurred.
            // Reobserve without deleting anything to qualify the old component.
            let still_named = fs::statat(parent, name, fs::AtFlags::SYMLINK_NOFOLLOW)
                .ok()
                .and_then(|stat| identity(&stat).ok())
                == Some(expected);
            failure(
                if still_named {
                    CleanupState::Residual
                } else {
                    CleanupState::Unknown
                },
                still_named.then(|| name.to_owned()),
                Some(error.into()),
            )
        }
    }
}
/// A newly created directory whose identity/parent are retained for explicit cleanup.
/// Dropping this value closes handles; it never recursively removes a pathname.
pub struct CreatedDirectory {
    directory: BoundDirectory,
    parent: BoundDirectory,
    name: OsString,
}
impl CreatedDirectory {
    /// Borrow the held directory; a caller cannot replace its ownership identity.
    pub fn directory(&self) -> &BoundDirectory {
        &self.directory
    }
    /// Keep an intended output directory instead of automatically deleting it.
    pub fn persist(self) -> BoundDirectory {
        self.directory
    }
    /// Remove only this observed owned empty entry under an admitted parent.
    pub fn remove_empty(self) -> CleanupReceipt {
        let mut receipt = remove_known_empty(&self.parent, &self.name, self.directory.identity);
        if receipt.state == CleanupState::Unknown
            && let Ok(stat) = fs::fstat(&self.directory)
        {
            receipt.state = if stat.st_nlink == 0 {
                CleanupState::Complete
            } else {
                CleanupState::Residual
            };
        }
        receipt
    }
}
impl BoundDirectory {
    pub(crate) fn check_private(&self) -> io::Result<()> {
        self.admit_namespace()?;
        let actual = fs::fstat(self)?;
        if actual.st_uid != self.uid || actual.st_mode & 0o777 != 0o700 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Owned staging directory is not private",
            ));
        }
        Ok(())
    }
    pub(crate) fn owner_uid(&self) -> u32 {
        self.uid
    }
    /// Request synchronization of this exact held directory namespace.
    pub fn sync_namespace(&self) -> crate::SyncReceipt {
        crate::candidate::portable_sync(self)
    }
    /// Compare the current requested address with the held directory. An error
    /// means unverified; a different identity means changed, never redirected I/O.
    pub fn check_requested_address(&self) -> io::Result<bool> {
        Ok(Self::open(&self.path)?.identity == self.identity)
    }
    /// Bind an absolute selected directory. Selected-root links follow normal OS semantics.
    pub fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "A directory binding requires an anchored absolute expression",
            ));
        }
        let fd = fs::open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let stat = fs::fstat(&fd)?;
        if stat.st_ino == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "The filesystem did not provide a usable directory identity",
            ));
        }
        let identity = identity(&stat)?;
        Ok(Self {
            fd: Arc::new(fd),
            path: path.to_owned(),
            identity,
            uid: rustix::process::geteuid().as_raw(),
        })
    }
    /// Open one existing child without following its final link.
    pub fn open_child(&self, name: &OsStr) -> io::Result<Self> {
        component(name)?;
        let observed = fs::statat(self, name, fs::AtFlags::SYMLINK_NOFOLLOW)?;
        let fd = fs::openat(
            self,
            name,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let held = fs::fstat(&fd)?;
        if identity(&observed)? != identity(&held)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Child directory changed while binding",
            ));
        }
        Ok(Self {
            fd: Arc::new(fd),
            path: self.path.join(name),
            identity: identity(&held)?,
            uid: self.uid,
        })
    }
    /// Create one owned private directory after namespace admission. Existing
    /// names are refused, never chmodded or stripped of ACLs.
    pub fn create_private_child(&self, name: &OsStr) -> Result<CreatedDirectory, CreationFailure> {
        component(name).map_err(no_creation)?;
        self.admit_namespace().map_err(no_creation)?;
        fs::mkdirat(self, name, Mode::from_raw_mode(0o700)).map_err(|error| {
            // Interrupted or I/O/remote results do not prove the server created
            // nothing. Never remove an unbound name to guess at reconciliation.
            if !matches!(
                error,
                rustix::io::Errno::EXIST
                    | rustix::io::Errno::ACCESS
                    | rustix::io::Errno::PERM
                    | rustix::io::Errno::ROFS
                    | rustix::io::Errno::NAMETOOLONG
                    | rustix::io::Errno::NOENT
                    | rustix::io::Errno::NOTDIR
                    | rustix::io::Errno::INVAL
            ) {
                unknown_creation(error.into())
            } else {
                no_creation(error.into())
            }
        })?;
        let observed = fs::statat(self, name, fs::AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| unknown_creation(error.into()))?;
        let owned = identity(&observed).map_err(unknown_creation)?;
        if observed.st_uid != self.uid
            || fs::FileType::from_raw_mode(observed.st_mode) != fs::FileType::Directory
        {
            return Err(unknown_creation(io::Error::new(
                io::ErrorKind::InvalidData,
                "Created directory binding could not be established",
            )));
        }
        let fd = match fs::openat(
            self,
            name,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(error) => {
                return Err(CreationFailure {
                    error: error.into(),
                    cleanup: remove_known_empty(self, name, owned),
                });
            }
        };
        let created = CreatedDirectory {
            directory: Self {
                fd: Arc::new(fd),
                path: self.path.join(name),
                identity: owned,
                uid: self.uid,
            },
            parent: self.clone(),
            name: name.to_owned(),
        };
        let result = (|| -> io::Result<()> {
            if identity(&fs::fstat(&created.directory)?)? != owned {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Created entry/handle identities differ",
                ));
            }
            fs::fchmod(&created.directory, Mode::from_raw_mode(0o700))?;
            #[cfg(target_os = "macos")]
            crate::darwin_acl::clear_owned(&*created.directory.fd)?;
            let actual = fs::fstat(&created.directory)?;
            if actual.st_uid != self.uid || actual.st_mode & 0o777 != 0o700 {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "The volume did not enforce private directory mode",
                ));
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(created),
            Err(error) => Err(CreationFailure {
                error,
                cleanup: created.remove_empty(),
            }),
        }
    }
    /// Object bound by the live descriptor, independent of later pathname changes.
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Requested native expression; it does not itself qualify a current address.
    pub fn requested_path(&self) -> &Path {
        &self.path
    }
    /// Actual filesystem name limit where available; an indeterminate limit uses 255 units.
    pub fn name_limit(&self) -> io::Result<usize> {
        let value = fs::fstatvfs(&*self.fd)?.f_namemax;
        Ok(usize::try_from(value)
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(255))
    }
    /// Establish the other-UID namespace predicate before creating or removing an owned entry.
    /// This is access-control admission, not an atomic compare-and-unlink operation.
    pub fn admit_namespace(&self) -> io::Result<()> {
        let stat = fs::fstat(&*self.fd)?;
        crate::namespace::admit_model(self, stat.st_uid)?;
        if self.identity != identity(&stat)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Held directory identity changed",
            ));
        }
        if stat.st_uid != self.uid && stat.st_uid != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Choose a destination owned by the current user or trusted root",
            ));
        }
        if stat.st_mode & 0o022 != 0 && stat.st_mode & 0o1000 == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Choose a private destination or a supported trusted-owner sticky directory",
            ));
        }
        #[cfg(target_os = "macos")]
        crate::darwin_acl::admit_namespace(&*self.fd)?;
        Ok(())
    }
}
impl AsFd for BoundDirectory {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    #[test]
    fn zero_object_identity_never_supplies_a_binding_or_cleanup_proof() {
        assert_eq!(
            usable_identity(1, 0).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            usable_identity(0, 1).unwrap(),
            Identity::Posix {
                device: 0,
                inode: 1
            }
        );
    }
    fn parent() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fs-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(base)
            .unwrap()
    }
    #[test]
    fn namespace_admission_checks_modes_without_creating_entries() {
        let root = parent();
        let bound = BoundDirectory::open(root.path()).unwrap();
        let volume = fs::fstatfs(&bound).unwrap();
        #[cfg(target_os = "linux")]
        eprintln!(
            "Native namespace test filesystem f_type={:#x}",
            volume.f_type
        );
        #[cfg(target_os = "macos")]
        eprintln!("Native namespace test volume f_flags={:#x}", volume.f_flags);
        assert!(bound.name_limit().unwrap() >= 19);
        for (mode, allowed) in [
            (0o700, true),
            (0o755, true),
            (0o777, false),
            (0o775, false),
            (0o1777, true),
        ] {
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(bound.admit_namespace().is_ok(), allowed, "{mode:o}");
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }
    #[test]
    fn bound_identity_survives_requested_name_replacement() {
        let root = parent();
        let path = root.path().join("destination");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let held = BoundDirectory::open(&path).unwrap();
        std::fs::rename(&path, root.path().join("moved")).unwrap();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let replacement = BoundDirectory::open(&path).unwrap();
        assert_ne!(held.identity(), replacement.identity());
        assert!(held.admit_namespace().is_ok());
    }
    #[test]
    fn directory_binding_does_not_open_regular_files_or_relative_paths() {
        let root = parent();
        let file = root.path().join("regular");
        std::fs::write(&file, b"synthetic").unwrap();
        assert!(BoundDirectory::open(&file).is_err());
        assert!(BoundDirectory::open(Path::new(".")).is_err());
    }
    #[test]
    fn private_creation_refuses_shared_parent_before_mkdir_and_never_modifies_existing() {
        let root = parent();
        let bound = BoundDirectory::open(root.path()).unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let failure = bound
            .create_private_child(OsStr::new("missing"))
            .err()
            .unwrap();
        assert_eq!(failure.cleanup.state, CleanupState::NotNeeded);
        assert!(!root.path().join("missing").exists());
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let child = bound.create_private_child(OsStr::new("private")).unwrap();
        assert_eq!(fs::fstat(child.directory()).unwrap().st_mode & 0o777, 0o700);
        let original = child.directory().identity();
        let failure = bound
            .create_private_child(OsStr::new("private"))
            .err()
            .unwrap();
        assert_eq!(failure.error.raw_os_error(), Some(libc::EEXIST));
        assert_eq!(failure.cleanup.state, CleanupState::NotNeeded);
        assert_eq!(
            bound.open_child(OsStr::new("private")).unwrap().identity(),
            original
        );
        assert_eq!(child.remove_empty().state, CleanupState::Complete);
    }
    #[test]
    fn cleanup_retains_owned_directory_after_name_replacement_and_leaves_substitute() {
        let root = parent();
        let bound = BoundDirectory::open(root.path()).unwrap();
        let child = bound.create_private_child(OsStr::new("stage")).unwrap();
        std::fs::rename(root.path().join("stage"), root.path().join("moved")).unwrap();
        std::fs::create_dir(root.path().join("stage")).unwrap();
        let replacement = bound.open_child(OsStr::new("stage")).unwrap().identity();
        let receipt = child.remove_empty();
        assert_eq!(receipt.state, CleanupState::Residual);
        assert_eq!(receipt.residual_name, None);
        assert!(root.path().join("moved").exists());
        assert_eq!(
            bound.open_child(OsStr::new("stage")).unwrap().identity(),
            replacement
        );
    }
    #[test]
    fn unsafe_cleanup_parent_and_nonempty_directory_are_reported_without_recursive_removal() {
        let root = parent();
        let bound = BoundDirectory::open(root.path()).unwrap();
        let child = bound.create_private_child(OsStr::new("stage")).unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(child.remove_empty().state, CleanupState::Residual);
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let child = bound.create_private_child(OsStr::new("occupied")).unwrap();
        std::fs::write(root.path().join("occupied/keep"), b"synthetic").unwrap();
        let receipt = child.remove_empty();
        assert_eq!(receipt.state, CleanupState::Residual);
        assert_eq!(
            receipt.residual_name.as_deref(),
            Some(OsStr::new("occupied"))
        );
        assert_eq!(
            std::fs::read(root.path().join("occupied/keep")).unwrap(),
            b"synthetic"
        );
    }
    #[test]
    fn bound_child_operations_refuse_components_and_links() {
        let root = parent();
        let bound = BoundDirectory::open(root.path()).unwrap();
        for name in ["", ".", "..", "a/b", "a\0b"] {
            assert_eq!(
                bound
                    .create_private_child(OsStr::new(name))
                    .err()
                    .unwrap()
                    .error
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
        let child = bound.create_private_child(OsStr::new("ordinary")).unwrap();
        assert_eq!(
            bound.open_child(OsStr::new("ordinary")).unwrap().identity(),
            child.directory().identity()
        );
        std::os::unix::fs::symlink("ordinary", root.path().join("link")).unwrap();
        assert!(bound.open_child(OsStr::new("link")).is_err());
        assert_eq!(child.remove_empty().state, CleanupState::Complete);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn namespace_acl_read_inheritance_is_allowed_but_delete_child_is_not() {
        let root = parent();
        let bound = BoundDirectory::open(root.path()).unwrap();
        let chmod = |rule: &str| {
            std::process::Command::new("/bin/chmod")
                .args(["+a", rule])
                .arg(root.path())
                .status()
                .unwrap()
                .success()
        };
        assert!(chmod(
            "everyone allow readattr,readsecurity,list,search,directory_inherit,file_inherit"
        ));
        assert!(bound.admit_namespace().is_ok());
        assert!(chmod("everyone allow delete_child"));
        assert!(bound.admit_namespace().is_err());
    }
}
