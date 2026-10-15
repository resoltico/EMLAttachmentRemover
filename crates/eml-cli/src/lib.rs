//! Frontend transport for the native Rust business library.
//!
//! Request decoding is implemented independently of stdin reading, signals and
//! execution. A successful decode is not filesystem admission or a completed run.
#![forbid(unsafe_code)]

mod request;
pub use request::{MAX_REQUEST_BYTES, RequestDecodeError, decode_request};
