//! Immutable selection snapshots and serial report reservations.
use eml_core::{OutputLocation, RunMode, RunRequest};
use eml_protocol::{MAX_REQUEST_BYTES, NativePathValue};
use serde::Serialize;
use std::{collections::VecDeque, path::PathBuf};
/// Maximum admitted waiting selections, excluding the one active run.
pub const MAX_WAITING_REQUESTS: usize = 8;
/// Maximum admitted requests and retained results in one session.
pub const MAX_SESSION_REQUESTS: usize = 20;
/// Maximum exact stream bytes retained in private session storage.
pub const MAX_STORED_REPORT_BYTES: u64 = 5 * 1024 * 1024 * 1024 / 2;

/// Session-local identity; never derived from a displayed filename or queue index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RunId(u64);
impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

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
    /// The finite session report byte budget is occupied.
    StoredBytesFull,
    /// Session storage failed for three reports retained in memory.
    MemoryReportsFull,
    /// All session result reservations are occupied, including waiting requests.
    SessionFull,
    /// Completion does not belong to the active run.
    InactiveRun,
    /// The finite session identity space has been exhausted.
    IdentityLimit,
    /// The frontend has committed to closing and cannot admit more work.
    ShuttingDown,
    /// A pending confirmation must settle before draft changes.
    DecisionPending,
    /// The chosen GUI destination folder no longer exists.
    DestinationUnavailable,
}
impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => write!(
                f,
                "{MAX_WAITING_REQUESTS} requests are already waiting. Remove a waiting run before adding another."
            ),
            Self::AbsoluteSelectionRequired => f.write_str("Select at least one file or folder using absolute paths."),
            Self::InvalidRequest(error) => write!(f, "Cannot add this selection: {error}"),
            Self::RequestEncoding => f.write_str("Cannot encode this selection. Select fewer paths or shorten folder names and try again."),
            Self::MemoryReportsFull => f.write_str("Waiting for space — close a finished run"),
            Self::StoredBytesFull => f.write_str("Close a finished run to add more"),
            Self::SessionFull => write!(f, "This session already holds {MAX_SESSION_REQUESTS} tasks and results. Close a result or remove a waiting task before adding another."),
            Self::InactiveRun => f.write_str("The completion does not belong to the active request."),
            Self::IdentityLimit => f.write_str("This session cannot start another request. Save needed reports and restart the application."),
            Self::DestinationUnavailable => f.write_str("Choose an existing folder to save copies"),
            Self::DecisionPending => f.write_str("Finish or cancel the current decision before changing the draft."),
            Self::ShuttingDown => f.write_str("The application is closing. Reopen it before starting another request."),
        }
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
    pub fn new(
        paths: Vec<PathBuf>,
        destination: Option<PathBuf>,
        mode: RunMode,
    ) -> Result<Self, QueueError> {
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
            mode,
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
            dry_run: mode == RunMode::DryRun,
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
    report_bytes: Vec<(RunId, u64)>,
    stored_bytes: u64,
}
impl RunQueue {
    /// Stored bytes charged only after a successful write; closing releases them.
    pub fn stored_bytes(&self) -> u64 {
        self.stored_bytes
    }
    /// Charge one retained result without exceeding the session byte cap.
    pub fn store_report(&mut self, id: RunId, bytes: u64) -> Result<(), QueueError> {
        if !self.reports.contains(&id) || self.active == Some(id) {
            return Err(QueueError::InactiveRun);
        }
        if self.report_bytes.iter().any(|(run, _)| *run == id) {
            return Err(QueueError::InactiveRun);
        }
        let total = self
            .stored_bytes
            .checked_add(bytes)
            .filter(|n| *n <= MAX_STORED_REPORT_BYTES)
            .ok_or(QueueError::StoredBytesFull)?;
        self.stored_bytes = total;
        self.report_bytes.push((id, bytes));
        Ok(())
    }
    /// Current admission refusal without changing captured requests.
    pub fn capacity_error(&self) -> Option<QueueError> {
        if self.stored_bytes >= MAX_STORED_REPORT_BYTES {
            Some(QueueError::StoredBytesFull)
        } else if self.waiting.len() >= MAX_WAITING_REQUESTS {
            Some(QueueError::Full)
        } else if self.reports.len() + self.waiting.len() >= MAX_SESSION_REQUESTS {
            Some(QueueError::SessionFull)
        } else {
            None
        }
    }
    /// Admit up to eight waiting immutable requests in selection order.
    pub fn enqueue(&mut self, request: RequestSnapshot) -> Result<RunId, QueueError> {
        if let Some(error) = self.capacity_error() {
            return Err(error);
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
    /// Start the next admitted snapshot; retained results never pause idle work.
    pub fn start_next(&mut self) -> Result<Option<(RunId, RequestSnapshot)>, QueueError> {
        if self.active.is_some() || self.waiting.is_empty() {
            return Ok(None);
        }
        if self.stored_bytes >= MAX_STORED_REPORT_BYTES {
            return Err(QueueError::StoredBytesFull);
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
        if let Some(index) = self.report_bytes.iter().position(|(run, _)| *run == id) {
            self.stored_bytes -= self.report_bytes.remove(index).1;
        }
        let before = self.reports.len();
        self.reports.retain(|report| *report != id);
        self.reports.len() != before
    }
    /// Remove one waiting selection without affecting active or completed work.
    pub fn remove_waiting(&mut self, id: RunId) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|entry| entry.0 != id);
        self.waiting.len() != before
    }
    /// Cancel queued work, retaining active work and completed reports.
    pub fn cancel_waiting(&mut self) {
        self.waiting.clear();
    }
    /// Number of waiting requests, excluding the active run.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
    /// Immutable waiting selections in admission order. Core discovery counts
    /// remain unknown until execution; these entries are Not started.
    pub fn waiting_requests(
        &self,
    ) -> impl DoubleEndedIterator<Item = (RunId, &RequestSnapshot)> + ExactSizeIterator {
        self.waiting.iter().map(|(id, request)| (*id, request))
    }
    /// Reserved reports, including the active run.
    pub fn reports(&self) -> &[RunId] {
        &self.reports
    }
}
