//! Actual kernel and emulation observations, independent of environment labels.
use std::{io, mem::size_of};
use windows_sys::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::System::{
        SystemInformation::OSVERSIONINFOW,
        Threading::{GetCurrentProcess, IsWow64Process2},
    },
};

/// Native observations; callers compare them with the requested target policy.
#[derive(Clone, Copy, Debug)]
pub struct HostInfo {
    /// Zero for native execution; otherwise the emulated process machine value.
    pub process_machine: u16,
    /// Actual native system IMAGE_FILE_MACHINE value.
    pub native_machine: u16,
    /// Actual NT kernel major version, unaffected by application manifests.
    pub major: u32,
    /// Actual NT kernel minor version.
    pub minor: u32,
    /// Actual NT kernel build.
    pub build: u32,
}
/// Inspect this exact process's machine and the actual native kernel version.
pub fn host_info() -> io::Result<HostInfo> {
    let mut process_machine = 0;
    let mut native_machine = 0;
    // SAFETY: current-process pseudo handle is borrowed, never closed; two valid
    // writable machine outputs are supplied with the exact generated types.
    if unsafe {
        IsWow64Process2(
            GetCurrentProcess(),
            &mut process_machine,
            &mut native_machine,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: exact initialized writable generated structure. Preserve primary
    // NTSTATUS; do not reinterpret an unrelated GetLastError value as its cause.
    let status = unsafe { RtlGetVersion(&mut version) };
    if status != 0 {
        return Err(io::Error::other(format!(
            "Native kernel-version query failed with NTSTATUS {status:#010x}"
        )));
    }
    Ok(HostInfo {
        process_machine,
        native_machine,
        major: version.dwMajorVersion,
        minor: version.dwMinorVersion,
        build: version.dwBuildNumber,
    })
}
