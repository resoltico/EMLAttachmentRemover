//! Native documented-Win32 primitive probe; product and archive qualification remain separate.
use std::{
    error::Error,
    ffi::OsStr,
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
    Storage::FileSystem::*,
    System::Threading::{GetCurrentProcess, IsWow64Process2},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[allow(
    unsafe_code,
    reason = "The isolated Windows primitive probe includes the shared owned-token ABI adapter to establish real restricted credentials; only the private token adapter, not the probe scenarios, permits unsafe calls."
)]
mod actor {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../support/windows_actor.rs"
    ));
}
use actor::RestrictedToken;

fn wide(path: &OsStr) -> Vec<u16> {
    path.encode_wide().chain(Some(0)).collect()
}
#[allow(
    unsafe_code,
    reason = "Only this independent probe adapter opens and validates ordinary Win32 directory handles; all buffers are initialized and successful handles become uniquely owned."
)]
fn directory(path: &Path, writable: bool) -> Result<OwnedHandle> {
    let path = wide(path.as_os_str());
    let access = if writable {
        GENERIC_READ | GENERIC_WRITE
    } else {
        FILE_READ_ATTRIBUTES | FILE_TRAVERSE
    };
    // SAFETY: bounded terminated path, no inherited security/template handle;
    // returned handle is immediately owned, read/write sharing and no delete sharing.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
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
#[allow(
    unsafe_code,
    reason = "Only this independent probe adapter reads native file identity through a borrowed live handle into a fixed-size initialized output."
)]
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

#[allow(
    unsafe_code,
    reason = "Only this independent native rename adapter constructs the validated Win32 rename buffer and calls the kernel API on a borrowed live candidate handle."
)]
fn rename_absolute_no_replace(
    file: &std::fs::File,
    parent: &Path,
    name: &OsStr,
    max_units: usize,
) -> std::io::Result<()> {
    let component: Vec<u16> = name.encode_wide().collect();
    if component.is_empty()
        || component.len() > max_units
        || component == [46]
        || component == [46, 46]
        || component
            .iter()
            .any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Rename requires a bounded native basename",
        ));
    }
    let full = parent.join(name);
    let name: Vec<u16> = full.as_os_str().encode_wide().collect();
    let name_bytes = name
        .len()
        .checked_mul(2)
        .ok_or_else(|| std::io::Error::other("Name overflow"))?;
    let bytes = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes)
        .and_then(|size| size.checked_add(2))
        .ok_or_else(|| std::io::Error::other("Buffer overflow"))?
        .max(size_of::<FILE_RENAME_INFO>());
    let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: generated aligned buffer and checked absolute UTF16 tail; replace
    // false, null RootDirectory, retained exact DELETE-capable candidate.
    let result = unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = ptr::null_mut();
        (*info).FileNameLength = u32::try_from(name_bytes).map_err(std::io::Error::other)?;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            storage
                .as_mut_ptr()
                .cast::<u8>()
                .add(offset_of!(FILE_RENAME_INFO, FileName))
                .cast::<u16>(),
            name.len(),
        );
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            u32::try_from(bytes).map_err(std::io::Error::other)?,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
#[allow(
    unsafe_code,
    reason = "Only this borrowed-handle adapter calls the native file synchronization ABI and retains its OS error."
)]
fn flush(handle: &OwnedHandle, label: &str) -> Result<()> {
    // SAFETY: live write-capable directory handle, retained for the whole call.
    if unsafe { FlushFileBuffers(handle.as_raw_handle()) } == 0 {
        return Err(format!("{label}: {}", std::io::Error::last_os_error()).into());
    }
    println!("PASS: {label}");
    Ok(())
}
#[allow(
    unsafe_code,
    reason = "Only this independent probe adapter tests DELETE sharing with a native open, closes successful handles and retains the actual sharing error."
)]
fn delete_excluded(path: &Path) -> Result<bool> {
    let name = wide(path.as_os_str());
    // SAFETY: terminated live path; successful handle (which means the guard failed)
    // is immediately owned and dropped. We never attempt an actual mutation.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            DELETE,
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
        println!("UNSAFE EXPERIMENT: pinned destination admitted a DELETE handle");
        return Ok(false);
    }
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(32));
    println!("PASS: pinned ordinary directory excludes DELETE access");
    Ok(true)
}

pub(super) fn run() -> Result<()> {
    require_native_machine()?;
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
    // Hold the resolved expression top down, allowing read/write sharing but excluding delete sharing.
    let ancestors: Vec<_> = destination.ancestors().skip(1).collect();
    for ancestor in ancestors.into_iter().rev() {
        pins.push(directory(ancestor, false)?);
    }
    let parent = directory(&destination, true)?;
    let max_component = require_ntfs_volume(&parent)?;
    let parent_id = identity(parent.as_raw_handle())?;
    println!("Resolved ordinary ancestor chain pinned without delete sharing");
    assert!(delete_excluded(&destination)?);
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
        let rejected = rename_absolute_no_replace(
            &stage,
            &destination,
            OsStr::new(invalid),
            max_component as usize,
        )
        .unwrap_err();
        assert_eq!(rejected.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(rejected.raw_os_error(), None);
    }
    let over_limit = "x".repeat(max_component as usize + 1);
    let rejected = rename_absolute_no_replace(
        &stage,
        &destination,
        OsStr::new(&over_limit),
        max_component as usize,
    )
    .unwrap_err();
    assert_eq!(rejected.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(rejected.raw_os_error(), None);
    let publish = |file: &std::fs::File| {
        rename_absolute_no_replace(
            file,
            &destination,
            OsStr::new("copy.eml"),
            max_component as usize,
        )
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

#[allow(
    unsafe_code,
    reason = "Only this process-machine observation calls the native ABI with the current process pseudo-handle and initialized outputs; emulation is explicitly refused."
)]
fn require_native_machine() -> Result<()> {
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
    Ok(())
}

#[allow(
    unsafe_code,
    reason = "Only this volume observation calls the native ABI on a borrowed directory handle with initialized capacity-bounded outputs; the actual volume must be NTFS."
)]
fn require_ntfs_volume(parent: &OwnedHandle) -> Result<u32> {
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
    Ok(max_component)
}
