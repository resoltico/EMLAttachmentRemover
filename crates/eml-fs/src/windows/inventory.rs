//! Read-only selected directories and names from their exclusively borrowed cursor.
use super::{child, handle, path};
use crate::{EntryObservation, Identity};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    mem::offset_of,
    os::windows::{ffi::OsStringExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows_sys::Win32::{Foundation::ERROR_NO_MORE_FILES, Storage::FileSystem::*};

const BUFFER_BYTES: usize = 64 * 1024;
const NAME_OFFSET: usize = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);

/// Observe an explicit selected absolute extended DOS/UNC expression, following
/// its final link, with only a temporary metadata handle. No object data is opened.
/// This metadata observation supplies no held working-directory or physical
/// admission binding. Core must compare its identity when later opening a root.
pub fn observe_selected(path: &Path) -> io::Result<EntryObservation> {
    let file = handle::open(
        path,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        FILE_FLAG_BACKUP_SEMANTICS,
    )?;
    handle::entry_observation(&file)
}

/// A read-only selected directory, held by native handle and full object identity.
/// This handle provides observation, not protection of a creation address.
pub struct BoundDirectory {
    pub(super) file: File,
    pub(super) identity: Identity,
}
/// A metadata-only native object lifetime, without data or namespace mutation
/// rights. It prevents identity reuse while a strict candidate is released.
pub struct EntryPin {
    file: File,
    observation: EntryObservation,
}
impl EntryPin {
    /// Native facts captured while opening this still-held object.
    pub fn observation(&self) -> EntryObservation {
        self.observation
    }
    /// Release this exact metadata owner with a checked native close.
    pub fn close(self) -> io::Result<()> {
        handle::close(self.file)
    }
}
impl BoundDirectory {
    /// Revalidate held ordinary-directory/full-ID facts without mutation access.
    pub fn check_held(&self) -> io::Result<()> {
        self.check_identity()
    }
    /// Observe support for generated names, without probing writes.
    pub fn admit_name_domain(&self) -> io::Result<()> {
        super::name_domain::admit(&self.file)
    }
    /// Read-only name-domain qualification; actual creation access is checked at its boundary.
    pub fn inspect_creation_support(&self) -> io::Result<()> {
        self.admit_name_domain()
    }
    /// Open a nondeletable metadata/traverse child pin.
    pub fn pin_directory(&self, name: &OsStr, expected: Identity) -> io::Result<Self> {
        self.check_identity()?;
        let file = child::ancestor_pin(&self.file, name)?;
        let child = Self::from_file(file)?;
        if child.identity != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Pinned mirror identity changed",
            ));
        }
        Ok(child)
    }
    /// Retain one existing no-follow child's metadata handle. This grants no
    /// creation-address protection and does not qualify the requested path.
    pub fn pin_child(&self, name: &OsStr) -> io::Result<EntryPin> {
        let file = child::observe(&self.file, name)?;
        let observation = handle::entry_observation(&file)?;
        Ok(EntryPin { file, observation })
    }
    /// Observe one no-follow child, mapping only structured native NAME_NOT_FOUND
    /// from its existing-only lookup to point absence. All other errors survive.
    /// None is not a parent/address qualification or permission to create.
    pub fn observe_child_if_present(&self, name: &OsStr) -> io::Result<Option<EntryObservation>> {
        match self.observe_child(name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if child_missing(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// Observed maximum filename-component length in WCHARs for the held volume.
    /// Query failure/zero remains an error; this supplies no write-admission proof.
    pub fn name_limit(&self) -> io::Result<usize> {
        handle::component_limit(&self.file)
    }
    /// Query the held ordinary directory's normalized DOS/UNC name, preserving
    /// native units. This observation is not a pinned ancestor or address receipt;
    /// a later binding must compare its endpoint with this directory's identity.
    pub fn resolved_path(&self) -> io::Result<PathBuf> {
        self.check_identity()?;
        let mut buffer = vec![0; path::MAX_EXTENDED_CONTENT + 1];
        // SAFETY: live held ordinary directory, writable UTF-16 buffer covers
        // its stated capacity including NUL, fixed normalized DOS flags. This
        // Win32 query completes before the borrowed handle/buffer can be released.
        let written = unsafe {
            GetFinalPathNameByHandleW(
                self.file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        if written >= buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Resolved directory name exceeds the supported native path bound",
            ));
        }
        let resolved = PathBuf::from(OsString::from_wide(&buffer[..written]));
        path::validate_operation(&resolved)?;
        self.check_identity()?;
        Ok(resolved)
    }
    pub(super) fn check_identity(&self) -> io::Result<()> {
        if handle::directory_identity(&self.file)? != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Held directory identity differs from its binding",
            ));
        }
        Ok(())
    }
    /// Open an admitted absolute extended DOS/UNC selected root. Normal final
    /// directory links are followed; the opened target must be an ordinary disk directory.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = handle::open(
            path,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_BACKUP_SEMANTICS,
        )?;
        Self::from_file(file)
    }
    fn from_file(file: File) -> io::Result<Self> {
        Ok(Self {
            identity: handle::directory_identity(&file)?,
            file,
        })
    }
    /// Observe one existing child without following its final reparse point.
    /// Reparse kind is established before requesting ordinary-file signatures.
    pub fn observe_child(&self, name: &OsStr) -> io::Result<EntryObservation> {
        let file = child::observe(&self.file, name)?;
        handle::entry_observation(&file)
    }
    /// Open one ordinary existing directory relative to this held parent,
    /// refusing reparse processing. Before descent the collector must compare
    /// this returned identity with its earlier entry observation.
    pub fn open_child(&self, name: &OsStr) -> io::Result<Self> {
        Self::from_file(child::directory(&self.file, name)?)
    }
    /// Full native identity of the opened selected directory.
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Restart enumeration on this held file object. Exclusive borrowing prevents
    /// simultaneous iterators from sharing its native cursor. Dot entries are retained.
    pub fn read_entries(&mut self) -> io::Result<DirectoryEntries<'_>> {
        Ok(DirectoryEntries {
            directory: self,
            buffer: vec![0; BUFFER_BYTES / 8],
            offset: None,
            restart: true,
            ended: false,
        })
    }
}
fn child_missing(error: &io::Error) -> bool {
    error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<super::status::NtStatusError>())
        .is_some_and(|cause| {
            cause.nt_status() == windows_sys::Win32::Foundation::STATUS_OBJECT_NAME_NOT_FOUND
        })
}

/// Streaming native names. Drop before restarting or otherwise using the directory.
/// Entry IDs in the enumeration structure are deliberately not used as identities.
pub struct DirectoryEntries<'a> {
    directory: &'a mut BoundDirectory,
    buffer: Vec<u64>,
    offset: Option<usize>,
    restart: bool,
    ended: bool,
}
impl DirectoryEntries<'_> {
    fn query(&mut self) -> io::Result<bool> {
        self.buffer.fill(0);
        let class = if self.restart {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        // SAFETY: live exclusively borrowed directory file object; u64 storage is
        // eight-byte aligned and exactly BUFFER_BYTES writable bytes. Generated
        // information class matches the record layout parsed below.
        let result = unsafe {
            GetFileInformationByHandleEx(
                self.directory.file.as_raw_handle(),
                class,
                self.buffer.as_mut_ptr().cast(),
                BUFFER_BYTES as u32,
            )
        };
        self.restart = false;
        if result == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                Ok(false)
            } else {
                Err(error)
            };
        }
        self.offset = Some(0);
        Ok(true)
    }
}
impl Iterator for DirectoryEntries<'_> {
    type Item = io::Result<OsString>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.ended {
            return None;
        }
        if self.offset.is_none() {
            match self.query() {
                Ok(true) => {}
                Ok(false) => {
                    self.ended = true;
                    return None;
                }
                Err(error) => {
                    self.ended = true;
                    return Some(Err(error));
                }
            }
        }
        // SAFETY: initialized owned u64 allocation, viewed only as bytes within
        // its exact allocation size. No native typed reference is formed.
        let bytes =
            unsafe { std::slice::from_raw_parts(self.buffer.as_ptr().cast::<u8>(), BUFFER_BYTES) };
        match record(
            bytes,
            self.offset
                .take()
                .expect("successful query supplies a record"),
        ) {
            Ok((name, next)) => {
                self.offset = next;
                Some(Ok(name))
            }
            Err(error) => {
                self.ended = true;
                Some(Err(error))
            }
        }
    }
}
fn malformed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid native directory-name record",
    )
}
fn record(bytes: &[u8], offset: usize) -> io::Result<(OsString, Option<usize>)> {
    if !offset.is_multiple_of(8) {
        return Err(malformed());
    }
    let header = bytes
        .get(offset..)
        .and_then(|tail| tail.get(..NAME_OFFSET))
        .ok_or_else(malformed)?;
    let field = |at: usize| {
        u32::from_le_bytes(
            header[at..at + 4]
                .try_into()
                .expect("generated header field"),
        ) as usize
    };
    let next = field(offset_of!(FILE_ID_BOTH_DIR_INFO, NextEntryOffset));
    let length = field(offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength));
    if length == 0 || !length.is_multiple_of(2) {
        return Err(malformed());
    }
    let end = if next == 0 {
        bytes.len()
    } else {
        if !next.is_multiple_of(8) || next < NAME_OFFSET {
            return Err(malformed());
        }
        let end = offset
            .checked_add(next)
            .filter(|end| *end < bytes.len())
            .ok_or_else(malformed)?;
        if bytes.len() - end < NAME_OFFSET {
            return Err(malformed());
        }
        end
    };
    let start = offset.checked_add(NAME_OFFSET).ok_or_else(malformed)?;
    let name_end = start
        .checked_add(length)
        .filter(|name_end| *name_end <= end)
        .ok_or_else(malformed)?;
    let units: Vec<u16> = bytes
        .get(start..name_end)
        .ok_or_else(malformed)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.iter().any(|unit| matches!(*unit, 0 | 47 | 92)) {
        return Err(malformed());
    }
    Ok((
        OsString::from_wide(&units),
        if next == 0 { None } else { Some(end) },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntryKind;
    use std::{collections::BTreeSet, ffi::OsStr, os::windows::ffi::OsStrExt};
    #[test]
    fn optional_child_observation_preserves_present_dangling_and_invalid_entries() {
        use super::super::{
            test_actor::RestrictedToken,
            test_fixture::{directory, operation},
        };
        let fixture = directory();
        std::fs::write(fixture.path().join("file.eml"), b"owned").unwrap();
        let target = fixture.path().join("target");
        let alias = fixture.path().join("dangling");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&alias).unwrap();
        let link = super::super::test_reparse::writer(&alias).unwrap();
        super::super::test_reparse::set_mount_point(&link, &target).unwrap();
        drop(link);
        std::fs::remove_dir(target).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let root = BoundDirectory::open(&operation(fixture.path())).unwrap();
        assert!(
            root.observe_child_if_present(OsStr::new("missing"))
                .unwrap()
                .is_none()
        );
        let file = root
            .observe_child_if_present(OsStr::new("file.eml"))
            .unwrap()
            .unwrap();
        assert_eq!(file, root.observe_child(OsStr::new("file.eml")).unwrap());
        assert_eq!(file.kind, EntryKind::File);
        assert_eq!(
            root.observe_child_if_present(OsStr::new("dangling"))
                .unwrap()
                .unwrap()
                .kind,
            EntryKind::Reparse
        );
        assert_eq!(
            root.observe_child_if_present(OsStr::new("a/b"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        drop(root);
        drop(actor);
        std::fs::remove_dir(alias).unwrap();
        fixture.close().unwrap();
    }
    #[test]
    fn scripted_missing_mapping_requires_exact_native_status_and_provenance() {
        use windows_sys::Win32::Foundation::{
            STATUS_ACCESS_DENIED, STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND,
        };
        // Mapping only, not injected native filesystem failures.
        assert!(child_missing(&super::super::status::error(
            STATUS_OBJECT_NAME_NOT_FOUND
        )));
        for status in [STATUS_OBJECT_PATH_NOT_FOUND, STATUS_ACCESS_DENIED] {
            assert!(!child_missing(&super::super::status::error(status)));
        }
        assert!(!child_missing(&io::Error::from_raw_os_error(2)));
        assert!(!child_missing(&io::Error::from_raw_os_error(3)));
        assert!(!child_missing(&io::Error::new(
            io::ErrorKind::NotFound,
            "cosmetic"
        )));
    }

    #[test]
    fn selected_root_metadata_observes_file_directory_and_native_missing_errors() {
        use super::super::{
            test_actor::RestrictedToken,
            test_fixture::{directory, operation},
        };
        let fixture = directory();
        let name = OsString::from_wide(&[0x61, 0xd800, 0x62]);
        let source = fixture.path().join(&name);
        std::fs::write(&source, b"owned selected metadata").unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let parent = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let root = observe_selected(&operation(fixture.path())).unwrap();
        assert_eq!(root.kind, EntryKind::Directory);
        assert_eq!(root.identity, parent.identity());
        assert!(root.file.is_none());
        let file = observe_selected(&operation(&source)).unwrap();
        assert_eq!(file.kind, EntryKind::File);
        let independently_opened = crate::BoundFile::open(&operation(&source), false).unwrap();
        assert_eq!(file.file, Some(independently_opened.initial_signature()));
        assert_eq!(
            file.identity,
            independently_opened.initial_signature().identity
        );
        assert_eq!(file.file.unwrap().size, 23);
        assert_eq!(parent.observe_child(&name).unwrap(), file);
        let missing = observe_selected(&operation(&fixture.path().join("missing"))).unwrap_err();
        assert_eq!(missing.raw_os_error(), Some(2));
        assert_eq!(
            observe_selected(Path::new("relative")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            observe_selected(Path::new("\\\\?\\C:\\a\0b"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        drop(independently_opened);
        drop(parent);
        drop(actor);
        fixture.close().unwrap();
    }

    #[test]
    fn selected_directory_alias_resolves_target_name_while_walked_entry_remains_reparse() {
        use super::super::{
            test_actor::RestrictedToken,
            test_fixture::{directory, operation},
        };
        let fixture = directory();
        let target = fixture.path().join("target");
        let alias = fixture.path().join("selected-link");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&alias).unwrap();
        // Static explicitly selected alias setup only, before observation.
        let link = super::super::test_reparse::writer(&alias).unwrap();
        super::super::test_reparse::set_mount_point(&link, &target).unwrap();
        drop(link);
        let actor = RestrictedToken::enter().unwrap();
        let parent = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let target = BoundDirectory::open(&operation(&target)).unwrap();
        let selected = observe_selected(&operation(&alias)).unwrap();
        assert_eq!(selected.kind, EntryKind::Directory);
        assert_eq!(selected.identity, target.identity());
        assert!(selected.file.is_none());
        let selected_root = BoundDirectory::open(&operation(&alias)).unwrap();
        let resolved = selected_root.resolved_path().unwrap();
        // Reopening with normal final-link following would also accept the
        // unresolved alias. NOFOLLOW must observe an ordinary TARGET directory.
        let returned_file = handle::open(
            &resolved,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
        .unwrap();
        let returned = handle::entry_observation(&returned_file).unwrap();
        assert_eq!(returned.kind, EntryKind::Directory);
        assert_eq!(returned.identity, target.identity());
        drop(returned_file);
        let walked = parent.observe_child(OsStr::new("selected-link")).unwrap();
        assert_eq!(walked.kind, EntryKind::Reparse);
        assert_ne!(walked.identity, selected.identity);
        assert!(parent.open_child(OsStr::new("selected-link")).is_err());
        drop(selected_root);
        drop(target);
        drop(parent);
        drop(actor);
        std::fs::remove_dir(alias).unwrap();
        fixture.close().unwrap();
    }

    #[test]
    fn resolved_directory_name_preserves_long_native_units_and_reopens_same_object() {
        use super::super::{
            test_actor::RestrictedToken,
            test_fixture::{directory, operation},
        };
        let fixture = directory();
        let mut leaf = operation(fixture.path());
        for letter in ['a', 'b', 'c'] {
            leaf.push(letter.to_string().repeat(100));
        }
        let native = OsString::from_wide(&[0x61, 0xd800, 0x62]);
        leaf.push(&native);
        std::fs::create_dir_all(&leaf).unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let selected = BoundDirectory::open(&leaf).unwrap();
        let resolved = selected.resolved_path().unwrap();
        let units: Vec<_> = resolved.as_os_str().encode_wide().collect();
        assert!(units.len() > 260);
        assert!(units.ends_with(&[92, 0x61, 0xd800, 0x62]));
        let reopened = BoundDirectory::open(&resolved).unwrap();
        assert_eq!(reopened.identity(), selected.identity());
        let returned_file = handle::open(
            &resolved,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
        .unwrap();
        let returned = handle::entry_observation(&returned_file).unwrap();
        assert_eq!(returned.kind, EntryKind::Directory);
        assert_eq!(returned.identity, selected.identity());
        drop(returned_file);
        drop(reopened);
        drop(selected);
        drop(actor);
        fixture.close().unwrap();
    }

    #[test]
    fn native_child_observation_and_descent_use_the_held_parent_and_full_ids() {
        let fixture = tempfile::tempdir().unwrap();
        let selected = fixture.path().join("selected");
        let moved = fixture.path().join("moved");
        std::fs::create_dir_all(selected.join("mail")).unwrap();
        std::fs::write(selected.join("mail/source.eml"), b"synthetic").unwrap();
        let native = OsString::from_wide(&[0x61, 0xd800, 0x62]);
        std::fs::write(selected.join(&native), b"native synthetic").unwrap();
        let context = crate::PathContext::capture(&std::env::current_dir().unwrap()).unwrap();
        let actor = super::super::test_actor::RestrictedToken::enter().unwrap();
        let mut root = BoundDirectory::open(&context.resolve(&selected).unwrap()).unwrap();
        let observed = root.observe_child(OsStr::new("mail")).unwrap();
        assert_eq!(observed.kind, EntryKind::Directory);
        assert!(observed.file.is_none());
        let mut child = root.open_child(OsStr::new("mail")).unwrap();
        // This is the earlier observation, not one recreated after the child open.
        assert_eq!(child.identity(), observed.identity);
        assert!(names(&mut child).contains(OsStr::new("source.eml")));
        let source = child.observe_child(OsStr::new("source.eml")).unwrap();
        assert_eq!(source.kind, EntryKind::File);
        assert_eq!(source.file.unwrap().size, 9);
        let direct = crate::BoundFile::open(
            &context.resolve(&selected.join("mail/source.eml")).unwrap(),
            false,
        )
        .unwrap();
        assert_eq!(source.file.unwrap(), direct.initial_signature());
        drop(direct);
        let native_observed = root.observe_child(&native).unwrap();
        assert_eq!(native_observed.kind, EntryKind::File);
        assert_eq!(native_observed.file.unwrap().size, 16);
        let parent_id = root.identity();
        // This case tests the continuously held root, not whether the OS permits
        // moving an ancestor while a finished descendant directory remains open.
        drop(child);
        // Moving the owned fixture is setup by the original runner context;
        // every product lookup remains under a verified restricted actor.
        drop(actor);
        std::fs::rename(&selected, &moved).unwrap();
        let actor = super::super::test_actor::RestrictedToken::enter().unwrap();
        assert!(!selected.try_exists().unwrap());
        assert_eq!(handle::identity(&root.file).unwrap(), parent_id);
        assert_eq!(root.observe_child(OsStr::new("mail")).unwrap(), observed);
        let after_move = root.observe_child(&native).unwrap();
        assert_eq!(after_move.identity, native_observed.identity);
        assert_eq!(after_move.kind, EntryKind::File);
        assert_eq!(after_move.file.unwrap().size, 16);
        let reopened = root.open_child(OsStr::new("mail")).unwrap();
        assert_eq!(reopened.identity(), observed.identity);
        assert!(names(&mut root).contains(&native));
        drop(reopened);
        drop(root);
        drop(actor);
        fixture.close().unwrap();
    }

    #[test]
    fn native_child_reparse_observation_keeps_the_entry_separate_from_its_target() {
        let fixture = tempfile::tempdir().unwrap();
        let target = fixture.path().join("target");
        let selected = fixture.path().join("entry");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&selected).unwrap();
        std::fs::write(target.join("source.eml"), b"synthetic").unwrap();
        let link = super::super::test_reparse::writer(&selected).unwrap();
        super::super::test_reparse::set_mount_point(&link, &target).unwrap();
        drop(link);
        let context = crate::PathContext::capture(&std::env::current_dir().unwrap()).unwrap();
        let actor = super::super::test_actor::RestrictedToken::enter().unwrap();
        let root = BoundDirectory::open(&context.resolve(fixture.path()).unwrap()).unwrap();
        let target = root.observe_child(OsStr::new("target")).unwrap();
        let entry = root.observe_child(OsStr::new("entry")).unwrap();
        assert_eq!(entry.kind, EntryKind::Reparse);
        assert!(entry.file.is_none());
        assert_ne!(entry.identity, target.identity);
        assert!(root.open_child(OsStr::new("entry")).is_err());
        drop(root);
        drop(actor);
        std::fs::remove_dir(selected).unwrap();
        fixture.close().unwrap();
    }

    #[test]
    fn native_child_validation_precedes_open_and_missing_entry_retains_both_error_domains() {
        let fixture = tempfile::tempdir().unwrap();
        let context = crate::PathContext::capture(&std::env::current_dir().unwrap()).unwrap();
        let actor = super::super::test_actor::RestrictedToken::enter().unwrap();
        let mut root = BoundDirectory::open(&context.resolve(fixture.path()).unwrap()).unwrap();
        let before = names(&mut root);
        for name in ["", ".", "..", "a/b", r"a\b", "a:b", "NUL", "a\0b"] {
            let name = OsStr::new(name);
            let observation = root.observe_child(name).unwrap_err();
            let directory = root
                .open_child(name)
                .err()
                .expect("invalid component admitted");
            for error in [observation, directory] {
                assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
                assert!(
                    error
                        .get_ref()
                        .and_then(|cause| cause.downcast_ref::<crate::NtStatusError>())
                        .is_none()
                );
            }
        }
        let too_long = OsString::from_wide(&vec![120; 32768]);
        assert_eq!(
            root.observe_child(&too_long).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let failure = root.observe_child(OsStr::new("missing")).unwrap_err();
        let native = failure
            .get_ref()
            .unwrap()
            .downcast_ref::<crate::NtStatusError>()
            .unwrap();
        assert!(native.nt_status() < 0);
        assert_ne!(native.win32_code(), 0);
        assert!(
            failure.raw_os_error().is_none(),
            "do not relabel a translated code as primary"
        );
        assert!(
            failure
                .to_string()
                .contains(&format!("0x{:08X}", native.nt_status() as u32))
        );
        eprintln!("owned missing-child native error: {native:?}");
        assert_eq!(names(&mut root), before);
        drop(root);
        drop(actor);
        fixture.close().unwrap();
    }

    fn encoded(name: &[u16], next: u32) -> Vec<u8> {
        let mut bytes = vec![0; NAME_OFFSET + name.len() * 2];
        bytes[..4].copy_from_slice(&next.to_le_bytes());
        let at = offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength);
        bytes[at..at + 4].copy_from_slice(&(name.len() as u32 * 2).to_le_bytes());
        for (pair, unit) in bytes[NAME_OFFSET..]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(name)
        {
            pair.copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn record_preserves_native_units_and_ignores_entry_identity() {
        let mut bytes = encoded(&[b'.' as u16, 0xd800], 0);
        let id = offset_of!(FILE_ID_BOTH_DIR_INFO, FileId);
        bytes[id..id + 8].fill(255);
        let (name, next) = record(&bytes, 0).unwrap();
        assert_eq!(name.encode_wide().collect::<Vec<_>>(), [46, 0xd800]);
        assert_eq!(next, None);
        assert_eq!(record(&encoded(&[46, 46], 0), 0).unwrap().0, "..");
    }
    #[test]
    fn malformed_record_boundaries_and_names_are_rejected() {
        let good = encoded(&[97], 0);
        for length in 0..NAME_OFFSET {
            assert!(record(&good[..length], 0).is_err());
        }
        assert!(record(&good, 1).is_err());
        for units in [&[][..], &[0][..], &[47][..], &[92][..]] {
            assert!(record(&encoded(units, 0), 0).is_err());
        }
        for next in [1, 8, u32::MAX, (NAME_OFFSET + 8) as u32] {
            assert!(record(&encoded(&[97], next), 0).is_err());
        }
        for length in [1, 4, u32::MAX] {
            let mut bytes = good.clone();
            let at = offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength);
            bytes[at..at + 4].copy_from_slice(&length.to_le_bytes());
            assert!(record(&bytes, 0).is_err());
        }
        let mut bytes = vec![0; 512];
        let first = encoded(&[97; 20], 112);
        bytes[..first.len()].copy_from_slice(&first);
        assert!(
            record(&bytes, 0).is_err(),
            "name crosses its own record span"
        );
    }
    #[test]
    fn next_offset_advances_within_the_buffer() {
        let first = encoded(&[97], 112);
        let second = encoded(&[98], 0);
        let mut bytes = vec![0; 112 + second.len()];
        bytes[..first.len()].copy_from_slice(&first);
        bytes[112..].copy_from_slice(&second);
        let (name, next) = record(&bytes, 0).unwrap();
        assert_eq!(name, "a");
        assert_eq!(next, Some(112));
        assert_eq!(record(&bytes, next.unwrap()).unwrap().0, "b");
    }
    fn names(directory: &mut BoundDirectory) -> BTreeSet<OsString> {
        directory
            .read_entries()
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
    #[test]
    fn native_multiple_batches_and_restart_preserve_all_names() {
        let root = tempfile::tempdir().unwrap();
        let mut expected = BTreeSet::new();
        for index in 0..900 {
            let name = format!("message-{index:04}-{}.eml", "x".repeat(90));
            std::fs::write(root.path().join(&name), b"synthetic").unwrap();
            expected.insert(OsString::from(name));
        }
        let native = OsString::from_wide(&[b'n' as u16, 0xd800, b'e' as u16]);
        std::fs::write(root.path().join(&native), b"native units").unwrap();
        expected.insert(native);
        std::fs::write(root.path().join(".hidden"), b"synthetic").unwrap();
        expected.insert(OsString::from(".hidden"));
        let path = crate::PathContext::capture(&std::env::current_dir().unwrap())
            .unwrap()
            .resolve(root.path())
            .unwrap();
        let actor = super::super::test_actor::RestrictedToken::enter().unwrap();
        let mut directory = BoundDirectory::open(&path).unwrap();
        assert_eq!(
            directory.identity(),
            handle::identity(&directory.file).unwrap()
        );
        // The expected filename payload alone exceeds the native query buffer.
        assert!(
            expected
                .iter()
                .map(|name| name.encode_wide().count() * 2)
                .sum::<usize>()
                > BUFFER_BYTES
        );
        let observed = names(&mut directory);
        assert!(observed.contains(OsStr::new(".")) && observed.contains(OsStr::new("..")));
        assert_eq!(
            observed
                .into_iter()
                .filter(|name| name != "." && name != "..")
                .collect::<BTreeSet<_>>(),
            expected
        );
        {
            let mut partial = directory.read_entries().unwrap();
            assert!(partial.next().unwrap().is_ok());
        }
        for _ in 0..2 {
            let mut restarted = names(&mut directory);
            restarted.remove(OsStr::new("."));
            restarted.remove(OsStr::new(".."));
            assert_eq!(restarted, expected);
        }
        drop(directory);
        drop(actor);
        root.close().unwrap();
    }
    #[test]
    fn native_selected_root_is_read_only_and_open_failures_are_visible() {
        let root = tempfile::tempdir().unwrap();
        let context = crate::PathContext::capture(&std::env::current_dir().unwrap()).unwrap();
        let path = context.resolve(root.path()).unwrap();
        let mut directory = BoundDirectory::open(&path).unwrap();
        let mut entries = directory.read_entries().unwrap();
        for entry in entries.by_ref() {
            entry.unwrap();
        }
        assert!(entries.next().is_none());
        assert!(entries.next().is_none());
        drop(entries);
        std::fs::write(root.path().join("later.eml"), b"owned synthetic").unwrap();
        assert!(names(&mut directory).contains(OsStr::new("later.eml")));
        assert!(
            BoundDirectory::open(&context.resolve(&root.path().join("later.eml")).unwrap())
                .is_err()
        );
        assert!(
            BoundDirectory::open(&context.resolve(&root.path().join("missing")).unwrap()).is_err()
        );
        drop(directory);
        root.close().unwrap();
    }
    #[test]
    fn parser_failure_is_reported_once_and_a_fresh_stream_restarts() {
        let root = tempfile::tempdir().unwrap();
        let path = crate::PathContext::capture(&std::env::current_dir().unwrap())
            .unwrap()
            .resolve(root.path())
            .unwrap();
        let mut directory = BoundDirectory::open(&path).unwrap();
        let mut stream = directory.read_entries().unwrap();
        // Synthetic malformed-buffer control, deliberately not native failure evidence.
        stream.offset = Some(0);
        assert_eq!(
            stream.next().unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(stream.next().is_none());
        drop(stream);
        assert!(names(&mut directory).contains(OsStr::new(".")));
        drop(directory);
        root.close().unwrap();
    }
    #[test]
    fn native_query_failure_is_not_an_empty_directory() {
        let root = tempfile::tempdir().unwrap();
        let ordinary = root.path().join("ordinary.eml");
        std::fs::write(&ordinary, b"synthetic").unwrap();
        let path = crate::PathContext::capture(&std::env::current_dir().unwrap())
            .unwrap()
            .resolve(root.path())
            .unwrap();
        let actor = super::super::test_actor::RestrictedToken::enter().unwrap();
        // A real owned regular-file handle cannot supply directory records.
        // This private construction deliberately violates public admission;
        // it tests native query failure without invalid or borrowed raw handles.
        let file = handle::open(
            &crate::PathContext::capture(&std::env::current_dir().unwrap())
                .unwrap()
                .resolve(&ordinary)
                .unwrap(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_BACKUP_SEMANTICS,
        )
        .unwrap();
        let mut directory = BoundDirectory {
            identity: handle::identity(&file).unwrap(),
            file,
        };
        let mut stream = directory.read_entries().unwrap();
        let error = stream.next().unwrap().unwrap_err();
        assert!(error.raw_os_error().is_some());
        assert_ne!(error.raw_os_error(), Some(ERROR_NO_MORE_FILES as i32));
        eprintln!("owned non-directory query error: {error:?}");
        assert!(stream.next().is_none());
        drop(stream);
        let mut restarted = directory.read_entries().unwrap();
        let repeated = restarted.next().unwrap().unwrap_err();
        assert_eq!(repeated.raw_os_error(), error.raw_os_error());
        assert!(restarted.next().is_none());
        drop(restarted);
        drop(directory);
        let mut admitted = BoundDirectory::open(&path).unwrap();
        assert!(names(&mut admitted).contains(OsStr::new(".")));
        drop(admitted);
        drop(actor);
        root.close().unwrap();
    }
    #[test]
    fn native_explicit_directory_link_opens_its_ordinary_target() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let selected = root.path().join("selected");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&selected).unwrap();
        std::fs::write(target.join("message.eml"), b"synthetic").unwrap();
        let link = super::super::test_reparse::writer(&selected).unwrap();
        super::super::test_reparse::set_mount_point(&link, &target).unwrap();
        drop(link);
        let context = crate::PathContext::capture(&std::env::current_dir().unwrap()).unwrap();
        let actor = super::super::test_actor::RestrictedToken::enter().unwrap();
        let mut directory = BoundDirectory::open(&context.resolve(&selected).unwrap()).unwrap();
        let direct = BoundDirectory::open(&context.resolve(&target).unwrap()).unwrap();
        assert_eq!(directory.identity(), direct.identity());
        assert!(names(&mut directory).contains(OsStr::new("message.eml")));
        drop(directory);
        drop(direct);
        drop(actor);
        // Remove only the owned fixture's link before recursive fixture cleanup.
        std::fs::remove_dir(selected).unwrap();
        root.close().unwrap();
    }
}
