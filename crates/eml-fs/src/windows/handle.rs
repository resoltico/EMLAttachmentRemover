//! Generated typed native metadata buffers and uniquely owned ordinary handles.
use crate::{BoundFileError, EntryKind, EntryObservation, FileSignature, Identity};
use std::{
    fs::File,
    io,
    mem::size_of,
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
};
use windows_sys::Win32::{
    Foundation::{GetLastError, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
};

pub(super) fn component_limit(handle: &impl AsHandle) -> io::Result<usize> {
    let mut maximum = 0;
    // SAFETY: live borrowed handle; only the writable DWORD output is requested.
    if unsafe {
        GetVolumeInformationByHandleW(
            handle.as_handle().as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut maximum,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if maximum == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Filesystem supplied no usable component limit",
        ));
    }
    Ok(maximum as usize)
}

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
pub(super) fn directory_identity(file: &File) -> io::Result<Identity> {
    if !is_disk(file)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Opened directory is not a disk object",
        ));
    }
    let attributes = attributes(file)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Opened object is not an ordinary disk directory",
        ));
    }
    identity(file)
}
pub(super) fn entry_observation(file: &File) -> io::Result<EntryObservation> {
    if !is_disk(file)? {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Entry observation is not a disk object",
        ));
    }
    let attributes = attributes(file)?;
    let kind = if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        EntryKind::Reparse
    } else if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    };
    let observed = if kind == EntryKind::File {
        Some(signature(file).map_err(BoundFileError::into_io_error)?)
    } else {
        None
    };
    Ok(EntryObservation {
        identity: match observed {
            Some(value) => value.identity,
            None => identity(file)?,
        },
        kind,
        file: observed,
    })
}
pub(crate) fn directory_observation(path: &Path) -> io::Result<Option<(File, Identity)>> {
    // Wider sharing here is observation only, never creation protection. It can
    // coexist with an already-live strict writable destination pin.
    let file = open(
        path,
        FILE_READ_ATTRIBUTES | FILE_TRAVERSE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        FILE_FLAG_BACKUP_SEMANTICS,
    )?;
    if !is_disk(&file)? {
        return Ok(None);
    }
    let attributes = attributes(&file)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Ok(None);
    }
    let identity = identity(&file)?;
    Ok(Some((file, identity)))
}
fn file_type_is_disk(kind: u32, native_error: u32) -> io::Result<bool> {
    if kind == FILE_TYPE_UNKNOWN && native_error != 0 {
        return Err(io::Error::from_raw_os_error(native_error as i32));
    }
    Ok(kind == FILE_TYPE_DISK)
}
pub(super) fn is_disk(handle: &impl AsHandle) -> io::Result<bool> {
    // SAFETY: borrowed live handle; this query owns no returned resource.
    let kind = unsafe { GetFileType(handle.as_handle().as_raw_handle()) };
    let native_error = if kind == FILE_TYPE_UNKNOWN {
        // SAFETY: capture the thread-local result immediately, before another
        // native call. UNKNOWN with NO_ERROR is a valid type observation.
        unsafe { GetLastError() }
    } else {
        0
    };
    file_type_is_disk(kind, native_error)
}
pub(crate) fn signature(handle: &impl AsHandle) -> Result<FileSignature, BoundFileError> {
    if !is_disk(handle)? {
        return Err(BoundFileError::NotRegular);
    }
    let attributes = attributes(handle)?;
    if attributes.FileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return Err(BoundFileError::NotRegular);
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
        return Err(io::Error::last_os_error().into());
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
        return Err(io::Error::last_os_error().into());
    }
    if standard.Directory {
        return Err(BoundFileError::NotRegular);
    }
    if standard.EndOfFile < 0 || standard.DeletePending || standard.NumberOfLinks == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Ordinary file type, length or deletion binding is invalid",
        )
        .into());
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
    fn unknown_file_type_keeps_api_failure_distinct_from_valid_non_disk_type() {
        assert!(!file_type_is_disk(FILE_TYPE_UNKNOWN, 0).unwrap());
        assert_eq!(
            file_type_is_disk(FILE_TYPE_UNKNOWN, 6)
                .unwrap_err()
                .raw_os_error(),
            Some(6)
        );
        assert!(file_type_is_disk(FILE_TYPE_DISK, 6).unwrap());
        assert!(!file_type_is_disk(FILE_TYPE_PIPE, 0).unwrap());
    }
    #[test]
    fn actual_directory_metadata_is_a_nonregular_initial_file_observation() {
        let root = tempfile::tempdir().unwrap();
        let path = crate::PathContext::capture(&std::env::current_dir().unwrap())
            .unwrap()
            .resolve(root.path())
            .unwrap();
        let (directory, _) = directory_observation(&path).unwrap().unwrap();
        assert!(matches!(
            signature(&directory),
            Err(BoundFileError::NotRegular)
        ));
        drop(directory);
        root.close().unwrap();
    }
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
