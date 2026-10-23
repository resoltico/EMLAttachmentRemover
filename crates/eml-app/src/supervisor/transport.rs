//! Owned child transport and endpoint settlement.
use super::*;

pub(super) enum OperationInput {
    Processing {
        operation_id: eml_protocol::desktop::OperationId,
        bytes: Vec<u8>,
    },
    Export {
        operation_id: eml_protocol::desktop::OperationId,
        destination: PathBuf,
        report: crate::TerminalReport,
    },
}
impl OperationInput {
    fn write(self, writer: impl Write) -> io::Result<()> {
        use eml_protocol::desktop::{BodyWriter, Header, Operation};
        match self {
            Self::Processing {
                operation_id,
                bytes,
            } => {
                let mut body = BodyWriter::new(
                    writer,
                    &Header {
                        operation_id,
                        operation: Operation::Processing,
                    },
                )?;
                body.write_all(&bytes)?;
                body.finish()?;
            }
            Self::Export {
                operation_id,
                destination,
                report,
            } => {
                let mut body = BodyWriter::new(
                    writer,
                    &Header {
                        operation_id,
                        operation: Operation::Export { destination },
                    },
                )?;
                report.write_document(&mut body)?;
                body.finish()?;
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct ReadResult {
    pub(super) total_bytes: usize,
    pub(super) eof: bool,
    pub(super) closed: bool,
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
    pub(super) tail: bool,
    pub(super) error: Option<String>,
}
#[derive(Default)]
pub(super) struct WriteResult {
    pub(super) report_too_large: bool,
    pub(super) broken_pipe: bool,
    pub(super) closed: bool,
    pub(super) error: Option<String>,
}
#[derive(Default)]
pub(super) struct ControlFacts {
    stream: WorkerControlStream,
    error: Option<String>,
}
pub(super) struct Observed {
    pub(super) ready: bool,
    pub(super) killed: bool,
    pub(super) transport_ok: bool,
    pub(super) exit: Option<ChildExit>,
    pub(super) stdout: ReadResult,
    pub(super) stderr: ReadResult,
    pub(super) control: ReadResult,
    pub(super) writer: WriteResult,
    pub(super) first: bool,
    pub(super) interrupts: u8,
    pub(super) faults: Vec<String>,
}
pub(super) fn issue(faults: &mut Vec<String>, message: impl Into<String>) {
    if faults.len() < 32 {
        faults.push(message.into().chars().take(1000).collect());
    }
}
pub(super) fn append_error(slot: &mut Option<String>, message: String) {
    *slot = Some(slot.take().map_or_else(
        || message.clone(),
        |earlier| format!("{earlier}; {message}"),
    ));
}
pub(super) fn finish_reader(reader: io::PipeReader, result: &mut ReadResult, role: &str) {
    match close_reader(reader) {
        Ok(()) => result.closed = true,
        Err(error) => append_error(&mut result.error, format!("{role} close: {error}")),
    }
}
pub(super) fn close_input(input: io::PipeWriter) -> WriteResult {
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
pub(super) fn capture(mut reader: &io::PipeReader, limit: usize) -> ReadResult {
    let mut result = ReadResult::default();
    let mut buffer = [0; 16384];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                result.eof = true;
                break;
            }
            Ok(count) => {
                let Some(total) = result.total_bytes.checked_add(count) else {
                    result.error = Some("Transport byte counter overflow".into());
                    break;
                };
                result.total_bytes = total;
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
pub(super) fn outcomes(
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
                let Some(total) = result.total_bytes.checked_add(count) else {
                    result.error = Some("Transport byte counter overflow".into());
                    break;
                };
                result.total_bytes = total;
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
pub(super) fn control_reader(
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
                let Some(total) = result.total_bytes.checked_add(count) else {
                    result.error = Some("Transport byte counter overflow".into());
                    break;
                };
                result.total_bytes = total;
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
pub(super) struct ReadRole {
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
pub(super) struct WriteRole {
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
    fn start(&mut self, bytes: OperationInput, token: WorkerControl) {
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
                        if let Err(error) = bytes.write(&mut writer) {
                            result.report_too_large = matches!(
                                error.get_ref().and_then(|e| {
                                    e.downcast_ref::<crate::report_document::ReportWriteError>()
                                }),
                                Some(crate::report_document::ReportWriteError::ReportTooLarge)
                            );
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
pub(super) struct OwnedTransportScope {
    child: OwnedChild,
    output: ReadRole,
    errors: ReadRole,
    control: ReadRole,
    input: WriteRole,
    request: Option<OperationInput>,
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
        } else if self.processing {
            self.output.start("export-receipt", |reader| {
                capture(reader, eml_protocol::desktop::MAX_RECEIPT_BYTES)
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
            self.poll_native_owner();
            self.poll_endpoints();
            let intent = self.observe_intent();
            let stopped = intent.is_some_and(|intent| intent.interrupts > 0 || intent.closing);
            let requested = intent.map_or(2, |intent| intent.interrupts);
            let (ready, first) = self.observe_control();
            let kill_due = self.check_allowances(intent, first);
            self.signal_child(ready, first, requested, kill_due);
            self.feed_request(ready, stopped, kill_due);
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
        self.finish_observations()
    }
    fn poll_native_owner(&mut self) {
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
    }
    fn poll_endpoints(&mut self) {
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
    }
    fn observe_intent(&mut self) -> Option<crate::control::WorkerIntent> {
        match self.token.intent(Instant::now()) {
            Ok(intent) => Some(intent),
            Err(error) => {
                self.fail(error.to_string());
                None
            }
        }
    }
    fn observe_control(&mut self) -> (bool, bool) {
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
        (ready, first)
    }
    fn check_allowances(
        &mut self,
        intent: Option<crate::control::WorkerIntent>,
        first: bool,
    ) -> bool {
        let stopped = intent
            .as_ref()
            .is_some_and(|intent| intent.interrupts > 0 || intent.closing);
        let fed = intent
            .as_ref()
            .is_none_or(|intent| intent.input_may_have_been_sent);
        let requested = intent.as_ref().map_or(2, |intent| intent.interrupts);
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
            self.fail(
                "First interrupt acknowledgment unavailable; second interrupt was not delivered"
                    .into(),
            );
        }
        if !fed
            && self.started.elapsed() >= Duration::from_secs(10)
            && self.exit.is_none()
            && !stopped
            && !self.aborting
        {
            self.fail("Worker readiness/feed allowance exhausted".into());
        }
        self.aborting
            || intent
                .as_ref()
                .is_none_or(|intent| intent.aborted || intent.kill_due)
            || (stopped && !fed)
    }
    fn signal_child(&mut self, ready: bool, first: bool, requested: u8, kill_due: bool) {
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
    }
    fn feed_request(&mut self, ready: bool, stopped: bool, kill_due: bool) {
        if self.processing && !self.input.started() {
            if kill_due || stopped || self.exit.is_some() || self.owner_lost {
                self.input.withhold();
            } else if ready && let Some(bytes) = self.request.take() {
                self.input.start(bytes, self.token.clone());
            }
        }
    }
    fn finish_observations(&mut self) -> Observed {
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
pub(super) fn observe(
    helper: &std::path::Path,
    request: Option<OperationInput>,
    token: &WorkerControl,
    report: Option<Arc<Mutex<RunReport>>>,
    health: Health,
) -> Result<Observed, String> {
    let processing = request.is_some();
    let observation = if processing {
        eml_macos::spawn_with_control_pipe(helper, &[OsStr::new("--desktop-operation")])
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
mod tests;
