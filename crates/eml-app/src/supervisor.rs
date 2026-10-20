//! One native owner polls/signals independently of pipe/model/UI backpressure.
use crate::{
    FrameError, FrameSplitter, ProtocolFailure, RequestSnapshot, RunReport, WorkerControl,
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

/// A desktop lifecycle/transport failure; Core's per-item outcomes stay separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerFailure {
    /// Core refused the captured output before any helper launch.
    Request,
    /// The exact bundled executable could not be started or set up.
    Launch,
    /// Exact private worker version/compiled CPU/OS did not match.
    Identity,
    /// A pipe, framing, decoder or owned-thread operation failed.
    Transport,
    /// Native ownership or complete reader/writer settlement is unverified.
    Unverified,
}
impl std::fmt::Display for WorkerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Desktop worker {self:?}")
    }
}
impl std::error::Error for WorkerFailure {}
/// Read-only access to accepted report facts; no mutable decoder escapes.
#[derive(Clone)]
pub struct ReportHandle(Arc<Mutex<RunReport>>);
impl ReportHandle {
    pub(crate) fn new(version: String) -> Self {
        Self(Arc::new(Mutex::new(RunReport::for_native_worker(version))))
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
    /// Serialize on an explicitly owned background export task, never the UI thread.
    pub fn export_bytes(&self) -> io::Result<Vec<u8>> {
        self.0
            .try_lock()
            .map_err(|error| match error {
                TryLockError::WouldBlock => {
                    io::Error::new(io::ErrorKind::WouldBlock, "Report is busy")
                }
                TryLockError::Poisoned(_) => io::Error::other("Report ownership is unverified"),
            })?
            .technical_report()
            .map_err(io::Error::other)
    }
    pub(crate) fn export_terminal_bytes(
        &self,
        id: crate::RunId,
        completion: &WorkerCompletion,
    ) -> io::Result<Vec<u8>> {
        self.0
            .try_lock()
            .map_err(|error| match error {
                TryLockError::WouldBlock => {
                    io::Error::new(io::ErrorKind::WouldBlock, "Report is busy")
                }
                TryLockError::Poisoned(_) => io::Error::other("Report ownership is unverified"),
            })?
            .technical_report_with_lifecycle(id, completion)
            .map_err(io::Error::other)
    }
}
/// Actual completed lifecycle observations. A valid Core failure is not a broken
/// transport, and Stop intent never changes a received terminal item/summary.
#[derive(Clone, Debug, serde::Serialize)]
pub struct WorkerCompletion {
    /// Processing child's actual normal exit; absent if no processing child or signalled.
    pub exit_code: Option<i32>,
    /// Processing child's actual terminating signal.
    pub signal: Option<i32>,
    /// A complete processing request might have started delivery.
    pub input_may_have_been_sent: bool,
    /// Actual process/reader/writer/control settlement was observed.
    pub settled: bool,
    /// A later queued request can safely be admitted after final UI application.
    pub reusable: bool,
    /// First lifecycle failure, separate from Core's typed item errors.
    pub failure: Option<WorkerFailure>,
    /// Bounded authored/native diagnostics, excluding raw stdout/message bytes.
    pub faults: Vec<String>,
    /// At most64KiB of separately drained stderr for explicit failure inspection.
    pub stderr: Vec<u8>,
    /// Retained stderr reached its cap; the reader still drained to EOF.
    pub stderr_truncated: bool,
    /// Actual successful native interrupt calls, not inferred handler delivery.
    pub interrupt_calls_succeeded: u8,
    /// The private first-interrupt observation byte was actually received.
    pub first_interrupt_observed: bool,
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
        let report = ReportHandle::new(version.clone());
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
    // This owner thread may block in native filesystem I/O. No UI thread waits
    // on it, and its lifetime remains owned until it actually settles. Core
    // repeats the binding during processing; this is never publication authority.
    if let Err(error) = eml_core::check_output_location(request.request()) {
        completion.failure = Some(WorkerFailure::Request);
        completion.settled = true;
        completion.reusable = true;
        issue(
            &mut completion.faults,
            format!(
                "Selection not started: {error}. Choose an existing copy destination or Beside originals. Originals are unchanged."
            ),
        );
        return completion;
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
        Some(request.encoded().to_vec()),
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
