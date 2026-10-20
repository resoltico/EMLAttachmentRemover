//! Exact native Job ownership; no process-name/PID-group termination.
use super::{Controller, ProcessResult, Result};
use eml_windows::{Job, ProcessSpec, environment_name_equal};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io::{self, Write},
    os::windows::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn complete_environment(command: &Command) -> io::Result<Vec<(OsString, OsString)>> {
    // Reviewed repository constructors inherit the environment and only override
    // or remove named variables; none use env_clear. This is the same command
    // subset as the Unix controller. Native comparison also handles Path/PATH.
    let mut values = std::env::vars_os().collect::<Vec<_>>();
    for (name, value) in command.get_envs() {
        let mut index = 0;
        while index < values.len() {
            if environment_name_equal(name, &values[index].0)? {
                values.remove(index);
            } else {
                index += 1;
            }
        }
        if let Some(value) = value {
            values.push((name.to_owned(), value.to_owned()));
        }
    }
    Ok(values)
}
fn executable(
    program: &OsStr,
    directory: &Path,
    environment: &[(OsString, OsString)],
) -> io::Result<PathBuf> {
    let program = Path::new(program);
    if program.components().count() > 1 || program.is_absolute() {
        return directory.join(program).canonicalize();
    }
    let mut search = None;
    for (name, value) in environment {
        if environment_name_equal(name, OsStr::new("PATH"))? {
            search = Some(value);
            break;
        }
    }
    let search = search
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Tool PATH is unavailable"))?;
    for parent in std::env::split_paths(search) {
        if !parent.is_absolute() {
            continue;
        }
        let mut candidate = parent.join(program);
        if candidate.extension().is_none() {
            candidate.set_extension("exe");
        }
        if candidate
            .extension()
            .is_some_and(|name| name.eq_ignore_ascii_case("exe"))
            && candidate.is_file()
        {
            return candidate.canonicalize();
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Selected native executable was not found on absolute PATH entries",
    ))
}
fn output(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}
fn record(log: &Path, value: &serde_json::Value) -> Result<()> {
    let path = log.with_extension("control.json");
    let writing = (|| -> Result<()> {
        let mut file = output(&path)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        Ok(())
    })();
    writing.map_err(|error| {
        format!("Native evidence write failed: {error}; retained process facts: {value}").into()
    })
}

pub(super) fn run(
    controller: &Controller,
    command: Command,
    log: &Path,
    deadline: Duration,
    separate: bool,
) -> Result<ProcessResult> {
    let started = Instant::now();
    let environment = complete_environment(&command)?;
    let directory = command
        .get_current_dir()
        .map_or_else(std::env::current_dir, |path| {
            std::env::current_dir().map(|cwd| cwd.join(path))
        })?
        .canonicalize()?;
    let executable = executable(command.get_program(), &directory, &environment)?;
    let arguments = command.get_args().map(OsStr::to_owned).collect::<Vec<_>>();
    let stdout = output(log)?;
    let stderr = if separate {
        output(&log.with_extension("stderr"))?
    } else {
        stdout.try_clone()?
    };
    let job = Job::new()
        .map_err(|error| format!("Private Job setup failed before process admission: {error}"))?;
    let launch = job.launch(
        &ProcessSpec {
            executable: &executable,
            arguments: &arguments,
            directory: &directory,
            environment: &environment,
        },
        &stdout,
        &stderr,
    );
    let process = match launch {
        Ok(process) => process,
        Err(primary) => {
            let termination = job.terminate(125);
            let began = Instant::now();
            let cleanup = (|| -> io::Result<bool> {
                loop {
                    if job.active_processes()? == 0 {
                        return Ok(true);
                    }
                    if began.elapsed() >= Duration::from_secs(5) {
                        return Ok(false);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            })();
            let facts = serde_json::json!({"stage":"launch","primary_error":primary.to_string(),"native_code":primary.raw_os_error(),"termination_error":termination.as_ref().err().map(ToString::to_string),"zero_active_members":cleanup.as_ref().ok(),"cleanup_error":cleanup.as_ref().err().map(ToString::to_string),"scope":"failed launch; no successful execution claim"});
            record(log, &facts)?;
            return Err(format!("Native process launch failed: {facts}").into());
        }
    };
    let mut cancelled = false;
    let mut timed_out = false;
    let mut unexpected = 0usize;
    let observation = (|| -> io::Result<Option<u32>> {
        loop {
            cancelled = controller.stopped.load(Ordering::Relaxed);
            timed_out = started.elapsed() >= deadline;
            if cancelled || timed_out {
                return Ok(None);
            }
            if let Some(code) = process.exit_code()? {
                // Process termination and Job accounting are independent native
                // observations. Allow normal child teardown before classifying
                // anything still active as an unexpected survivor.
                let settled = Instant::now();
                while job.active_processes()? != 0 && settled.elapsed() < Duration::from_millis(250)
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                unexpected = job.active_processes()? as usize;
                return Ok(Some(code));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    let cleanup_started = Instant::now();
    let needs_cleanup = cancelled || timed_out || unexpected != 0 || observation.is_err();
    let termination = if needs_cleanup {
        job.terminate(130)
    } else {
        Ok(())
    };
    let cleanup = (|| -> io::Result<(Option<u32>, bool)> {
        loop {
            let code = process.exit_code()?;
            let empty = job.active_processes()? == 0;
            if code.is_some() && empty {
                return Ok((code, true));
            }
            if cleanup_started.elapsed() >= Duration::from_secs(5) {
                return Ok((code, false));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    let code = cleanup.as_ref().ok().and_then(|(code, _)| *code);
    let complete = cleanup.as_ref().is_ok_and(|(_, complete)| *complete) && termination.is_ok();
    let record = serde_json::json!({"native_exit_code":code,"cancelled":cancelled,"timed_out":timed_out,"unexpected_survivors":unexpected,"cleanup_complete":complete,
        "observation_error":observation.as_ref().err().map(ToString::to_string),"termination_error":termination.as_ref().err().map(ToString::to_string),"cleanup_error":cleanup.as_ref().err().map(ToString::to_string),
        "elapsed_ms":started.elapsed().as_millis(),"cleanup_ms":cleanup_started.elapsed().as_millis(),"scope":"private Job assigned before execution; actual process exit and zero active members; no console-control claim"});
    self::record(log, &record)?;
    // A primary API/control failure stays failed even if a later cleanup succeeds.
    if observation.is_err()
        || termination.is_err()
        || cleanup.is_err()
        || !complete
        || code.is_none()
    {
        return Err(format!(
            "Native Job observation failed; retained {}",
            log.with_extension("control.json").display()
        )
        .into());
    }
    drop(process);
    drop(job);
    drop(stdout);
    drop(stderr);
    Ok(ProcessResult {
        status: std::process::ExitStatus::from_raw(code.expect("observed native exit")),
        timed_out,
        cancelled,
        unexpected_survivors: unexpected,
        cleanup_complete: complete,
        elapsed: started.elapsed(),
        cleanup_elapsed: cleanup_started.elapsed(),
    })
}
