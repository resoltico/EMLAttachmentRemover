//! Desktop queue/lifecycle orchestration; views explicitly acknowledge final application.
use crate::{
    ExportCompletion, ExportObservation, QueueError, ReportHandle, RequestSnapshot, RunId,
    RunQueue, WorkerCompletion, WorkerControl, WorkerFailure, WorkerObservation, WorkerRun,
};
use std::{path::PathBuf, time::Instant};
/// A session admission or lifecycle refusal, before any additional work is started.
#[derive(Debug)]
pub enum SessionError {
    /// A confirmed shutdown has closed admission.
    Closing,
    /// Earlier transport/lifecycle failure has closed admission.
    Blocked,
    /// Queue/request/report reservation refused admission.
    Queue(QueueError),
    /// The view acknowledgment does not identify the active run.
    InactiveRun,
    /// Actual settlement and a terminal observation are still required.
    Unsettled,
    /// An export child is still owned; there is no export queue.
    ExportBusy,
    /// Pure exact-address validation refused the report draft.
    InvalidExportDraft,
    /// The export owner thread could not start.
    ExportLaunch(std::io::Error),
}
impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queue(error) => error.fmt(f),
            _ => write!(f, "Desktop session: {self:?}"),
        }
    }
}
impl std::error::Error for SessionError {}
impl From<QueueError> for SessionError {
    fn from(error: QueueError) -> Self {
        Self::Queue(error)
    }
}
/// One reserved report, never silently evicted by a later selection.
pub struct SessionReport {
    id: RunId,
    operation_id: eml_protocol::desktop::OperationId,
    report: ReportHandle,
    completion: Option<WorkerCompletion>,
    request: RequestSnapshot,
}
impl SessionReport {
    /// Stable session-local identity.
    pub fn id(&self) -> RunId {
        self.id
    }
    /// Exact child operation retained alongside its processing report.
    pub fn operation_id(&self) -> eml_protocol::desktop::OperationId {
        self.operation_id
    }
    /// Captured immutable processing request, including mode and destination.
    pub fn request(&self) -> &RequestSnapshot {
        &self.request
    }
    /// Bounded accepted Core observations, including observations before failure.
    pub fn report(&self) -> ReportHandle {
        self.report.clone()
    }
    /// Actual terminal lifecycle observations, if available.
    pub fn completion(&self) -> Option<&WorkerCompletion> {
        self.completion.as_ref()
    }
    /// Capture terminal facts and retain this exact report for a save operation.
    pub fn export_snapshot(&self) -> Option<crate::TerminalReport> {
        crate::TerminalReport::new(self.id, self.report.clone(), self.completion.clone()?)
    }
}
struct ActiveRun {
    id: RunId,
    worker: Option<WorkerRun>,
}
/// Current application activity, independent of which retained report is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionActivity {
    /// No active or capacity-blocked request.
    Idle,
    /// A worker is owned and has no accepted terminal completion yet.
    Running(RunId),
    /// A user stop request is pending; this does not assert signal delivery.
    StopRequested {
        /// Exact currently owned run.
        id: RunId,
        /// More than one stop request was made.
        repeated: bool,
    },
    /// Terminal facts await the view's explicit acknowledgement.
    Settled(RunId),
    /// Waiting requests need an explicitly dismissed report reservation.
    WaitingForReportSpace,
    /// Confirmed shutdown has closed admission.
    Closing,
    /// Earlier lifecycle evidence prevents further processing.
    Blocked,
}
/// Serial desktop runs using the reusable core-backed worker. Native sheets,
/// preferences, selection and rendering remain the frontend's responsibility.
pub struct DesktopSession {
    operation_sequence: u64,
    export: Option<crate::supervisor::ExportRun>,
    export_completion: Option<ExportCompletion>,
    export_reserved: Option<RunId>,
    helper: PathBuf,
    version: String,
    queue: RunQueue,
    active: Option<ActiveRun>,
    reports: Vec<SessionReport>,
    closing: bool,
    blocked: bool,
}
impl DesktopSession {
    /// Bind the exact bundled helper and version, without launching any process.
    pub fn new(helper: PathBuf, version: String) -> Self {
        Self {
            operation_sequence: 0,
            export: None,
            export_completion: None,
            export_reserved: None,
            helper,
            version,
            queue: RunQueue::default(),
            active: None,
            reports: Vec::new(),
            closing: false,
            blocked: false,
        }
    }
    fn next_operation(&mut self) -> Result<eml_protocol::desktop::OperationId, SessionError> {
        let next = self
            .operation_sequence
            .checked_add(1)
            .ok_or(QueueError::IdentityLimit)?;
        self.operation_sequence = next;
        Ok(eml_protocol::desktop::OperationId::new(next).expect("checked nonzero operation"))
    }
    /// Enqueue an immutable Core-admitted selection; no automatic eviction.
    pub fn enqueue(&mut self, request: RequestSnapshot) -> Result<RunId, SessionError> {
        if self.closing {
            return Err(SessionError::Closing);
        }
        if self.blocked {
            return Err(SessionError::Blocked);
        }
        Ok(self.queue.enqueue(request)?)
    }
    /// Launch at most one request after report reservation and final UI application.
    /// A report-space refusal retains all waiting requests.
    pub fn start_next(&mut self) -> Result<bool, SessionError> {
        if self.closing {
            return Err(SessionError::Closing);
        }
        if self.blocked {
            return Err(SessionError::Blocked);
        }
        if self.active.is_some() {
            return Ok(false);
        }
        if self.queue.waiting() == 0 {
            return Ok(false);
        }
        let operation_id = self.next_operation()?;
        let Some((id, request)) = self.queue.start_next()? else {
            return Ok(false);
        };
        let captured_request = request.clone();
        let control = WorkerControl::for_operation(operation_id, id);
        let (worker, report, completion) =
            match WorkerRun::start(self.helper.clone(), self.version.clone(), request, control) {
                Ok(worker) => {
                    let report = worker.report();
                    (Some(worker), report, None)
                }
                Err(error) => {
                    // Thread creation failed before any native child could be
                    // spawned. Keep the reservation until the view shows this fact.
                    self.blocked = true;
                    (
                        None,
                        ReportHandle::new(self.version.clone()),
                        Some(WorkerCompletion {
                            operation_id,
                            exit_code: None,
                            signal: None,
                            input_may_have_been_sent: false,
                            settled: true,
                            reusable: false,
                            failure: Some(WorkerFailure::Launch),
                            faults: vec![format!("Worker owner could not start: {error}")],
                            stderr: Vec::new(),
                            stderr_truncated: false,
                            interrupt_calls_succeeded: 0,
                            first_interrupt_observed: false,
                        }),
                    )
                }
            };
        self.reports.push(SessionReport {
            id,
            operation_id,
            report,
            completion,
            request: captured_request,
        });
        self.active = Some(ActiveRun { id, worker });
        Ok(true)
    }
    /// Nonblocking lifecycle observation. A completed result stays reserved and
    /// active until the frontend explicitly applies and acknowledges it.
    pub fn poll(&mut self) -> Option<WorkerObservation> {
        let active = self.active.as_mut()?;
        let report = self
            .reports
            .iter_mut()
            .find(|report| report.id == active.id)?;
        if let Some(completion) = &report.completion {
            return Some(WorkerObservation::Settled(completion.clone()));
        }
        let observation = active.worker.as_mut()?.poll();
        if matches!(
            &observation,
            WorkerObservation::Pending { warning: Some(_) }
        ) {
            self.blocked = true;
        }
        if let WorkerObservation::Settled(completion) = &observation {
            self.blocked |= !completion.reusable;
            report.completion = Some(completion.clone());
        }
        Some(observation)
    }
    /// Acknowledge final rendering only after actual lifecycle settlement.
    pub fn acknowledge_completion(&mut self, id: RunId) -> Result<(), SessionError> {
        if !self.active.as_ref().is_some_and(|active| active.id == id) {
            return Err(SessionError::InactiveRun);
        }
        if !self
            .reports
            .iter()
            .find(|report| report.id == id)
            .and_then(|report| report.completion.as_ref())
            .is_some_and(|completion| completion.settled)
        {
            return Err(SessionError::Unsettled);
        }
        self.queue.complete(id)?;
        self.active = None;
        Ok(())
    }
    /// Current run identity, retained through final view application.
    pub fn active(&self) -> Option<RunId> {
        self.active.as_ref().map(|active| active.id)
    }
    /// Run-bound stop/confirmed-shutdown control; never exposes a PID.
    pub fn control(&self) -> Option<&WorkerControl> {
        self.active
            .as_ref()?
            .worker
            .as_ref()
            .map(WorkerRun::control)
    }
    /// Resolve a captured confirmation's exact run, refusing stale callbacks.
    pub fn control_for(&self, id: RunId) -> Option<&WorkerControl> {
        self.active
            .as_ref()
            .filter(|active| active.id == id)?
            .worker
            .as_ref()
            .map(WorkerRun::control)
    }
    /// Reserved reports, including the active run.
    pub fn reports(&self) -> &[SessionReport] {
        &self.reports
    }
    /// Waiting requests, excluding the active run.
    pub fn waiting(&self) -> usize {
        self.queue.waiting()
    }
    /// Derive activity from current ownership and reservation facts. A terminal
    /// completion outranks the still-active acknowledgement reservation.
    pub fn activity(&self) -> SessionActivity {
        if self.closing {
            return SessionActivity::Closing;
        }
        if let Some(active) = &self.active {
            if self
                .reports
                .iter()
                .any(|r| r.id == active.id && r.completion.is_some())
            {
                return SessionActivity::Settled(active.id);
            }
            if !self.blocked
                && let Some(control) = self.control_for(active.id)
                && let Ok(intent) = control.intent(Instant::now())
                && intent.interrupts > 0
            {
                return SessionActivity::StopRequested {
                    id: active.id,
                    repeated: intent.interrupts > 1,
                };
            }
            return if self.blocked {
                SessionActivity::Blocked
            } else {
                SessionActivity::Running(active.id)
            };
        }
        if self.blocked {
            SessionActivity::Blocked
        } else if self.queue.is_paused_for_report_space() {
            SessionActivity::WaitingForReportSpace
        } else {
            SessionActivity::Idle
        }
    }
    /// Immutable Not started selections; no separate mutable frontend queue.
    pub fn waiting_requests(
        &self,
    ) -> impl DoubleEndedIterator<Item = (RunId, &RequestSnapshot)> + ExactSizeIterator {
        self.queue.waiting_requests()
    }
    /// Remove only this queued selection; active/completed runs are unchanged.
    pub fn remove_waiting(&mut self, id: RunId) -> bool {
        self.queue.remove_waiting(id)
    }
    /// Explicitly dismiss only a completed, already-applied report after all
    /// external read/export handles are released. A pending export keeps its
    /// full report reservation, so dismissal cannot admit a third retained report.
    pub fn dismiss(&mut self, id: RunId) -> bool {
        if self.export_reserved == Some(id) {
            return false;
        }
        if !self
            .reports
            .iter()
            .find(|report| report.id == id)
            .is_some_and(|report| !report.report.has_external_readers())
        {
            return false;
        }
        if !self.queue.dismiss(id) {
            return false;
        }
        self.reports.retain(|report| report.id != id);
        true
    }
    /// Remove waiting requests without modifying active or completed work.
    pub fn cancel_waiting(&mut self) {
        self.queue.cancel_waiting();
    }
    /// Confirmed Quit permanently closes admission and starts the active run's
    /// fixed escalation deadlines. The frontend must settle its own dialogs/exports too.
    pub fn confirm_shutdown(&mut self, now: Instant) -> Result<(), crate::ControlError> {
        self.closing = true;
        self.queue.cancel_waiting();
        if let Some(control) = self.control() {
            control.confirm_shutdown(now)?;
        }
        if let Some(export) = &self.export {
            export.control().confirm_shutdown(now)?;
        }
        Ok(())
    }
    /// Confirmed shutdown has no active worker or final rendering left to apply.
    pub fn worker_shutdown_settled(&self) -> bool {
        self.closing && self.active.is_none() && self.export.is_none()
    }
    /// Earlier lifecycle failure permanently closed further processing admission.
    pub fn blocked(&self) -> bool {
        self.blocked
    }
    /// Admit one terminal-report export; reservation lasts through actual actor settlement.
    pub fn start_export(
        &mut self,
        run_id: RunId,
        destination: PathBuf,
    ) -> Result<eml_protocol::desktop::OperationId, SessionError> {
        if self.closing {
            return Err(SessionError::Closing);
        }
        if self.blocked {
            return Err(SessionError::Blocked);
        }
        if self.export.is_some() {
            return Err(SessionError::ExportBusy);
        }
        crate::validate_destination(&destination).map_err(|_| SessionError::InvalidExportDraft)?;
        let report = self
            .reports
            .iter()
            .find(|r| r.id == run_id)
            .and_then(SessionReport::export_snapshot)
            .ok_or(SessionError::Unsettled)?;
        let operation_id = self.next_operation()?;
        let control = WorkerControl::for_operation(operation_id, run_id);
        let export = crate::supervisor::ExportRun::start(
            self.helper.clone(),
            self.version.clone(),
            report,
            destination,
            control,
        )
        .map_err(SessionError::ExportLaunch)?;
        self.export = Some(export);
        self.export_completion = None;
        self.export_reserved = Some(run_id);
        Ok(operation_id)
    }
    /// Exact currently owned export child identity.
    pub fn active_export(&self) -> Option<eml_protocol::desktop::OperationId> {
        self.export.as_ref().map(|e| e.control().operation_id())
    }
    /// Report pinned until the export child and all transport actors settle.
    pub fn export_reserved_run(&self) -> Option<RunId> {
        self.export_reserved
    }
    /// Control only the exact active export operation; stale callbacks refuse.
    pub fn export_control_for(
        &self,
        id: eml_protocol::desktop::OperationId,
    ) -> Option<&WorkerControl> {
        self.export
            .as_ref()
            .filter(|e| e.control().operation_id() == id)
            .map(|e| e.control())
    }
    /// Compact terminal export observations retained independently of report slots.
    pub fn export_completion(&self) -> Option<&ExportCompletion> {
        self.export_completion.as_ref()
    }
    /// Nonblocking export settlement; release the report only after all owners settle.
    pub fn poll_export(&mut self) -> Option<ExportObservation> {
        let observation = self.export.as_mut()?.poll();
        if matches!(
            &observation,
            ExportObservation::Pending { warning: Some(_) }
        ) {
            self.blocked = true;
        }
        if let ExportObservation::Settled(completion) = &observation {
            if !completion.worker.settled {
                return Some(observation);
            }
            self.blocked |= !completion.worker.reusable;
            self.export_completion = Some(completion.clone());
            self.export = None;
            self.export_reserved = None;
        }
        Some(observation)
    }
}
