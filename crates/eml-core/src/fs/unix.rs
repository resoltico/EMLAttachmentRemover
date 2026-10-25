//! Descriptor-relative Unix operations; all unsafe OS details stay in rustix.
use super::{BoundFileError, EntryKind, EntryObservation, FileSignature, Identity};
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, RenameFlags};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    os::{fd::OwnedFd, unix::ffi::OsStringExt},
    path::Path,
};
pub(super) struct Directory(OwnedFd);
pub(super) struct Entries(fs::Dir);
fn identity(stat: &fs::Stat) -> io::Result<Identity> {
    if stat.st_ino == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Filesystem did not supply a usable file identity",
        ));
    }
    #[expect(
        clippy::unnecessary_cast,
        reason = "Native device width differs between Unix targets"
    )]
    Ok(Identity::Posix {
        device: stat.st_dev as u64,
        inode: stat.st_ino,
    })
}
pub(super) fn open_directory(
    parent: Option<&Directory>,
    path: &Path,
    follow: bool,
) -> io::Result<Directory> {
    let mut flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NONBLOCK | OFlags::CLOEXEC;
    if !follow {
        flags |= OFlags::NOFOLLOW;
    }
    Ok(Directory(match parent {
        Some(parent) => fs::openat(&parent.0, path, flags, Mode::empty())?,
        None => fs::open(path, flags, Mode::empty())?,
    }))
}
pub(super) fn directory_identity(directory: &Directory) -> io::Result<Identity> {
    let stat = fs::fstat(&directory.0)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Held object is not a directory",
        ));
    }
    identity(&stat)
}
pub(super) fn name_limit(directory: &Directory) -> io::Result<usize> {
    Ok(usize::try_from(fs::fstatvfs(&directory.0)?.f_namemax)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(255))
}
pub(super) fn sync_directory(directory: &Directory) -> io::Result<()> {
    Ok(fs::fsync(&directory.0)?)
}
pub(super) fn entries(directory: &Directory) -> io::Result<Entries> {
    Ok(Entries(fs::Dir::read_from(&directory.0)?))
}
pub(super) fn next_entry(entries: &mut Entries) -> Option<io::Result<OsString>> {
    entries.0.next().map(|entry| {
        entry
            .map(|entry| OsString::from_vec(entry.file_name().to_bytes().to_vec()))
            .map_err(Into::into)
    })
}
fn file_signature(stat: &fs::Stat) -> Result<FileSignature, BoundFileError> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(BoundFileError::NotRegular);
    }
    if stat.st_size < 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Negative file length").into());
    }
    Ok(FileSignature {
        identity: identity(stat)?,
        size: stat.st_size as u64,
        modified_native: [stat.st_mtime, nanos(stat.st_mtime_nsec)?],
        changed_native: [stat.st_ctime, nanos(stat.st_ctime_nsec)?],
    })
}
fn nanos(value: impl TryInto<i64>) -> io::Result<i64> {
    value
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid native timestamp"))
}
pub(super) fn signature(file: &File) -> Result<FileSignature, BoundFileError> {
    file_signature(&fs::fstat(file)?)
}
pub(super) fn observe(
    directory: &Directory,
    path: &Path,
    follow: bool,
) -> io::Result<EntryObservation> {
    let stat = fs::statat(
        &directory.0,
        path,
        if follow {
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
            Some(file_signature(&stat).map_err(BoundFileError::into_io_error)?)
        } else {
            None
        },
    })
}
pub(super) fn open_file(directory: &Directory, name: &OsStr, follow: bool) -> io::Result<File> {
    let mut flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC;
    if !follow {
        flags |= OFlags::NOFOLLOW;
    }
    Ok(File::from(fs::openat(
        &directory.0,
        name,
        flags,
        Mode::empty(),
    )?))
}
pub(super) fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    Ok(rustix::io::pread(file, buffer, offset)?)
}
pub(super) fn sync_existing(file: &File, _: &Directory, _: &OsStr, _: Identity) -> io::Result<()> {
    file.sync_all()
}
pub(super) fn create_directory(directory: &Directory, name: &OsStr) -> io::Result<()> {
    Ok(fs::mkdirat(&directory.0, name, Mode::from_raw_mode(0o700))?)
}
pub(super) fn create_file(directory: &Directory, name: &OsStr) -> io::Result<File> {
    Ok(File::from(fs::openat(
        &directory.0,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?))
}
pub(super) fn remove(directory: &Directory, name: &OsStr, is_directory: bool) -> io::Result<()> {
    Ok(fs::unlinkat(
        &directory.0,
        name,
        if is_directory {
            AtFlags::REMOVEDIR
        } else {
            AtFlags::empty()
        },
    )?)
}
pub(super) fn publish(directory: &Directory, from: &OsStr, to: &OsStr) -> io::Result<()> {
    let rename = || fs::renameat_with(&directory.0, from, &directory.0, to, RenameFlags::NOREPLACE);
    #[cfg(target_os = "linux")]
    {
        publish_with(directory, from, to, rename)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(rename()?)
    }
}
#[cfg(target_os = "linux")]
fn publish_with(
    directory: &Directory,
    from: &OsStr,
    to: &OsStr,
    rename: impl FnOnce() -> Result<(), rustix::io::Errno>,
) -> io::Result<()> {
    match rename() {
        Ok(()) => Ok(()),
        // Some Linux filesystems implement linkat but not RENAME_NOREPLACE.
        #[cfg(target_os = "linux")]
        Err(rustix::io::Errno::INVAL) => {
            fs::linkat(&directory.0, from, &directory.0, to, AtFlags::empty())?;
            Ok(fs::unlinkat(&directory.0, from, AtFlags::empty())?)
        }
        Err(error) => Err(error.into()),
    }
}
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn unsupported_rename_falls_back_without_replacing_an_existing_name() {
        let root = tempfile::tempdir().unwrap();
        let dir = open_directory(None, root.path(), false).unwrap();
        std::fs::write(root.path().join("staging"), b"complete").unwrap();
        let fallback = || Err(rustix::io::Errno::INVAL);
        publish_with(&dir, OsStr::new("staging"), OsStr::new("final"), fallback).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("final")).unwrap(),
            b"complete"
        );
        assert!(!root.path().join("staging").exists());
        std::fs::write(root.path().join("staging"), b"loser").unwrap();
        assert_eq!(
            publish_with(&dir, OsStr::new("staging"), OsStr::new("final"), fallback)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            std::fs::read(root.path().join("final")).unwrap(),
            b"complete"
        );
        assert_eq!(
            std::fs::read(root.path().join("staging")).unwrap(),
            b"loser"
        );
    }
}
