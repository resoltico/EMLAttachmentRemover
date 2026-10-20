//! Bounded desktop application state and transport, independent of the UI toolkit.
#![forbid(unsafe_code)]
mod control;
#[cfg(target_os = "macos")]
mod export;
mod framing;
mod queue;
mod report;
#[cfg(target_os = "macos")]
mod session;
#[cfg(target_os = "macos")]
mod supervisor;
pub use control::{ControlError, WorkerControl, WorkerIntent};
#[cfg(target_os = "macos")]
pub use export::{ReportExportOutcome, TerminalReport, export_report};
pub use framing::{FrameError, FrameSplitter};
pub use queue::{MAX_WAITING_REQUESTS, QueueError, RequestSnapshot, RunId, RunQueue};
pub use report::{ProtocolFailure, RunReport};
#[cfg(target_os = "macos")]
pub use session::{DesktopSession, SessionError, SessionReport};
#[cfg(target_os = "macos")]
pub use supervisor::{ReportHandle, WorkerCompletion, WorkerFailure, WorkerObservation, WorkerRun};
