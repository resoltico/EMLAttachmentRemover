//! Fixed existing-only child opens. No absolute NT namespace or creation options.
use super::path::validate_component;
use std::{
    ffi::OsStr,
    fmt,
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
        Foundation::{
            INVALID_HANDLE_VALUE, OBJ_DONT_REPARSE, RtlNtStatusToDosError, UNICODE_STRING,
        },
        Storage::FileSystem::{
            FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

/// An existing-only native open failure, preserving both native error domains.
/// When stored inside `io::Error`, inspect its payload rather than treating the
/// Win32 translation as if it were the primary NTSTATUS.
#[derive(Debug)]
pub struct NtOpenError {
    status: i32,
    win32: u32,
}
impl NtOpenError {
    /// Exact completed NTSTATUS returned by NtOpenFile.
    pub fn nt_status(&self) -> i32 {
        self.status
    }
    /// Secondary Win32 translation; it never replaces the original NTSTATUS.
    pub fn win32_code(&self) -> u32 {
        self.win32
    }
}
impl fmt::Display for NtOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Existing-entry open failed (NTSTATUS 0x{:08X}; Win32 {})",
            self.status as u32, self.win32
        )
    }
}
impl std::error::Error for NtOpenError {}
fn native_error(status: i32) -> io::Error {
    // SAFETY: pure documented error translation, no retained pointer or resource.
    let win32 = unsafe { RtlNtStatusToDosError(status) };
    io::Error::new(
        io::Error::from_raw_os_error(win32 as i32).kind(),
        NtOpenError { status, win32 },
    )
}
enum Kind {
    Observation,
    Directory,
}
pub(super) fn observe(parent: &File, name: &OsStr) -> io::Result<File> {
    open(parent, name, Kind::Observation)
}
pub(super) fn directory(parent: &File, name: &OsStr) -> io::Result<File> {
    open(parent, name, Kind::Directory)
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
    let (access, options, attributes) = match kind {
        Kind::Observation => (
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            0,
        ),
        Kind::Directory => (
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_DIRECTORY_FILE,
            OBJ_DONT_REPARSE,
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
    let status = unsafe {
        NtOpenFile(
            &mut raw,
            access,
            &attributes,
            &mut iosb,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            options,
        )
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_error_adapter_retains_the_known_access_denied_domains_and_literal_display() {
        // Fixed independent domain oracle: STATUS_ACCESS_DENIED maps to
        // ERROR_ACCESS_DENIED. This invokes translation, not a synthetic open.
        let error = native_error(0xC0000022_u32 as i32);
        let native = error
            .get_ref()
            .unwrap()
            .downcast_ref::<NtOpenError>()
            .unwrap();
        assert_eq!(native.nt_status(), -1_073_741_790);
        assert_eq!(native.win32_code(), 5);
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.raw_os_error().is_none());
        assert_eq!(
            error.to_string(),
            "Existing-entry open failed (NTSTATUS 0xC0000022; Win32 5)"
        );
    }
}
