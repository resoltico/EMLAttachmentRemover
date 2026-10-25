//! Bounded desktop queue, protocol reports and owned CLI supervision.
#![forbid(unsafe_code)]
mod export;
mod framing;
mod queue;
mod report;
mod supervisor;
pub use export::export_report;
pub use framing::{FrameError, FrameSplitter};
pub use queue::{MAX_WAITING_REQUESTS, QueueError, RequestSnapshot, RunId, RunQueue};
pub use report::{ProtocolFailure, RunReport};
pub use supervisor::{
    MAX_STDERR_BYTES, ReportHandle, WorkerCompletion, WorkerFailure, WorkerRun, WorkerState,
    bundled_cli,
};
