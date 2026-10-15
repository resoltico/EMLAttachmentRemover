//! Owned verification processes with a separate session, guarded groups and explicit cleanup evidence.
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
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
    pub(crate) helper: PathBuf,
    pub(crate) root: PathBuf,
    pub(crate) stopped: Arc<AtomicBool>,
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
            helper,
            root: root.to_owned(),
            stopped: Arc::new(AtomicBool::new(false)),
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
    /// Run a worker; both worker and cleanup evidence must be checked.
    pub fn run(&self, command: Command, log: &Path, deadline: Duration) -> Result<ProcessResult> {
        #[cfg(unix)]
        {
            unix::run(self, command, log, deadline, false)
        }
        #[cfg(not(unix))]
        {
            let _ = (command, log, deadline);
            Err("Owned Unix-session verification control is unavailable on this host".into())
        }
    }
    /// Capture a bounded stdout document through the same lifetime controller.
    pub fn capture(&self, command: &mut Command) -> Result<Vec<u8>> {
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
        #[cfg(unix)]
        let result = unix::run(
            self,
            owned,
            &directory.join("output.log"),
            Duration::from_secs(10 * 60),
            true,
        )?;
        #[cfg(not(unix))]
        let result = self.run(
            owned,
            &directory.join("output.log"),
            Duration::from_secs(10 * 60),
        )?;
        let errors = directory.join("output.stderr");
        if errors.is_file() {
            std::io::stderr().write_all(&fs::read(errors)?)?;
        }
        if !result.success() {
            return Err(format!(
                "Setup command failed: {}; evidence {}",
                result.status,
                directory.display()
            )
            .into());
        }
        let path = directory.join("output.log");
        let length = fs::metadata(&path)?.len();
        if length > 16 * 1024 * 1024 {
            return Err("Captured setup output exceeds its bounded document size".into());
        }
        Ok(fs::read(path)?)
    }
}
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
