// Early native feasibility gate, adapted from the owner-approved design probe.
// Synthetic data and an exclusively owned temporary root only; not a product processor.
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
    assert!(
        std::process::Command::new("mkfifo")
            .arg(root.path().join("moved/fifo.eml"))
            .status()?
            .success()
    );
    let fifo = fs::openat(
        &parent,
        "fifo.eml",
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    assert_eq!(
        fs::FileType::from_raw_mode(fs::fstat(&fifo)?.st_mode),
        fs::FileType::Fifo
    );
    fs::fsync(&parent)?;
    fs::fsync(&rootfd)?;
    println!(
        "PASS: stage0700 candidate0600; descriptor-bound publication survives parent/stage path replacement; final identity matches; requested-address mismatch detected; EEXIST preserves winner; FIFO open returns and fstat rejects; file/parent/root fsync complete"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        ffi::c_void,
        io::Write,
        mem::{offset_of, size_of},
        os::windows::{
            ffi::OsStrExt,
            fs::OpenOptionsExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
    };
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::*,
        System::Threading::{GetCurrentProcess, IsWow64Process2},
    };
    let mut process_machine = 0;
    let mut native_machine = 0;
    // SAFETY: current-process pseudo-handle is valid, both output pointers reference live u16s.
    if unsafe {
        IsWow64Process2(
            GetCurrentProcess(),
            &mut process_machine,
            &mut native_machine,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let expected_machine = if cfg!(target_arch = "aarch64") {
        0xaa64
    } else {
        0x8664
    };
    assert_eq!(
        process_machine, 0,
        "emulated execution cannot qualify native behavior"
    );
    assert_eq!(native_machine, expected_machine, "unexpected runtime CPU");
    println!("Native Windows machine: {native_machine:#x}; no emulation");
    let root = tempfile::tempdir()?;
    let directory = root.path().join("destination");
    std::fs::create_dir(&directory)?;
    let wide: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: NUL-terminated path; null security/template pointers; returned handle immediately owned.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().into());
    }
    let parent = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut candidate = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
        .share_mode(0)
        .create_new(true)
        .open(directory.join("stage.tmp"))?;
    candidate.write_all(b"verified synthetic mail")?;
    candidate.sync_all()?;
    assert!(
        std::fs::rename(&directory, root.path().join("moved")).is_err(),
        "parent rename unexpectedly succeeded"
    );
    let rename = |file: &std::fs::File| -> std::io::Result<()> {
        let name: Vec<u16> = "copy.eml".encode_utf16().collect();
        let bytes = offset_of!(FILE_RENAME_INFO, FileName) + (name.len() + 1) * 2;
        let count = bytes
            .max(size_of::<FILE_RENAME_INFO>())
            .div_ceil(size_of::<usize>());
        let mut storage = vec![0usize; count];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        // SAFETY: usize allocation supplies native pointer alignment; capacity covers the complete
        // header, UTF-16 name and NUL; name length is checked/u32-bounded before FFI in product.
        unsafe {
            std::ptr::addr_of_mut!((*info).Anonymous.Flags).write(0);
            std::ptr::addr_of_mut!((*info).RootDirectory).write(parent.as_raw_handle());
            std::ptr::addr_of_mut!((*info).FileNameLength).write((name.len() * 2) as u32);
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                storage
                    .as_mut_ptr()
                    .cast::<u8>()
                    .add(offset_of!(FILE_RENAME_INFO, FileName))
                    .cast::<u16>(),
                name.len(),
            );
            if SetFileInformationByHandle(
                file.as_raw_handle(),
                FileRenameInfo,
                info.cast::<c_void>(),
                (count * size_of::<usize>()) as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    };
    rename(&candidate)?;
    let other = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
        .share_mode(0)
        .create_new(true)
        .open(directory.join("second.tmp"))?;
    let error = rename(&other).expect_err("exclusive rename replaced existing final");
    assert!(
        matches!(error.raw_os_error(), Some(80) | Some(183)),
        "unexpected rename error: {error}"
    );
    let namespace = unsafe { FlushFileBuffers(parent.as_raw_handle()) };
    let namespace_error = (namespace == 0).then(std::io::Error::last_os_error);
    drop(other);
    drop(candidate);
    assert_eq!(
        std::fs::read(directory.join("copy.eml"))?,
        b"verified synthetic mail"
    );
    if let Some(error) = namespace_error {
        return Err(format!(
            "publication passed; directory FlushFileBuffers unavailable/failed: {error}"
        )
        .into());
    }
    println!(
        "PASS: exclusive stage handle; parent rename denied; held-source/bound-directory no-replace rename; final bytes match; file and directory FlushFileBuffers complete"
    );
    Ok(())
}
