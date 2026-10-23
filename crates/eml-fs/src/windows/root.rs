//! Current-item ownership of a resolved root's ordinary ancestor pins.
use super::{child, handle, path};
use crate::{BoundDirectory, Identity};
use std::{
    ffi::OsStr,
    io,
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

/// Resolved ancestry binding stopped or encountered a genuine native failure.
#[derive(Debug)]
pub enum RootBindingError {
    /// Caller requested cooperative cancellation between bounded native steps.
    Interrupted,
    /// Actual path, native-open, ordinary-type or full-identity failure.
    Native(io::Error),
}
impl From<io::Error> for RootBindingError {
    fn from(error: io::Error) -> Self {
        Self::Native(error)
    }
}
impl std::fmt::Display for RootBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interrupted => f.write_str("Resolved root binding interrupted"),
            Self::Native(error) => write!(f, "Resolved root binding failed: {error}"),
        }
    }
}
impl std::error::Error for RootBindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Interrupted => None,
            Self::Native(error) => Some(error),
        }
    }
}
/// One metadata/traverse endpoint and all its ordinary existing ancestor pins.
///
/// Core must keep this owner live through the current item's namespace work,
/// candidate close/final reopen and requested-address qualification. Child and
/// candidate values retain only immediate physical parents, not this ancestry.
/// No original selected expression, mirror policy or whole-batch pins are stored.
pub struct PinnedRoot {
    directory: BoundDirectory,
    ancestors: Vec<BoundDirectory>,
}
impl PinnedRoot {
    /// Bind an already resolved absolute extended DOS/UNC root top-down. Every
    /// ancestor is metadata/traverse-only, no-follow, read/write sharing and no delete sharing; the endpoint
    /// opens with metadata/traverse access and earlier full identity.
    ///
    /// The validated native representation bounds the chain, including expansion
    /// beyond the raw request's component count. Never creates, flushes or changes
    /// permissions. Partial constructions close by ownership without pretending
    /// that unchecked Drop is a successful checked-close receipt.
    pub fn bind_resolved(
        resolved: &Path,
        expected: Identity,
        mut is_stopped: impl FnMut() -> bool,
    ) -> Result<Self, RootBindingError> {
        checkpoint(&mut is_stopped)?;
        let (root, mut names) = root_and_names(resolved)?;
        checkpoint(&mut is_stopped)?;
        let Some(endpoint) = names.pop() else {
            let file = handle::open(
                &root,
                child::ANCESTOR_PIN_ACCESS,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            )?;
            let identity = handle::directory_identity(&file)?;
            if identity != expected {
                return Err(io::Error::other("Root identity changed").into());
            }
            let directory = BoundDirectory { file, identity };
            checkpoint(&mut is_stopped)?;
            return Ok(Self {
                directory,
                ancestors: Vec::new(),
            });
        };
        let file = handle::open(
            &root,
            child::ANCESTOR_PIN_ACCESS,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )?;
        let identity = handle::directory_identity(&file)?;
        let mut ancestors = Vec::with_capacity(names.len() + 1);
        ancestors.push(BoundDirectory { file, identity });
        for name in names {
            checkpoint(&mut is_stopped)?;
            let parent = ancestors.last().expect("root pin always present");
            check_pin(parent)?;
            let file = child::ancestor_pin(&parent.file, name)?;
            let identity = handle::directory_identity(&file)?;
            ancestors.push(BoundDirectory { file, identity });
        }
        checkpoint(&mut is_stopped)?;
        let directory = {
            let parent = ancestors.last().expect("root pin");
            let file = child::ancestor_pin(&parent.file, endpoint)?;
            let identity = handle::directory_identity(&file)?;
            if identity != expected {
                return Err(io::Error::other("Root identity changed").into());
            }
            BoundDirectory { file, identity }
        };
        let bound = Self {
            directory,
            ancestors,
        };
        bound.check_held(&mut is_stopped)?;
        Ok(bound)
    }
    /// Borrow only the physical endpoint. No detached endpoint is advertised as
    /// retaining this owner's ancestry or as qualifying an original address.
    pub fn directory(&self) -> &BoundDirectory {
        &self.directory
    }
    /// Reobserve every held ordinary-directory/full-ID fact. Success is a current
    /// observation, not continuous metadata immutability or original-address proof.
    /// Check stop between native steps; post-edge reconciliation passes a
    /// never-stopped predicate so it finishes the current publication observation.
    pub fn check_held(&self, mut is_stopped: impl FnMut() -> bool) -> Result<(), RootBindingError> {
        for pin in &self.ancestors {
            checkpoint(&mut is_stopped)?;
            check_pin(pin)?;
        }
        checkpoint(&mut is_stopped)?;
        self.directory.check_held()?;
        checkpoint(&mut is_stopped)
    }
}
fn checkpoint(is_stopped: &mut impl FnMut() -> bool) -> Result<(), RootBindingError> {
    if is_stopped() {
        Err(RootBindingError::Interrupted)
    } else {
        Ok(())
    }
}
fn check_pin(pin: &BoundDirectory) -> io::Result<()> {
    if handle::directory_identity(&pin.file)? == pin.identity {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Held ancestor identity changed",
        ))
    }
}
pub(super) fn root_and_names(path: &Path) -> io::Result<(PathBuf, Vec<&OsStr>)> {
    path::validate_operation(path)?;
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        return Err(invalid_root());
    };
    let unc = matches!(prefix.kind(), Prefix::VerbatimUNC(..));
    if !matches!(
        prefix.kind(),
        Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(..)
    ) {
        return Err(invalid_root());
    }
    match parts.next() {
        Some(Component::RootDir) => {}
        None if unc => {} // A bare complete UNC share has an implicit root.
        _ => return Err(invalid_root()),
    }
    let mut root = prefix.as_os_str().to_owned();
    root.push(std::path::MAIN_SEPARATOR_STR);
    let names = parts
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            _ => Err(invalid_root()),
        })
        .collect::<io::Result<_>>()?;
    Ok((root.into(), names))
}
fn invalid_root() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Resolved binding requires a validated absolute extended DOS/UNC directory",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::{
        test_actor::RestrictedToken,
        test_fixture::{directory as fixture, operation},
    };
    use std::{fs, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::{
        Storage::FileSystem::DELETE,
        Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_WRITE},
    };
    fn delete_access(path: &Path) -> io::Result<std::fs::File> {
        handle::open(
            path,
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    }
    #[test]
    fn resolved_components_preserve_dos_unc_and_expanded_native_bounds() {
        let (root, names) = root_and_names(Path::new(r"\\?\C:\alpha\beta")).unwrap();
        assert_eq!(root, Path::new(r"\\?\C:\"));
        assert_eq!(names, [OsStr::new("alpha"), OsStr::new("beta")]);
        let (root, names) = root_and_names(Path::new(r"\\?\UNC\host\share\alpha")).unwrap();
        assert_eq!(root, Path::new(r"\\?\UNC\host\share\"));
        assert_eq!(names, [OsStr::new("alpha")]);
        for share in [r"\\?\UNC\host\share", r"\\?\UNC\host\share\"] {
            let (root, names) = root_and_names(Path::new(share)).unwrap();
            assert_eq!(root, Path::new(r"\\?\UNC\host\share\"));
            assert!(names.is_empty());
        }
        let (_, names) = root_and_names(Path::new(r"\\?\C:\")).unwrap();
        assert!(names.is_empty());
        let expanded = format!(r"\\?\C:\{}leaf", "a\\".repeat(300));
        assert_eq!(root_and_names(Path::new(&expanded)).unwrap().1.len(), 301);
        // Representation-only ceiling, not filesystem component-capacity evidence.
        let exact = format!(r"\\?\C:\{}", "x".repeat(32_759));
        assert_eq!(OsStr::new(&exact).encode_wide().count(), 32_766);
        assert!(root_and_names(Path::new(&exact)).is_ok());
        assert!(root_and_names(Path::new(&(exact + "x"))).is_err());
        for invalid in [
            r"C:\alpha",
            r"\\?\C:\a\..\b",
            r"\\.\C:\alpha",
            r"\\?\UNC\host\PIPE\alpha",
            r"\\?\UNC\host",
            r"\\?\UNC\host\",
            r"\\?\C:",
        ] {
            assert!(root_and_names(Path::new(invalid)).is_err());
        }
    }
    #[test]
    fn native_root_pins_ordinary_ancestors_and_supports_endpoint_storage_and_flush() {
        let fixture = fixture();
        let upper = fixture.path().join("upper");
        let selected = upper.join("selected");
        fs::create_dir_all(&selected).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(&selected)).unwrap();
        let upper_operand = operation(&upper);
        handle::close(delete_access(&upper_operand).unwrap()).unwrap();
        let bound =
            PinnedRoot::bind_resolved(&read.resolved_path().unwrap(), read.identity(), || false)
                .unwrap();
        assert_eq!(bound.directory().identity(), read.identity());
        assert!(!bound.ancestors.is_empty());
        bound.check_held(|| false).unwrap();
        let denied = delete_access(&upper_operand).unwrap_err();
        assert_eq!(denied.raw_os_error(), Some(32));
        let receipt = crate::SyncDirectory::acquire(bound.directory())
            .unwrap()
            .sync_namespace();
        assert_eq!(receipt.state, crate::SyncState::Succeeded, "{receipt:?}");
        let candidate = crate::CreationDirectory::acquire(bound.directory())
            .unwrap()
            .stage_bytes(b"owned complete storage", || Ok(()))
            .unwrap();
        assert_eq!(candidate.length(), 22);
        assert_eq!(candidate.file_sync().state, crate::SyncState::Succeeded);
        bound.check_held(|| false).unwrap();
        assert_eq!(candidate.cleanup().state, crate::CleanupState::Complete);
        assert_eq!(fs::read_dir(&selected).unwrap().count(), 0);
        drop(bound);
        handle::close(delete_access(&upper_operand).unwrap()).unwrap();
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
    #[test]
    fn native_existing_pinned_child_is_relative_ordinary_and_identity_checked() {
        let fixture = fixture();
        let selected = fixture.path().join("selected");
        fs::create_dir_all(selected.join("existing")).unwrap();
        fs::write(selected.join("ordinary-file"), b"owned sentinel").unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(&selected)).unwrap();
        let child = read.observe_child(OsStr::new("existing")).unwrap();
        let file = read.observe_child(OsStr::new("ordinary-file")).unwrap();
        let bound =
            PinnedRoot::bind_resolved(&read.resolved_path().unwrap(), read.identity(), || false)
                .unwrap();
        let existing = bound
            .directory()
            .pin_directory(OsStr::new("existing"), child.identity)
            .unwrap();
        assert_eq!(existing.identity(), child.identity);
        existing.check_held().unwrap();
        assert_eq!(
            crate::SyncDirectory::acquire(&existing)
                .unwrap()
                .sync_namespace()
                .state,
            crate::SyncState::Succeeded
        );
        drop(existing);
        let mismatch = bound
            .directory()
            .pin_directory(OsStr::new("existing"), read.identity())
            .err()
            .unwrap();
        assert_eq!(mismatch.kind(), io::ErrorKind::InvalidData);
        assert!(
            bound
                .directory()
                .pin_directory(OsStr::new("ordinary-file"), file.identity)
                .is_err()
        );
        assert!(
            bound
                .directory()
                .pin_directory(OsStr::new("missing"), child.identity)
                .is_err()
        );
        assert_eq!(
            bound
                .directory()
                .pin_directory(OsStr::new(r"existing\other"), child.identity)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            fs::read(selected.join("ordinary-file")).unwrap(),
            b"owned sentinel"
        );
        assert_eq!(fs::read_dir(selected.join("existing")).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&selected).unwrap().count(), 2);
        drop(bound);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
    #[test]
    fn native_root_identity_failure_releases_partial_pins_without_creating_names() {
        let fixture = fixture();
        let upper = fixture.path().join("upper");
        let selected = upper.join("selected");
        let other = fixture.path().join("other");
        fs::create_dir_all(&selected).unwrap();
        fs::create_dir(&other).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(&selected)).unwrap();
        let other = BoundDirectory::open(&operation(&other)).unwrap();
        let result =
            PinnedRoot::bind_resolved(&read.resolved_path().unwrap(), other.identity(), || false);
        let Err(RootBindingError::Native(error)) = result else {
            panic!("Wrong root identity must be refused");
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        handle::close(delete_access(&operation(&upper)).unwrap()).unwrap();
        assert_eq!(fs::read_dir(&selected).unwrap().count(), 0);
        drop(read);
        drop(other);
        drop(actor);
        fixture.close().unwrap();
    }
    #[test]
    fn native_root_stop_is_distinct_and_partial_pin_exclusion_is_released() {
        let fixture = fixture();
        let upper = fixture.path().join("upper");
        let selected = upper.join("nested/selected");
        fs::create_dir_all(&selected).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(&selected)).unwrap();
        let resolved = read.resolved_path().unwrap();
        assert!(matches!(
            PinnedRoot::bind_resolved(&resolved, read.identity(), || true),
            Err(RootBindingError::Interrupted)
        ));
        handle::close(delete_access(&operation(&upper)).unwrap()).unwrap();
        let upper_operand = operation(&upper);
        let mut observed_pin = false;
        let stopped =
            PinnedRoot::bind_resolved(&resolved, read.identity(), || {
                match delete_access(&upper_operand) {
                    Ok(file) => {
                        handle::close(file).unwrap();
                        false
                    }
                    Err(error) => {
                        assert_eq!(error.raw_os_error(), Some(32));
                        observed_pin = true;
                        true
                    }
                }
            });
        assert!(matches!(stopped, Err(RootBindingError::Interrupted)));
        assert!(observed_pin);
        handle::close(delete_access(&operation(&upper)).unwrap()).unwrap();
        assert_eq!(fs::read_dir(&selected).unwrap().count(), 0);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
}
