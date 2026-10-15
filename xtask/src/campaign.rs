//! Source-isolated owned campaigns and exact process exit/deadline handling.
use crate::{Result, capture, git};
use serde::Serialize;
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub(crate) struct Campaign {
    pub directory: PathBuf,
    pub source: PathBuf,
    pub sha: String,
}
impl Campaign {
    pub fn create(root: &Path, role: &str) -> Result<Self> {
        let dirty = git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
        if !dirty.is_empty() {
            return Err("Commit the current workspace before a source-isolated campaign; dirty/new inputs must not be omitted".into());
        }
        let sha = git(root, &["rev-parse", "HEAD"])?;
        let parent = root.join("target").join(role);
        fs::create_dir_all(&parent)?;
        let directory = tempfile::Builder::new()
            .prefix("run-")
            .tempdir_in(&parent)?
            .keep();
        let source = directory.join("source");
        capture(
            Command::new("git")
                .args(["clone", "--no-local", "--quiet"])
                .arg(root)
                .arg(&source),
        )?;
        if git(&source, &["rev-parse", "HEAD"])? != sha {
            return Err("Campaign source clone identity differs from admitted HEAD".into());
        }
        fs::create_dir(directory.join("tmp"))?;
        Ok(Self {
            directory,
            source,
            sha,
        })
    }
    pub fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(&self.source)
            .env("TMPDIR", self.directory.join("tmp"))
            .env("TMP", self.directory.join("tmp"))
            .env("TEMP", self.directory.join("tmp"));
        command.env("CARGO_TARGET_DIR", self.directory.join("build"));
        command
    }
    pub fn report(&self, value: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        File::options()
            .write(true)
            .create_new(true)
            .open(self.directory.join("result.json"))?
            .write_all(&bytes)?;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&bytes)?;
        stdout.write_all(b"\n")?;
        Ok(())
    }
}
pub(crate) fn pin(root: &Path, name: &str) -> Result<String> {
    let table: toml::Value = toml::from_str(&fs::read_to_string(root.join("xtask/tools.toml"))?)?;
    let version = table
        .get("versions")
        .and_then(|value| value.get(name))
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or_else(|| format!("Missing or invalid tool pin {name}"))?;
    Ok(version.to_owned())
}
pub(crate) fn tool_version(root: &Path, tool: &str, pin_name: &str) -> Result<String> {
    let expected = pin(root, pin_name)?;
    let actual = String::from_utf8(capture(
        Command::new("cargo")
            .current_dir(root)
            .args([tool, "--version"]),
    )?)?;
    if actual.trim() != format!("cargo-{tool} {expected}") {
        return Err(format!(
            "Expected cargo-{tool} {expected}; install that version with cargo install --locked"
        )
        .into());
    }
    Ok(expected)
}
pub(crate) struct ProcessResult {
    pub status: ExitStatus,
    pub timed_out: bool,
    pub elapsed: Duration,
}
struct OwnedProcess {
    child: std::process::Child,
    reaped: bool,
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        // This private Child has no other waiter. Its unreaped PID still owns
        // the process group created at spawn, including ordinary descendants.
        #[cfg(unix)]
        {
            use rustix::process::{Pid, Signal, kill_process_group};
            let _ = kill_process_group(Pid::from_child(&self.child), Signal::KILL);
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
pub(crate) fn run_logged(
    mut command: Command,
    log: &Path,
    deadline: Duration,
) -> Result<ProcessResult> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let output = File::options().create_new(true).write(true).open(log)?;
    command
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::from(output));
    let started = Instant::now();
    let mut owned = OwnedProcess {
        child: command.spawn()?,
        reaped: false,
    };
    loop {
        if let Some(status) = owned.child.try_wait()? {
            owned.reaped = true;
            return Ok(ProcessResult {
                status,
                timed_out: false,
                elapsed: started.elapsed(),
            });
        }
        if started.elapsed() > deadline {
            #[cfg(unix)]
            {
                use rustix::process::{Pid, Signal, kill_process_group};
                let group = Pid::from_child(&owned.child);
                let _ = kill_process_group(group, Signal::TERM);
                // Do not reap between signals: the child's reserved PID prevents PGID reuse.
                std::thread::sleep(Duration::from_secs(2));
                let _ = kill_process_group(group, Signal::KILL);
            }
            #[cfg(not(unix))]
            {
                owned.child.kill()?;
            }
            let status = owned.child.wait()?;
            owned.reaped = true;
            return Ok(ProcessResult {
                status,
                timed_out: true,
                elapsed: started.elapsed(),
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
pub(crate) fn require_unix_campaign() -> Result<()> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err("These bounded local campaigns require a Unix host for owned process-group deadline cleanup".into())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn deadline_terminates_and_reaps_only_the_owned_process_group() {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/xtask-tests");
        fs::create_dir_all(&parent).unwrap();
        let directory = tempfile::tempdir_in(parent).unwrap();
        let mut child = Command::new("/bin/sleep");
        child.arg("30");
        let result = run_logged(
            child,
            &directory.path().join("deadline.log"),
            Duration::from_millis(20),
        )
        .unwrap();
        assert!(result.timed_out);
        assert!(!result.status.success());
    }
}
