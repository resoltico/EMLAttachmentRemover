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
const CLEANUP_GRACE: Duration = Duration::from_secs(10);
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
}
impl Drop for ParentSession {
    fn drop(&mut self) {
        drop(self.writer.take());
        if self.reaped {
            return;
        }
        let began = Instant::now();
        while began.elapsed() < CLEANUP_GRACE + Duration::from_secs(2) {
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
    };
    let (send, receive) = mpsc::channel();
    let reading = std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let result = read_frame(&mut reader)
                .and_then(|bytes| Ok(serde_json::from_slice::<Message>(&bytes)?));
            let finished = result.is_err() || matches!(result, Ok(Message::Finished { .. }));
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
        inspector: controller.inspector.as_os_str().as_bytes().to_vec(),
        capture,
    };
    let mut encoded = serde_json::to_vec(&request)?;
    encoded.push(b'\n');
    if encoded.len() > FRAME_LIMIT {
        drop(owned.writer.take());
        let _ = owned.child.wait();
        return Err("Private process request exceeds its bounded frame".into());
    }
    let stdin = owned
        .writer
        .as_mut()
        .ok_or("Missing process control writer")?;
    let flags = rustix::fs::fcntl_getfl(&*stdin)?;
    rustix::fs::fcntl_setfl(&*stdin, flags | rustix::fs::OFlags::NONBLOCK)?;
    let mut sent = 0;
    let sending = Instant::now();
    while sent < encoded.len() {
        if controller.stopped.load(Ordering::Relaxed) || sending.elapsed() > Duration::from_secs(5)
        {
            drop(owned.writer.take());
            let _ = owned.child.wait();
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
    owned.activated = true;
    let mut result = None;
    loop {
        if controller.stopped.load(Ordering::Relaxed) {
            drop(owned.writer.take());
        }
        match receive.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(message @ Message::Finished { .. })) => {
                result = Some(message);
                break;
            }
            Ok(Ok(_)) => return Err("Duplicate supervisor readiness frame".into()),
            Ok(Err(error)) => {
                drop(owned.writer.take());
                return Err(format!("Supervisor control failed: {error}").into());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
        if started.elapsed() > deadline + CLEANUP_GRACE + Duration::from_secs(10) {
            drop(owned.writer.take());
            return Err("Supervisor result missing after bounded execution/cleanup grace; cleanup unconfirmed".into());
        }
    }
    drop(owned.writer.take());
    let status = owned.child.wait()?;
    owned.reaped = true;
    let _ = reading.join();
    let Some(report @ Message::Finished { .. }) = result else {
        return Err("Missing supervisor completion evidence".into());
    };
    let control = log.with_extension("control.json");
    fs::write(&control, serde_json::to_vec_pretty(&report)?)?;
    let Message::Finished {
        raw_status,
        timed_out,
        cancelled,
        unexpected_survivors,
        cleanup_complete,
        execution_ms: _,
        cleanup_ms,
        detail,
    } = report
    else {
        unreachable!()
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
pub(crate) fn pin(helper: &Path, session: Pid, group: Option<Pid>) -> Result<Pin> {
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
    match receive.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(line)) if line == "ready\n" => Ok(Pin { child, id }),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            Err("Group pin startup failed; no signal authorized for guessed group".into())
        }
    }
}
#[derive(Clone)]
struct Row {
    pid: Pid,
    group: i32,
    state: String,
}
fn inspect(program: &Path, session: Pid) -> Result<(Vec<Row>, bool)> {
    let mut process = Command::new(program)
        .args(["-axo", "pid=,pgid=,state="])
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
    let began = Instant::now();
    let status = loop {
        if let Some(status) = process.try_wait()? {
            break status;
        }
        if began.elapsed() > Duration::from_secs(2) {
            process.kill()?;
            let _ = process.wait();
            return Err("Process inspection exceeded its bound".into());
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
    let output = std::str::from_utf8(&bytes)?;
    let mut rows = Vec::new();
    let mut self_seen = false;
    let mut unstable = false;
    for line in output.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err("Malformed numeric process inspection".into());
        }
        let numeric: i32 = fields[0].parse()?;
        let Some(pid) = Pid::from_raw(numeric) else {
            return Err("Invalid inspected process ID".into());
        };
        let group = fields[1].parse()?;
        let state = fields[2].to_owned();
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
                rows.push(Row { pid, group, state });
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
struct Session {
    id: Pid,
    helper: PathBuf,
    inspector: PathBuf,
    pins: BTreeMap<i32, Pin>,
    worker: Option<Child>,
    worker_pid: i32,
    raw_status: Option<i32>,
    cleaned: bool,
}
impl Session {
    fn cleanup(&mut self, ordinary_exit: bool) -> (bool, usize, Duration, Option<String>) {
        let began = Instant::now();
        let mut unexpected = BTreeSet::new();
        let mut detail = None;
        let mut stable = 0;
        // Freeze the known root first, including cargo-mutants' spawning/reaping threads.
        for pin in self.pins.values() {
            let _ = kill_process_group(pin.id, Signal::STOP);
        }
        while began.elapsed() < CLEANUP_GRACE / 2 {
            let scan = inspect(&self.inspector, self.id);
            let (rows, unstable) = match scan {
                Ok(scan) => scan,
                Err(error) => {
                    detail = Some(error.to_string());
                    break;
                }
            };
            let pins = self
                .pins
                .values()
                .map(|pin| pin.child.id() as i32)
                .collect::<BTreeSet<_>>();
            let mut added = false;
            let mut running = false;
            for row in rows {
                let pid = number(row.pid);
                if row.pid == self.id || pins.contains(&pid) {
                    continue;
                }
                if row.group == number(self.id) {
                    detail =
                        Some("Unexpected worker joined the supervisor's protected group".into());
                    continue;
                }
                if ordinary_exit && pid != self.worker_pid && !row.state.starts_with('Z') {
                    unexpected.insert(pid);
                }
                if !self.pins.contains_key(&row.group) {
                    let Some(group) = Pid::from_raw(row.group) else {
                        detail = Some("Invalid inspected group ID".into());
                        continue;
                    };
                    match pin(&self.helper, self.id, Some(group)) {
                        Ok(held) => {
                            self.pins.insert(row.group, held);
                            added = true;
                        }
                        Err(_) => {
                            running = true;
                            continue;
                        }
                    }
                }
                if let Some(held) = self.pins.get(&row.group) {
                    let _ = kill_process_group(held.id, Signal::STOP);
                }
                if !row.state.starts_with('T') && !row.state.starts_with('Z') {
                    running = true;
                }
            }
            if detail.is_some() {
                break;
            }
            if !added && !running && !unstable {
                stable += 1;
            } else {
                stable = 0;
            }
            if stable >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if stable < 2 && detail.is_none() {
            detail = Some("Session quiescence remained unconfirmed within cleanup grace".into());
        }
        // Every signal below is authorized by a retained, unreaped pin. No pin is
        // polled/reaped until its last group signal, even if already a zombie.
        for held in self.pins.values() {
            if let Err(error) = kill_process_group(held.id, Signal::KILL)
                && error != rustix::io::Errno::SRCH
            {
                detail = Some("Owned group termination failed".into());
            }
        }
        if self.raw_status.is_none()
            && let Some(worker) = &mut self.worker
        {
            while began.elapsed() < CLEANUP_GRACE {
                match worker.try_wait() {
                    Ok(Some(status)) => {
                        self.raw_status = Some(status.into_raw());
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    Err(error) => {
                        detail = Some(error.to_string());
                        break;
                    }
                }
            }
        }
        let mut clean_observations = 0;
        while began.elapsed() < CLEANUP_GRACE {
            match inspect(&self.inspector, self.id) {
                Ok((rows, unstable)) => {
                    if !unstable
                        && !rows
                            .iter()
                            .any(|row| row.pid != self.id && !row.state.starts_with('Z'))
                    {
                        clean_observations += 1;
                    } else {
                        clean_observations = 0;
                    }
                    if clean_observations >= 2 {
                        break;
                    }
                }
                Err(error) => {
                    detail = Some(error.to_string());
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if clean_observations < 2 {
            detail = Some("Post-termination live-member observation is unconfirmed".into());
        }
        // Pins can now be reaped; no later group signal will use their numbers.
        for held in self.pins.values_mut() {
            let mut reaped = false;
            while began.elapsed() < CLEANUP_GRACE {
                match held.child.try_wait() {
                    Ok(Some(_)) => {
                        reaped = true;
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    Err(_) => break,
                }
            }
            if !reaped {
                detail = Some("Owned pin could not be reaped within cleanup grace".into());
            }
        }
        self.pins.clear();
        if self.worker.is_some() && self.raw_status.is_none() {
            detail = Some("Worker could not be reaped within cleanup grace".into());
        }
        self.cleaned = detail.is_none();
        (self.cleaned, unexpected.len(), began.elapsed(), detail)
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
        frame(
            &mut std::io::stdout().lock(),
            &Message::Ready {
                session: number(session),
            },
        )?;
        let mut input = BufReader::new(std::io::stdin());
        let bytes = read_frame(&mut input)?;
        let request: Request = serde_json::from_slice(&bytes)?;
        // Refuse an unsupported/malformed inspector before starting any worker.
        let inspector = PathBuf::from(OsString::from_vec(request.inspector.clone()));
        let _ = inspect(&inspector, session)?;
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
            worker_pid: 0,
            raw_status: None,
            cleaned: false,
        };
        let root = pin(&state.helper, session, None)?;
        let group = root.id;
        state.pins.insert(number(group), root);
        if stopped.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let mut worker = Command::new(OsString::from_vec(request.program));
        worker.args(request.args.into_iter().map(OsString::from_vec));
        if let Some(cwd) = request.cwd {
            worker.current_dir(PathBuf::from(OsString::from_vec(cwd)));
        }
        for (key, value) in request.env {
            if let Some(value) = value {
                worker.env(OsString::from_vec(key), OsString::from_vec(value));
            } else {
                worker.env_remove(OsString::from_vec(key));
            }
        }
        let log = PathBuf::from(OsString::from_vec(request.log));
        let output = fs::File::options()
            .write(true)
            .create_new(true)
            .open(&log)?;
        let errors = if request.capture {
            fs::File::options()
                .write(true)
                .create_new(true)
                .open(log.with_extension("stderr"))?
        } else {
            output.try_clone()?
        };
        worker
            .stdin(Stdio::null())
            .stdout(Stdio::from(output.try_clone()?))
            .stderr(Stdio::from(errors))
            .process_group(number(group));
        let started = Instant::now();
        let child = worker.spawn()?;
        state.worker_pid = child.id() as i32;
        state.worker = Some(child);
        let deadline = Duration::from_millis(request.deadline_ms);
        let (timed_out, cancelled) = loop {
            let status = state.worker.as_mut().ok_or("Worker absent")?.try_wait()?;
            let elapsed = started.elapsed();
            if let Some(status) = status {
                state.raw_status = Some(status.into_raw());
            }
            let overdue = elapsed >= deadline;
            let cancel = stopped.load(Ordering::Relaxed);
            if state.raw_status.is_some() || overdue || cancel {
                break (overdue, cancel);
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let execution = started.elapsed();
        let ordinary = state.raw_status.is_some() && !timed_out && !cancelled;
        let (complete, survivors, cleanup, detail) = state.cleanup(ordinary);
        frame(
            &mut std::io::stdout().lock(),
            &Message::Finished {
                raw_status: state.raw_status,
                timed_out,
                cancelled,
                unexpected_survivors: survivors,
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
