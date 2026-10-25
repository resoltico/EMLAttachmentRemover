//! Nonblocking supervision of one bundled CLI, with one reader per output pipe.
use crate::{FrameError, FrameSplitter, ProtocolFailure, RequestSnapshot, RunReport};
use eml_core::RunMode;
use rustix::process::{Pid, Signal, kill_process};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
};
/// Retained stderr bound; excess bytes are drained and marked as truncated.
pub const MAX_STDERR_BYTES: usize = 64 * 1024;
/// Terminal worker state, independent of the earlier accepted item facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerState {
    /// Complete successful run.
    Completed,
    /// Complete run with item or run failures.
    CompletedWithFailures,
    /// Cooperative stop with a complete interrupted summary.
    Stopped,
    /// Interrupt arrived before the first start record.
    StoppedBeforeStart,
    /// The owner requested SIGKILL; publication can have preceded it.
    ForceStopped,
    /// CLI refused the request before producing a start record.
    RequestRefused,
    /// The stream or process did not complete its contract.
    Incomplete,
}
/// Worker lifecycle or transport failure; accepted records are retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerFailure {
    /// First record was not the expected CLI start identity.
    Identity,
    /// Invalid or incomplete protocol stream.
    Protocol(ProtocolFailure),
    /// A pipe reader or writer failed or panicked.
    Transport,
}
impl From<FrameError> for WorkerFailure {
    fn from(_: FrameError) -> Self {
        Self::Protocol(ProtocolFailure::ResourceLimit)
    }
}
/// Shared accepted facts, read without blocking the UI while decoding a record.
#[derive(Clone)]
pub struct ReportHandle(Arc<Mutex<RunReport>>);
impl ReportHandle {
    /// Render a consistent report snapshot if its reader is currently available.
    /// Poisoned state is unavailable and is never presented as an empty success.
    pub fn with_report<T>(&self, render: impl FnOnce(&RunReport) -> T) -> Option<T> {
        self.0.try_lock().ok().map(|report| render(&report))
    }
}
/// Settled process and pipe observations, plus the exact bounded stream for export.
pub struct WorkerCompletion {
    /// Terminal lifecycle state.
    pub state: WorkerState,
    /// Actual reaped process status.
    pub exit: ExitStatus,
    /// Transport failure, if any.
    pub failure: Option<WorkerFailure>,
    /// Earlier accepted protocol facts.
    pub report: ReportHandle,
    /// Received stdout bytes, without an app-owned report wrapper.
    pub jsonl: Arc<[u8]>,
    /// Bounded diagnostic stderr.
    pub stderr: Vec<u8>,
    /// Whether stderr exceeded its retained byte bound.
    pub stderr_truncated: bool,
}
struct StreamEnd {
    jsonl: Vec<u8>,
    tail: bool,
    failure: Option<WorkerFailure>,
}
struct Diagnostics {
    bytes: Vec<u8>,
    truncated: bool,
}
/// Resolve the CLI relative to the desktop executable; never search PATH.
pub fn bundled_cli(executable: &Path) -> io::Result<PathBuf> {
    let contents = executable.parent().and_then(Path::parent).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Desktop executable has no Contents directory",
        )
    })?;
    Ok(contents.join("Helpers").join(eml_core::product::CLI_NAME))
}
/// One owned child; polling, Stop and Force Stop never wait on its pipes.
pub struct WorkerRun {
    child: Option<Child>,
    report: ReportHandle,
    output: Option<JoinHandle<StreamEnd>>,
    diagnostics: Option<JoinHandle<io::Result<Diagnostics>>>,
    input: Option<JoinHandle<io::Result<()>>>,
    events: mpsc::Receiver<Option<WorkerFailure>>,
    exit: Option<ExitStatus>,
    failure: Option<WorkerFailure>,
    stop_requested: bool,
    forced: bool,
}
impl WorkerRun {
    /// Spawn an absolute CLI and feed one EOF-terminated protocol-1 request.
    pub fn start(cli: &Path, request: &RequestSnapshot, home: &Path) -> io::Result<Self> {
        if !cli.is_absolute() || !home.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Worker executable and home directory must be absolute",
            ));
        }
        let mut child = Command::new(cli)
            .args(["--format", "json", "--request", "-"])
            .current_dir(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().expect("piped stdin");
        let output = child.stdout.take().expect("piped stdout");
        let diagnostics = child.stderr.take().expect("piped stderr");
        let report = ReportHandle(Arc::new(Mutex::new(RunReport::for_native_worker(
            eml_core::product::VERSION,
        ))));
        let (sender, events) = mpsc::channel();
        let mut run = Self {
            child: Some(child),
            report: report.clone(),
            output: None,
            diagnostics: None,
            input: None,
            events,
            exit: None,
            failure: None,
            stop_requested: false,
            forced: false,
        };
        let dry_run = request.request().mode == RunMode::DryRun;
        run.output = Some(
            thread::Builder::new()
                .name("cli-stdout".into())
                .spawn(move || read_output(output, report, dry_run, sender))?,
        );
        run.diagnostics = Some(
            thread::Builder::new()
                .name("cli-stderr".into())
                .spawn(move || read_diagnostics(diagnostics))?,
        );
        let encoded = request.encoded().to_vec();
        run.input = Some(
            thread::Builder::new()
                .name("cli-request".into())
                .spawn(move || {
                    input.write_all(&encoded)?;
                    input.flush()
                })?,
        );
        Ok(run)
    }
    /// Accepted progress, available before process completion.
    pub fn report(&self) -> &ReportHandle {
        &self.report
    }
    /// Send exactly one cooperative interrupt to the still-owned, unreaped child.
    pub fn stop(&mut self) -> io::Result<()> {
        if self.stop_requested || self.forced {
            return Ok(());
        }
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        self.stop_requested = true;
        kill_process(Pid::from_child(child), Signal::INT).map_err(Into::into)
    }
    /// Force Stop uses the child's kill operation, never a second interrupt.
    pub fn force_stop(&mut self) -> io::Result<()> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        child.kill()?;
        self.forced = true;
        Ok(())
    }
    /// Reap when ready; return a completion only after all three pipe threads settle.
    pub fn poll(&mut self) -> io::Result<Option<WorkerCompletion>> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        if self.exit.is_none() {
            self.exit = child.try_wait()?;
        }
        for event in self.events.try_iter() {
            if let Some(failure) = event {
                self.failure.get_or_insert(failure);
            }
            let summary = self
                .report
                .with_report(|report| report.summary().is_some())
                .unwrap_or(false);
            if self.failure.is_some() || !summary {
                child.kill()?;
            }
        }
        if self.exit.is_none() {
            self.exit = child.try_wait()?;
        }
        let Some(exit) = self.exit else {
            return Ok(None);
        };
        if !self.output.as_ref().is_some_and(|task| task.is_finished())
            || !self
                .diagnostics
                .as_ref()
                .is_some_and(|task| task.is_finished())
            || !self.input.as_ref().is_some_and(|task| task.is_finished())
        {
            return Ok(None);
        }
        let output = self
            .output
            .take()
            .expect("finished reader")
            .join()
            .unwrap_or_else(|_| StreamEnd {
                jsonl: Vec::new(),
                tail: true,
                failure: Some(WorkerFailure::Transport),
            });
        let diagnostics = self
            .diagnostics
            .take()
            .expect("finished diagnostics")
            .join()
            .ok()
            .and_then(Result::ok);
        if !self
            .input
            .take()
            .expect("finished writer")
            .join()
            .ok()
            .is_some_and(|result| result.is_ok())
        {
            self.failure.get_or_insert(WorkerFailure::Transport);
        }
        self.failure = self.failure.or(output.failure);
        let mut report = match self.report.0.lock() {
            Ok(report) => report,
            Err(_) => {
                self.child.take();
                return Ok(Some(WorkerCompletion {
                    state: WorkerState::Incomplete,
                    exit,
                    failure: Some(WorkerFailure::Transport),
                    report: self.report.clone(),
                    jsonl: output.jsonl.into(),
                    stderr: diagnostics.map_or_else(Vec::new, |d| d.bytes),
                    stderr_truncated: true,
                }));
            }
        };
        let started = report.start().is_some();
        let interrupted = report.summary().is_some_and(|summary| summary.interrupted);
        if let Err(failure) = report.finish(exit.code(), output.tail) {
            self.failure.get_or_insert(WorkerFailure::Protocol(failure));
        }
        let state = if self.forced {
            WorkerState::ForceStopped
        } else if !started && interrupted_exit(exit) {
            WorkerState::StoppedBeforeStart
        } else if !started && exit.code() == Some(2) {
            WorkerState::RequestRefused
        } else if self.failure.is_some() || diagnostics.is_none() {
            WorkerState::Incomplete
        } else if interrupted {
            WorkerState::Stopped
        } else if exit.success() {
            WorkerState::Completed
        } else {
            WorkerState::CompletedWithFailures
        };
        drop(report);
        if diagnostics.is_none() {
            self.failure.get_or_insert(WorkerFailure::Transport);
        }
        let diagnostics = diagnostics.unwrap_or(Diagnostics {
            bytes: Vec::new(),
            truncated: false,
        });
        self.child.take();
        Ok(Some(WorkerCompletion {
            state,
            exit,
            failure: self.failure,
            report: self.report.clone(),
            jsonl: output.jsonl.into(),
            stderr: diagnostics.bytes,
            stderr_truncated: diagnostics.truncated,
        }))
    }
}
impl Drop for WorkerRun {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            // Abandoned ownership is settled off the UI thread.
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
fn interrupted_exit(status: ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    status.code() == Some(130) || status.signal() == Some(2)
}
fn read_output(
    mut reader: impl Read,
    report: ReportHandle,
    dry_run: bool,
    events: mpsc::Sender<Option<WorkerFailure>>,
) -> StreamEnd {
    let mut framing = FrameSplitter::default();
    let mut jsonl = Vec::new();
    let mut buffer = [0; 16 * 1024];
    let mut failure = None;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                failure = Some(WorkerFailure::Transport);
                break;
            }
        };
        if jsonl.len().saturating_add(count) >= eml_protocol::MAX_RUN_BYTES {
            failure = Some(WorkerFailure::Protocol(ProtocolFailure::ResourceLimit));
            break;
        }
        jsonl.extend_from_slice(&buffer[..count]);
        let accepted = framing.append(&buffer[..count], |frame| {
            let mut report = report.0.lock().map_err(|_| WorkerFailure::Transport)?;
            let first = report.start().is_none();
            report.accept_frame(frame).map_err(|error| {
                if first {
                    WorkerFailure::Identity
                } else {
                    WorkerFailure::Protocol(error)
                }
            })?;
            if report.start().is_some_and(|start| start.dry_run != dry_run) {
                return Err(WorkerFailure::Protocol(ProtocolFailure::InvalidOrder));
            }
            Ok::<_, WorkerFailure>(())
        });
        if let Err(error) = accepted {
            failure = Some(
                if report.with_report(|report| report.start().is_none()) == Some(true) {
                    WorkerFailure::Identity
                } else {
                    error
                },
            );
            break;
        }
    }
    let tail = framing.finish();
    let _ = events.send(failure);
    StreamEnd {
        jsonl,
        tail,
        failure,
    }
}
fn read_diagnostics(mut reader: impl Read) -> io::Result<Diagnostics> {
    let mut diagnostics = Diagnostics {
        bytes: Vec::new(),
        truncated: false,
    };
    let mut buffer = [0; 4096];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let retained = count.min(MAX_STDERR_BYTES - diagnostics.bytes.len());
        diagnostics.bytes.extend_from_slice(&buffer[..retained]);
        diagnostics.truncated |= retained < count;
    }
    Ok(diagnostics)
}
