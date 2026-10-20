//! Minimal macOS system boundaries for the desktop product; no UI or business policy.
#![deny(unsafe_code)]
#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Reviewed Darwin spawn/signal/reap ABI boundary")]
mod child;
#[cfg(target_os = "macos")]
pub use child::{
    ChildExit, ChildState, OwnedChild, PipeEndpoints, SpawnIssue, SpawnObservation, StopSignal,
    close_reader, close_writer, spawn, spawn_with_control_pipe,
};
#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "Checked duplication and bounded nonblocking IO on inherited private control pipe"
)]
mod control_pipe;
#[cfg(target_os = "macos")]
pub use control_pipe::InheritedControlWriter;
