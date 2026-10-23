//! Bounded desktop application state and transport, independent of the UI toolkit.
#![forbid(unsafe_code)]
mod control;
#[cfg(target_os = "macos")]
mod export;
mod export_draft;
mod framing;
pub use export_draft::{
    EXPORT_RESERVATION_EXPLANATION, ExportDraft, InvalidExportDraft, NO_REPLACE_EXPLANATION,
    validate_destination,
};
mod lifecycle;
pub mod report_document;
pub use lifecycle::{RunMeaning, WorkerCompletion, WorkerFailure};
mod queue;
mod report;
#[cfg(target_os = "macos")]
mod session;
#[cfg(target_os = "macos")]
mod supervisor;
pub use control::{ControlError, WorkerControl, WorkerIntent};
#[cfg(target_os = "macos")]
pub use eml_fs::PublicationState;
#[cfg(target_os = "macos")]
pub use export::{
    ReportExportFailure, ReportExportOutcome, ReportExportStage, TerminalReport, export_report,
};
pub use framing::{FrameError, FrameSplitter};
pub use queue::{MAX_WAITING_REQUESTS, QueueError, RequestSnapshot, RunId, RunQueue};
pub use report::{ProtocolFailure, RunReport};
#[cfg(target_os = "macos")]
pub use session::{DesktopSession, SessionActivity, SessionError, SessionReport};
#[cfg(target_os = "macos")]
pub use supervisor::{
    ExportCompletion, ExportObservation, ExportRun, ReportHandle, WorkerObservation, WorkerRun,
};
