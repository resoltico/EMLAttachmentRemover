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
#[derive(Default)]
struct ReadResult {
    total_bytes: usize,
    eof: bool,
    closed: bool,
    bytes: Vec<u8>,
    truncated: bool,
    tail: bool,
    error: Option<String>,
}
#[derive(Default)]
struct WriteResult {
    broken_pipe: bool,
    closed: bool,
    error: Option<String>,
}
#[derive(Default)]
struct ControlFacts {
    stream: WorkerControlStream,
    error: Option<String>,
}
struct Observed {
    ready: bool,
    killed: bool,
    transport_ok: bool,
    exit: Option<ChildExit>,
    stdout: ReadResult,
    stderr: ReadResult,
    control: ReadResult,
    writer: WriteResult,
    first: bool,
    interrupts: u8,
    faults: Vec<String>,
}
fn issue(faults: &mut Vec<String>, message: impl Into<String>) {
    if faults.len() < 32 {
        faults.push(message.into().chars().take(1000).collect());
    }
}
fn append_error(slot: &mut Option<String>, message: String) {
    *slot = Some(slot.take().map_or_else(
        || message.clone(),
        |earlier| format!("{earlier}; {message}"),
    ));
}
fn finish_reader(reader: io::PipeReader, result: &mut ReadResult, role: &str) {
    match close_reader(reader) {
        Ok(()) => result.closed = true,
        Err(error) => append_error(&mut result.error, format!("{role} close: {error}")),
    }
}
fn close_input(input: io::PipeWriter) -> WriteResult {
    match close_writer(input) {
        Ok(()) => WriteResult {
            closed: true,
            ..WriteResult::default()
        },
        Err(error) => WriteResult {
            error: Some(format!("request close: {error}")),
            ..WriteResult::default()
        },
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
    let intent = match token.intent(Instant::now()) {
        Ok(intent) => intent,
        Err(error) => {
            completion.failure = Some(WorkerFailure::Unverified);
            issue(&mut completion.faults, error.to_string());
            return completion;
        }
    };
    let stopping = intent.interrupts > 0 || intent.closing || intent.aborted;
    let probe_settled = settled(&probe);
    let expected_identity = identity_line(&version);
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
        || probe.stdout.bytes != identity_line(&version).as_bytes()
        || !probe.stderr.bytes.is_empty()
        || stopping
    {
        completion.settled = probe_settled;
        completion.failure = if !probe_settled {
            Some(WorkerFailure::Unverified)
        } else if !probe.transport_ok {
            Some(WorkerFailure::Transport)
        } else if cancelled_probe {
            None
        } else {
            Some(WorkerFailure::Identity)
        };
        completion.reusable = cancelled_probe && probe_settled;
        completion.faults = probe.faults;
        completion.stderr = probe.stderr.bytes;
        completion.stderr_truncated = probe.stderr.truncated;
        if completion.failure == Some(WorkerFailure::Identity) {
            let _ = finalize_report(&report, &health, |report| {
                report.invalidate(ProtocolFailure::IncompatibleHelper)
            });
        }
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
    completion.exit_code = observed.exit.and_then(|exit| exit.code);
    completion.signal = observed.exit.and_then(|exit| exit.signal);
    completion.settled = settled(&observed);
    completion.faults = observed.faults.clone();
    completion.stderr = observed.stderr.bytes.clone();
    completion.stderr_truncated = observed.stderr.truncated;
    completion.interrupt_calls_succeeded = observed.interrupts;
    completion.first_interrupt_observed = observed.first;
    let intent = token.intent(Instant::now());
    completion.input_may_have_been_sent = intent
        .as_ref()
        .map_or(true, |intent| intent.input_may_have_been_sent);
    let stopped = intent.is_ok_and(|intent| intent.interrupts > 0 || intent.closing);
    let protocol = finalize_report(&report, &health, |report| {
        report.finish(completion.exit_code, observed.stdout.tail)
    })
    .unwrap_or_else(Err);
    let expected_partial = stopped
        && (completion.exit_code == Some(130) || (completion.signal == Some(9) && observed.killed))
        && matches!(
            protocol,
            Err(ProtocolFailure::MissingSummary
                | ProtocolFailure::IncompleteTail
                | ProtocolFailure::ExitDisagreement)
        );
    let refused =
        completion.exit_code == Some(2) && observed.stdout.total_bytes == 0 && observed.ready;
    completion.reusable = completion.settled
        && observed.transport_ok
        && (protocol.is_ok() || expected_partial || refused);
    if !completion.settled {
        completion.failure = Some(WorkerFailure::Unverified);
    } else if !completion.reusable {
        completion.failure = Some(WorkerFailure::Transport);
    }
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
fn capture(mut reader: &io::PipeReader, limit: usize) -> ReadResult {
    let mut result = ReadResult::default();
    let mut buffer = [0; 16384];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                result.eof = true;
                break;
            }
            Ok(count) => {
                result.total_bytes = result.total_bytes.saturating_add(count);
                let retain = count.min(limit.saturating_sub(result.bytes.len()));
                result.bytes.extend_from_slice(&buffer[..retain]);
                result.truncated |= retain != count;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                result.error = Some(error.to_string());
                break;
            }
        }
    }
    result
}
fn outcomes(
    mut reader: &io::PipeReader,
    report: Arc<Mutex<RunReport>>,
    token: WorkerControl,
    broken: Arc<AtomicBool>,
) -> ReadResult {
    let mut result = ReadResult::default();
    let mut framing = FrameSplitter::default();
    let mut buffer = [0; 65536];
    let mut accepting = true;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                result.eof = true;
                break;
            }
            Ok(count) => {
                result.total_bytes = result.total_bytes.saturating_add(count);
                if accepting {
                    let accepted = framing.append::<ProtocolFailure>(&buffer[..count], |frame| {
                        report
                            .lock()
                            .map_err(|_| ProtocolFailure::MalformedRecord)?
                            .accept_frame(frame)
                    });
                    if let Err(error) = accepted {
                        if let Ok(mut report) = report.lock() {
                            report.invalidate(error);
                        }
                        accepting = false;
                        broken.store(true, Ordering::Release);
                        let _ = token.abort_transport();
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                result.error = Some(error.to_string());
                broken.store(true, Ordering::Release);
                break;
            }
        }
    }
    result.tail = framing.finish();
    result
}
impl From<FrameError> for ProtocolFailure {
    fn from(error: FrameError) -> Self {
        match error {
            FrameError::LineLimit | FrameError::RunLimit => Self::ResourceLimit,
            FrameError::EmptyRecord | FrameError::Failed => Self::MalformedRecord,
        }
    }
}
fn control_reader(
    mut reader: &io::PipeReader,
    facts: Arc<Mutex<ControlFacts>>,
    broken: Arc<AtomicBool>,
    token: WorkerControl,
) -> ReadResult {
    let mut result = ReadResult::default();
    let mut bytes = [0; 16];
    let mut accepting = true;
    loop {
        match reader.read(&mut bytes) {
            Ok(0) => {
                result.eof = true;
                break;
            }
            Ok(count) => {
                result.total_bytes = result.total_bytes.saturating_add(count);
                if accepting {
                    let mut invalid = false;
                    match facts.lock() {
                        Ok(mut facts) => {
                            if facts.stream.append(&bytes[..count]).is_err() {
                                let _ = token.abort_transport();
                                facts.error = Some("Private control sequence failed".into());
                                invalid = true;
                            }
                        }
                        Err(_) => {
                            let _ = token.abort_transport();
                            invalid = true;
                        }
                    }
                    if invalid {
                        accepting = false;
                        result.error = Some("Private control sequence failed".into());
                        broken.store(true, Ordering::Release);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                let _ = token.abort_transport();
                broken.store(true, Ordering::Release);
                result.error = Some(error.to_string());
                break;
            }
        }
    }
    result
}
type ReadSlot = Arc<Mutex<Option<io::PipeReader>>>;
type WriteSlot = Arc<Mutex<Option<io::PipeWriter>>>;
struct ReadRole {
    slot: ReadSlot,
    thread: Option<JoinHandle<ReadResult>>,
    result: Option<ReadResult>,
    reported: bool,
}
impl ReadRole {
    fn new(reader: Option<io::PipeReader>) -> Self {
        let absent = reader.is_none();
        Self {
            slot: Arc::new(Mutex::new(reader)),
            thread: None,
            result: absent.then_some(ReadResult {
                eof: true,
                closed: true,
                ..ReadResult::default()
            }),
            reported: false,
        }
    }
    fn start(
        &mut self,
        name: &str,
        read: impl FnOnce(&io::PipeReader) -> ReadResult + Send + 'static,
    ) {
        if self.thread.is_some() || self.result.is_some() {
            return;
        }
        let slot = Arc::clone(&self.slot);
        match thread::Builder::new().name(name.into()).spawn(move || {
            // This slot contains only an opaque owned endpoint. A reader panic
            // cannot drop it; the scope recovers ownership after joining. This
            // does not recover/qualify a poisoned report or application model.
            let held = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match held.as_ref() {
                Some(reader) => read(reader),
                None => ReadResult {
                    error: Some("Reader endpoint unavailable".into()),
                    ..ReadResult::default()
                },
            }
        }) {
            Ok(thread) => self.thread = Some(thread),
            Err(error) => {
                self.result = Some(ReadResult {
                    error: Some(format!("Reader setup: {error}")),
                    ..ReadResult::default()
                })
            }
        }
    }
    fn poll(&mut self) -> Option<String> {
        if self.thread.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(thread) = self.thread.take()
        {
            self.result = Some(thread.join().unwrap_or(ReadResult {
                error: Some("Reader panicked".into()),
                ..ReadResult::default()
            }));
        }
        if let Some(result) = &mut self.result {
            let endpoint = self
                .slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(reader) = endpoint {
                finish_reader(reader, result, "reader");
            }
            if !self.reported {
                self.reported = true;
                return result.error.clone();
            }
        }
        None
    }
    fn done(&self) -> bool {
        self.result.is_some() && self.thread.is_none()
    }
    fn take(&mut self) -> ReadResult {
        self.result.take().unwrap_or(ReadResult {
            error: Some("Reader result unavailable".into()),
            ..ReadResult::default()
        })
    }
}
struct WriteRole {
    slot: WriteSlot,
    thread: Option<JoinHandle<WriteResult>>,
    result: Option<WriteResult>,
    reported: bool,
}
impl WriteRole {
    fn new(writer: io::PipeWriter) -> Self {
        Self {
            slot: Arc::new(Mutex::new(Some(writer))),
            thread: None,
            result: None,
            reported: false,
        }
    }
    fn start(&mut self, bytes: Vec<u8>, token: WorkerControl) {
        let slot = Arc::clone(&self.slot);
        match thread::Builder::new()
            .name("worker-request".into())
            .spawn(move || {
                let held = slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut result = WriteResult::default();
                match (held.as_ref(), token.begin_feeding()) {
                    (Some(mut writer), Ok(true)) => {
                        if let Err(error) = writer.write_all(&bytes) {
                            result.broken_pipe = error.kind() == io::ErrorKind::BrokenPipe;
                            result.error = Some(error.to_string());
                        }
                    }
                    (_, Ok(false)) => {}
                    (_, Err(error)) => result.error = Some(error.to_string()),
                    (None, _) => result.error = Some("Writer endpoint unavailable".into()),
                }
                result
            }) {
            Ok(thread) => self.thread = Some(thread),
            Err(error) => {
                self.result = Some(WriteResult {
                    error: Some(format!("Request setup: {error}")),
                    ..WriteResult::default()
                })
            }
        }
    }
    fn withhold(&mut self) {
        if self.thread.is_none() && self.result.is_none() {
            self.result = Some(WriteResult::default());
        }
    }
    fn poll(&mut self) -> Option<String> {
        if self.thread.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(thread) = self.thread.take()
        {
            self.result = Some(thread.join().unwrap_or(WriteResult {
                error: Some("Request writer panicked".into()),
                ..WriteResult::default()
            }));
        }
        if let Some(result) = &mut self.result {
            let endpoint = self
                .slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(writer) = endpoint {
                let closed = close_input(writer);
                result.closed = closed.closed;
                if let Some(error) = closed.error {
                    append_error(&mut result.error, error);
                }
            }
            if !self.reported {
                self.reported = true;
                return result.error.clone();
            }
        }
        None
    }
    fn started(&self) -> bool {
        self.thread.is_some() || self.result.is_some()
    }
    fn done(&self) -> bool {
        self.result.is_some() && self.thread.is_none()
    }
    fn take(&mut self) -> WriteResult {
        self.result.take().unwrap_or(WriteResult {
            error: Some("Writer result unavailable".into()),
            ..WriteResult::default()
        })
    }
}
struct OwnedTransportScope {
    child: OwnedChild,
    output: ReadRole,
    errors: ReadRole,
    control: ReadRole,
    input: WriteRole,
    request: Option<Vec<u8>>,
    processing: bool,
    token: WorkerControl,
    report: Option<Arc<Mutex<RunReport>>>,
    health: Health,
    facts: Arc<Mutex<ControlFacts>>,
    broken: Arc<AtomicBool>,
    issues: Vec<eml_macos::SpawnIssue>,
    faults: Vec<String>,
    exit: Option<ChildExit>,
    owner_lost: bool,
    kill_attempted: bool,
    kill_succeeded: bool,
    interrupts: u8,
    first_sent: Option<Instant>,
    cleanup_started: Option<Instant>,
    control_ended: Option<Instant>,
    started: Instant,
    aborting: bool,
}
impl OwnedTransportScope {
    fn fail(&mut self, message: String) {
        issue(&mut self.faults, message);
        self.aborting = true;
        let _ = self.token.abort_transport();
        self.cleanup_started.get_or_insert_with(Instant::now);
    }
    fn readers(&mut self) {
        self.errors
            .start("worker-stderr", |reader| capture(reader, 65536));
        if let Some(report) = &self.report {
            let report = Arc::clone(report);
            let token = self.token.clone();
            let broken = Arc::clone(&self.broken);
            self.output.start("worker-outcomes", move |reader| {
                outcomes(reader, report, token, broken)
            });
        } else {
            self.output
                .start("worker-identity", |reader| capture(reader, 256));
        }
        let facts = Arc::clone(&self.facts);
        let broken = Arc::clone(&self.broken);
        let token = self.token.clone();
        self.control.start("worker-control", move |reader| {
            control_reader(reader, facts, broken, token)
        });
        if !self.processing {
            self.input.withhold();
        }
    }
    fn drive(&mut self) -> Observed {
        // Scope exists outside catch_unwind before issues are formatted or any
        // I/O role is started. Transfer slots survive Builder::spawn errors.
        for issue in std::mem::take(&mut self.issues) {
            self.fail(format!("{} (OS{})", issue.operation, issue.native_code));
        }
        self.readers();
        loop {
            // Native ownership is always observed before EOF/control decisions.
            if self.exit.is_none() && !self.owner_lost {
                match self.child.poll() {
                    Ok(ChildState::Exited(exit)) => {
                        self.exit = Some(exit);
                        self.cleanup_started.get_or_insert_with(Instant::now);
                        if self.processing && self.token.process_reaped().is_err() {
                            self.fail("Control state unavailable after reap".into());
                        }
                    }
                    Ok(ChildState::Running) => {}
                    Err(error) => {
                        self.owner_lost = self.child.authority_revoked().unwrap_or(false);
                        self.fail(format!("Native ownership lost: {error}"));
                    }
                }
            }
            for error in [self.output.poll(), self.errors.poll(), self.control.poll()]
                .into_iter()
                .flatten()
            {
                self.fail(error);
            }
            if let Some(error) = self.input.poll() {
                let expected = self
                    .input
                    .result
                    .as_ref()
                    .is_some_and(|result| result.broken_pipe && result.closed)
                    && self
                        .token
                        .intent(Instant::now())
                        .is_ok_and(|intent| intent.interrupts > 0 || intent.closing);
                if expected {
                    issue(
                        &mut self.faults,
                        format!("Expected stopped request pipe: {error}"),
                    );
                } else {
                    self.fail(error);
                }
            }
            let intent = match self.token.intent(Instant::now()) {
                Ok(intent) => Some(intent),
                Err(error) => {
                    self.fail(error.to_string());
                    None
                }
            };
            let stopped = intent
                .as_ref()
                .is_some_and(|intent| intent.interrupts > 0 || intent.closing);
            let fed = intent
                .as_ref()
                .is_none_or(|intent| intent.input_may_have_been_sent);
            let requested = intent.as_ref().map_or(2, |intent| intent.interrupts);
            let (ready, first, invalid) = self
                .facts
                .lock()
                .map(|facts| {
                    (
                        facts.stream.ready(),
                        facts.stream.first_interrupt(),
                        facts.error.clone(),
                    )
                })
                .unwrap_or((false, false, Some("Control observation unavailable".into())));
            if let Some(error) = invalid
                && !self.aborting
            {
                self.fail(error);
            }
            if self.broken.load(Ordering::Acquire) && !self.aborting {
                self.fail("Outcome/control transport failed".into());
            }
            // A valid R-only EOF may race normal exit. Only a freshly observed
            // still-live child after this fixed grace makes it a control failure.
            if self.processing && self.control.done() && self.exit.is_none() && !self.owner_lost {
                let ended = *self.control_ended.get_or_insert_with(Instant::now);
                if ended.elapsed() >= Duration::from_millis(250)
                    && !(stopped && !fed && self.kill_succeeded)
                    && !self.aborting
                {
                    self.fail("Private control ended while worker remained live".into());
                }
            }
            if self.exit.is_none()
                && requested >= 2
                && self.interrupts == 1
                && !first
                && self
                    .first_sent
                    .is_some_and(|sent| sent.elapsed() >= Duration::from_secs(2))
                && !self.aborting
            {
                self.fail("First interrupt acknowledgment unavailable; second interrupt was not delivered".into());
            }
            if !fed
                && self.started.elapsed() >= Duration::from_secs(10)
                && self.exit.is_none()
                && !stopped
                && !self.aborting
            {
                self.fail("Worker readiness/feed allowance exhausted".into());
            }
            let kill_due = self.aborting
                || intent
                    .as_ref()
                    .is_none_or(|intent| intent.aborted || intent.kill_due)
                || (stopped && !fed);
            if self.exit.is_none() && !self.owner_lost {
                if kill_due && !self.kill_attempted {
                    self.kill_attempted = true;
                    self.cleanup_started.get_or_insert_with(Instant::now);
                    match self.child.signal(StopSignal::Kill) {
                        Ok(true) => self.kill_succeeded = true,
                        Ok(false) => {}
                        Err(error) => self.fail(format!("Owned Kill failed: {error}")),
                    }
                } else if !self.kill_attempted
                    && self.processing
                    && ready
                    && requested > self.interrupts
                    && (self.interrupts == 0 || first)
                {
                    match self.child.signal(StopSignal::Interrupt) {
                        Ok(true) => {
                            self.interrupts += 1;
                            if self.interrupts == 1 {
                                self.first_sent = Some(Instant::now());
                            }
                        }
                        Ok(false) => {}
                        Err(error) => self.fail(format!("Owned interrupt failed: {error}")),
                    }
                }
            }
            if self.processing && !self.input.started() {
                if kill_due || stopped || self.exit.is_some() || self.owner_lost {
                    self.input.withhold();
                } else if ready && let Some(bytes) = self.request.take() {
                    self.input.start(bytes, self.token.clone());
                }
            }
            let roles_done = self.output.done()
                && self.errors.done()
                && self.control.done()
                && self.input.done();
            if (self.exit.is_some() || self.owner_lost) && roles_done {
                break;
            }
            if self
                .cleanup_started
                .is_some_and(|start| start.elapsed() >= Duration::from_secs(2))
            {
                warn(
                    &self.health,
                    "Owned worker/IO settlement is unverified; retaining the run and all pending resources",
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
        let stdout = self.output.take();
        let stderr = self.errors.take();
        let control = self.control.take();
        let writer = self.input.take();
        let (ready, first, error) = self
            .facts
            .lock()
            .map(|facts| {
                (
                    facts.stream.ready(),
                    facts.stream.first_interrupt(),
                    facts.error.clone(),
                )
            })
            .unwrap_or((false, false, Some("Control facts unavailable".into())));
        if let Some(error) = error {
            self.fail(error);
        }
        let stopped = self
            .token
            .intent(Instant::now())
            .is_ok_and(|intent| intent.interrupts > 0 || intent.closing);
        // Only the proved empty never-fed owned-cancel case excuses no R.
        // Malformed bytes and independent I/O failures remain failures.
        let cancelled_before_ready = self.processing
            && control.total_bytes == 0
            && control.error.is_none()
            && control.eof
            && control.closed
            && stopped
            && !self
                .token
                .intent(Instant::now())
                .map_or(true, |intent| intent.input_may_have_been_sent)
            && self.kill_succeeded
            && self.exit
                == Some(ChildExit {
                    code: None,
                    signal: Some(9),
                })
            && stdout.eof
            && stdout.closed
            && stdout.total_bytes == 0
            && stderr.total_bytes == 0
            && !self.aborting
            && stderr.eof
            && stderr.closed
            && writer.closed;
        if self.processing && !ready && !cancelled_before_ready {
            self.fail("Worker ended before private readiness".into());
        }
        let expected_pipe = stopped && writer.broken_pipe && writer.closed;
        let transport_ok = !self.aborting
            && stdout.error.is_none()
            && stderr.error.is_none()
            && control.error.is_none()
            && (writer.error.is_none() || expected_pipe)
            && self.health.lock().is_ok_and(|warning| warning.is_none());
        Observed {
            ready,
            killed: self.kill_succeeded,
            transport_ok,
            exit: self.exit,
            stdout,
            stderr,
            control,
            writer,
            first,
            interrupts: self.interrupts,
            faults: std::mem::take(&mut self.faults),
        }
    }
}
fn observe(
    helper: &std::path::Path,
    request: Option<Vec<u8>>,
    token: &WorkerControl,
    report: Option<Arc<Mutex<RunReport>>>,
    health: Health,
) -> Result<Observed, String> {
    let processing = request.is_some();
    let observation = if processing {
        eml_macos::spawn_with_control_pipe(helper, &[OsStr::new("--desktop-control")])
    } else {
        eml_macos::spawn(helper, &[OsStr::new("--version")])
    }
    .map_err(|error| error.to_string())?;
    let PipeEndpoints {
        input,
        output,
        errors,
        control,
    } = observation.pipes;
    let Some(child) = observation.child else {
        let mut faults = Vec::new();
        for native_issue in observation.issues {
            issue(
                &mut faults,
                format!(
                    "{} (OS{})",
                    native_issue.operation, native_issue.native_code
                ),
            );
        }
        for result in [
            close_writer(input),
            close_reader(output),
            close_reader(errors),
        ]
        .into_iter()
        {
            if let Err(error) = result {
                issue(&mut faults, error.to_string());
            }
        }
        if let Some(control) = control
            && let Err(error) = close_reader(control)
        {
            issue(&mut faults, error.to_string());
        }
        return Err(format!("Worker did not spawn: {}", faults.join("; ")));
    };
    // This scope is retained outside the panic boundary before issues are
    // formatted or any fallible I/O setup. A panic switches to the same keeper;
    // no native/endpoint/join ownership is dropped or detached during unwinding.
    let mut scope = OwnedTransportScope {
        child,
        output: ReadRole::new(Some(output)),
        errors: ReadRole::new(Some(errors)),
        control: ReadRole::new(control),
        input: WriteRole::new(input),
        request,
        processing,
        token: token.clone(),
        report,
        health,
        facts: Arc::new(Mutex::new(ControlFacts::default())),
        broken: Arc::new(AtomicBool::new(false)),
        issues: observation.issues,
        faults: Vec::new(),
        exit: None,
        owner_lost: false,
        kill_attempted: false,
        kill_succeeded: false,
        interrupts: 0,
        first_sent: None,
        cleanup_started: None,
        control_ended: None,
        started: Instant::now(),
        aborting: false,
    };
    loop {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scope.drive())) {
            Ok(result) => return Ok(result),
            Err(_) => {
                warn(
                    &scope.health,
                    "Worker owner panicked; retaining native/IO ownership during cleanup",
                );
                scope.fail("Worker owner panicked".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_refuses_busy_model_without_waiting_for_inspector() {
        let report = ReportHandle::new("test-version".into());
        let held = report.0.lock().unwrap();
        let captured = report.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let actor = thread::spawn(move || {
            send.send(captured.export_bytes().map_err(|error| error.kind()))
                .unwrap();
        });
        let observed = receive.recv_timeout(Duration::from_secs(1));
        drop(held);
        actor.join().unwrap();
        assert_eq!(observed.unwrap(), Err(io::ErrorKind::WouldBlock));
    }
    fn wait_reader(role: &mut ReadRole) -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let error = role.poll();
            if role.done() {
                return error;
            }
            assert!(Instant::now() < deadline, "Owned reader did not finish");
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn panicking_reader_retains_endpoint_until_join_and_checked_close() {
        let (reader, writer) = io::pipe().unwrap();
        let mut role = ReadRole::new(Some(reader));
        role.start("owned-reader-panic", |_| panic!("synthetic reader failure"));
        assert_eq!(wait_reader(&mut role).as_deref(), Some("Reader panicked"));
        let result = role.take();
        assert!(result.closed);
        assert!(!result.eof);
        assert!(
            role.slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
        );
        close_writer(writer).unwrap();
    }
    #[test]
    fn ordinary_reader_requires_actual_eof_and_checked_close() {
        let (reader, mut writer) = io::pipe().unwrap();
        let mut role = ReadRole::new(Some(reader));
        role.start("owned-reader-eof", |reader| capture(reader, 3));
        writer.write_all(b"five!").unwrap();
        close_writer(writer).unwrap();
        assert!(wait_reader(&mut role).is_none());
        let result = role.take();
        assert!(result.closed && result.eof && result.truncated);
        assert_eq!(result.total_bytes, 5);
        assert_eq!(result.bytes, b"fiv");
    }
    #[test]
    fn held_final_report_warns_then_finishes_without_erasing_the_warning() {
        use std::sync::mpsc;
        let report = Arc::new(Mutex::new(RunReport::new("test-version")));
        let health = Arc::new(Mutex::new(None));
        let (locked, wait_locked) = mpsc::sync_channel(1);
        let (release, wait_release) = mpsc::sync_channel(1);
        let held = Arc::clone(&report);
        let holder = thread::spawn(move || {
            let _guard = held.lock().unwrap();
            locked.send(()).unwrap();
            wait_release.recv_timeout(Duration::from_secs(6)).unwrap();
        });
        wait_locked.recv_timeout(Duration::from_secs(1)).unwrap();
        let observed = Arc::clone(&health);
        let (done, wait_done) = mpsc::sync_channel(1);
        let owner = thread::spawn(move || {
            let result = finalize_report(&report, &observed, |_| 17);
            done.send(result).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(4);
        while health.lock().unwrap().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let warning = health.lock().unwrap().clone();
        let premature = wait_done.try_recv();
        release.send(()).unwrap();
        holder.join().unwrap();
        let result = wait_done.recv_timeout(Duration::from_secs(1)).unwrap();
        owner.join().unwrap();
        assert!(warning.is_some(), "No bounded pending warning");
        assert!(matches!(premature, Err(mpsc::TryRecvError::Empty)));
        assert_eq!(result, Ok(17));
        assert_eq!(*health.lock().unwrap(), warning);
    }
    #[test]
    fn poisoned_final_report_is_not_recovered_as_a_qualified_empty_report() {
        let report = Arc::new(Mutex::new(RunReport::new("test-version")));
        let held = Arc::clone(&report);
        assert!(
            thread::spawn(move || {
                let _guard = held.lock().unwrap();
                panic!("synthetic report failure");
            })
            .join()
            .is_err()
        );
        let health = Arc::new(Mutex::new(None));
        assert_eq!(
            finalize_report(&report, &health, |_| ()),
            Err(ProtocolFailure::MalformedRecord)
        );
        assert!(health.lock().unwrap().is_some());
        assert!(report.is_poisoned());
    }
    #[test]
    fn independent_prelaunch_abort_is_not_hidden_by_either_stop_order() {
        use crate::RunQueue;
        for abort_first in [false, true] {
            let request =
                RequestSnapshot::new(vec![PathBuf::from("/owned-synthetic/message.eml")], None)
                    .unwrap();
            let mut queue = RunQueue::default();
            let id = queue.enqueue(request.clone()).unwrap();
            let token = WorkerControl::new(id);
            if abort_first {
                token.abort_transport().unwrap();
            }
            token.request_stop().unwrap();
            if !abort_first {
                token.abort_transport().unwrap();
            }
            let health = Arc::new(Mutex::new(Some("Earlier retained warning".into())));
            let completion = execute(
                PathBuf::from("/owned-synthetic/absent-helper"),
                "test-version".into(),
                request,
                token,
                Arc::new(Mutex::new(RunReport::new("test-version"))),
                health,
            );
            assert!(!completion.reusable);
            assert_eq!(completion.failure, Some(WorkerFailure::Transport));
            assert!(!completion.input_may_have_been_sent);
            assert_eq!(completion.faults, ["Earlier retained warning"]);
        }
    }
}
