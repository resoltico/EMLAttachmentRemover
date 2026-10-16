//! Fixed existing-only child opens. No absolute NT namespace or creation options.
use super::{path::validate_component, status::error as native_error};
use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtOpenFile,
        },
    },
    Win32::{
        Foundation::{INVALID_HANDLE_VALUE, OBJ_DONT_REPARSE, UNICODE_STRING},
        Storage::FileSystem::{
            FILE_GENERIC_WRITE, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

enum Kind {
    Observation,
    Directory,
    AncestorPin,
    WritableDirectory,
}
pub(super) const ANCESTOR_PIN_ACCESS: u32 = FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
pub(super) fn observe(parent: &File, name: &OsStr) -> io::Result<File> {
    open(parent, name, Kind::Observation)
}
pub(super) fn directory(parent: &File, name: &OsStr) -> io::Result<File> {
    open(parent, name, Kind::Directory)
}
pub(super) fn ancestor_pin(parent: &File, name: &OsStr) -> io::Result<File> {
    open(parent, name, Kind::AncestorPin)
}
pub(super) fn writable_directory(parent: &File, name: &OsStr) -> io::Result<File> {
    open(parent, name, Kind::WritableDirectory)
}
fn open(parent: &File, name: &OsStr, kind: Kind) -> io::Result<File> {
    // UNICODE_STRING counts bytes in u16 fields; no terminator is part of its name.
    validate_component(name, u16::MAX as usize / 2)?;
    let mut units: Vec<u16> = name.encode_wide().collect();
    let length = u16::try_from(units.len() * 2).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "Native child name is too long")
    })?;
    let name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: units.as_mut_ptr(),
    };
    let (access, options, attributes, sharing) = match kind {
        Kind::Observation => (
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        ),
        Kind::Directory => (
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_DIRECTORY_FILE,
            OBJ_DONT_REPARSE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        ),
        Kind::AncestorPin => (
            ANCESTOR_PIN_ACCESS,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_DIRECTORY_FILE,
            OBJ_DONT_REPARSE,
            FILE_SHARE_READ,
        ),
        Kind::WritableDirectory => (
            FILE_GENERIC_WRITE
                | FILE_LIST_DIRECTORY
                | FILE_READ_ATTRIBUTES
                | FILE_TRAVERSE
                | SYNCHRONIZE,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_DIRECTORY_FILE,
            OBJ_DONT_REPARSE,
            FILE_SHARE_READ,
        ),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &name,
        Attributes: attributes,
        ..Default::default()
    };
    let mut raw = std::ptr::null_mut();
    let mut iosb = IO_STATUS_BLOCK::default();
    // SAFETY: one validated relative component in owned UTF-16; generated aligned
    // structures remain live with the borrowed parent through final return.
    // NtOpenFile's existing-open IRP_MJ_CREATE is synchronously completed by the
    // I/O manager, including lower-driver pending (Microsoft Breaking Oplocks).
    // Fixed NONALERT+SYNCHRONIZE options admit no async/oplock-bypass mode. See
    // docs/windows-publication.md for the independently reviewed completion basis.
    let status = unsafe { NtOpenFile(&mut raw, access, &attributes, &mut iosb, sharing, options) };
    // Final returned status, not zero-initialized IOSB, determines success.
    // NT_SUCCESS includes informational statuses; warnings/errors are negative.
    if status < 0 {
        return Err(native_error(status));
    }
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Successful existing open supplied no owned handle",
        ));
    }
    // SAFETY: completed successful native open transferred exactly one valid,
    // noninherited file handle. File owns the sole CloseHandle responsibility.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(raw) }))
}
