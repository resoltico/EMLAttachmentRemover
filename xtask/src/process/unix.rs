//! Unix ownership is authorized by live session membership and unreaped group pins,
//! never by a process-name match or a guessed/recycled process-group number.
use super::*;
use rustix::process::{Pid, Signal, getpgid, getpgrp, getpid, getsid, kill_process_group, setsid};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        process::{CommandExt, ExitStatusExt},
    },
    process::{Child, Stdio},
    sync::mpsc,
    time::Instant,
};
const FRAME_LIMIT: usize = 8 * 1024 * 1024;
fn number(pid: Pid) -> i32 {
    pid.as_raw_nonzero().get()
}
fn frame(writer: &mut impl Write, value: &Message) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
fn read_frame(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((FRAME_LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.is_empty() || bytes.len() > FRAME_LIMIT || bytes.last() != Some(&b'\n') {
        return Err("Missing/oversized/incomplete private process frame".into());
    }
    Ok(bytes)
}
fn waitable() -> Result<()> {
    let mut probe = Command::new("/usr/bin/true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if !probe.wait()?.success() {
        return Err(
            "Owned children must be waitable; inherited SIGCHLD/reaper environment is unsupported"
                .into(),
        );
    }
    Ok(())
}
struct ParentSession {
    child: Child,
    writer: Option<std::process::ChildStdin>,
    activated: bool,
    reaped: bool,
    cooperative_grace: Duration,
}
impl Drop for ParentSession {
    fn drop(&mut self) {
        drop(self.writer.take());
        if self.reaped {
            return;
        }
        let began = Instant::now();
        while began.elapsed() < self.cooperative_grace + CLEANUP_GRACE + Duration::from_secs(2) {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.reaped = true;
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => return,
            }
        }
        // Before the complete request frame, no worker could start. Afterwards
        // never kill the cleanup supervisor merely to manufacture completion.
        if !self.activated {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
pub(super) fn run(
    controller: &Controller,
    command: Command,
    log: &Path,
    deadline: Duration,
    capture: bool,
    cooperative_grace: Duration,
) -> Result<ProcessResult> {
    if controller.stopped.load(Ordering::Relaxed) {
        return Err("Verification was cancelled before process launch".into());
    }
    waitable()?;
    let started = Instant::now();
    let mut child = Command::new(&controller.helper)
        .arg("--eml-process-session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let writer = child.stdin.take();
    let output = child
        .stdout
        .take()
        .ok_or("Missing process control reader")?;
    let mut owned = ParentSession {
        child,
        writer,
        activated: false,
        reaped: false,
        cooperative_grace,
    };
    let (send, receive) = mpsc::channel();
    let reading = std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let result = read_frame(&mut reader)
                .and_then(|bytes| Ok(serde_json::from_slice::<Message>(&bytes)?));
            let finished = result.is_err()
                || matches!(
                    result,
                    Ok(Message::Finished { .. } | Message::InspectionFailed { .. })
                );
            let _ = send.send(result.map_err(|error| error.to_string()));
            if finished {
                break;
            }
        }
    });
    let ready = receive.recv_timeout(Duration::from_secs(5));
    let expected = owned.child.id() as i32;
    if !matches!(ready,Ok(Ok(Message::Ready{session})) if session==expected) {
        drop(owned.writer.take());
        let _ = owned.child.kill();
        let _ = owned.child.wait();
        let _ = reading.join();
        return Err("Supervisor startup failed before worker launch".into());
    }
    fs::write(
        log.with_extension("started.json"),
        serde_json::to_vec(&Message::Ready { session: expected })?,
    )?;
    let request = process_request(
        &command,
        log,
        deadline,
        capture,
        cooperative_grace,
        &controller.inspector,
    )?;
    let mut encoded = serde_json::to_vec(&request)?;
    encoded.push(b'\n');
    owned.send_request(controller, &encoded)?;
    let result = owned.wait_result(controller, &receive, started, deadline)?;
    drop(owned.writer.take());
    let status = owned.child.wait()?;
    owned.reaped = true;
    let _ = reading.join();
    qualify_completion(result, status, started, log)
}

pub(super) fn pin_entry(session: Option<OsString>, nonce: Option<OsString>) -> ExitCode {
    let result = (|| -> Result<()> {
        let expected: i32 = session
            .ok_or("Missing pin session")?
            .into_string()
            .map_err(|_| "Invalid pin session")?
            .parse()?;
        if number(getsid(None)?) != expected || number(getpgrp()) == expected {
            return Err("Pin does not belong to its guarded worker session/group".into());
        }
        let nonce = nonce
            .ok_or("Missing private pin nonce")?
            .into_string()
            .map_err(|_| "Invalid pin nonce")?;
        if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Invalid pin handshake".into());
        }
        let mut input = BufReader::new(std::io::stdin());
        let mut handshake = String::new();
        (&mut input).take(256).read_line(&mut handshake)?;
        if handshake != format!("{expected}:{}:{nonce}\n", number(getpgrp())) {
            return Err("Private pin control handshake did not authorize this group".into());
        }
        // Survive HUP/INT/TERM until control EOF, including orphaned stopped
        // groups' HUP/CONT cascade after their supervisor/parent groups die.
        ctrlc::set_handler(|| {})?;
        std::io::stdout().write_all(b"ready\n")?;
        let mut buffer = [0u8; 1];
        loop {
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => (),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
                Err(error) => return Err(error.into()),
            }
        }
        // The live pin itself proves ownership of this exact process group.
        kill_process_group(getpgrp(), Signal::KILL)?;
        Ok(())
    })();
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
pub(crate) struct Pin {
    pub(crate) child: Child,
    pub(crate) id: Pid,
}
pub(crate) fn pin(helper: &Path, session: Pid, group: Option<Pid>, until: Instant) -> Result<Pin> {
    if Instant::now() >= until {
        return Err("Group pin budget exhausted before spawn".into());
    }
    let mut entropy = [0u8; 16];
    getrandom::fill(&mut entropy)?;
    let nonce = entropy
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut command = Command::new(helper);
    command
        .args(["--eml-process-pin", &number(session).to_string(), &nonce])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(group.map_or(0, number));
    // setpgid at spawn atomically refuses an existing group in another session.
    let mut child = command.spawn()?;
    let id = group.unwrap_or_else(|| Pid::from_child(&child));
    child
        .stdin
        .as_mut()
        .ok_or("Missing pin control writer")?
        .write_all(format!("{}:{}:{nonce}\n", number(session), number(id)).as_bytes())?;
    let output = child.stdout.take().ok_or("Missing pin startup pipe")?;
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(output).read_line(&mut line).map(|_| line);
        let _ = send.send(result);
    });
    let remaining = until
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(2));
    match receive.recv_timeout(remaining) {
        Ok(Ok(line)) if line == "ready\n" && Instant::now() < until => Ok(Pin { child, id }),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            Err("Group pin startup failed; no signal authorized for guessed group".into())
        }
    }
}
fn retained_pin_waitability(helper: &Path, session: Pid) -> Result<()> {
    use rustix::process::{WaitId, WaitIdOptions, kill_process, waitid};
    let until = Instant::now() + Duration::from_secs(2);
    let mut first = pin(helper, session, None, until)?;
    kill_process(Pid::from_child(&first.child), Signal::KILL)?;
    loop {
        match waitid(
            WaitId::Pid(Pid::from_child(&first.child)),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
        ) {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = first.child.wait();
                return Err("Exited group pins must remain waitable before reaping".into());
            }
        }
    }
    // A second owned member must join while the dead first pin is unreaped.
    let joined = pin(helper, session, Some(first.id), until);
    let mut second = match joined {
        Ok(pin) => pin,
        Err(error) => {
            let _ = first.child.wait();
            return Err(error);
        }
    };
    kill_process_group(first.id, Signal::KILL)?;
    let first_status = first.child.wait()?;
    let second_status = second.child.wait()?;
    if first_status.success() || second_status.success() {
        return Err("Waitability pin probe did not observe real termination".into());
    }
    Ok(())
}
#[derive(Clone)]
struct Row {
    pid: Pid,
    group: i32,
    state: String,
    command: String,
}
enum InspectionScope {
    Supervisor,
    Session,
}
fn inspect(
    program: &Path,
    session: Pid,
    scope: InspectionScope,
    until: Instant,
) -> Result<(Vec<Row>, bool)> {
    let until = until.min(Instant::now() + Duration::from_secs(2));
    if Instant::now() >= until {
        return Err("Inspection budget exhausted before spawn".into());
    }
    let mut command = Command::new(program);
    // The short kernel command name is diagnostic only, never signal authority.
    match scope {
        InspectionScope::Supervisor => {
            command.args([
                "-p",
                &number(session).to_string(),
                "-o",
                "pid=,pgid=,state=,ucomm=",
            ]);
        }
        InspectionScope::Session => {
            command.args(["-axo", "pid=,pgid=,state=,ucomm="]);
        }
    }
    let mut process = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let inspector = process.id() as i32;
    let stdout = process.stdout.take().ok_or("Missing inspector output")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let status = loop {
        let status = process.try_wait()?;
        if Instant::now() >= until {
            if status.is_none() {
                let _ = process.kill();
                let _ = process.wait();
            }
            return Err("Process inspection exceeded its remaining budget".into());
        }
        if let Some(status) = status {
            break status;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if !status.success() {
        return Err("Process inspection command failed".into());
    }
    let bytes = reader
        .join()
        .map_err(|_| "Process inspector reader failed")??;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Process inspection output exceeds its bound".into());
    }
    inspected_rows(&bytes, session, inspector)
}

fn admit_inspection(program: &Path, session: Pid) -> Result<()> {
    let began = Instant::now();
    if let Err(error) = inspect(
        program,
        session,
        InspectionScope::Supervisor,
        began + Duration::from_secs(2),
    ) {
        frame(
            &mut std::io::stdout().lock(),
            &Message::InspectionFailed {
                detail: error.to_string(),
                elapsed_ms: began.elapsed().as_millis().try_into()?,
            },
        )?;
        return Err(error);
    }
    Ok(())
}

struct Session {
    id: Pid,
    helper: PathBuf,
    inspector: PathBuf,
    pins: BTreeMap<i32, Pin>,
    worker: Option<Child>,
    raw_status: Option<i32>,
    cleaned: bool,
    root_group: Option<Pid>,
    cooperative_grace: Duration,
    cooperative_requested: bool,
}
impl Session {
    fn wait_worker(
        &mut self,
        stopped: &AtomicBool,
        started: Instant,
        deadline: Duration,
    ) -> Result<(bool, bool)> {
        Ok(loop {
            let status = self.worker.as_mut().ok_or("Worker absent")?.try_wait()?;
            let elapsed = started.elapsed();
            if let Some(status) = status {
                self.raw_status = Some(status.into_raw());
            }
            let overdue = elapsed >= deadline;
            let cancel = stopped.load(Ordering::Relaxed);
            if self.raw_status.is_some() || overdue || cancel {
                break (overdue, cancel);
            }
            std::thread::sleep(Duration::from_millis(10));
        })
    }
    fn cooperate(&mut self) -> Result<()> {
        if self.cooperative_requested
            || self.cooperative_grace.is_zero()
            || self.raw_status.is_some()
            || self.worker.is_none()
        {
            return Ok(());
        }
        self.cooperative_requested = true;
        let group = self
            .root_group
            .ok_or("Cooperative worker root group is absent")?;
        let pin = self
            .pins
            .get(&number(group))
            .ok_or("Cooperative worker root pin is absent")?;
        // As in hard cleanup, no pin has been polled/reaped before the last
        // group signal. An exited pin remains an unreaped PID/group reservation.
        kill_process_group(pin.id, Signal::TERM)?;
        let began = Instant::now();
        while began.elapsed() < self.cooperative_grace {
            if let Some(status) = self
                .worker
                .as_mut()
                .ok_or("Cooperative worker disappeared")?
                .try_wait()?
            {
                self.raw_status = Some(status.into_raw());
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if !self.cleaned && !self.pins.is_empty() {
            let _ = self.cleanup(false);
        }
    }
}
pub(super) fn session_entry() -> ExitCode {
    let result = (|| -> Result<bool> {
        let session = setsid()?;
        let helper = std::env::current_exe()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let signal = stopped.clone();
        ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
        waitable()?;
        retained_pin_waitability(&helper, session)?;
        frame(
            &mut std::io::stdout().lock(),
            &Message::Ready {
                session: number(session),
            },
        )?;
        let mut input = BufReader::new(std::io::stdin());
        let bytes = read_frame(&mut input)?;
        let request: Request = serde_json::from_slice(&bytes)?;
        if Duration::from_millis(request.cooperative_stop_ms) > COOPERATIVE_STOP_LIMIT {
            return Err("Private process cooperative stop exceeds its policy bound".into());
        }
        // Check row grammar and supervisor visibility before any worker exists.
        // Full-session enumeration and topology are checked during cleanup.
        let inspector = PathBuf::from(OsString::from_vec(request.inspector.clone()));
        admit_inspection(&inspector, session)?;
        let lost = stopped.clone();
        std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            let _ = input.read(&mut byte);
            lost.store(true, Ordering::Relaxed);
        });
        let mut state = Session {
            id: session,
            helper,
            inspector: PathBuf::from(OsString::from_vec(request.inspector)),
            pins: BTreeMap::new(),
            worker: None,
            raw_status: None,
            cleaned: false,
            root_group: None,
            cooperative_grace: Duration::from_millis(request.cooperative_stop_ms),
            cooperative_requested: false,
        };
        let root = pin(
            &state.helper,
            session,
            None,
            Instant::now() + Duration::from_secs(2),
        )?;
        let group = root.id;
        state.root_group = Some(group);
        state.pins.insert(number(group), root);
        if stopped.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let mut worker = worker_command(request.program, request.args, request.cwd, request.env);
        let log = PathBuf::from(OsString::from_vec(request.log));
        let (output, errors) = worker_outputs(&log, request.capture)?;
        worker
            .stdin(Stdio::null())
            .stdout(Stdio::from(output.try_clone()?))
            .stderr(Stdio::from(errors))
            .process_group(number(group));
        let started = Instant::now();
        let child = worker.spawn()?;
        state.worker = Some(child);
        let deadline = Duration::from_millis(request.deadline_ms);
        let (timed_out, cancelled) = state.wait_worker(&stopped, started, deadline)?;
        let execution = started.elapsed();
        let ordinary = state.raw_status.is_some() && !timed_out && !cancelled;
        let (complete, survivors, cleanup, detail, observations) = state.cleanup(ordinary);
        frame(
            &mut std::io::stdout().lock(),
            &Message::Finished {
                raw_status: state.raw_status,
                timed_out,
                cancelled,
                unexpected_survivors: survivors,
                survivor_observations: observations,
                cleanup_complete: complete,
                execution_ms: execution.as_millis().try_into()?,
                cleanup_ms: cleanup.as_millis().try_into()?,
                detail,
            },
        )?;
        Ok(complete)
    })();
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "Process supervisor: {error}");
            ExitCode::FAILURE
        }
    }
}

fn process_request(
    command: &Command,
    log: &Path,
    deadline: Duration,
    capture: bool,
    cooperative_grace: Duration,
    inspector: &Path,
) -> Result<Request> {
    let request = Request {
        program: command.get_program().as_bytes().to_vec(),
        args: command
            .get_args()
            .map(|value| value.as_bytes().to_vec())
            .collect(),
        cwd: command
            .get_current_dir()
            .map(|path| path.as_os_str().as_bytes().to_vec()),
        env: command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.as_bytes().to_vec(),
                    value.map(|value| value.as_bytes().to_vec()),
                )
            })
            .collect(),
        log: log.as_os_str().as_bytes().to_vec(),
        deadline_ms: deadline.as_millis().try_into()?,
        cooperative_stop_ms: cooperative_grace.as_millis().try_into()?,
        inspector: inspector.as_os_str().as_bytes().to_vec(),
        capture,
    };
    Ok(request)
}

impl ParentSession {
    fn send_request(&mut self, controller: &Controller, encoded: &[u8]) -> Result<()> {
        if encoded.len() > FRAME_LIMIT {
            drop(self.writer.take());
            let _ = self.child.wait();
            return Err("Private process request exceeds its bounded frame".into());
        }
        let stdin = self
            .writer
            .as_mut()
            .ok_or("Missing process control writer")?;
        let flags = rustix::fs::fcntl_getfl(&*stdin)?;
        rustix::fs::fcntl_setfl(&*stdin, flags | rustix::fs::OFlags::NONBLOCK)?;
        let mut sent = 0;
        let sending = Instant::now();
        while sent < encoded.len() {
            if controller.stopped.load(Ordering::Relaxed)
                || sending.elapsed() > Duration::from_secs(5)
            {
                drop(self.writer.take());
                let _ = self.child.wait();
                return Err(
                    "Process request delivery cancelled or overdue before worker launch".into(),
                );
            }
            match stdin.write(&encoded[sent..]) {
                Ok(0) => return Err("Process request control pipe closed".into()),
                Ok(count) => sent += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.activated = true;
        Ok(())
    }
}

impl ParentSession {
    fn wait_result(
        &mut self,
        controller: &Controller,
        receive: &mpsc::Receiver<std::result::Result<Message, String>>,
        started: Instant,
        deadline: Duration,
    ) -> Result<Option<Message>> {
        let mut result = None;
        loop {
            if controller.stopped.load(Ordering::Relaxed) {
                drop(self.writer.take());
            }
            match receive.recv_timeout(Duration::from_millis(20)) {
                Ok(Ok(message @ (Message::Finished { .. } | Message::InspectionFailed { .. }))) => {
                    result = Some(message);
                    break;
                }
                Ok(Ok(_)) => return Err("Duplicate supervisor readiness frame".into()),
                Ok(Err(error)) => {
                    drop(self.writer.take());
                    return Err(format!("Supervisor control failed: {error}").into());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
            if started.elapsed()
                > deadline + self.cooperative_grace + CLEANUP_GRACE + Duration::from_secs(10)
            {
                drop(self.writer.take());
                return Err("Supervisor result missing after bounded execution/cleanup grace; cleanup unconfirmed".into());
            }
        }
        Ok(result)
    }
}

fn qualify_completion(
    result: Option<Message>,
    status: ExitStatus,
    started: Instant,
    log: &Path,
) -> Result<ProcessResult> {
    let Some(report) = result else {
        return Err("Missing supervisor completion evidence".into());
    };
    let control = log.with_extension("control.json");
    fs::write(&control, serde_json::to_vec_pretty(&report)?)?;
    if let Message::InspectionFailed { detail, elapsed_ms } = &report {
        return Err(format!(
            "Supervisor inspection failed before worker launch after {elapsed_ms} ms: {detail}; evidence {}",
            control.display()
        ).into());
    }
    let Message::Finished {
        raw_status,
        timed_out,
        cancelled,
        unexpected_survivors,
        survivor_observations: _,
        cleanup_complete,
        execution_ms: _,
        cleanup_ms,
        detail,
    } = report
    else {
        return Err("Unexpected supervisor completion evidence".into());
    };
    if !status.success() || !cleanup_complete {
        return Err(format!(
            "Supervisor cleanup failed: {}; evidence {}",
            detail.unwrap_or_else(|| "unconfirmed".into()),
            control.display()
        )
        .into());
    }
    let raw = raw_status.ok_or("Worker exit unconfirmed")?;
    Ok(ProcessResult {
        status: ExitStatus::from_raw(raw),
        timed_out,
        cancelled,
        unexpected_survivors,
        cleanup_complete,
        elapsed: started.elapsed(),
        cleanup_elapsed: Duration::from_millis(cleanup_ms),
    })
}

fn inspected_rows(bytes: &[u8], session: Pid, inspector: i32) -> Result<(Vec<Row>, bool)> {
    let mut rows = Vec::new();
    let mut self_seen = false;
    let mut unstable = false;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let fields = line
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        if fields.len() < 4 {
            return Err("Malformed numeric process inspection".into());
        }
        let numeric: i32 = std::str::from_utf8(fields[0])?.parse()?;
        let Some(pid) = Pid::from_raw(numeric) else {
            return Err("Invalid inspected process ID".into());
        };
        let group = std::str::from_utf8(fields[1])?.parse()?;
        let state = std::str::from_utf8(fields[2])?.to_owned();
        // Diagnostic only: never use this executable name for ownership/signals.
        let command = String::from_utf8_lossy(&fields[3..].join(&b' '))
            .chars()
            .take(512)
            .collect();
        if pid == getpid() {
            self_seen = true;
        }
        if numeric == inspector {
            continue;
        }
        if group == 0 {
            continue;
        }
        match getsid(Some(pid)) {
            Ok(sid) if sid == session => {
                let current = match getpgid(Some(pid)) {
                    Ok(current) => number(current),
                    Err(_) => {
                        unstable = true;
                        continue;
                    }
                };
                if current != group {
                    unstable = true;
                    continue;
                }
                rows.push(Row {
                    pid,
                    group,
                    state,
                    command,
                });
            }
            Ok(_) => (),
            Err(error) if error == rustix::io::Errno::SRCH => {
                if !state.starts_with('Z') {
                    unstable = true;
                }
            }
            Err(_) => return Err("Kernel process ownership inspection failed".into()),
        }
    }
    if !self_seen {
        return Err("Process inspection omitted its required supervisor control".into());
    }
    Ok((rows, unstable))
}

mod cleanup;

fn worker_command(
    program: Vec<u8>,
    args: Vec<Vec<u8>>,
    cwd: Option<Vec<u8>>,
    env: Vec<(Vec<u8>, Option<Vec<u8>>)>,
) -> Command {
    let mut worker = Command::new(OsString::from_vec(program));
    worker.args(args.into_iter().map(OsString::from_vec));
    if let Some(cwd) = cwd {
        worker.current_dir(PathBuf::from(OsString::from_vec(cwd)));
    }
    for (key, value) in env {
        if let Some(value) = value {
            worker.env(OsString::from_vec(key), OsString::from_vec(value));
        } else {
            worker.env_remove(OsString::from_vec(key));
        }
    }
    worker
}

fn worker_outputs(log: &Path, capture: bool) -> Result<(fs::File, fs::File)> {
    let output = fs::File::options().write(true).create_new(true).open(log)?;
    let errors = if capture {
        fs::File::options()
            .write(true)
            .create_new(true)
            .open(log.with_extension("stderr"))?
    } else {
        output.try_clone()?
    };
    Ok((output, errors))
}
