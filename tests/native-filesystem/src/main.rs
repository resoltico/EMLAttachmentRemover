//! Native filesystem contract probe using synthetic data and an exclusively owned
//! temporary root. This executable does not process user messages.
#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use rustix::fs::{self, AtFlags, Mode, OFlags};
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let root = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()?;
    std::fs::create_dir(root.path().join("destination"))?;
    let rootfd = fs::open(
        root.path(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let parent = fs::openat(
        &rootfd,
        "destination",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    fs::mkdirat(&parent, "stage", Mode::from_raw_mode(0o700))?;
    let stage = fs::openat(
        &parent,
        "stage",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    fs::fchmod(&stage, Mode::from_raw_mode(0o700))?;
    #[cfg(target_os = "macos")]
    clear_acl(&stage)?;
    assert_eq!(fs::fstat(&stage)?.st_mode & 0o777, 0o700);
    let mut candidate = std::fs::File::from(fs::openat(
        &stage,
        "candidate",
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    candidate.write_all(b"verified synthetic mail")?;
    assert_eq!(fs::fstat(&candidate)?.st_mode & 0o777, 0o600);
    let held = fs::fstat(&candidate)?;
    let entry = fs::statat(&stage, "candidate", AtFlags::SYMLINK_NOFOLLOW)?;
    assert_eq!((held.st_dev, held.st_ino), (entry.st_dev, entry.st_ino));
    fs::fsync(&candidate)?;
    #[cfg(target_os = "macos")]
    fs::fcntl_fullfsync(&candidate)?;
    // Replace BOTH movable path prefixes, leaving the original descriptors live.
    std::fs::rename(root.path().join("destination"), root.path().join("moved"))?;
    std::fs::create_dir(root.path().join("destination"))?;
    std::fs::create_dir(root.path().join("destination/stage"))?;
    std::fs::write(
        root.path().join("destination/stage/candidate"),
        b"substitution",
    )?;
    #[cfg(target_os = "macos")]
    fs::renameat_with(
        &stage,
        "candidate",
        &parent,
        "copy.eml",
        fs::RenameFlags::NOREPLACE,
    )?;
    #[cfg(target_os = "linux")]
    fs::linkat(&stage, "candidate", &parent, "copy.eml", AtFlags::empty())?;
    assert_eq!(
        std::fs::read(root.path().join("moved/copy.eml"))?,
        b"verified synthetic mail"
    );
    assert!(!root.path().join("destination/copy.eml").exists());
    let final_entry = fs::statat(&parent, "copy.eml", AtFlags::SYMLINK_NOFOLLOW)?;
    assert_eq!(
        (held.st_dev, held.st_ino),
        (final_entry.st_dev, final_entry.st_ino)
    );
    let requested_parent = fs::statat(&rootfd, "destination", AtFlags::SYMLINK_NOFOLLOW)?;
    let bound_parent = fs::fstat(&parent)?;
    assert_ne!(
        (requested_parent.st_dev, requested_parent.st_ino),
        (bound_parent.st_dev, bound_parent.st_ino)
    );
    let second = fs::openat(
        &stage,
        "second",
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )?;
    #[cfg(target_os = "macos")]
    let conflict = fs::renameat_with(
        &stage,
        "second",
        &parent,
        "copy.eml",
        fs::RenameFlags::NOREPLACE,
    );
    #[cfg(target_os = "linux")]
    let conflict = fs::linkat(&stage, "second", &parent, "copy.eml", AtFlags::empty());
    assert_eq!(conflict, Err(rustix::io::Errno::EXIST));
    drop(second);
    reject_fifo(&parent, root.path())?;
    fs::fsync(&parent)?;
    fs::fsync(&rootfd)?;
    println!(
        "PASS: stage0700 candidate0600; descriptor-bound publication survives parent/stage path replacement; final identity matches; requested-address mismatch detected; EEXIST preserves winner; FIFO open returns and fstat rejects; file/parent/root fsync complete"
    );
    Ok(())
}

#[cfg(unix)]
fn reject_fifo(
    parent: &impl std::os::fd::AsFd,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use rustix::fs::{self, Mode, OFlags};
    assert!(
        std::process::Command::new("mkfifo")
            .arg(root.join("moved/fifo.eml"))
            .status()?
            .success()
    );
    let fifo = fs::openat(
        parent,
        "fifo.eml",
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    assert_eq!(
        fs::FileType::from_raw_mode(fs::fstat(&fifo)?.st_mode),
        fs::FileType::Fifo
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "The independent native filesystem probe calls the Darwin opaque ACL ABI only in this borrowed-descriptor adapter and frees every allocated ACL. Reusing product ACL code would remove its independence as a native boundary oracle."
)]
fn clear_acl(fd: &impl std::os::fd::AsRawFd) -> std::io::Result<()> {
    use std::{
        ffi::{c_int, c_void},
        ptr,
    };
    unsafe extern "C" {
        fn acl_init(count: c_int) -> *mut c_void;
        fn acl_set_fd(fd: c_int, acl: *mut c_void) -> c_int;
        fn acl_get_fd(fd: c_int) -> *mut c_void;
        fn acl_get_entry(acl: *mut c_void, entry_id: c_int, entry: *mut *mut c_void) -> c_int;
        fn acl_free(acl: *mut c_void) -> c_int;
    }
    // SAFETY: Apple system ACL ABI; fd is borrowed/live; each returned ACL is freed once.
    unsafe {
        let empty = acl_init(0);
        if empty.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let set = acl_set_fd(fd.as_raw_fd(), empty);
        let set_error = (set != 0).then(std::io::Error::last_os_error);
        let free = acl_free(empty);
        if let Some(error) = set_error {
            return Err(error);
        }
        if free != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let actual = acl_get_fd(fd.as_raw_fd());
        if actual.is_null() {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(2) {
                return Ok(());
            }
            return Err(e);
        }
        let mut entry = ptr::null_mut();
        // ACL_FIRST_ENTRY is 0 on Darwin (SDK sys/acl.h).
        let result = acl_get_entry(actual, 0, &mut entry);
        let inspect_error = (result != 0).then(std::io::Error::last_os_error);
        let released = acl_free(actual);
        if let Some(error) = inspect_error {
            // Darwin uses -1/EINVAL when the requested first ACL entry does not exist.
            if error.raw_os_error() != Some(22) {
                return Err(error);
            }
        }
        if !entry.is_null() {
            return Err(std::io::Error::other("ACL contains entries"));
        }
        if released != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows::run()
}
