//! Immutable native requests and their protocol encoding.
use eml_core::{OutputLocation, RunMode, RunRequest};
use eml_protocol::{MAX_REQUEST_BYTES, NativePathValue};
use serde::Serialize;
use std::path::PathBuf;
/// Admission failures before any filesystem operation.
#[derive(Debug)]
pub enum RequestError {
    /// Selections must contain absolute native paths.
    AbsoluteSelectionRequired,
    /// An explicit destination must be absolute before it is captured.
    AbsoluteDestinationRequired,
    /// Core resource admission refused the request.
    InvalidRequest(eml_core::RequestPathError),
    /// Encoding failed or exceeded its bound.
    RequestEncoding,
}
impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AbsoluteSelectionRequired => {
                f.write_str("Select at least one absolute file or folder path.")
            }
            Self::AbsoluteDestinationRequired => {
                f.write_str("Choose an absolute save folder path.")
            }
            Self::InvalidRequest(e) => write!(f, "Cannot add this selection: {e}"),
            Self::RequestEncoding => f.write_str("Cannot encode this selection. Add fewer paths."),
        }
    }
}
impl std::error::Error for RequestError {}
/// An admitted request with a destination captured when the request is constructed.
#[derive(Clone, Debug)]
pub struct RequestSnapshot {
    request: RunRequest,
    encoded: Vec<u8>,
}
impl RequestSnapshot {
    /// Capture paths without resolving or opening them; core owns filesystem admission.
    pub fn new(
        paths: Vec<PathBuf>,
        destination: Option<PathBuf>,
        mode: RunMode,
    ) -> Result<Self, RequestError> {
        if paths.is_empty() || paths.iter().any(|path| !path.is_absolute()) {
            return Err(RequestError::AbsoluteSelectionRequired);
        }
        if destination.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err(RequestError::AbsoluteDestinationRequired);
        }
        let working_directory = paths[0]
            .ancestors()
            .last()
            .ok_or(RequestError::AbsoluteSelectionRequired)?
            .to_path_buf();
        let request = RunRequest {
            paths,
            working_directory,
            output: destination.map_or(OutputLocation::BesideSource, OutputLocation::Directory),
            mode,
        };
        request
            .validate_path_limits()
            .map_err(RequestError::InvalidRequest)?;
        #[derive(Serialize)]
        struct Wire<'a> {
            protocol: u8,
            paths: Vec<NativePathValue<'a>>,
            output_directory: Option<NativePathValue<'a>>,
            dry_run: bool,
        }
        let encoded = serde_json::to_vec(&Wire {
            protocol: 1,
            paths: request
                .paths
                .iter()
                .map(|path| NativePathValue::new(path))
                .collect(),
            output_directory: match &request.output {
                OutputLocation::BesideSource => None,
                OutputLocation::Directory(path) => Some(NativePathValue::new(path)),
            },
            dry_run: mode == RunMode::DryRun,
        })
        .map_err(|_| RequestError::RequestEncoding)?;
        if encoded.len() > MAX_REQUEST_BYTES {
            return Err(RequestError::RequestEncoding);
        }
        Ok(Self { request, encoded })
    }
    /// Immutable native request facts, including the captured destination.
    pub fn request(&self) -> &RunRequest {
        &self.request
    }
    /// Complete protocol-1 input, including required null members.
    pub fn encoded(&self) -> &[u8] {
        &self.encoded
    }
}
