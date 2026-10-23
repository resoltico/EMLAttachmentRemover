//! Shared frontend transport for the native Rust business library.
//!
//! Request decoding and native path output are independent of stdin reading,
//! signals and execution. Successful transport is not filesystem admission or a run.
#![forbid(unsafe_code)]

/// Protocol 1's canonical program identity, shared by both product adapters.
/// It identifies the wire contract; it does not imply invoking the CLI binary.
pub use eml_core::product::CLI_NAME as PROGRAM_ID;

pub mod desktop;
mod event;
pub mod meaning;
mod output;
mod output_buffer;
mod path;
pub mod records;
mod request;
pub mod worker_control;
pub use output::{JsonSink, MAX_LINE_BYTES, MAX_RUN_BYTES};
pub use output_buffer::BoundedEventBuffer;
pub use path::{DisplayText, NativePathValue, PathValue, TerminalPath};
pub use request::{
    MAX_REQUEST_BYTES, RequestDecodeError, RequestEncodeError, decode_request, encode_request,
};
