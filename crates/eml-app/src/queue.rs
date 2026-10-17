//! Immutable selection snapshots and serial report reservations.
use eml_core::{OutputLocation, RunMode, RunRequest};
use eml_protocol::{MAX_REQUEST_BYTES, NativePathValue};
use serde::Serialize;
use std::{collections::VecDeque, path::PathBuf};

/// Session-local identity; never derived from a displayed filename or queue index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunId(u64);

/// Application admission failure; no filesystem changes have been attempted.
#[derive(Debug)]
pub enum QueueError {
    /// Selections must be nonempty absolute native paths.
    AbsoluteSelectionRequired,
    /// Core's native representation/resource admission refused the request.
    InvalidRequest(eml_core::RequestPathError),
    /// Protocol request encoding failed or exceeds its byte bound.
    RequestEncoding,
    /// Eight waiting requests are already retained.
    Full,
    /// Two full reports are reserved; explicit dismissal is needed.
    ReportSpace,
    /// Completion does not belong to the active run.
    InactiveRun,
    /// The finite session identity space has been exhausted.
    IdentityLimit,
}
impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Application admission: {self:?}")
    }
}
impl std::error::Error for QueueError {}

/// An admitted request with a destination captured at selection/enqueue time.
#[derive(Clone, Debug)]
pub struct RequestSnapshot {
    request: RunRequest,
    encoded: Vec<u8>,
}
impl RequestSnapshot {
    /// Capture paths without resolving or opening them; core owns filesystem admission.
    pub fn new(paths: Vec<PathBuf>, destination: Option<PathBuf>) -> Result<Self, QueueError> {
        if paths.is_empty()
            || paths.iter().any(|path| !path.is_absolute())
            || destination.as_ref().is_some_and(|path| !path.is_absolute())
        {
            return Err(QueueError::AbsoluteSelectionRequired);
        }
        let working_directory = paths[0]
            .ancestors()
            .last()
            .ok_or(QueueError::AbsoluteSelectionRequired)?
            .to_path_buf();
        let request = RunRequest {
            paths,
            working_directory,
            output: destination.map_or(OutputLocation::BesideSource, OutputLocation::Directory),
            mode: RunMode::Create,
        };
        request
            .validate_path_limits()
            .map_err(QueueError::InvalidRequest)?;
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
            dry_run: false,
        })
        .map_err(|_| QueueError::RequestEncoding)?;
        if encoded.len() > MAX_REQUEST_BYTES {
            return Err(QueueError::RequestEncoding);
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

/// Serial queue with explicit report retention; admission never evicts outcomes.
#[derive(Default)]
pub struct RunQueue {
    sequence: u64,
    waiting: VecDeque<(RunId, RequestSnapshot)>,
    active: Option<RunId>,
    reports: Vec<RunId>,
}
impl RunQueue {
    /// Admit up to eight waiting immutable requests in selection order.
    pub fn enqueue(&mut self, request: RequestSnapshot) -> Result<RunId, QueueError> {
        if self.waiting.len() >= 8 {
            return Err(QueueError::Full);
        }
        let next = self
            .sequence
            .checked_add(1)
            .ok_or(QueueError::IdentityLimit)?;
        let id = RunId(next);
        self.sequence = next;
        self.waiting.push_back((id, request));
        Ok(id)
    }
    /// Reserve a report before removing the next request; pause without data loss.
    pub fn start_next(&mut self) -> Result<Option<(RunId, RequestSnapshot)>, QueueError> {
        if self.active.is_some() || self.waiting.is_empty() {
            return Ok(None);
        }
        if self.reports.len() >= 2 {
            return Err(QueueError::ReportSpace);
        }
        let Some(next) = self.waiting.pop_front() else {
            return Ok(None);
        };
        self.active = Some(next.0);
        self.reports.push(next.0);
        Ok(Some(next))
    }
    /// Complete only the exact active run, retaining its report reservation.
    pub fn complete(&mut self, id: RunId) -> Result<(), QueueError> {
        if self.active != Some(id) {
            return Err(QueueError::InactiveRun);
        }
        self.active = None;
        Ok(())
    }
    /// Explicitly dismiss a completed report; active reports cannot be removed.
    pub fn dismiss(&mut self, id: RunId) -> bool {
        if self.active == Some(id) {
            return false;
        }
        let before = self.reports.len();
        self.reports.retain(|report| *report != id);
        self.reports.len() != before
    }
    /// Remove one waiting selection without affecting active or completed work.
    pub fn remove_waiting(&mut self, id: RunId) {
        self.waiting.retain(|entry| entry.0 != id);
    }
    /// Cancel queued work, retaining active work and completed reports.
    pub fn cancel_waiting(&mut self) {
        self.waiting.clear();
    }
    /// Number of waiting requests, excluding the active run.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
    /// Reserved reports, including the active run.
    pub fn reports(&self) -> &[RunId] {
        &self.reports
    }
}
