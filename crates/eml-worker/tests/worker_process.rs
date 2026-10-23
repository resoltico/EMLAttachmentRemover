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
    let mut command = Command::new(BIN);
    command.current_dir(root).args(args);
    invoke_command(command, input)
}
fn invoke_command(mut command: Command, input: &[u8]) -> Output {
    let framed;
    let input = if command.get_args().next().is_none() {
        command.arg("--desktop-operation");
        let header = eml_protocol::desktop::Header {
            operation_id: eml_protocol::desktop::OperationId::new(1).unwrap(),
            operation: eml_protocol::desktop::Operation::Processing,
        };
        let mut bytes = Vec::new();
        let mut writer = eml_protocol::desktop::BodyWriter::new(&mut bytes, &header).unwrap();
        writer.write_all(input).unwrap();
        writer.finish().unwrap();
        framed = bytes;
        &framed[..]
    } else {
        input
    };
    let mut child = OwnedChild(
        command
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
            fs::read(
                root.path()
                    .join("message.eudtfo5quf3d2hlohgknt7qpaa.no-attachments.eml")
            )
            .unwrap(),
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
        eml_protocol::desktop::identity_line(
            env!("CARGO_PKG_VERSION"),
            &eml_core::product::BUILD_NUMBER.to_string()
        )
        .as_bytes()
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

#[cfg(all(target_os = "macos", feature = "native-fixtures"))]
#[test]
fn fixture_configuration_is_ignored_by_identity_probe_and_invalid_processing_fails() {
    let root = root();
    let missing = root.path().join("missing-observation");
    for version in [true, false] {
        let mut command = Command::new(BIN);
        command
            .current_dir(root.path())
            .env("EML_OPERATION_OBSERVATION", &missing);
        if version {
            command.arg("--version");
        }
        let output = invoke_command(command, &[]);
        assert_eq!(output.status.code(), Some(if version { 0 } else { 70 }));
        if version {
            assert_eq!(
                output.stdout,
                eml_protocol::desktop::identity_line(
                    env!("CARGO_PKG_VERSION"),
                    &eml_core::product::BUILD_NUMBER.to_string()
                )
                .as_bytes()
            );
            assert!(output.stderr.is_empty());
        } else {
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("operation observation refused")
            );
        }
    }
    assert!(!missing.exists());
}

#[cfg(all(target_os = "macos", not(feature = "native-fixtures")))]
#[test]
fn shipping_worker_ignores_operation_observation_configuration() {
    let root = root();
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let request = eml_app::RequestSnapshot::new(vec![source.clone()], None).unwrap();
    let missing = root.path().join("missing-observation");
    for version in [true, false] {
        let mut command = Command::new(BIN);
        command
            .current_dir(root.path())
            .env("EML_OPERATION_OBSERVATION", &missing);
        if version {
            command.arg("--version");
        }
        let output = invoke_command(command, if version { &[] } else { request.encoded() });
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
    }
    assert_eq!(
        fs::read(
            root.path()
                .join("message.eudtfo5quf3d2hlohgknt7qpaa.no-attachments.eml")
        )
        .unwrap(),
        GOLDEN
    );
    assert_eq!(fs::read(source).unwrap(), INPUT);
    assert!(!missing.exists());
}

#[cfg(all(target_os = "macos", feature = "native-fixtures"))]
#[test]
fn observed_worker_processes_real_message_and_reports_real_post_publication_address_change() {
    use std::os::unix::fs::PermissionsExt;
    let root = root();
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let destination = root.path().join("output");
    let moved = root.path().join("moved-output");
    fs::create_dir(&destination).unwrap();
    let observation = root.path().join("control");
    fs::create_dir(&observation).unwrap();
    fs::set_permissions(&observation, fs::Permissions::from_mode(0o700)).unwrap();
    let observation = observation.canonicalize().unwrap();
    fs::write(
        observation.join("observation"),
        b"processing:after-publication:0\n",
    )
    .unwrap();
    let request =
        eml_app::RequestSnapshot::new(vec![source.clone()], Some(destination.clone())).unwrap();
    let mut command = Command::new(BIN);
    command
        .current_dir(root.path())
        .env("EML_OPERATION_OBSERVATION", &observation);
    let output = thread::scope(|scope| {
        let actor = scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while fs::read(observation.join("ready")).ok().as_deref()
                != Some(b"operation=processing phase=after-publication index=0 operation_id=1\n")
            {
                assert!(
                    Instant::now() < deadline,
                    "Real publication boundary was not reached"
                );
                thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(
                fs::read(destination.join("message.eudtfo5quf3d2hlohgknt7qpaa.no-attachments.eml"))
                    .unwrap(),
                GOLDEN
            );
            fs::rename(&destination, &moved).unwrap();
            fs::create_dir(&destination).unwrap();
            let release = observation.join("owned-release");
            fs::write(&release, b"release\n").unwrap();
            fs::rename(release, observation.join("release")).unwrap();
        });
        let output = invoke_command(command, request.encoded());
        actor.join().unwrap();
        output
    });
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events: Vec<serde_json::Value> = output
        .stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["type"], "start");
    assert_eq!(events[1]["status"], "failed");
    assert_eq!(events[1]["publication"]["state"], "published");
    assert_eq!(events[1]["publication"]["address"], "changed");
    assert_eq!(events[1]["error"]["code"], "ADDRESS_CHANGED");
    assert_eq!(events[2]["type"], "summary");
    assert_eq!(events[2]["counts"]["failed"], 1);
    assert_eq!(events[2]["exit_code"], 1);
    assert_eq!(fs::read(source).unwrap(), INPUT);
    assert_eq!(
        fs::read(moved.join("message.eudtfo5quf3d2hlohgknt7qpaa.no-attachments.eml")).unwrap(),
        GOLDEN
    );
    assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
}

#[test]
fn removed_private_routes_and_incomplete_desktop_frames_never_touch_targets() {
    use eml_protocol::desktop::{BodyWriter, Header, Operation, OperationId};
    let root = root();
    let source = root.path().join("untouched.eml");
    fs::write(&source, INPUT).unwrap();
    let request = eml_app::RequestSnapshot::new(vec![source.clone()], None).unwrap();
    let mut framed = Vec::new();
    let mut w = BodyWriter::new(
        &mut framed,
        &Header {
            operation_id: OperationId::new(42).unwrap(),
            operation: Operation::Processing,
        },
    )
    .unwrap();
    w.write_all(request.encoded()).unwrap();
    w.finish().unwrap();
    for prefix in [
        0,
        1,
        eml_protocol::desktop::MAGIC.len(),
        framed.len() - 4,
        framed.len() - 1,
    ] {
        let output = invoke(root.path(), &["--desktop-operation"], &framed[..prefix]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    let mut extra = framed.clone();
    extra.push(b'x');
    let output = invoke(root.path(), &["--desktop-operation"], &extra);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    for args in [vec!["--desktop-control"], vec!["--format", "json"]] {
        let output = invoke(root.path(), &args, request.encoded());
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    let output = Command::new(BIN).current_dir(root.path()).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
