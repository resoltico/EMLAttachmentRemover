//! Minimal macOS system boundaries for the desktop product; no UI or business policy.
#![deny(unsafe_code)]
#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Reviewed Darwin spawn/signal/reap ABI boundary")]
mod child;
#[cfg(target_os = "macos")]
pub use child::{
    ChildExit, ChildState, OwnedChild, PipeEndpoints, SpawnIssue, SpawnObservation, StopSignal,
    spawn,
};
