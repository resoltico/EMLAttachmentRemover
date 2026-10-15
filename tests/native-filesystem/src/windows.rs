//! Early native experiment. Neither mode is a shipped processor or platform qualification.
use std::{
    error::Error,
    ffi::{OsStr, c_void},
    io::{Read, Seek, SeekFrom, Write},
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE},
    Security::*,
    Storage::FileSystem::*,
    System::Threading::{GetCurrentProcess, IsWow64Process2, OpenProcessToken},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct OrdinaryToken(OwnedHandle);
impl OrdinaryToken {
    fn enter() -> Result<Self> {
        let mut raw = ptr::null_mut();
        // SAFETY: process pseudo-handle is valid; output is a live HANDLE slot.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut raw) }
            == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: successful OpenProcessToken transfers ownership of exactly one handle.
        let original = unsafe { OwnedHandle::from_raw_handle(raw) };
        // SID storage is DWORD aligned and larger than SECURITY_MAX_SID_SIZE.
        let mut sid = [0u32; 20];
        let mut sid_bytes = size_of_val(&sid) as u32;
        // SAFETY: buffer size describes allocated writable SID storage; no domain SID required.
        if unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                ptr::null_mut(),
                sid.as_mut_ptr().cast(),
                &mut sid_bytes,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let disabled = SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: 0,
        };
        let mut restricted = ptr::null_mut();
        // SAFETY: input token and SID remain live, all optional empty arrays are null,
        // output points at an owned-handle slot. Admin SID becomes deny-only; privileges disabled.
        if unsafe {
            CreateRestrictedToken(
                original.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE,
                1,
                &disabled,
                0,
                ptr::null(),
                0,
                ptr::null(),
                &mut restricted,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: successful CreateRestrictedToken transfers ownership of one handle.
        let token = Self(unsafe { OwnedHandle::from_raw_handle(restricted) });
        // SAFETY: token is a live token derived from our process; impersonation is thread local.
        if unsafe { ImpersonateLoggedOnUser(token.0.as_raw_handle()) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut admin_member = 1;
        // SAFETY: null token selects this thread's effective impersonation token;
        // SID storage and BOOL output remain live for the call.
        if unsafe { CheckTokenMembership(ptr::null_mut(), disabled.Sid, &mut admin_member) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        assert_eq!(
            admin_member, 0,
            "effective token still enables Administrators"
        );
        println!("PASS: executing with Administrators SID deny-only and privileges disabled");
        Ok(token)
    }
}
impl Drop for OrdinaryToken {
    fn drop(&mut self) {
        // SAFETY: restores this thread's previous process identity before releasing token.
        if unsafe { RevertToSelf() } == 0 {
            // Do not continue an experiment under an unexpectedly retained identity.
            std::process::abort();
        }
    }
}

fn wide(path: &OsStr) -> Vec<u16> {
    path.encode_wide().chain(Some(0)).collect()
}
fn directory(path: &Path, writable: bool, share_write: bool) -> Result<OwnedHandle> {
    let path = wide(path.as_os_str());
    let access = if writable {
        GENERIC_READ | GENERIC_WRITE
    } else {
        FILE_READ_ATTRIBUTES | FILE_TRAVERSE
    };
    // SAFETY: bounded terminated path, no inherited security/template handle;
    // returned handle is immediately owned, no write/delete sharing permitted.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | if share_write { FILE_SHARE_WRITE } else { 0 },
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(format!("pin directory: {}", std::io::Error::last_os_error()).into());
    }
    // SAFETY: raw is the successfully returned, uniquely owned handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: output buffer has generated type, proper alignment and exact byte size.
    if unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut attributes as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("reparse directory cannot be pinned as an ordinary ancestor".into());
    }
    Ok(handle)
}
fn identity(handle: HANDLE) -> Result<(u64, [u8; 16])> {
    let mut id = FILE_ID_INFO::default();
    // SAFETY: caller retains a live handle for this call, generated output type is aligned/sized.
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok((id.VolumeSerialNumber, id.FileId.Identifier))
}
fn rename(file: &std::fs::File, root: HANDLE, name: &OsStr) -> std::io::Result<()> {
    let name: Vec<u16> = name.encode_wide().collect();
    let name_bytes = name
        .len()
        .checked_mul(2)
        .ok_or_else(|| std::io::Error::other("name overflow"))?;
    let bytes = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes)
        .ok_or_else(|| std::io::Error::other("buffer overflow"))?
        .max(size_of::<FILE_RENAME_INFO>());
    let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: native usize alignment covers FILE_RENAME_INFO on both CPUs, storage covers
    // header plus exactly the checked name bytes; Windows does not require a trailing NUL.
    // file and optional root stay live; flags zero means ReplaceIfExists=false.
    unsafe {
        ptr::addr_of_mut!((*info).Anonymous.Flags).write(0);
        ptr::addr_of_mut!((*info).RootDirectory).write(root);
        ptr::addr_of_mut!((*info).FileNameLength).write(
            u32::try_from(name_bytes).map_err(|_| std::io::Error::other("name exceeds DWORD"))?,
        );
        ptr::copy_nonoverlapping(
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
            u32::try_from(bytes).map_err(|_| std::io::Error::other("buffer exceeds DWORD"))?,
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}
fn candidate(path: &Path, bytes: &[u8]) -> Result<std::fs::File> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
        .share_mode(0)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    file.seek(SeekFrom::Start(0))?;
    let mut observed = Vec::new();
    file.read_to_end(&mut observed)?;
    assert_eq!(observed, bytes);
    Ok(file)
}
fn flush(handle: &OwnedHandle, label: &str) -> Result<()> {
    // SAFETY: live write-capable directory handle, retained for the whole call.
    if unsafe { FlushFileBuffers(handle.as_raw_handle()) } == 0 {
        return Err(format!("{label}: {}", std::io::Error::last_os_error()).into());
    }
    println!("PASS: {label}");
    Ok(())
}
fn write_excluded(path: &Path) -> Result<bool> {
    let name = wide(path.as_os_str());
    // SAFETY: terminated live path; successful handle (which means the guard failed)
    // is immediately owned and dropped. We never attempt an actual mutation.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if raw != INVALID_HANDLE_VALUE {
        // SAFETY: the unexpected successful raw handle still has one owner.
        drop(unsafe { OwnedHandle::from_raw_handle(raw) });
        println!("UNSAFE EXPERIMENT: pinned destination admits concurrent reparse-write handle");
        return Ok(false);
    }
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(32));
    println!("PASS: pinned ordinary directory excludes concurrent reparse-write handle");
    Ok(true)
}

pub(super) fn run() -> Result<()> {
    let mut process_machine = 0;
    let mut native_machine = 0;
    // SAFETY: current process pseudo-handle and live u16 outputs.
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
    assert_eq!(
        process_machine, 0,
        "emulation does not qualify native behavior"
    );
    assert_eq!(
        native_machine,
        if cfg!(target_arch = "aarch64") {
            0xaa64
        } else {
            0x8664
        }
    );
    println!("Native Windows machine: {native_machine:#x}; no emulation");
    let _ordinary_token = OrdinaryToken::enter()?;
    let share_write = std::env::args_os().any(|arg| arg == "--diagnose-absolute-write-sharing");
    let absolute =
        share_write || std::env::args_os().any(|arg| arg == "--diagnose-absolute-pinned-name");
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    std::fs::create_dir(&destination)?;
    let destination = std::fs::canonicalize(destination)?;
    let mut pins = Vec::new();
    // Hold the resolved expression top down, never allowing write/delete sharing.
    let ancestors: Vec<_> = destination.ancestors().skip(1).collect();
    for ancestor in ancestors.into_iter().rev() {
        pins.push(directory(ancestor, false, false)?);
    }
    let parent = directory(&destination, true, share_write)?;
    let mut fs_name = [0u16; 32];
    // SAFETY: live directory handle; optional outputs null; writable fs-name array
    // has the stated UTF-16 capacity. This observes the actual test volume.
    if unsafe {
        GetVolumeInformationByHandleW(
            parent.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let fs_end = fs_name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(fs_name.len());
    let fs_name = String::from_utf16(&fs_name[..fs_end])?;
    assert_eq!(fs_name, "NTFS", "test requires an actual NTFS volume");
    println!("Observed filesystem: {fs_name}");
    let parent_id = identity(parent.as_raw_handle())?;
    println!("Resolved ordinary ancestor chain pinned; destination write sharing={share_write}");
    assert_eq!(write_excluded(&destination)?, !share_write);
    flush(&parent, "directory FlushFileBuffers before publication")?;
    let bytes = b"verified synthetic mail";
    let stage = candidate(&destination.join("stage.tmp"), bytes)?;
    let held_id = identity(stage.as_raw_handle())?;
    assert!(
        std::fs::File::open(destination.join("stage.tmp")).is_err(),
        "exclusive staging allowed a reader"
    );
    assert!(
        std::fs::rename(&destination, root.path().join("moved")).is_err(),
        "pinned parent renamed"
    );
    println!("PASS: exclusive stage, readback, file flush, identity and parent-rename denial");
    let final_path = destination.join("copy.eml");
    let (rename_root, rename_name): (HANDLE, PathBuf) = if absolute {
        println!(
            "EXPERIMENT: absolute FileRenameInfo name under pinned ancestry; contract revision required"
        );
        (ptr::null_mut(), final_path.clone())
    } else {
        (parent.as_raw_handle(), PathBuf::from("copy.eml"))
    };
    rename(&stage, rename_root, rename_name.as_os_str()).map_err(|error| {
        format!(
            "FileRenameInfo absolute={absolute}, offset={}, size={}: {error}",
            offset_of!(FILE_RENAME_INFO, FileName),
            size_of::<FILE_RENAME_INFO>()
        )
    })?;
    println!("PASS: held-candidate replace-false publication");
    let other = candidate(&destination.join("second.tmp"), b"must not replace winner")?;
    let conflict = rename(&other, rename_root, rename_name.as_os_str())
        .expect_err("replaced an existing copy");
    assert!(
        matches!(conflict.raw_os_error(), Some(80) | Some(183)),
        "unexpected conflict: {conflict}"
    );
    assert_eq!(identity(stage.as_raw_handle())?, held_id);
    assert_eq!(identity(parent.as_raw_handle())?, parent_id);
    flush(&parent, "directory FlushFileBuffers after publication")?;
    drop(other);
    drop(stage);
    let reopened = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&final_path)?;
    assert_eq!(identity(reopened.as_raw_handle())?, held_id);
    assert_eq!(std::fs::read(&final_path)?, bytes);
    drop(reopened);
    drop(parent);
    drop(pins);
    root.close()?;
    println!("PASS: no overwrite, final identity/bytes, directory flush and owned cleanup");
    Ok(())
}
