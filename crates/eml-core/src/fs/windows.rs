//! Win32 handles and no-replace moves; hostile namespace races are outside the threat model.
pub(super) mod path;
use super::{BoundFileError, EntryKind, EntryObservation, FileSignature, Identity};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    mem::{size_of, zeroed},
    os::windows::{
        fs::FileExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
};
pub(super) struct Directory {
    file: File,
    path: PathBuf,
}
pub(super) struct Entries(std::fs::ReadDir);
fn open(path: &Path, access: u32, creation: u32, follow: bool) -> io::Result<File> {
    let path = path::openable(path)?;
    let flags = FILE_FLAG_BACKUP_SEMANTICS
        | if follow {
            0
        } else {
            FILE_FLAG_OPEN_REPARSE_POINT
        };
    // SAFETY: NUL-terminated validated path; null optional security/template;
    // the returned valid handle is transferred once into File.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            creation,
            flags,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a valid owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}
fn information<T>(file: &File, class: FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
    // SAFETY: Win32 information structs used here admit an all-zero initialization.
    let mut info: T = unsafe { zeroed() };
    // SAFETY: writable struct covers the supplied size and class; the private
    // helper is called only with its corresponding documented Win32 struct.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            class,
            (&mut info as *mut T).cast(),
            size_of::<T>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}
fn identity(file: &File) -> io::Result<Identity> {
    let info: FILE_ID_INFO = information(file, FileIdInfo)?;
    if info.FileId.Identifier == [0; 16] {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Filesystem did not supply a usable file identity",
        ));
    }
    Ok(Identity::Windows {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    })
}
fn basic(file: &File) -> io::Result<FILE_BASIC_INFO> {
    information(file, FileBasicInfo)
}
pub(super) fn open_directory(
    parent: Option<&Directory>,
    expression: &Path,
    follow: bool,
) -> io::Result<Directory> {
    let path = if expression.is_absolute() {
        expression.to_owned()
    } else {
        parent
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Relative directory needs a captured parent",
                )
            })?
            .path
            .join(expression)
    };
    let path = path::absolute(&path)?;
    let file = open(&path, FILE_READ_ATTRIBUTES, OPEN_EXISTING, follow)?;
    let attributes = basic(&file)?.FileAttributes;
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || (!follow && attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Destination edge is not an ordinary directory",
        ));
    }
    Ok(Directory { file, path })
}
pub(super) fn directory_identity(directory: &Directory) -> io::Result<Identity> {
    if basic(&directory.file)?.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Held object is not a directory",
        ));
    }
    identity(&directory.file)
}
pub(super) fn name_limit(directory: &Directory) -> io::Result<usize> {
    let mut maximum = 0;
    // SAFETY: valid held directory handle, writable maximum, optional outputs null.
    if unsafe {
        GetVolumeInformationByHandleW(
            directory.file.as_raw_handle(),
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
            io::ErrorKind::Unsupported,
            "Filesystem returned a zero component limit",
        ));
    }
    Ok(maximum as usize)
}
fn flush(file: &File) -> io::Result<()> {
    // SAFETY: valid owned file/directory handle with write access.
    if unsafe { FlushFileBuffers(file.as_raw_handle()) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) fn sync_directory(directory: &Directory) -> io::Result<()> {
    let writer = open(
        &directory.path,
        GENERIC_WRITE | FILE_READ_ATTRIBUTES,
        OPEN_EXISTING,
        false,
    )?;
    if identity(&writer)? != identity(&directory.file)?
        || basic(&writer)?.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Directory changed before synchronization",
        ));
    }
    flush(&writer)
}
pub(super) fn entries(directory: &Directory) -> io::Result<Entries> {
    Ok(Entries(std::fs::read_dir(&directory.path)?))
}
pub(super) fn next_entry(entries: &mut Entries) -> Option<io::Result<OsString>> {
    entries
        .0
        .next()
        .map(|entry| entry.map(|entry| entry.file_name()))
}
pub(super) fn signature(file: &File) -> Result<FileSignature, BoundFileError> {
    let basic = basic(file)?;
    if basic.FileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return Err(BoundFileError::NotRegular);
    }
    // SAFETY: live file handle; GetFileType has no pointer outputs.
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(BoundFileError::NotRegular);
    }
    let standard: FILE_STANDARD_INFO = information(file, FileStandardInfo)?;
    if standard.EndOfFile < 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Negative file length").into());
    }
    Ok(FileSignature {
        identity: identity(file)?,
        size: standard.EndOfFile as u64,
        modified_native: [basic.LastWriteTime, 0],
        changed_native: [basic.ChangeTime, 0],
    })
}
pub(super) fn observe(
    directory: &Directory,
    expression: &Path,
    follow: bool,
) -> io::Result<EntryObservation> {
    let file = open(
        &directory.path.join(expression),
        FILE_READ_ATTRIBUTES,
        OPEN_EXISTING,
        follow,
    )?;
    let attributes = basic(&file)?.FileAttributes;
    let kind = if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        EntryKind::Reparse
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    };
    Ok(EntryObservation {
        identity: identity(&file)?,
        kind,
        file: if kind == EntryKind::File {
            Some(signature(&file).map_err(BoundFileError::into_io_error)?)
        } else {
            None
        },
    })
}
pub(super) fn open_file(directory: &Directory, name: &OsStr, follow: bool) -> io::Result<File> {
    open(
        &directory.path.join(name),
        GENERIC_READ,
        OPEN_EXISTING,
        follow,
    )
}
pub(super) fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    file.seek_read(buffer, offset)
}
pub(super) fn sync_existing(
    _: &File,
    directory: &Directory,
    name: &OsStr,
    expected: Identity,
) -> io::Result<()> {
    let writer = open(
        &directory.path.join(name),
        GENERIC_WRITE | FILE_READ_ATTRIBUTES,
        OPEN_EXISTING,
        false,
    )?;
    if signature(&writer)
        .map_err(BoundFileError::into_io_error)?
        .identity
        != expected
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Existing destination changed before synchronization",
        ));
    }
    flush(&writer)
}
pub(super) fn create_directory(directory: &Directory, name: &OsStr) -> io::Result<()> {
    std::fs::create_dir(directory.path.join(name))
}
pub(super) fn create_file(directory: &Directory, name: &OsStr) -> io::Result<File> {
    open(
        &directory.path.join(name),
        GENERIC_READ | GENERIC_WRITE,
        CREATE_NEW,
        false,
    )
}
pub(super) fn remove(directory: &Directory, name: &OsStr, is_directory: bool) -> io::Result<()> {
    if is_directory {
        std::fs::remove_dir(directory.path.join(name))
    } else {
        std::fs::remove_file(directory.path.join(name))
    }
}
pub(super) fn publish(directory: &Directory, from: &OsStr, to: &OsStr) -> io::Result<()> {
    let from = path::openable(&directory.path.join(from))?;
    let to = path::openable(&directory.path.join(to))?;
    // SAFETY: two NUL-terminated native paths. Flags deliberately omit
    // MOVEFILE_REPLACE_EXISTING and MOVEFILE_COPY_ALLOWED: no overwrite/copy fallback.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
