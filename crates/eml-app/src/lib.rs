//! Immutable requests, bounded protocol reports and owned CLI supervision.
#![forbid(unsafe_code)]
mod export;
mod framing;
mod report;
mod request;
mod supervisor;
pub use export::export_report;
pub use framing::{FrameError, FrameSplitter};
pub use report::{ProtocolFailure, RunReport};
pub use request::{RequestError, RequestSnapshot};
pub use supervisor::{
    MAX_STDERR_BYTES, ReportHandle, WorkerCompletion, WorkerFailure, WorkerRun, WorkerState,
    bundled_cli,
};
