//! Actual private worker calls the core without the CLI executable.
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
const BIN: &str = env!("CARGO_BIN_EXE_eml-worker");
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => return,
            Err(error) => unconfirmed(&format!("initial cleanup poll: {error}")),
            Ok(None) => {}
        }
        if let Err(error) = self.0.kill() {
            eprintln!("Owned worker cleanup kill failed: {error}; still polling terminal state");
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Err(error) => unconfirmed(&format!("cleanup poll: {error}")),
                Ok(None) => thread::sleep(Duration::from_millis(5)),
            }
        }
        unconfirmed("cleanup allowance exhausted");
    }
}
fn unconfirmed(reason: &str) -> ! {
    eprintln!(
        "Owned worker fixture cleanup unconfirmed ({reason}); earlier failure remains, abort later tests and retain owned resources"
    );
    std::process::exit(1);
}
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/worker-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
fn invoke(root: &Path, args: &[&str], input: &[u8]) -> Output {
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
    let mut stdout = child.0.stdout.take().unwrap();
    let mut stderr = child.0.stderr.take().unwrap();
    let (send, receive) = mpsc::sync_channel(2);
    let errors = send.clone();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .unwrap();
        drop(send.send((false, bytes)));
    });
    thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .by_ref()
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .unwrap();
        drop(errors.send((true, bytes)));
    });
    let mut stdin = child.0.stdin.take().unwrap();
    let bytes = input.to_vec();
    let (written, write_result) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = stdin.write_all(&bytes);
        drop(stdin);
        drop(written.send(result));
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "Owned worker exceeded finite execution allowance"
        );
        thread::sleep(Duration::from_millis(10));
    };
    write_result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let mut output = Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    for _ in 0..2 {
        let (error, bytes) = receive.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            bytes.len() <= 1_048_576,
            "Worker fixture exceeded bounded output"
        );
        if error {
            output.stderr = bytes;
        } else {
            output.stdout = bytes;
        }
    }
    output
}
#[test]
fn gui_request_calls_core_and_retains_byte_exact_create_and_existing_outcomes() {
    let root = root();
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let request = eml_app::RequestSnapshot::new(vec![source.clone()], None).unwrap();
    for expected in ["created", "up_to_date"] {
        let output = invoke(root.path(), &[], request.encoded());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(output.stdout.last(), Some(&b'\n'));
        let events: Vec<serde_json::Value> = output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["type"], "start");
        assert_eq!(events[1]["status"], expected);
        assert_eq!(events[1]["source_bytes"], 1028);
        assert_eq!(events[1]["copy_bytes"], 513);
        assert_eq!(events[2]["type"], "summary");
        assert_eq!(events[2]["exit_code"], 0);
        assert_eq!(fs::read(&source).unwrap(), INPUT);
        assert_eq!(
            fs::read(root.path().join("message.no-attachments.eml")).unwrap(),
            GOLDEN
        );
    }
}
#[test]
fn private_worker_identity_and_malformed_input_do_not_invoke_another_product() {
    let root = root();
    let version = invoke(root.path(), &["--version"], &[]);
    assert!(version.status.success());
    assert_eq!(
        version.stdout,
        format!("eml-worker {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
    );
    for (args, input) in [
        (&[][..], b"{}".as_slice()),
        (&["--format", "json"][..], b"".as_slice()),
    ] {
        let output = invoke(root.path(), args, input);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
