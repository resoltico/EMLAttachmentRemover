//! Early native feasibility gate; not product or archive qualification.
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
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE},
    Security::*,
    Storage::FileSystem::*,
    System::Threading::{GetCurrentProcess, IsWow64Process2, OpenProcessToken},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct RestrictedToken(OwnedHandle);
impl RestrictedToken {
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
        // output points at an owned-handle slot. Admin SID becomes deny-only;
        // DISABLE_MAX_PRIVILEGE retains SeChangeNotifyPrivilege for directory traversal.
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
        println!(
            "PASS: restricted runner-account thread token; Administrators deny-only, traverse privilege retained"
        );
        Ok(token)
    }
}
impl Drop for RestrictedToken {
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
fn directory(path: &Path, writable: bool) -> Result<OwnedHandle> {
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
            FILE_SHARE_READ,
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

// Reviewed narrow amendment proposed by the independent design/QA chat:
// native simple-basename rename in the held candidate's current directory.
// This avoids a Win32 absolute-path target reopen and requires no NtCreateFile.
fn rename_same_directory(
    file: &std::fs::File,
    name: &OsStr,
    max_units: usize,
) -> std::io::Result<()> {
    use windows_sys::{
        Wdk::Storage::FileSystem::{
            FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
        },
        Win32::{
            Foundation::{RtlNtStatusToDosError, STATUS_PENDING, WAIT_OBJECT_0},
            System::{
                IO::IO_STATUS_BLOCK,
                Threading::{INFINITE, WaitForSingleObject},
            },
        },
    };
    let name: Vec<u16> = name.encode_wide().collect();
    if name.is_empty()
        || name.len() > max_units
        || name == [46]
        || name == [46, 46]
        || name.iter().any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "rename requires a bounded native basename",
        ));
    }
    let name_bytes = name
        .len()
        .checked_mul(2)
        .ok_or_else(|| std::io::Error::other("name overflow"))?;
    let bytes = size_of::<FILE_RENAME_INFORMATION>()
        .checked_add(name_bytes)
        .and_then(|length| length.checked_add(2))
        .ok_or_else(|| std::io::Error::other("buffer overflow"))?
        .max(size_of::<FILE_RENAME_INFORMATION>());
    let name_bytes =
        u32::try_from(name_bytes).map_err(|_| std::io::Error::other("name exceeds DWORD"))?;
    let info_bytes =
        u32::try_from(bytes).map_err(|_| std::io::Error::other("buffer exceeds DWORD"))?;
    let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let mut io = IO_STATUS_BLOCK::default();
    io.Anonymous.Status = STATUS_PENDING;
    // SAFETY: generated native structure; usize storage meets alignment on both supported
    // CPUs and covers checked tail including zero padding. Source is a live synchronous,
    // DELETE/SYNCHRONIZE-capable, exclusive handle. Name is one component with checked size.
    // NULL root selects that source file's current directory. All pins stay live. Both
    // buffers are retained through any pending completion before they can be released.
    let mut status = unsafe {
        ptr::addr_of_mut!((*info).Anonymous.Flags).write(0);
        ptr::addr_of_mut!((*info).RootDirectory).write(ptr::null_mut());
        ptr::addr_of_mut!((*info).FileNameLength).write(name_bytes);
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            storage
                .as_mut_ptr()
                .cast::<u8>()
                .add(offset_of!(FILE_RENAME_INFORMATION, FileName))
                .cast::<u16>(),
            name.len(),
        );
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut io,
            info.cast::<c_void>(),
            info_bytes,
            FileRenameInformation,
        )
    };
    let returned = status;
    if status == STATUS_PENDING {
        // SAFETY: the owned synchronous handle is waitable and remains live; IO buffers
        // are retained. OS wait has no artificial deadline; force-stop belongs to owner.
        if unsafe { WaitForSingleObject(file.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            // Cannot free an in-flight buffer when completion cannot be established.
            std::process::abort();
        }
        // SAFETY: successful wait establishes completion before inspecting the union.
        status = unsafe { io.Anonymous.Status };
        if status == STATUS_PENDING {
            std::process::abort();
        }
    }
    // SAFETY: no native operation remains pending; inspect the generated status member.
    let completion = unsafe { io.Anonymous.Status };
    if status < 0 {
        println!(
            "NtSetInformationFile returned={returned:#010x}; IO status slot not authoritative on immediate failure"
        );
        // SAFETY: pure status translation; never use stale Win32 GetLastError for NT calls.
        return Err(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    println!("NtSetInformationFile returned={returned:#010x} completed_status={completion:#010x}");
    if completion < 0 {
        // SAFETY: completed native failure is translated with its raw status retained above.
        return Err(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(completion) } as i32,
        ));
    }
    if completion == STATUS_PENDING {
        return Err(std::io::Error::other(
            "native success lacked completed IO status",
        ));
    }
    Ok(())
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
    let _restricted_token = RestrictedToken::enter()?;
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    std::fs::create_dir(&destination)?;
    let renamed = root.path().join("rename-control");
    std::fs::rename(&destination, &renamed)?;
    std::fs::rename(&renamed, &destination)?;
    println!("PASS: unpinned directory rename succeeds under the same restricted token");
    let destination = std::fs::canonicalize(destination)?;
    let mut pins = Vec::new();
    // Hold the resolved expression top down, never allowing write/delete sharing.
    let ancestors: Vec<_> = destination.ancestors().skip(1).collect();
    for ancestor in ancestors.into_iter().rev() {
        pins.push(directory(ancestor, false)?);
    }
    let parent = directory(&destination, true)?;
    let mut fs_name = [0u16; 32];
    let mut max_component = 0;
    // SAFETY: live directory handle; optional outputs null; writable fs-name array
    // has the stated UTF-16 capacity. This observes the actual test volume.
    if unsafe {
        GetVolumeInformationByHandleW(
            parent.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut max_component,
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
    println!("Resolved ordinary ancestor chain pinned without write/delete sharing");
    assert!(write_excluded(&destination)?);
    flush(&parent, "directory FlushFileBuffers before publication")?;
    let bytes = b"verified synthetic mail";
    let stage = candidate(&destination.join("stage.tmp"), bytes)?;
    let held_id = identity(stage.as_raw_handle())?;
    let reader = std::fs::File::open(destination.join("stage.tmp"))
        .expect_err("exclusive staging allowed a reader");
    assert_eq!(reader.raw_os_error(), Some(32));
    let parent_rename = std::fs::rename(&destination, root.path().join("moved"))
        .expect_err("pinned parent renamed");
    assert_eq!(parent_rename.raw_os_error(), Some(32));
    println!("PASS: exclusive stage, readback, file flush, identity and parent-rename denial");
    let final_path = destination.join("copy.eml");
    for invalid in ["", ".", "..", "a/b", "a\\b", "a:b", "a\0b"] {
        let rejected =
            rename_same_directory(&stage, OsStr::new(invalid), max_component as usize).unwrap_err();
        assert_eq!(rejected.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(rejected.raw_os_error(), None);
    }
    let over_limit = "x".repeat(max_component as usize + 1);
    let rejected =
        rename_same_directory(&stage, OsStr::new(&over_limit), max_component as usize).unwrap_err();
    assert_eq!(rejected.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(rejected.raw_os_error(), None);
    let publish = |file: &std::fs::File| {
        rename_same_directory(file, OsStr::new("copy.eml"), max_component as usize)
    };
    publish(&stage)?;
    println!("PASS: held-candidate replace-false publication");
    let other = candidate(&destination.join("second.tmp"), b"must not replace winner")?;
    let conflict = publish(&other).expect_err("replaced an existing copy");
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
