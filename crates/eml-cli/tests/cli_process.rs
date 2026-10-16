//! Actual owned executable/pipes, with finite deadlines and no process-name lookup.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
const BIN: &str = env!("CARGO_BIN_EXE_remove-eml-attachments");
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
fn root() -> tempfile::TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/cli-tests");
    fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|status| status.is_none()) {
            drop(self.0.kill());
            drop(self.0.wait());
        }
    }
}
fn wait(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            let status = child.wait().unwrap();
            panic!("Owned CLI exceeded deadline; killed with {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn drain(
    mut reader: impl Read + Send + 'static,
    observed: Arc<AtomicUsize>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0; 16 * 1024];
        loop {
            let n = reader.read(&mut buffer).unwrap();
            if n == 0 {
                return bytes;
            }
            observed.fetch_add(n, Ordering::Relaxed);
            if bytes.len() + n <= 256 * 1024 * 1024 {
                bytes.extend_from_slice(&buffer[..n]);
            } else {
                panic!("Owned CLI output exceeded retention bound");
            }
        }
    })
}
fn written_input(child: &mut Child, bytes: &[u8]) -> std::process::ChildStdin {
    let mut input = child.stdin.take().unwrap();
    let bytes = bytes.to_owned();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = input.write_all(&bytes);
        drop(sender.send((input, result)));
    });
    match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok((input, result)) => {
            result.unwrap();
            input
        }
        Err(error) => {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("Owned input write did not complete: {error}");
        }
    }
}
fn invoke(root: &Path, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut child = OwnedChild(
        Command::new(BIN)
            .current_dir(root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = drain(
        child.0.stdout.take().unwrap(),
        Arc::new(AtomicUsize::new(0)),
    );
    let stderr = drain(
        child.0.stderr.take().unwrap(),
        Arc::new(AtomicUsize::new(0)),
    );
    if let Some(input) = input {
        drop(written_input(&mut child.0, input));
    } else {
        drop(child.0.stdin.take());
    }
    let status = wait(&mut child.0);
    Output {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}
fn document(dry: bool) -> Vec<u8> {
    #[cfg(unix)]
    let (native, encoding) = (b"a.eml".to_vec(), "unix-bytes");
    #[cfg(windows)]
    let (native, encoding) = (
        "a.eml"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        "windows-utf16le",
    );
    serde_json::to_vec(&serde_json::json!({"protocol":1,"paths":[{"encoding":encoding,"base64":STANDARD.encode(native)}],"output_directory":null,"dry_run":dry})).unwrap()
}
fn events(bytes: &[u8]) -> Vec<serde_json::Value> {
    assert_eq!(bytes.last(), Some(&b'\n'));
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect()
}
#[test]
fn actual_binary_version_help_and_argument_conflicts_have_checked_channel_contracts() {
    let root = root();
    let version = invoke(root.path(), &["--version"], None);
    assert!(version.status.success());
    assert_eq!(
        version.stdout,
        format!("remove-eml-attachments {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
    );
    assert!(version.stderr.is_empty());
    let help = invoke(root.path(), &["--help"], None);
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("--request")
    );
    for args in [
        vec!["--request", "-", "a.eml"],
        vec!["--request", "-", "--dry-run"],
        vec!["--request", "-", "--output-dir", "out"],
        vec!["--format", "bogus"],
    ] {
        let output = invoke(root.path(), &args, None);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    root.close().unwrap();
}
#[test]
fn actual_binary_positional_create_and_rerun_match_independent_bytes_and_exit() {
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    for status in ["created", "up_to_date"] {
        let output = invoke(root.path(), &["--format", "json", "--", "a.eml"], None);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let values = events(&output.stdout);
        assert_eq!(values.len(), 3);
        assert_eq!(values[1]["status"], status);
        assert_eq!(values[2]["exit_code"], 0);
        assert_eq!(
            fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
            GOLDEN
        );
        assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), INPUT);
    }
    root.close().unwrap();
}
#[test]
fn actual_request_file_and_stdin_accept_both_output_formats_without_creating_dry_run_files() {
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    let bytes = document(true);
    fs::write(root.path().join("request.json"), &bytes).unwrap();
    for format in ["human", "json"] {
        let file = invoke(
            root.path(),
            &["--request", "request.json", "--format", format],
            None,
        );
        assert_eq!(file.status.code(), Some(0));
        let pipe = invoke(
            root.path(),
            &["--request", "-", "--format", format],
            Some(&bytes),
        );
        assert_eq!(pipe.status.code(), Some(0));
        assert_eq!(file.stdout, pipe.stdout);
        assert!(file.stderr.is_empty() && pipe.stderr.is_empty());
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    assert!(!root.path().join("a.no-attachments.eml").exists());
    root.close().unwrap();
}
#[test]
fn malformed_and_oversize_request_errors_are_pre_stream_and_do_not_echo_document() {
    let root = root();
    for bytes in [
        b"{\"secret-synthetic-field\":true}".to_vec(),
        vec![b' '; 8 * 1024 * 1024 + 1],
    ] {
        let output = invoke(
            root.path(),
            &["--request", "-", "--format", "json"],
            Some(&bytes),
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.len() < 2048);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret-synthetic-field"));
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    root.close().unwrap();
}
#[test]
fn missing_source_is_streamed_admission_failure_while_invalid_output_is_exit_two() {
    let root = root();
    let output = invoke(
        root.path(),
        &["--format", "json", "--", "missing.eml"],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    let values = events(&output.stdout);
    assert_eq!(values[2]["run_error"]["code"], "INPUT_ADMISSION_FAILED");
    assert_eq!(values[1]["status"], "failed");
    let output = invoke(
        root.path(),
        &["--format", "json", "--output-dir", "missing", "--", "a.eml"],
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    root.close().unwrap();
}
#[test]
fn request_pipe_requires_eof_before_any_stream_or_copy() {
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    let mut child = OwnedChild(
        Command::new(BIN)
            .current_dir(root.path())
            .args(["--request", "-", "--format", "json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let observed = Arc::new(AtomicUsize::new(0));
    let stdout = drain(child.0.stdout.take().unwrap(), observed.clone());
    let stderr = drain(
        child.0.stderr.take().unwrap(),
        Arc::new(AtomicUsize::new(0)),
    );
    let mut padded = document(false);
    padded.resize(8_388_608, b' ');
    let input = written_input(&mut child.0, &padded);
    thread::sleep(Duration::from_millis(150));
    assert!(child.0.try_wait().unwrap().is_none());
    assert_eq!(observed.load(Ordering::Relaxed), 0);
    assert!(!root.path().join("a.no-attachments.eml").exists());
    drop(input);
    assert_eq!(wait(&mut child.0).code(), Some(0));
    let values = events(&stdout.join().unwrap());
    assert_eq!(values[1]["status"], "created");
    assert!(stderr.join().unwrap().is_empty());
    assert_eq!(
        fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
        GOLDEN
    );
    root.close().unwrap();
}
#[test]
fn closed_stdout_before_request_eof_stops_before_business_work() {
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    let mut child = OwnedChild(
        Command::new(BIN)
            .current_dir(root.path())
            .args(["--request", "-", "--format", "json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stderr = drain(
        child.0.stderr.take().unwrap(),
        Arc::new(AtomicUsize::new(0)),
    );
    // Close the sole reader before admitting any request. This establishes the
    // delivery failure without racing the helper's first event write.
    drop(child.0.stdout.take());
    drop(written_input(&mut child.0, &document(false)));
    assert_eq!(wait(&mut child.0).code(), Some(1));
    assert!(stderr.join().unwrap().is_empty());
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    root.close().unwrap();
}
#[cfg(unix)]
fn signal(child: &Child, signal: rustix::process::Signal) {
    let pid = rustix::process::Pid::from_raw(i32::try_from(child.id()).unwrap()).unwrap();
    rustix::process::kill_process(pid, signal).unwrap();
}
#[cfg(unix)]
fn fifo_writer(path: &Path, child: &mut Child) -> fs::File {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match rustix::fs::open(
            path,
            rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        ) {
            Ok(fd) => return fs::File::from(fd),
            Err(rustix::io::Errno::NXIO) => {}
            Err(error) => panic!("Owned FIFO writer failed: {error}"),
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "CLI exited before its owned request reader opened"
        );
        assert!(Instant::now() < deadline, "Owned FIFO reader did not open");
        thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(unix)]
#[test]
fn first_stop_interrupts_unfinished_request_fifo_after_real_reader_handshake() {
    for delivered in [
        rustix::process::Signal::INT,
        rustix::process::Signal::TERM,
        rustix::process::Signal::HUP,
    ] {
        let root = root();
        fs::write(root.path().join("a.eml"), INPUT).unwrap();
        let fifo = root.path().join("request.fifo");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let mut child = OwnedChild(
            Command::new(BIN)
                .current_dir(root.path())
                .args(["--request", "request.fifo", "--format", "json"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let observed = Arc::new(AtomicUsize::new(0));
        let stdout = drain(child.0.stdout.take().unwrap(), observed.clone());
        let stderr = drain(
            child.0.stderr.take().unwrap(),
            Arc::new(AtomicUsize::new(0)),
        );
        // NONBLOCK write-open succeeds only after the actual CLI reader has
        // reached its FIFO open, which follows successful handler installation.
        let mut input = fifo_writer(&fifo, &mut child.0);
        input.write_all(&document(false)).unwrap();
        assert!(child.0.try_wait().unwrap().is_none());
        assert_eq!(observed.load(Ordering::Relaxed), 0);
        signal(&child.0, delivered);
        assert_eq!(wait(&mut child.0).code(), Some(130));
        drop(input);
        assert!(stdout.join().unwrap().is_empty());
        assert!(stderr.join().unwrap().is_empty());
        assert!(!root.path().join("a.no-attachments.eml").exists());
        assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), INPUT);
        root.close().unwrap();
    }
}
#[test]
fn positional_usage_requires_paths_but_empty_request_is_core_no_inputs() {
    let root = root();
    for args in [vec![], vec!["--dry-run"], vec!["--format", "json"]] {
        let output = invoke(root.path(), &args, None);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let empty = br#"{"protocol":1,"paths":[],"output_directory":null,"dry_run":false}"#;
    let output = invoke(
        root.path(),
        &["--request", "-", "--format", "json"],
        Some(empty),
    );
    assert_eq!(output.status.code(), Some(1));
    let values = events(&output.stdout);
    assert_eq!(values.len(), 2);
    assert_eq!(values[1]["run_error"]["code"], "NO_INPUTS");
    root.close().unwrap();
}
#[test]
fn invalid_argument_newlines_and_controls_cannot_forge_terminal_structure() {
    let root = root();
    let output = invoke(
        root.path(),
        &["--format", "bad\nforged\u{1b}", "--", "a.eml"],
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert!(!output.stderr.contains(&0x1b));
    assert!(String::from_utf8(output.stderr).unwrap().contains("\\n"));
    root.close().unwrap();
}
#[cfg(unix)]
#[test]
fn caller_argv_zero_does_not_replace_authored_help_program_name() {
    use std::os::unix::process::CommandExt;
    let root = root();
    let mut child = OwnedChild(
        Command::new(BIN)
            .arg0("fake\n\u{1b}name")
            .arg("--help")
            .current_dir(root.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = drain(
        child.0.stdout.take().unwrap(),
        Arc::new(AtomicUsize::new(0)),
    );
    let stderr = drain(
        child.0.stderr.take().unwrap(),
        Arc::new(AtomicUsize::new(0)),
    );
    assert_eq!(wait(&mut child.0).code(), Some(0));
    let bytes = stdout.join().unwrap();
    assert!(!bytes.contains(&0x1b));
    assert!(!String::from_utf8(bytes).unwrap().contains("fake"));
    assert!(stderr.join().unwrap().is_empty());
    root.close().unwrap();
}

#[cfg(unix)]
fn large_report_message() -> (Vec<u8>, Vec<u8>) {
    let prefix=b"MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=owned\r\n\r\n--owned\r\nContent-Type: text/plain\r\n\r\nbody\r\n--owned\r\nContent-Type: text/html\r\n\r\n<p>body</p>\r\n";
    let closing = b"--owned--\r\n";
    let mut input = prefix.to_vec();
    let name = "x".repeat(200);
    for _ in 0..5000 {
        input.extend_from_slice(format!("--owned\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"{name}\"\r\n\r\nx\r\n").as_bytes());
    }
    input.extend_from_slice(closing);
    let mut golden = prefix.to_vec();
    golden.extend_from_slice(closing);
    (input, golden)
}
#[cfg(unix)]
#[test]
fn distinct_termination_requests_force_owned_process_after_observed_partial_report() {
    let root = root();
    let (input, golden) = large_report_message();
    fs::write(root.path().join("a.eml"), &input).unwrap();
    let mut child = OwnedChild(
        Command::new(BIN)
            .current_dir(root.path())
            .args(["--format", "json", "--", "a.eml"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stderr = drain(
        child.0.stderr.take().unwrap(),
        Arc::new(AtomicUsize::new(0)),
    );
    let mut stdout = child.0.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut prefix = vec![0; 65_536];
        let read = stdout.read_exact(&mut prefix);
        drop(sender.send((stdout, prefix, read)));
    });
    let (mut stdout, mut partial, read) = match receiver.recv_timeout(Duration::from_secs(15)) {
        Ok(observation) => observation,
        Err(error) => {
            child.0.kill().unwrap();
            child.0.wait().unwrap();
            panic!("Owned CLI did not deliver the report prefix: {error}");
        }
    };
    read.unwrap();
    assert!(String::from_utf8_lossy(&partial).contains("\"removed\":["));
    let copy = root.path().join("a.no-attachments.eml");
    assert_eq!(fs::read(&copy).unwrap(), golden);
    assert!(child.0.try_wait().unwrap().is_none());
    // Both handlers are installed before the actual report is emitted. Distinct
    // standard signals cannot coalesce with each other; ctrlc queues a semaphore
    // post for each native delivery. Do not infer callback readiness from sleeps.
    signal(&child.0, rustix::process::Signal::INT);
    signal(&child.0, rustix::process::Signal::TERM);
    assert_eq!(wait(&mut child.0).code(), Some(130));
    stdout.read_to_end(&mut partial).unwrap();
    assert!(!partial.is_empty());
    assert!(!String::from_utf8_lossy(&partial).contains("\"type\":\"summary\""));
    assert!(stderr.join().unwrap().is_empty());
    assert_eq!(fs::read(&copy).unwrap(), golden);
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
    root.close().unwrap();
}
