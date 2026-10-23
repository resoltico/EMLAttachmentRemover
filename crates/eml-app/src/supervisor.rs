//! One native owner polls/signals independently of pipe/model/UI backpressure.
use crate::{
    FrameError, FrameSplitter, ProtocolFailure, RequestSnapshot, RunReport, WorkerCompletion,
    WorkerControl, WorkerFailure,
};
use eml_macos::{
    ChildExit, ChildState, OwnedChild, PipeEndpoints, StopSignal, close_reader, close_writer,
};
use eml_protocol::worker_control::{WorkerControlStream, identity_line};
use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Read-only access to accepted report facts; no mutable decoder escapes.
#[derive(Clone)]
pub struct ReportHandle(Arc<Mutex<RunReport>>);
impl ReportHandle {
    pub(crate) fn new(version: String) -> Self {
        Self(Arc::new(Mutex::new(RunReport::for_native_mode(
            version,
            eml_core::RunMode::Create,
        ))))
    }
    pub(crate) fn for_mode(version: String, mode: eml_core::RunMode) -> Self {
        Self(Arc::new(Mutex::new(RunReport::for_native_mode(
            version, mode,
        ))))
    }
    pub(crate) fn write_document(
        &self,
        id: crate::RunId,
        completion: &WorkerCompletion,
        writer: impl Write,
    ) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("Report ownership is unverified"))?
            .write_document(id, completion, writer)
            .map_err(io::Error::other)
    }
    pub(crate) fn has_external_readers(&self) -> bool {
        Arc::strong_count(&self.0) > 1
    }
    /// Nonblocking bounded UI inspection. Keep the closure short and release it
    /// before any UI/native call; None means a parser currently owns the model.
    pub fn inspect<T>(
        &self,
        inspect: impl FnOnce(&RunReport) -> T,
    ) -> Result<Option<T>, WorkerFailure> {
        match self.0.try_lock() {
            Ok(report) => Ok(Some(inspect(&report))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => Err(WorkerFailure::Unverified),
        }
    }
}
/// Nonblocking observation. A warning retains the live run/keeper and must not
/// cause callers to drop ownership, start another run or approve Quit.
#[derive(Clone, Debug)]
pub enum WorkerObservation {
    /// Still owns pending resources; optional bounded warning is fail-closed.
    Pending {
        /// First retained lifecycle warning.
        warning: Option<String>,
    },
    /// All actor observations are available, including retained failures.
    Settled(WorkerCompletion),
}
type Health = Arc<Mutex<Option<String>>>;
fn warn(health: &Health, message: &str) {
    let mut health = health
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if health.is_none() {
        *health = Some(message.chars().take(1000).collect());
    }
}
/// Retain this run through native settlement and final UI model application.
/// All blocking spawn/pipe/parsing work belongs to its dedicated threads.
pub struct WorkerRun {
    control: WorkerControl,
    report: ReportHandle,
    completion: Arc<Mutex<Option<WorkerCompletion>>>,
    health: Health,
    owner: Option<JoinHandle<()>>,
}
impl WorkerRun {
    /// Capture this run only after all native actors have actually settled.
    pub fn export_snapshot(&mut self) -> Option<crate::TerminalReport> {
        match self.poll() {
            WorkerObservation::Settled(completion) => {
                crate::TerminalReport::new(self.control.id(), self.report.clone(), completion)
            }
            WorkerObservation::Pending { .. } => None,
        }
    }
    /// Launch a version-bound private worker; no shell/PATH/interpreter fallback.
    pub fn start(
        helper: PathBuf,
        version: String,
        request: RequestSnapshot,
        control: WorkerControl,
    ) -> io::Result<Self> {
        let report = ReportHandle::for_mode(version.clone(), request.mode());
        let completion = Arc::new(Mutex::new(None));
        let health = Arc::new(Mutex::new(None));
        let warnings = Arc::clone(&health);
        let published = Arc::clone(&completion);
        let result = Arc::clone(&report.0);
        let token = control.clone();
        let owner = thread::Builder::new()
            .name("desktop-worker-owner".into())
            .spawn(move || {
                let outcome = execute(helper, version, request, token, result, warnings);
                if let Ok(mut slot) = published.lock() {
                    *slot = Some(outcome);
                }
            })?;
        Ok(Self {
            control,
            report,
            completion,
            health,
            owner: Some(owner),
        })
    }
    /// Immutable run-bound control reference; it owns no numeric PID.
    pub fn control(&self) -> &WorkerControl {
        &self.control
    }
    /// Clone read-only access without copying the retained report.
    pub fn report(&self) -> ReportHandle {
        self.report.clone()
    }
    /// Poll owner completion without a blocking UI join. Callers must apply the
    /// final report before completing the queue reservation or approving Quit.
    pub fn poll(&mut self) -> WorkerObservation {
        if self
            .owner
            .as_ref()
            .is_some_and(|owner| !owner.is_finished())
        {
            return WorkerObservation::Pending {
                warning: self.health.lock().ok().and_then(|warning| warning.clone()),
            };
        }
        if let Some(owner) = self.owner.take()
            && owner.join().is_err()
        {
            warn(
                &self.health,
                "Worker owner ended unexpectedly; ownership is unverified",
            );
        }
        // Final observations take priority over an earlier pending warning;
        // the completion retains that warning as a failed, non-reusable run.
        if let Ok(slot) = self.completion.lock()
            && let Some(result) = slot.as_ref()
        {
            return WorkerObservation::Settled(result.clone());
        }
        WorkerObservation::Pending {
            warning: self.health.lock().ok().and_then(|warning| warning.clone()),
        }
    }
}
fn execute(
    helper: PathBuf,
    version: String,
    request: RequestSnapshot,
    token: WorkerControl,
    report: Arc<Mutex<RunReport>>,
    health: Health,
) -> WorkerCompletion {
    let mut completion =
        execute_inner(helper, version, request, token, report, Arc::clone(&health));
    if let Some(warning) = health.lock().ok().and_then(|warning| warning.clone()) {
        completion.reusable = false;
        completion.failure.get_or_insert(WorkerFailure::Unverified);
        issue(&mut completion.faults, warning);
    }
    completion
}
fn finalize_report<T>(
    report: &Mutex<RunReport>,
    health: &Health,
    apply: impl FnOnce(&mut RunReport) -> T,
) -> Result<T, ProtocolFailure> {
    let started = Instant::now();
    loop {
        match report.try_lock() {
            Ok(mut report) => return Ok(apply(&mut report)),
            Err(TryLockError::Poisoned(_)) => {
                warn(
                    health,
                    "Final report is poisoned/unavailable; no qualified model application",
                );
                return Err(ProtocolFailure::MalformedRecord);
            }
            Err(TryLockError::WouldBlock) => {
                if started.elapsed() >= Duration::from_secs(2) {
                    warn(
                        health,
                        "Final report application is still pending; retain this run",
                    );
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
fn execute_inner(
    helper: PathBuf,
    version: String,
    request: RequestSnapshot,
    token: WorkerControl,
    report: Arc<Mutex<RunReport>>,
    health: Health,
) -> WorkerCompletion {
    let mut completion = WorkerCompletion {
        operation_id: token.operation_id(),
        exit_code: None,
        signal: None,
        input_may_have_been_sent: false,
        settled: false,
        reusable: false,
        failure: None,
        faults: Vec::new(),
        stderr: Vec::new(),
        stderr_truncated: false,
        interrupt_calls_succeeded: 0,
        first_interrupt_observed: false,
    };
    match token.intent(Instant::now()) {
        Ok(intent) if !intent.aborted && (intent.interrupts > 0 || intent.closing) => {
            completion.settled = true;
            completion.reusable = true;
            return completion;
        }
        Ok(intent) if intent.aborted => {
            completion.failure = Some(WorkerFailure::Transport);
            return completion;
        }
        Err(error) => {
            completion.failure = Some(WorkerFailure::Unverified);
            issue(&mut completion.faults, error.to_string());
            return completion;
        }
        _ => {}
    }
    let probe = observe(&helper, None, &token, None, Arc::clone(&health));
    let probe = match probe {
        Ok(value) => value,
        Err(error) => {
            completion.failure = Some(WorkerFailure::Launch);
            issue(&mut completion.faults, error);
            return completion;
        }
    };
    if !completion.admit_identity(&probe, &version, &token, &report, &health) {
        return completion;
    }
    let observed = observe(
        &helper,
        Some(OperationInput::Processing {
            operation_id: token.operation_id(),
            bytes: request.encoded().to_vec(),
        }),
        &token,
        Some(Arc::clone(&report)),
        Arc::clone(&health),
    );
    let observed = match observed {
        Ok(value) => value,
        Err(error) => {
            completion.failure = Some(WorkerFailure::Launch);
            issue(&mut completion.faults, error);
            return completion;
        }
    };
    completion.apply_observed(observed, &token, &report, &health);
    completion
}

fn settled(value: &Observed) -> bool {
    value.exit.is_some()
        && value.stdout.eof
        && value.stdout.closed
        && value.stderr.eof
        && value.stderr.closed
        && value.control.eof
        && value.control.closed
        && value.writer.closed
}
impl From<FrameError> for ProtocolFailure {
    fn from(error: FrameError) -> Self {
        match error {
            FrameError::LineLimit | FrameError::RunLimit => Self::ResourceLimit,
            FrameError::EmptyRecord | FrameError::Failed => Self::MalformedRecord,
        }
    }
}

mod transport;
use transport::*;

#[cfg(test)]
mod tests;

impl WorkerCompletion {
    fn admit_identity(
        &mut self,
        probe: &Observed,
        version: &str,
        token: &WorkerControl,
        report: &Mutex<RunReport>,
        health: &Health,
    ) -> bool {
        let intent = match token.intent(Instant::now()) {
            Ok(intent) => intent,
            Err(error) => {
                self.failure = Some(WorkerFailure::Unverified);
                issue(&mut self.faults, error.to_string());
                return false;
            }
        };
        let stopping = intent.interrupts > 0 || intent.closing || intent.aborted;
        let probe_settled = settled(probe);
        let expected_identity = identity_line(version);
        let identity_valid = probe.stdout.bytes == expected_identity.as_bytes()
            && probe.exit
                == Some(ChildExit {
                    code: Some(0),
                    signal: None,
                })
            && !probe.stdout.truncated
            && probe.stderr.total_bytes == 0;
        let cancelled_probe = stopping
            && !intent.aborted
            && probe.transport_ok
            && (identity_valid
                || (probe.killed
                    && probe.exit
                        == Some(ChildExit {
                            code: None,
                            signal: Some(9),
                        })
                    && !probe.stdout.truncated
                    && probe.stderr.total_bytes == 0
                    && expected_identity
                        .as_bytes()
                        .starts_with(&probe.stdout.bytes)));
        if !probe_settled
            || !probe.transport_ok
            || !probe.faults.is_empty()
            || probe.exit
                != Some(ChildExit {
                    code: Some(0),
                    signal: None,
                })
            || probe.stdout.bytes != identity_line(version).as_bytes()
            || !probe.stderr.bytes.is_empty()
            || stopping
        {
            self.settled = probe_settled;
            self.failure = if !probe_settled {
                Some(WorkerFailure::Unverified)
            } else if !probe.transport_ok {
                Some(WorkerFailure::Transport)
            } else if cancelled_probe {
                None
            } else {
                Some(WorkerFailure::Identity)
            };
            self.reusable = cancelled_probe && probe_settled;
            self.faults = probe.faults.clone();
            self.stderr = probe.stderr.bytes.clone();
            self.stderr_truncated = probe.stderr.truncated;
            if self.failure == Some(WorkerFailure::Identity) {
                let _ = finalize_report(report, health, |report| {
                    report.invalidate(ProtocolFailure::IncompatibleHelper)
                });
            }
            return false;
        }
        true
    }
    fn apply_observed(
        &mut self,
        observed: Observed,
        token: &WorkerControl,
        report: &Mutex<RunReport>,
        health: &Health,
    ) {
        self.exit_code = observed.exit.and_then(|exit| exit.code);
        self.signal = observed.exit.and_then(|exit| exit.signal);
        self.settled = settled(&observed);
        self.faults = observed.faults.clone();
        self.stderr = observed.stderr.bytes.clone();
        self.stderr_truncated = observed.stderr.truncated;
        self.interrupt_calls_succeeded = observed.interrupts;
        self.first_interrupt_observed = observed.first;
        let intent = token.intent(Instant::now());
        self.input_may_have_been_sent = intent
            .as_ref()
            .map_or(true, |intent| intent.input_may_have_been_sent);
        let stopped = intent.is_ok_and(|intent| intent.interrupts > 0 || intent.closing);
        let protocol = finalize_report(report, health, |report| {
            report.finish(self.exit_code, observed.stdout.tail)
        })
        .unwrap_or_else(Err);
        let expected_partial = stopped
            && (self.exit_code == Some(130) || (self.signal == Some(9) && observed.killed))
            && matches!(
                protocol,
                Err(ProtocolFailure::MissingSummary
                    | ProtocolFailure::IncompleteTail
                    | ProtocolFailure::ExitDisagreement)
            );
        let refused =
            self.exit_code == Some(2) && observed.stdout.total_bytes == 0 && observed.ready;
        self.reusable = self.settled
            && observed.transport_ok
            && (protocol.is_ok() || expected_partial || refused);
        if !self.settled {
            self.failure = Some(WorkerFailure::Unverified);
        } else if !self.reusable {
            self.failure = Some(WorkerFailure::Transport);
        }
    }
}

/// Accepted export receipt and independent actual child/transport completion.
#[derive(Clone, Debug)]
pub struct ExportCompletion {
    /// Exact operation that owns these receipt facts.
    pub operation_id: eml_protocol::desktop::OperationId,
    /// Immutable source processing report identity.
    pub run_id: crate::RunId,
    /// Exact requested report address.
    pub destination: PathBuf,
    /// Accepted bounded receipt, absent when outcome is unobserved.
    pub receipt: Option<Box<eml_protocol::desktop::ExportReceipt>>,
    /// Actual process, control and transport observations.
    pub worker: WorkerCompletion,
    /// Parent serialization exceeded its explicit report budget before the body terminator.
    pub report_too_large: bool,
}
impl ExportCompletion {
    /// Qualified receipt and clean, matching process completion.
    pub fn usable(&self) -> bool {
        !self.report_too_large
            && self.worker.operation_id == self.operation_id
            && self
                .receipt
                .as_ref()
                .is_some_and(|r| r.operation_id == self.operation_id && r.qualified())
            && self.worker.settled
            && self.worker.failure.is_none()
            && self.worker.exit_code == Some(0)
            && self.worker.signal.is_none()
    }
    /// Shared conservative export explanation.
    pub fn outcome_message(&self) -> &'static str {
        if self.report_too_large {
            "Report exceeds the 256 MiB export limit. The report is retained."
        } else if self.usable() {
            "Report exported and verified. Existing files are never replaced."
        } else if self
            .receipt
            .as_ref()
            .is_some_and(|r| r.publication == "published")
        {
            "A complete report was published; additional export qualification failed. Inspect the retained receipt."
        } else if self
            .receipt
            .as_ref()
            .is_some_and(|r| r.publication == "not_published")
        {
            "Report was not published. Existing files are never replaced."
        } else if !self.worker.input_may_have_been_sent {
            "Export not started."
        } else {
            "Export outcome unknown; a complete report may exist. Force termination can leave temporary artifacts when cleanup could not be observed."
        }
    }
}
#[derive(Clone, Debug)]
/// Nonblocking independent export-lane lifecycle observation.
pub enum ExportObservation {
    /// Actual owners remain pending; a warning does not establish settlement.
    Pending {
        /// First retained owner warning.
        warning: Option<String>,
    },
    /// All actual owners have settled; retain receipt and lifecycle facts.
    Settled(ExportCompletion),
}
/// One explicitly owned export child using the shared concrete child transport.
pub struct ExportRun {
    control: WorkerControl,
    completion: Arc<Mutex<Option<ExportCompletion>>>,
    health: Health,
    owner: Option<JoinHandle<()>>,
}
impl ExportRun {
    /// Start the exact export operation with a pinned terminal source report.
    pub fn start(
        helper: PathBuf,
        version: String,
        report: crate::TerminalReport,
        destination: PathBuf,
        control: WorkerControl,
    ) -> io::Result<Self> {
        let completion = Arc::new(Mutex::new(None));
        let published = Arc::clone(&completion);
        let health = Arc::new(Mutex::new(None));
        let warnings = Arc::clone(&health);
        let token = control.clone();
        let owner = thread::Builder::new()
            .name("desktop-export-owner".into())
            .spawn(move || {
                let operation_id = token.operation_id();
                let run_id = report.id();
                let mut worker = WorkerCompletion::empty(operation_id);
                let mut receipt = None;
                let mut report_too_large = false;
                let model = Mutex::new(RunReport::new(version.clone()));
                if token
                    .intent(Instant::now())
                    .is_ok_and(|i| !i.aborted && (i.interrupts > 0 || i.closing))
                {
                    worker.settled = true;
                    worker.reusable = true;
                } else {
                    match observe(&helper, None, &token, None, Arc::clone(&warnings)) {
                        Err(error) => {
                            worker.failure = Some(WorkerFailure::Launch);
                            worker.settled = true;
                            issue(&mut worker.faults, error);
                        }
                        Ok(probe) => {
                            if worker.admit_identity(&probe, &version, &token, &model, &warnings) {
                                match observe(
                                    &helper,
                                    Some(OperationInput::Export {
                                        operation_id,
                                        destination: destination.clone(),
                                        report,
                                    }),
                                    &token,
                                    None,
                                    Arc::clone(&warnings),
                                ) {
                                    Err(error) => {
                                        worker.failure = Some(WorkerFailure::Launch);
                                        worker.settled = true;
                                        issue(&mut worker.faults, error);
                                    }
                                    Ok(observed) => {
                                        worker.exit_code = observed.exit.and_then(|e| e.code);
                                        worker.signal = observed.exit.and_then(|e| e.signal);
                                        worker.settled = settled(&observed);
                                        worker.input_may_have_been_sent = token
                                            .intent(Instant::now())
                                            .map_or(true, |i| i.input_may_have_been_sent);
                                        worker.faults = observed.faults;
                                        worker.stderr = observed.stderr.bytes;
                                        worker.stderr_truncated = observed.stderr.truncated;
                                        worker.interrupt_calls_succeeded = observed.interrupts;
                                        worker.first_interrupt_observed = observed.first;
                                        let mut receipt_complete=false;
                                        match eml_protocol::desktop::observe_receipt(&observed.stdout.bytes,operation_id) {
                                            Ok((accepted,complete))=>{receipt=Some(Box::new(accepted));receipt_complete=complete && !observed.stdout.truncated;if !receipt_complete {issue(&mut worker.faults,"Export receipt was followed by invalid or excessive stdout");}},
                                            Err(error)=>issue(&mut worker.faults,error.to_string()),
                                        }
                                        report_too_large = observed.writer.report_too_large;
                                        let stopped=token.intent(Instant::now()).is_ok_and(|i|i.interrupts>0||i.closing);
                                        let interrupted_without_receipt=stopped && receipt.is_none() && observed.stdout.total_bytes==0 && (worker.exit_code==Some(130)||(worker.signal==Some(9)&&observed.killed));
                                        worker.reusable=worker.settled && ((observed.transport_ok && (receipt_complete||interrupted_without_receipt))||report_too_large);
                                        if !worker.settled {
                                            worker.failure = Some(WorkerFailure::Unverified);
                                        } else if !observed.transport_ok || !receipt_complete {
                                            worker.failure = Some(WorkerFailure::Transport);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(warning) = warnings.lock().ok().and_then(|w| w.clone()) {
                    worker.reusable = false;
                    worker.failure.get_or_insert(WorkerFailure::Unverified);
                    issue(&mut worker.faults, warning);
                }
                if let Ok(mut slot) = published.lock() {
                    *slot = Some(ExportCompletion {
                        operation_id,
                        run_id,
                        destination,
                        receipt,
                        worker,
                        report_too_large,
                    });
                }
            })?;
        Ok(Self {
            control,
            completion,
            health,
            owner: Some(owner),
        })
    }
    /// Exact-operation control intent; never a numeric process ID.
    pub fn control(&self) -> &WorkerControl {
        &self.control
    }
    /// Observe actual actor settlement without joining a running owner.
    pub fn poll(&mut self) -> ExportObservation {
        if self.owner.as_ref().is_some_and(|o| !o.is_finished()) {
            return ExportObservation::Pending {
                warning: self.health.lock().ok().and_then(|w| w.clone()),
            };
        }
        if let Some(owner) = self.owner.take()
            && owner.join().is_err()
        {
            warn(
                &self.health,
                "Export owner ended unexpectedly; ownership is unverified",
            );
        }
        if let Ok(slot) = self.completion.lock()
            && let Some(result) = slot.as_ref()
        {
            return ExportObservation::Settled(result.clone());
        }
        ExportObservation::Pending {
            warning: self.health.lock().ok().and_then(|w| w.clone()),
        }
    }
}
impl WorkerCompletion {
    fn empty(operation_id: eml_protocol::desktop::OperationId) -> Self {
        Self {
            operation_id,
            exit_code: None,
            signal: None,
            input_may_have_been_sent: false,
            settled: false,
            reusable: false,
            failure: None,
            faults: Vec::new(),
            stderr: Vec::new(),
            stderr_truncated: false,
            interrupt_calls_succeeded: 0,
            first_interrupt_observed: false,
        }
    }
}
