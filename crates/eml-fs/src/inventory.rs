//! Native observations and streamed names; ordering/limits/selection belong to core.
use crate::{
    BoundDirectory, EntryKind, EntryObservation,
    file::signature,
    unix::{component, identity},
};
use rustix::fs::{self, AtFlags, FileType};
use std::{
    ffi::{OsStr, OsString},
    io,
    os::unix::ffi::OsStringExt,
    path::Path,
};

/// Streaming directory names; caller must bound discovery before sorting.
pub struct DirectoryEntries(fs::Dir);
impl Iterator for DirectoryEntries {
    type Item = io::Result<OsString>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|entry| {
            entry
                .map(|entry| OsString::from_vec(entry.file_name().to_bytes().to_vec()))
                .map_err(Into::into)
        })
    }
}
impl BoundDirectory {
    /// Observe one no-follow child, mapping only native ENOENT to point absence.
    /// None is not a parent/address qualification or permission to create.
    pub fn observe_child_if_present(&self, name: &OsStr) -> io::Result<Option<EntryObservation>> {
        match self.observe_child(name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.raw_os_error() == Some(libc::ENOENT) => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// Observe an explicit selected expression through this held directory,
    /// following its final link. No object data is opened or read. Absolute
    /// expressions and symlink/dot-dot traversal retain native OS semantics.
    pub fn observe_selected(&self, expression: &Path) -> io::Result<EntryObservation> {
        observation(&fs::statat(self, expression, AtFlags::empty())?)
    }
    /// Enumerate a fresh directory stream from this held directory, independent of
    /// the selection pathname. Includes dot entries; core applies its skip policy.
    pub fn read_entries(&self) -> io::Result<DirectoryEntries> {
        Ok(DirectoryEntries(fs::Dir::read_from(self)?))
    }
    /// Observe one child component without following its final link or opening
    /// object data. Explicit selected expressions use observe_selected instead.
    pub fn observe_child(&self, name: &OsStr) -> io::Result<EntryObservation> {
        component(name)?;
        let stat = fs::statat(self, name, AtFlags::SYMLINK_NOFOLLOW)?;
        observation(&stat)
    }
}
fn observation(stat: &fs::Stat) -> io::Result<EntryObservation> {
    let kind = match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => EntryKind::File,
        FileType::Directory => EntryKind::Directory,
        FileType::Symlink => EntryKind::Symlink,
        _ => EntryKind::Special,
    };
    Ok(EntryObservation {
        identity: identity(stat)?,
        kind,
        file: if kind == EntryKind::File {
            Some(signature(stat).map_err(crate::BoundFileError::into_io_error)?)
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, os::unix::fs::PermissionsExt};
    #[test]
    fn optional_child_observation_keeps_dangling_links_and_invalid_inputs_visible() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file.eml"), b"owned").unwrap();
        std::os::unix::fs::symlink("missing-target", root.path().join("dangling")).unwrap();
        let directory = BoundDirectory::open(root.path()).unwrap();
        assert!(
            directory
                .observe_child_if_present(OsStr::new("missing"))
                .unwrap()
                .is_none()
        );
        let file = directory
            .observe_child_if_present(OsStr::new("file.eml"))
            .unwrap()
            .unwrap();
        assert_eq!(
            file,
            directory.observe_child(OsStr::new("file.eml")).unwrap()
        );
        assert_eq!(file.kind, EntryKind::File);
        assert_eq!(
            directory
                .observe_child_if_present(OsStr::new("dangling"))
                .unwrap()
                .unwrap()
                .kind,
            EntryKind::Symlink
        );
        assert_eq!(
            directory
                .observe_child_if_present(OsStr::new("a/b"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        drop(directory);
        root.close().unwrap();
    }
    #[test]
    fn selected_relative_expressions_use_the_held_working_directory_after_its_address_moves() {
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("working");
        let moved = root.path().join("moved");
        std::fs::create_dir_all(selected.join("mail")).unwrap();
        std::fs::write(
            selected.join("mail/source.eml"),
            b"held directory synthetic",
        )
        .unwrap();
        let cwd = BoundDirectory::open(&selected).unwrap();
        let expected = cwd.observe_selected(Path::new("mail/source.eml")).unwrap();
        std::fs::rename(&selected, &moved).unwrap();
        std::fs::create_dir_all(selected.join("mail")).unwrap();
        std::fs::write(selected.join("mail/source.eml"), b"different synthetic").unwrap();
        let substitute = BoundDirectory::open(&selected).unwrap();
        assert_ne!(
            expected.identity,
            substitute
                .observe_selected(Path::new("mail/source.eml"))
                .unwrap()
                .identity
        );
        assert_eq!(
            cwd.observe_selected(Path::new("mail/source.eml")).unwrap(),
            expected
        );
        let held_mail = cwd.open_selected(Path::new("mail")).unwrap();
        assert_eq!(
            held_mail.identity(),
            BoundDirectory::open(&moved.join("mail"))
                .unwrap()
                .identity()
        );
        assert_eq!(
            held_mail.requested_path().as_os_str(),
            selected.join("mail").as_os_str()
        );
        assert!(!held_mail.check_requested_address().unwrap());
        assert_eq!(
            held_mail.observe_child(OsStr::new("source.eml")).unwrap(),
            expected
        );
        // Absolute selection is independent of the base descriptor.
        assert_eq!(
            cwd.open_selected(&selected).unwrap().identity(),
            substitute.identity()
        );
        assert_eq!(
            cwd.observe_selected(&selected.join("mail/source.eml"))
                .unwrap(),
            substitute
                .observe_selected(Path::new("mail/source.eml"))
                .unwrap()
        );
        root.close().unwrap();
    }
    #[test]
    fn selected_symlink_dot_dot_is_resolved_by_the_kernel_without_lexical_collapse() {
        let root = tempfile::tempdir().unwrap();
        let working = root.path().join("working");
        let tree = root.path().join("tree");
        std::fs::create_dir(&working).unwrap();
        std::fs::create_dir_all(tree.join("deep")).unwrap();
        std::fs::write(tree.join("source.eml"), b"physical parent").unwrap();
        std::fs::write(working.join("source.eml"), b"lexical parent").unwrap();
        std::os::unix::fs::symlink(tree.join("deep"), working.join("alias")).unwrap();
        let cwd = BoundDirectory::open(&working).unwrap();
        let physical = BoundDirectory::open(&tree).unwrap();
        let selected = cwd.open_selected(Path::new("alias/..")).unwrap();
        assert_eq!(selected.identity(), physical.identity());
        assert_ne!(selected.identity(), cwd.identity());
        assert_eq!(
            selected.requested_path().as_os_str(),
            working.join("alias/..").as_os_str()
        );
        assert_eq!(
            cwd.observe_selected(Path::new("alias/.."))
                .unwrap()
                .identity,
            physical.identity()
        );
        let observed = cwd
            .observe_selected(Path::new("alias/../source.eml"))
            .unwrap();
        assert_eq!(
            observed,
            physical.observe_child(OsStr::new("source.eml")).unwrap()
        );
        assert_ne!(
            observed.identity,
            cwd.observe_child(OsStr::new("source.eml"))
                .unwrap()
                .identity
        );
        assert!(
            cwd.open_child(OsStr::new("alias")).is_err(),
            "walked children still refuse links"
        );
        root.close().unwrap();
    }
    #[test]
    fn selected_metadata_preserves_native_names_and_distinguishes_special_and_missing_entries() {
        let root = tempfile::tempdir().unwrap();
        let native = OsString::from_vec(vec![b'm', 0xff, b'l']);
        let creation = std::fs::write(root.path().join(&native), b"native synthetic");
        #[cfg(target_os = "macos")]
        let native = if creation
            .as_ref()
            .is_err_and(|error| error.raw_os_error() == Some(libc::EILSEQ))
        {
            eprintln!(
                "Native volume rejected invalid UTF-8 with EILSEQ; this volume provides no non-Unicode round-trip evidence"
            );
            let representable = OsString::from("mail-e\u{301}");
            std::fs::write(root.path().join(&representable), b"native synthetic").unwrap();
            representable
        } else {
            creation.unwrap();
            native
        };
        #[cfg(not(target_os = "macos"))]
        creation.unwrap();
        std::os::unix::fs::symlink(&native, root.path().join("selected-link")).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(root.path().join("fifo"))
                .status()
                .unwrap()
                .success()
        );
        let cwd = BoundDirectory::open(root.path()).unwrap();
        let source = cwd.observe_selected(Path::new(&native)).unwrap();
        assert_eq!(source.kind, EntryKind::File);
        assert_eq!(source.file.unwrap().size, 16);
        assert_eq!(
            cwd.observe_selected(Path::new("selected-link")).unwrap(),
            source
        );
        let fifo = cwd.observe_selected(Path::new("fifo")).unwrap();
        assert_eq!(fifo.kind, EntryKind::Special);
        assert!(fifo.file.is_none());
        assert!(cwd.open_selected(Path::new("fifo")).is_err());
        assert!(cwd.open_selected(Path::new(&native)).is_err());
        for expression in ["", "missing", "a\0b"] {
            assert!(cwd.observe_selected(Path::new(expression)).is_err());
            assert!(cwd.open_selected(Path::new(expression)).is_err());
        }
        assert_eq!(
            std::fs::read(root.path().join(&native)).unwrap(),
            b"native synthetic"
        );
        let actual: BTreeSet<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            actual,
            BTreeSet::from([
                native,
                OsString::from("selected-link"),
                OsString::from("fifo")
            ])
        );
        root.close().unwrap();
    }
    #[test]
    fn enumeration_and_observations_use_the_held_directory_after_address_replacement() {
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let selected = root.path().join("selected");
        std::fs::create_dir(&selected).unwrap();
        std::fs::write(selected.join("message.eml"), b"synthetic").unwrap();
        std::os::unix::fs::symlink("message.eml", selected.join("alias")).unwrap();
        let parent = BoundDirectory::open(&selected).unwrap();
        std::fs::rename(&selected, root.path().join("moved")).unwrap();
        std::fs::create_dir(&selected).unwrap();
        std::fs::write(selected.join("substitute"), b"unrelated synthetic").unwrap();
        let names: BTreeSet<_> = parent.read_entries().unwrap().map(Result::unwrap).collect();
        assert!(names.contains(OsStr::new("message.eml")));
        assert!(!names.contains(OsStr::new("substitute")));
        let file = parent.observe_child(OsStr::new("message.eml")).unwrap();
        assert_eq!(file.kind, EntryKind::File);
        assert_eq!(
            file.file.unwrap(),
            parent
                .open_file(OsStr::new("message.eml"), false)
                .unwrap()
                .initial_signature()
        );
        assert_eq!(
            parent.observe_child(OsStr::new("alias")).unwrap().kind,
            EntryKind::Symlink
        );
        assert_eq!(
            parent
                .observe_selected(Path::new("alias"))
                .unwrap()
                .identity,
            file.identity
        );
        assert_eq!(
            names,
            parent.read_entries().unwrap().map(Result::unwrap).collect()
        );
    }
}
