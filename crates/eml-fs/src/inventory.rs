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
    /// Enumerate a fresh directory stream from this held directory, independent of
    /// the selection pathname. Includes dot entries; core applies its skip policy.
    pub fn read_entries(&self) -> io::Result<DirectoryEntries> {
        Ok(DirectoryEntries(fs::Dir::read_from(self)?))
    }
    /// Observe one component without a blocking file open. Only explicit selected
    /// root links may request final-link following.
    pub fn observe_child(&self, name: &OsStr, follow_final: bool) -> io::Result<EntryObservation> {
        component(name)?;
        let stat = fs::statat(
            self,
            name,
            if follow_final {
                AtFlags::empty()
            } else {
                AtFlags::SYMLINK_NOFOLLOW
            },
        )?;
        let kind = match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => EntryKind::File,
            FileType::Directory => EntryKind::Directory,
            FileType::Symlink => EntryKind::Symlink,
            _ => EntryKind::Special,
        };
        Ok(EntryObservation {
            identity: identity(&stat)?,
            kind,
            file: if kind == EntryKind::File {
                Some(signature(&stat)?)
            } else {
                None
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, os::unix::fs::PermissionsExt};
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
        let file = parent
            .observe_child(OsStr::new("message.eml"), false)
            .unwrap();
        assert_eq!(file.kind, EntryKind::File);
        assert_eq!(
            file.file.unwrap(),
            parent
                .open_file(OsStr::new("message.eml"), false)
                .unwrap()
                .initial_signature()
        );
        assert_eq!(
            parent
                .observe_child(OsStr::new("alias"), false)
                .unwrap()
                .kind,
            EntryKind::Symlink
        );
        assert_eq!(
            parent
                .observe_child(OsStr::new("alias"), true)
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
