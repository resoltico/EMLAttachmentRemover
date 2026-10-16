//! Frontend transport for the native Rust business library.
//!
//! Request decoding and native path output are independent of stdin reading,
//! signals and execution. Successful transport is not filesystem admission or a run.
#![forbid(unsafe_code)]

mod path;
mod request;
pub use path::PathValue;
pub use request::{MAX_REQUEST_BYTES, RequestDecodeError, decode_request};
