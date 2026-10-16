//! Owned verification processes with a separate session, guarded groups and explicit cleanup evidence.
#[cfg(unix)]
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::{ffi::OsString, io::Read};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
#[cfg(unix)]
pub(crate) mod unix;
/// Tool-controller errors; no source mail is included.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
/// A failed command retaining actual process facts through caller propagation.
#[derive(Debug)]
pub struct CommandFailure {
    /// Exit/lifetime observations; these do not substitute for cleanup authority.
    pub process: serde_json::Value,
    /// Retained command output location.
    pub log: PathBuf,
    /// A separate bounded diagnostic read/forwarding failure, when observed.
    pub diagnostic_error: Option<String>,
}
impl std::fmt::Display for CommandFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Command did not qualify; retained {}: {}",
            self.log.display(),
            self.process
        )?;
        if let Some(error) = &self.diagnostic_error {
            write!(formatter, "; diagnostic forwarding also failed: {error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for CommandFailure {}
/// Actual worker exit plus independent deadline/lifetime observations.
pub struct ProcessResult {
    /// Worker status, never the supervisor's substitute status.
    pub status: ExitStatus,
    /// Execution was not observed complete within its deadline.
    pub timed_out: bool,
    /// Cancellation or parent-control loss was observed.
    pub cancelled: bool,
    /// Live unexpected descendants found after ordinary worker exit.
    pub unexpected_survivors: usize,
    /// Cleanup observation completed under the reviewed tool topology.
    pub cleanup_complete: bool,
    /// Total observed controller time.
    pub elapsed: Duration,
    /// Additional cleanup time after execution stopped/completed.
    pub cleanup_elapsed: Duration,
}
impl ProcessResult {
    /// Require successful exit and clean lifetime while preserving failure facts.
    pub fn require_success(&self, log: &Path) -> Result<()> {
        if self.success() {
            Ok(())
        } else {
            Err(Box::new(CommandFailure {
                process: self.evidence(),
                log: log.to_owned(),
                diagnostic_error: None,
            }))
        }
    }
    /// Actual exit and lifetime facts, independent of a caller's expected exit.
    pub fn evidence(&self) -> serde_json::Value {
        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            self.status.signal()
        };
        #[cfg(not(unix))]
        let signal: Option<i32> = None;
        serde_json::json!({"exit_code":self.status.code(),"signal":signal,"timed_out":self.timed_out,"cancelled":self.cancelled,"unexpected_survivors":self.unexpected_survivors,"cleanup_complete":self.cleanup_complete,"elapsed_seconds":self.elapsed.as_secs_f64(),"cleanup_seconds":self.cleanup_elapsed.as_secs_f64()})
    }
    /// Whether exit, timing and lifetime evidence all qualify this command.
    pub fn success(&self) -> bool {
        self.status.success()
            && !self.timed_out
            && !self.cancelled
            && self.unexpected_survivors == 0
            && self.cleanup_complete
    }
}
/// A developer-only controller using an immutable owned helper executable.
pub struct Controller {
    #[cfg(unix)]
    pub(crate) helper: PathBuf,
    pub(crate) root: PathBuf,
    pub(crate) stopped: Arc<AtomicBool>,
    admission_open: AtomicBool,
    #[cfg(unix)]
    pub(crate) inspector: PathBuf,
}
impl Controller {
    /// Copy the helper to owned target storage, avoiding later build-path replacement.
    pub fn new(root: &Path, executable: &Path) -> Result<Self> {
        let parent = root.join("target/process-control");
        fs::create_dir_all(&parent)?;
        let directory = tempfile::Builder::new()
            .prefix("helper-")
            .tempdir_in(&parent)?
            .keep();
        let helper = directory.join("xtask-helper");
        let mut input = fs::File::open(executable)?;
        let mut output = fs::File::options()
            .write(true)
            .create_new(true)
            .open(&helper)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            #[cfg(unix)]
            helper,
            root: root.to_owned(),
            stopped: Arc::new(AtomicBool::new(false)),
            admission_open: AtomicBool::new(true),
            #[cfg(unix)]
            inspector: PathBuf::from("/bin/ps"),
        })
    }
    /// Install this command-line process's cooperative INT/TERM/HUP adapter.
    pub fn install_signals(&self) -> Result<()> {
        let stopped = self.stopped.clone();
        ctrlc::set_handler(move || stopped.store(true, Ordering::Relaxed))?;
        Ok(())
    }
    /// Request cancellation; ordinary code performs owned cleanup.
    pub fn request_stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
    /// Whether new work remains admissible after cancellation/control failures.
    pub fn admits_work(&self) -> bool {
        self.admission_open.load(Ordering::Relaxed) && !self.stopped.load(Ordering::Relaxed)
    }
    fn observe(&self, result: Result<ProcessResult>) -> Result<ProcessResult> {
        let close = match &result {
            Err(_) => true,
            Ok(result) => {
                result.timed_out
                    || result.cancelled
                    || result.unexpected_survivors != 0
                    || !result.cleanup_complete
            }
        };
        if close {
            self.admission_open.store(false, Ordering::Relaxed);
        }
        result
    }
    /// Run a worker; both worker and cleanup evidence must be checked.
    pub fn run(&self, command: Command, log: &Path, deadline: Duration) -> Result<ProcessResult> {
        if !self.admits_work() {
            return Err("Process admission closed after cancellation or control failure".into());
        }
        #[cfg(unix)]
        {
            self.observe(unix::run(self, command, log, deadline, false))
        }
        #[cfg(not(unix))]
        {
            let _ = (command, log, deadline);
            self.observe(Err(
                "Owned Unix-session verification control is unavailable on this host".into(),
            ))
        }
    }
    /// Preserve stdout at `log` and stderr at `log.with_extension("stderr")`,
    /// returning the actual status even when nonzero. Callers must check timing,
    /// cancellation, survivors and cleanup independently of an expected exit.
    /// Use fresh disjoint log paths. This API does not cap streaming byte counts;
    /// callers must bound document lengths before reading/parsing the files.
    pub fn run_separate_output(
        &self,
        command: Command,
        log: &Path,
        deadline: Duration,
    ) -> Result<ProcessResult> {
        if !self.admits_work() {
            return Err("Process admission closed after cancellation or control failure".into());
        }
        #[cfg(unix)]
        {
            self.observe(unix::run(self, command, log, deadline, true))
        }
        #[cfg(not(unix))]
        {
            let _ = (command, log, deadline);
            self.observe(Err(
                "Owned Unix-session verification control is unavailable on this host".into(),
            ))
        }
    }
    /// Capture a bounded stdout document through the same lifetime controller.
    pub fn capture(&self, command: &mut Command) -> Result<Vec<u8>> {
        if !self.admits_work() {
            return Err("Process admission closed before capture".into());
        }
        let parent = self.root.join("target/process-control");
        let directory = tempfile::Builder::new()
            .prefix("capture-")
            .tempdir_in(parent)?
            .keep();
        let mut owned = Command::new(command.get_program());
        owned.args(command.get_args());
        if let Some(cwd) = command.get_current_dir() {
            owned.current_dir(cwd);
        }
        for (key, value) in command.get_envs() {
            if let Some(value) = value {
                owned.env(key, value);
            } else {
                owned.env_remove(key);
            }
        }
        let result = self.run_separate_output(
            owned,
            &directory.join("output.log"),
            Duration::from_secs(10 * 60),
        )?;
        let errors = directory.join("output.stderr");
        let diagnostic: Result<()> = (|| {
            if errors.is_file() {
                use std::io::Read;
                let mut bytes = Vec::new();
                fs::File::open(errors)?
                    .take(16 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err(
                        "Captured stderr exceeds its document bound; see retained log".into(),
                    );
                }
                std::io::stderr().write_all(&bytes)?;
            }
            Ok(())
        })();
        if !result.success() {
            return Err(Box::new(CommandFailure {
                process: result.evidence(),
                log: directory.join("output.log"),
                diagnostic_error: diagnostic.err().map(|error| error.to_string()),
            }));
        }
        diagnostic?;
        let path = directory.join("output.log");
        let length = fs::metadata(&path)?.len();
        if length > 16 * 1024 * 1024 {
            return Err("Captured setup output exceeds its bounded document size".into());
        }
        Ok(fs::read(path)?)
    }
}
#[cfg(unix)]
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    program: Vec<u8>,
    args: Vec<Vec<u8>>,
    cwd: Option<Vec<u8>>,
    env: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    log: Vec<u8>,
    deadline_ms: u64,
    inspector: Vec<u8>,
    capture: bool,
}
#[cfg(unix)]
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(crate) enum Message {
    Ready {
        session: i32,
    },
    Finished {
        raw_status: Option<i32>,
        timed_out: bool,
        cancelled: bool,
        unexpected_survivors: usize,
        cleanup_complete: bool,
        execution_ms: u64,
        cleanup_ms: u64,
        detail: Option<String>,
    },
}
/// Handle private helper roles before ordinary command-line parsing.
/// No worker is launched by a helper until session readiness is established.
pub fn helper_entry() -> Option<ExitCode> {
    let mut args = std::env::args_os();
    args.next();
    let mode = args.next()?;
    #[cfg(unix)]
    {
        if mode == "--eml-process-session" {
            return Some(unix::session_entry());
        }
        if mode == "--eml-process-pin" {
            return Some(unix::pin_entry(args.next(), args.next()));
        }
    }
    let _ = mode;
    None
}
