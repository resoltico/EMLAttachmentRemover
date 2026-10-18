//! Owned native Windows process lifetimes for real distribution verification.
#![deny(unsafe_code)]
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Generated Win32 process/job ABI behind uniquely owned handles"
)]
mod process;
#[cfg(windows)]
pub use process::{Job, Process, ProcessSpec, environment_name_equal};
