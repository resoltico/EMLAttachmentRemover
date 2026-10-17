//! Exact native status diagnostics, independent of semantic business error codes.
use std::{fmt, io};
use windows_sys::Win32::Foundation::RtlNtStatusToDosError;

/// A native file-operation failure, preserving both native error domains.
/// When stored inside `io::Error`, inspect its payload rather than treating the
/// Win32 translation as if it were the primary NTSTATUS.
#[derive(Debug)]
pub struct NtStatusError {
    status: i32,
    win32: u32,
}
impl NtStatusError {
    /// Exact completed NTSTATUS returned by the native operation.
    pub fn nt_status(&self) -> i32 {
        self.status
    }
    /// Secondary Win32 translation; it never replaces the original NTSTATUS.
    pub fn win32_code(&self) -> u32 {
        self.win32
    }
}
impl fmt::Display for NtStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Native file operation failed (NTSTATUS 0x{:08X}; Win32 {})",
            self.status as u32, self.win32
        )
    }
}
impl std::error::Error for NtStatusError {}
pub(super) fn error(status: i32) -> io::Error {
    // SAFETY: pure documented error translation, no retained pointer or resource.
    let win32 = unsafe { RtlNtStatusToDosError(status) };
    io::Error::new(
        io::Error::from_raw_os_error(win32 as i32).kind(),
        NtStatusError { status, win32 },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_error_adapter_retains_the_known_access_denied_domains_and_literal_display() {
        // Fixed independent domain oracle: STATUS_ACCESS_DENIED maps to
        // ERROR_ACCESS_DENIED. This invokes translation, not a synthetic open.
        let error = error(0xC0000022_u32 as i32);
        let native = error
            .get_ref()
            .unwrap()
            .downcast_ref::<NtStatusError>()
            .unwrap();
        assert_eq!(native.nt_status(), -1_073_741_790);
        assert_eq!(native.win32_code(), 5);
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.raw_os_error().is_none());
        assert_eq!(
            error.to_string(),
            "Native file operation failed (NTSTATUS 0xC0000022; Win32 5)"
        );
    }
}
