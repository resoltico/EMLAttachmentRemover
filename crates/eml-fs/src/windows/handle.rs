//! Generated typed native metadata buffers and uniquely owned ordinary handles.
use crate::{FileSignature, Identity};
use std::{
    fs::File,
    io,
    mem::size_of,
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
};
use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::*};

pub(crate) fn open(path: &Path, access: u32, share: u32, flags: u32) -> io::Result<File> {
    let path = super::path::openable(path)?;
    // SAFETY: validated absolute extended DOS/UNC expression, terminated/live;
    // no security inheritance or template handle. OPEN_EXISTING never creates.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            share,
            std::ptr::null(),
            OPEN_EXISTING,
            flags,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreateFileW transferred exactly one uniquely owned handle.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(raw) }))
}
pub(crate) fn attributes(handle: &impl AsHandle) -> io::Result<FILE_ATTRIBUTE_TAG_INFO> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: live borrowed handle and writable aligned generated type with exact
    // matching class/size; all fields are initialized integer representations.
    if unsafe {
        GetFileInformationByHandleEx(
            handle.as_handle().as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}
fn usable_id(info: FILE_ID_INFO) -> io::Result<Identity> {
    if info.FileId.Identifier == [0; 16] {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "The volume supplied no usable full file identity",
        ));
    }
    Ok(Identity::Windows {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    })
}
pub(crate) fn identity(handle: &impl AsHandle) -> io::Result<Identity> {
    let mut info = FILE_ID_INFO::default();
    // SAFETY: live borrowed handle; exact generated FileIdInfo output buffer.
    if unsafe {
        GetFileInformationByHandleEx(
            handle.as_handle().as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    usable_id(info)
}
pub(crate) fn directory_observation(path: &Path) -> io::Result<File> {
    // Wider sharing here is observation only, never creation protection. It can
    // coexist with an already-live strict writable destination pin.
    let file = open(
        path,
        FILE_READ_ATTRIBUTES | FILE_TRAVERSE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        FILE_FLAG_BACKUP_SEMANTICS,
    )?;
    require_disk(&file)?;
    let attributes = attributes(&file)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Requested parent did not resolve to an ordinary directory",
        ));
    }
    identity(&file)?;
    Ok(file)
}
fn require_disk(handle: &impl AsHandle) -> io::Result<()> {
    // SAFETY: borrowed live handle; this query owns no returned resource.
    if unsafe { GetFileType(handle.as_handle().as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The opened object is not an ordinary disk file",
        ));
    }
    Ok(())
}
pub(crate) fn signature(handle: &impl AsHandle) -> io::Result<FileSignature> {
    require_disk(handle)?;
    let attributes = attributes(handle)?;
    if attributes.FileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The opened object is a directory or reparse entry",
        ));
    }
    let mut standard = FILE_STANDARD_INFO::default();
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: live handle; both matching generated output types have exact class,
    // alignment/size and initialized integer/BOOLEAN fields. Calls are synchronous.
    if unsafe {
        GetFileInformationByHandleEx(
            handle.as_handle().as_raw_handle(),
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: same owned handle; matching FileBasicInfo generated buffer.
    if unsafe {
        GetFileInformationByHandleEx(
            handle.as_handle().as_raw_handle(),
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if standard.Directory
        || standard.EndOfFile < 0
        || standard.DeletePending
        || standard.NumberOfLinks == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Ordinary file type, length or deletion binding is invalid",
        ));
    }
    Ok(FileSignature {
        identity: identity(handle)?,
        size: standard.EndOfFile as u64,
        modified_native: [basic.LastWriteTime, 0],
        changed_native: [basic.ChangeTime, 0],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_full_identity_is_not_a_binding() {
        assert!(usable_id(FILE_ID_INFO::default()).is_err());
        let mut info = FILE_ID_INFO::default();
        info.FileId.Identifier[0] = 1;
        assert_eq!(
            usable_id(info).unwrap(),
            Identity::Windows {
                volume: 0,
                file: info.FileId.Identifier
            }
        );
    }
}
