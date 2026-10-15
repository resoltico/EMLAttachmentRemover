//! Regular-file handles. Reading does not apply destination namespace policy.
use crate::{
    BoundDirectory, FileSignature,
    unix::{component, identity},
};
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use std::{
    ffi::{OsStr, OsString},
    io,
    os::fd::OwnedFd,
};

pub(crate) fn signature(stat: &fs::Stat) -> io::Result<FileSignature> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The opened object is not an ordinary regular file",
        ));
    }
    if stat.st_ino == 0 || stat.st_size < 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "The volume supplied no usable regular-file identity or length",
        ));
    }
    Ok(FileSignature {
        identity: identity(stat)?,
        size: stat.st_size as u64,
        modified_seconds: stat.st_mtime,
        modified_nanoseconds: nanos(stat.st_mtime_nsec)?,
        changed_seconds: stat.st_ctime,
        changed_nanoseconds: nanos(stat.st_ctime_nsec)?,
    })
}
fn nanos(value: impl TryInto<i64>) -> io::Result<i64> {
    let value = value
        .try_into()
        .map_err(|_| io::Error::other("Invalid native nanoseconds"))?;
    if !(0..1_000_000_000).contains(&value) {
        return Err(io::Error::other("Native nanoseconds out of range"));
    }
    Ok(value)
}

/// A read-only ordinary file with the parent/name binding retained for revalidation.
pub struct BoundFile {
    fd: OwnedFd,
    parent: BoundDirectory,
    name: OsString,
    follow_final: bool,
    initial: FileSignature,
}
impl BoundDirectory {
    /// Open one regular file without blocking on a substituted FIFO. Only explicit
    /// selected links may set follow_final; walked/existing files must pass false.
    pub fn open_file(&self, name: &OsStr, follow_final: bool) -> io::Result<BoundFile> {
        component(name)?;
        let mut flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC;
        if !follow_final {
            flags |= OFlags::NOFOLLOW;
        }
        let fd = fs::openat(self, name, flags, Mode::empty())?;
        // Type is checked AFTER the nonblocking open, before any read.
        let initial = signature(&fs::fstat(&fd)?)?;
        let held = BoundFile {
            fd,
            parent: self.clone(),
            name: name.to_owned(),
            follow_final,
            initial,
        };
        held.check_binding()?;
        Ok(held)
    }
}
impl BoundFile {
    /// Resynchronize this held existing ordinary file without modifying its bytes
    /// or metadata. Namespace/address receipts are separate.
    pub fn sync_file(&self) -> crate::SyncReceipt {
        crate::candidate::file_sync(&self.fd)
    }
    /// Signature observed on the opened regular file.
    pub fn initial_signature(&self) -> FileSignature {
        self.initial
    }
    /// Observe current held-file metadata; changes are never silently rebaselined.
    pub fn current_signature(&self) -> io::Result<FileSignature> {
        signature(&fs::fstat(&self.fd)?)
    }
    /// Bounded positional read through the same held file. Interrupted syscalls
    /// return to the workflow so it can check its cancellation token.
    pub fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        Ok(rustix::io::pread(&self.fd, buffer, offset)?)
    }
    /// Check both observable metadata and the entry currently reached through the
    /// held parent. This detects observations, not an atomic POSIX content snapshot.
    pub fn check_binding(&self) -> io::Result<()> {
        let flags = if self.follow_final {
            AtFlags::empty()
        } else {
            AtFlags::SYMLINK_NOFOLLOW
        };
        let entry = signature(&fs::statat(&self.parent, &self.name, flags)?)?;
        if entry != self.initial || self.current_signature()? != self.initial {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "File identity or observable metadata changed",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::{ffi::OsStringExt, fs::PermissionsExt},
        time::Instant,
    };
    fn root() -> tempfile::TempDir {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fs-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(base)
            .unwrap()
    }
    #[test]
    fn file_read_preserves_native_name_and_selected_link_identity() {
        let root = root();
        let parent = BoundDirectory::open(root.path()).unwrap();
        let name = OsString::from_vec(b"mail-\xff.eml".to_vec());
        let creation = std::fs::write(root.path().join(&name), b"synthetic");
        #[cfg(target_os = "macos")]
        let name = if creation
            .as_ref()
            .is_err_and(|error| error.raw_os_error() == Some(libc::EILSEQ))
        {
            // APFS can reject invalid UTF-8 itself. Record that actual boundary;
            // it supplies no non-Unicode round-trip evidence for this volume.
            eprintln!(
                "Native volume rejected invalid UTF-8 with EILSEQ; continuing binding checks with a representable name"
            );
            let name = OsString::from("mail-e\u{301}.eml");
            std::fs::write(root.path().join(&name), b"synthetic").unwrap();
            name
        } else {
            creation.unwrap();
            name
        };
        #[cfg(not(target_os = "macos"))]
        creation.unwrap();
        let file = parent.open_file(&name, false).unwrap();
        let mut bytes = [0; 20];
        assert_eq!(file.read_at(&mut bytes, 0).unwrap(), 9);
        assert_eq!(&bytes[..9], b"synthetic");
        file.check_binding().unwrap();
        std::os::unix::fs::symlink(&name, root.path().join("alias")).unwrap();
        assert!(parent.open_file(OsStr::new("alias"), false).is_err());
        let alias = parent.open_file(OsStr::new("alias"), true).unwrap();
        assert_eq!(
            alias.initial_signature().identity,
            file.initial_signature().identity
        );
        std::fs::remove_file(root.path().join("alias")).unwrap();
        std::os::unix::fs::symlink("missing", root.path().join("alias")).unwrap();
        assert!(alias.check_binding().is_err());
    }
    #[test]
    fn fifo_and_directory_are_rejected_before_any_read() {
        let root = root();
        let parent = BoundDirectory::open(root.path()).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(root.path().join("fifo"))
                .status()
                .unwrap()
                .success()
        );
        let started = Instant::now();
        assert!(parent.open_file(OsStr::new("fifo"), false).is_err());
        assert!(started.elapsed().as_secs() < 2);
        std::fs::create_dir(root.path().join("dir")).unwrap();
        assert!(parent.open_file(OsStr::new("dir"), false).is_err());
    }
    #[test]
    fn replacement_growth_truncation_and_unsafe_destination_mode_do_not_hide_source_reads() {
        for change in ["replace", "grow", "truncate"] {
            let root = root();
            let parent = BoundDirectory::open(root.path()).unwrap();
            let path = root.path().join("source");
            std::fs::write(&path, b"synthetic").unwrap();
            let file = parent.open_file(OsStr::new("source"), false).unwrap();
            match change {
                "replace" => {
                    std::fs::rename(&path, root.path().join("moved")).unwrap();
                    std::fs::write(&path, b"synthetic").unwrap();
                }
                "grow" => std::fs::write(&path, b"synthetic extension").unwrap(),
                _ => std::fs::write(&path, b"").unwrap(),
            }
            assert!(file.check_binding().is_err(), "{change}");
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
            assert!(parent.admit_namespace().is_err());
            assert!(parent.open_file(OsStr::new("source"), false).is_ok());
        }
    }
}
