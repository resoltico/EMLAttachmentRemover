//! Positive-PID authority is revoked under the same lock as terminal waitpid.
use std::{
    ffi::{CString, OsStr},
    io::{self, PipeReader, PipeWriter},
    os::{
        fd::{AsFd, AsRawFd, IntoRawFd},
        unix::ffi::OsStrExt,
    },
    path::Path,
    ptr,
    sync::Mutex,
};

/// Only the two stop operations needed by the desktop lifecycle.
#[derive(Clone, Copy)]
pub enum StopSignal {
    /// Send SIGINT; the supervisor owns stop count, phase and escalation policy.
    Interrupt,
    /// Send SIGKILL through the still-owned capability during bounded settlement.
    Kill,
}
/// Actual terminal status, distinct from nonterminal traced stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildExit {
    /// Normal exit status, absent after signal termination.
    pub code: Option<i32>,
    /// Terminating signal, absent after normal exit.
    pub signal: Option<i32>,
}
/// A nonblocking child observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildState {
    /// No terminal reap; signal authority remains owned.
    Running,
    /// Terminal reap has already revoked signal authority.
    Exited(ChildExit),
}
struct Authority {
    pid: Option<libc::pid_t>,
    terminal: Option<ChildExit>,
}
/// Sole native child owner. Do not install competing SIGCHLD handlers/reapers.
/// Retain this capability until actual terminal settlement, including launch errors.
/// Dropping a Rust value does not establish termination or reap.
#[must_use]
pub struct OwnedChild(Mutex<Authority>);
impl OwnedChild {
    /// Snapshot for a nonshipping OS observer, never wait/signal authority.
    /// Callers must retain this owner and check its state after the observation.
    #[cfg(feature = "native-fixtures")]
    pub fn observation_pid(&self) -> io::Result<Option<i32>> {
        self.0
            .lock()
            .map(|authority| authority.pid)
            .map_err(|_| io::Error::other("Child authority lock poisoned"))
    }
    /// Read whether this owner has explicitly revoked its positive-PID authority.
    /// A lock error leaves authority unresolved; callers must retain the owner.
    pub fn authority_revoked(&self) -> io::Result<bool> {
        self.0
            .lock()
            .map(|authority| authority.pid.is_none())
            .map_err(|_| io::Error::other("Child authority lock poisoned"))
    }
    /// Poll under the same tiny lock used for signals; no blocking wait or I/O.
    pub fn poll(&self) -> io::Result<ChildState> {
        let mut authority = self
            .0
            .lock()
            .map_err(|_| io::Error::other("Child authority lock poisoned"))?;
        if let Some(status) = authority.terminal {
            return Ok(ChildState::Exited(status));
        }
        let pid = authority
            .pid
            .ok_or_else(|| io::Error::from_raw_os_error(libc::ECHILD))?;
        let mut status = 0;
        // SAFETY: this positive PID came directly from our successful spawn;
        // exclusive reaping and signalling are serialized by this lock.
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == 0 {
            return Ok(ChildState::Running);
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                return Ok(ChildState::Running);
            }
            if error.raw_os_error() == Some(libc::ECHILD) {
                authority.pid = None;
            }
            return Err(error);
        }
        if result != pid {
            return Err(io::Error::other("Unexpected child wait identity"));
        }
        if libc::WIFSTOPPED(status) {
            return Ok(ChildState::Running);
        }
        let exit = if libc::WIFEXITED(status) {
            ChildExit {
                code: Some(libc::WEXITSTATUS(status)),
                signal: None,
            }
        } else if libc::WIFSIGNALED(status) {
            ChildExit {
                code: None,
                signal: Some(libc::WTERMSIG(status)),
            }
        } else {
            return Err(io::Error::other("Unrecognized nonterminal child status"));
        };
        // Numeric PID reuse is possible after terminal wait. Revoke before the
        // lock is released, never by a delayed reader/UI/background notification.
        authority.pid = None;
        authority.terminal = Some(exit);
        Ok(ChildState::Exited(exit))
    }
    /// Signal only a still-owned positive PID. False means authority was revoked.
    pub fn signal(&self, signal: StopSignal) -> io::Result<bool> {
        let authority = self
            .0
            .lock()
            .map_err(|_| io::Error::other("Child authority lock poisoned"))?;
        let Some(pid) = authority.pid else {
            return Ok(false);
        };
        let signal = match signal {
            StopSignal::Interrupt => libc::SIGINT,
            StopSignal::Kill => libc::SIGKILL,
        };
        // SAFETY: PID is positive, launch-bound and cannot be reaped by this
        // owner until the same mutex is released. No negative/group-zero send.
        if unsafe { libc::kill(pid, signal) } == 0 {
            Ok(true)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
/// Parent-only pipe endpoints. Reader/writer settlement is separate from reap.
pub struct PipeEndpoints {
    /// Complete request writer; close explicitly after feeding or withholding input.
    pub input: PipeWriter,
    /// Protocol events; drain through EOF even after decoding fails.
    pub output: PipeReader,
    /// Diagnostics; drain independently with bounded retained bytes.
    pub errors: PipeReader,
    /// Optional private worker readiness/first-interrupt acknowledgement channel.
    pub control: Option<PipeReader>,
}
/// A native startup/cleanup failure without user content or path text.
#[derive(Debug)]
pub struct SpawnIssue {
    /// Actual failing system operation.
    pub operation: &'static str,
    /// Native error returned by that operation.
    pub native_code: i32,
}
/// Retains a successful spawn even if subsequent startup cleanup failed.
#[must_use]
pub struct SpawnObservation {
    /// The sole child capability, including after a post-spawn setup failure.
    pub child: Option<OwnedChild>,
    /// Parent endpoints, requiring explicit settlement whenever a child exists.
    pub pipes: PipeEndpoints,
    /// Every observed native setup/destroy failure, in observation order.
    pub issues: Vec<SpawnIssue>,
}
fn checked(operation: &'static str, code: i32) -> Result<(), SpawnIssue> {
    if code == 0 {
        Ok(())
    } else {
        Err(SpawnIssue {
            operation,
            native_code: code,
        })
    }
}
fn close_descriptor(
    descriptor: impl IntoRawFd,
    operation: &'static str,
    issues: &mut Vec<SpawnIssue>,
) {
    if let Err(error) = close_owned(descriptor) {
        issues.push(SpawnIssue {
            operation,
            native_code: error.raw_os_error().unwrap_or(libc::EIO),
        });
    }
}
fn close_owned(descriptor: impl IntoRawFd) -> io::Result<()> {
    // SAFETY: ownership is consumed exactly once. Never retry a close against
    // a numeric descriptor which may already have been released/reused.
    if unsafe { libc::close(descriptor.into_raw_fd()) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
/// Close this owned parent request writer once, retaining errors without retry.
pub fn close_writer(writer: PipeWriter) -> io::Result<()> {
    close_owned(writer)
}
/// Close this owned reader once after EOF/failure, retaining errors without retry.
pub fn close_reader(reader: PipeReader) -> io::Result<()> {
    close_owned(reader)
}

/// Spawn one exact absolute executable with explicit child signal policy.
/// No shell, PATH resolution, global signal changes or background reaper is used.
/// Errors returned as `Err` occur before spawning; inspect `issues` even on `Ok`.
pub fn spawn(executable: &Path, arguments: &[&OsStr]) -> io::Result<SpawnObservation> {
    spawn_bound(executable, arguments, false)
}
/// Spawn with an additional child-writer descriptor3 and an independent parent
/// control reader. All stable source copies are moved above descriptors0..3.
pub fn spawn_with_control_pipe(
    executable: &Path,
    arguments: &[&OsStr],
) -> io::Result<SpawnObservation> {
    spawn_bound(executable, arguments, true)
}
fn spawn_bound(
    executable: &Path,
    arguments: &[&OsStr],
    control_pipe: bool,
) -> io::Result<SpawnObservation> {
    if !executable.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Child executable must be absolute",
        ));
    }
    let (path, argv, environment) = command_strings(executable, arguments)?;
    let argv_pointers = terminated_pointers(&argv);
    let env_pointers = terminated_pointers(&environment);
    let (child_input, input) = io::pipe()?;
    let (output, child_output) = io::pipe()?;
    let (errors, child_errors) = io::pipe()?;
    let (control, child_control) = if control_pipe {
        let (read, write) = io::pipe()?;
        (Some(read), Some(write))
    } else {
        (None, None)
    };
    // SAFETY: only this newly owned request pipe is changed. A broken worker
    // pipe must return EPIPE without depending on a global SIGPIPE disposition.
    // Darwin sys/fcntl.h defines F_SETNOSIGPIPE=73; libc0.2.190 omits the binding.
    const SET_NO_SIGPIPE: libc::c_int = 73;
    if unsafe { libc::fcntl(input.as_raw_fd(), SET_NO_SIGPIPE, 1) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let descriptors = duplicate_child_sources(
        &child_input,
        &child_output,
        &child_errors,
        child_control.as_ref(),
    )?;
    let mut attributes = ptr::null_mut();
    let mut actions = ptr::null_mut();
    let mut attributes_ready = false;
    let mut actions_ready = false;
    let mut pid = 0;
    let mut spawned = false;
    let mut issues = Vec::new();
    // SAFETY: all C strings, pointer arrays, descriptors and initialized spawn
    // records remain alive through posix_spawn. Init flags guard every destroy;
    // native output storage is initialized or only read after successful calls.
    let attempt = unsafe {
        (|| -> Result<(), SpawnIssue> {
            initialize_spawn_records(
                &mut attributes,
                &mut attributes_ready,
                &mut actions,
                &mut actions_ready,
                &descriptors,
            )?;
            checked(
                "posix-spawn",
                libc::posix_spawn(
                    &mut pid,
                    path.as_ptr(),
                    &actions,
                    &attributes,
                    argv_pointers.as_ptr(),
                    env_pointers.as_ptr(),
                ),
            )?;
            if pid <= 0 {
                return Err(SpawnIssue {
                    operation: "spawn-positive-pid",
                    native_code: libc::EPROTO,
                });
            }
            spawned = true;
            Ok(())
        })()
    };
    if let Err(issue) = attempt {
        issues.push(issue);
    }
    // Preserve successful child ownership before inspecting any cleanup result.
    let child = spawned.then(|| {
        OwnedChild(Mutex::new(Authority {
            pid: Some(pid),
            terminal: None,
        }))
    });
    destroy_spawn_records(
        &mut actions,
        actions_ready,
        &mut attributes,
        attributes_ready,
        &mut issues,
    );
    for descriptor in descriptors {
        close_descriptor(descriptor, "close-stdio-duplicate", &mut issues);
    }
    close_descriptor(child_input, "close-parent-child-input", &mut issues);
    close_descriptor(child_output, "close-parent-child-output", &mut issues);
    close_descriptor(child_errors, "close-parent-child-errors", &mut issues);
    if let Some(writer) = child_control {
        close_descriptor(writer, "close-parent-child-control", &mut issues);
    }
    Ok(SpawnObservation {
        child,
        pipes: PipeEndpoints {
            input,
            output,
            errors,
            control,
        },
        issues,
    })
}

/// Check the sole-reaper prerequisite before creating native spawn records.
fn check_child_disposition() -> Result<(), SpawnIssue> {
    // SAFETY: sigaction writes this initialized local and does not change disposition.
    unsafe {
        let mut disposition = std::mem::zeroed::<libc::sigaction>();
        if libc::sigaction(libc::SIGCHLD, ptr::null(), &mut disposition) != 0 {
            return Err(SpawnIssue {
                operation: "sigaction-read",
                native_code: io::Error::last_os_error()
                    .raw_os_error()
                    .unwrap_or(libc::EIO),
            });
        }
        if disposition.sa_sigaction != libc::SIG_DFL
            || disposition.sa_flags & libc::SA_NOCLDWAIT != 0
        {
            return Err(SpawnIssue {
                operation: "exclusive-default-SIGCHLD",
                native_code: libc::EINVAL,
            });
        }
    }
    Ok(())
}

/// Configure child dispositions without changing the parent's signal state.
///
/// # Safety
/// `attributes` must be an initialized posix_spawn attribute record.
unsafe fn configure_signal_defaults(
    attributes: &mut libc::posix_spawnattr_t,
) -> Result<(), SpawnIssue> {
    // SAFETY: caller retains the initialized record; signal sets are local initialized storage.
    unsafe {
        let mut defaults = std::mem::zeroed::<libc::sigset_t>();
        let mut mask = std::mem::zeroed::<libc::sigset_t>();
        if libc::sigemptyset(&mut defaults) != 0 || libc::sigemptyset(&mut mask) != 0 {
            return Err(SpawnIssue {
                operation: "sigemptyset",
                native_code: io::Error::last_os_error()
                    .raw_os_error()
                    .unwrap_or(libc::EIO),
            });
        }
        for signal in [
            libc::SIGINT,
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGPIPE,
            libc::SIGCHLD,
        ] {
            if libc::sigaddset(&mut defaults, signal) != 0 {
                return Err(SpawnIssue {
                    operation: "sigaddset",
                    native_code: io::Error::last_os_error()
                        .raw_os_error()
                        .unwrap_or(libc::EIO),
                });
            }
        }
        checked(
            "spawn-default-signals",
            libc::posix_spawnattr_setsigdefault(attributes, &defaults),
        )?;
        checked(
            "spawn-unblocked-mask",
            libc::posix_spawnattr_setsigmask(attributes, &mask),
        )?;
        let flags = libc::c_short::try_from(
            libc::POSIX_SPAWN_SETSIGDEF
                | libc::POSIX_SPAWN_SETSIGMASK
                | libc::POSIX_SPAWN_CLOEXEC_DEFAULT,
        )
        .map_err(|_| SpawnIssue {
            operation: "spawn-flags-range",
            native_code: libc::EINVAL,
        })?;
        checked(
            "spawn-flags",
            libc::posix_spawnattr_setflags(attributes, flags),
        )?;
    }
    Ok(())
}

/// Configure duplicated descriptors above all child targets.
///
/// # Safety
/// `actions` must be initialized; descriptors must remain alive until posix_spawn returns.
unsafe fn configure_file_actions(
    actions: &mut libc::posix_spawn_file_actions_t,
    descriptors: &[std::os::fd::OwnedFd],
) -> Result<(), SpawnIssue> {
    // SAFETY: caller retains initialized actions and every source descriptor until spawn ends.
    unsafe {
        for (standard, descriptor) in descriptors.iter().enumerate() {
            checked(
                "spawn-dup2",
                libc::posix_spawn_file_actions_adddup2(
                    actions,
                    descriptor.as_raw_fd(),
                    standard as i32,
                ),
            )?;
            checked(
                "spawn-close-duplicate",
                libc::posix_spawn_file_actions_addclose(actions, descriptor.as_raw_fd()),
            )?;
        }
    }
    Ok(())
}

fn command_strings(
    executable: &Path,
    arguments: &[&OsStr],
) -> io::Result<(CString, Vec<CString>, Vec<CString>)> {
    let cstring = |bytes: &[u8]| {
        CString::new(bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Native child argument contains NUL",
            )
        })
    };
    let path = cstring(executable.as_os_str().as_bytes())?;
    let mut argv = vec![path.clone()];
    for argument in arguments {
        argv.push(cstring(argument.as_bytes())?);
    }
    let mut environment = Vec::new();
    for (key, value) in std::env::vars_os() {
        let mut bytes = key.as_bytes().to_vec();
        bytes.push(b'=');
        bytes.extend_from_slice(value.as_bytes());
        environment.push(cstring(&bytes)?);
    }
    Ok((path, argv, environment))
}

fn destroy_spawn_records(
    actions: &mut libc::posix_spawn_file_actions_t,
    actions_ready: bool,
    attributes: &mut libc::posix_spawnattr_t,
    attributes_ready: bool,
    issues: &mut Vec<SpawnIssue>,
) {
    // SAFETY: these records were initialized successfully and are destroyed once.
    if actions_ready
        && let Err(issue) = checked("spawn-actions-destroy", unsafe {
            libc::posix_spawn_file_actions_destroy(actions)
        })
    {
        issues.push(issue);
    }
    // SAFETY: same initialization/one-destroy invariant as file actions.
    if attributes_ready
        && let Err(issue) = checked("spawn-attributes-destroy", unsafe {
            libc::posix_spawnattr_destroy(attributes)
        })
    {
        issues.push(issue);
    }
}

/// Initialize each native record before configuring it, retaining successful-init flags on failure.
///
/// # Safety
/// Raw record slots must be uninitialized, with both readiness flags false.
unsafe fn initialize_spawn_records(
    attributes: &mut libc::posix_spawnattr_t,
    attributes_ready: &mut bool,
    actions: &mut libc::posix_spawn_file_actions_t,
    actions_ready: &mut bool,
    descriptors: &[std::os::fd::OwnedFd],
) -> Result<(), SpawnIssue> {
    // SAFETY: caller owns fresh storage and retains flags for exact checked destruction.
    unsafe {
        check_child_disposition()?;
        checked(
            "spawn-attributes-init",
            libc::posix_spawnattr_init(attributes),
        )?;
        *attributes_ready = true;
        checked(
            "spawn-actions-init",
            libc::posix_spawn_file_actions_init(actions),
        )?;
        *actions_ready = true;
        configure_signal_defaults(attributes)?;
        configure_file_actions(actions, descriptors)?;
    }
    Ok(())
}

fn duplicate_child_sources(
    child_input: &PipeReader,
    child_output: &PipeWriter,
    child_errors: &PipeWriter,
    child_control: Option<&PipeWriter>,
) -> io::Result<Vec<std::os::fd::OwnedFd>> {
    // Sources must be above ALL targets, including the optional descriptor3;
    // otherwise a dup/close action could destroy a later source or target.
    let minimum = if child_control.is_some() { 4 } else { 3 };
    let mut descriptors = vec![
        rustix::io::fcntl_dupfd_cloexec(child_input.as_fd(), minimum)?,
        rustix::io::fcntl_dupfd_cloexec(child_output.as_fd(), minimum)?,
        rustix::io::fcntl_dupfd_cloexec(child_errors.as_fd(), minimum)?,
    ];
    if let Some(writer) = child_control {
        descriptors.push(rustix::io::fcntl_dupfd_cloexec(writer.as_fd(), minimum)?);
    }
    Ok(descriptors)
}

/// CString owners remain in the caller until the native call consumes this pointer array.
fn terminated_pointers(values: &[CString]) -> Vec<*mut libc::c_char> {
    let mut pointers: Vec<_> = values
        .iter()
        .map(|value| value.as_ptr().cast_mut())
        .collect();
    pointers.push(ptr::null_mut());
    pointers
}
