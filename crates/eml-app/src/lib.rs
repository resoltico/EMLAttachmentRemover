//! Bounded desktop application state and transport, independent of the UI toolkit.
#![forbid(unsafe_code)]
mod framing;
mod queue;
mod report;
pub use framing::{FrameError, FrameSplitter};
pub use queue::{QueueError, RequestSnapshot, RunId, RunQueue};
pub use report::{ProtocolFailure, RunReport};
