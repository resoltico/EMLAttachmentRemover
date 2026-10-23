//! Actual CLI private-console interruption with owned process/pipe capabilities.
#![cfg(all(windows, feature = "native-fixtures"))]
use eml_windows::{
    ConsoleEvent, Job, Process, ProcessSpec, anonymous_pipe, anonymous_pipe_capacity,
};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};
const CLI: &str = env!("CARGO_BIN_EXE_remove-eml-attachments");
const SENDER: &str = env!("CARGO_BIN_EXE_console-observation");
#[path = "../../../tests/support/owned_scratch.rs"]
mod scratch;
fn root() -> scratch::OwnedScratch {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/windows-console-tests");
    fs::create_dir_all(&parent).unwrap();
    scratch::OwnedScratch::new(&parent).unwrap()
}
fn output(root: &Path, name: &str) -> fs::File {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .unwrap()
}
fn wait(process: &Process<'_>) -> u32 {
    let began = Instant::now();
    loop {
        if let Some(code) = process.exit_code().unwrap() {
            return code;
        }
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "Owned console process did not settle"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn empty(job: &Job) {
    let began = Instant::now();
    while job.active_processes().unwrap() != 0 {
        assert!(began.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }
}
fn send(process: &Process<'_>, event: ConsoleEvent, root: &Path, name: &str) {
    let stdout = output(root, &format!("{name}.log"));
    let stderr = output(root, &format!("{name}.stderr"));
    let sender = process
        .start_interrupt(Path::new(SENDER), event, &stdout, &stderr)
        .unwrap();
    let code = wait(&sender);
    assert_eq!(
        code,
        0,
        "Owned sender exit {code:#010x}; stderr: {:?}",
        fs::read(root.join(format!("{name}.stderr"))).unwrap()
    );
}
#[test]
fn windows_first_console_interrupt_cancels_unfinished_request_after_actual_reader_handshake() {
    let root = root();
    let original = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    fs::write(root.path().join("a.eml"), original).unwrap();
    let job = Job::new().unwrap();
    let (read, mut write) = anonymous_pipe().unwrap();
    let capacity = anonymous_pipe_capacity(&read).unwrap() as usize;
    let stdout = output(root.path(), "stdout");
    let stderr = output(root.path(), "stderr");
    let environment = std::env::vars_os().collect::<Vec<_>>();
    let arguments = vec![
        "--request".into(),
        "-".into(),
        "--format".into(),
        "json".into(),
    ];
    let process = job
        .launch_with_stdin(
            &ProcessSpec {
                executable: Path::new(CLI),
                arguments: &arguments,
                directory: root.path(),
                environment: &environment,
            },
            &read,
            &stdout,
            &stderr,
        )
        .unwrap();
    drop(read);
    // A completed write larger than the observed finite buffer proves actual
    // CLI consumption. Keep EOF absent. Do not PeekNamedPipe on the synchronous
    // read object shared with the child's potentially blocked ReadFile.
    let mut unfinished = b"{\"protocol\":1,".to_vec();
    unfinished.resize(capacity + unfinished.len() + 1, b' ');
    assert!(unfinished.len() < eml_protocol::MAX_REQUEST_BYTES);
    let (sent, received) = std::sync::mpsc::sync_channel(1);
    let writer = thread::spawn(move || {
        let result = write.write_all(&unfinished);
        let _ = sent.send((write, result));
    });
    let (write, written) = match received.recv_timeout(Duration::from_secs(10)) {
        Ok(value) => value,
        Err(error) => {
            job.terminate(125).unwrap();
            empty(&job);
            writer.join().unwrap();
            panic!("CLI did not consume the actual oversized pipe write: {error}");
        }
    };
    writer.join().unwrap();
    written.unwrap();
    assert!(process.exit_code().unwrap().is_none());
    send(&process, ConsoleEvent::Interrupt, root.path(), "interrupt");
    assert_eq!(wait(&process), 130);
    empty(&job);
    drop(process);
    drop(job);
    drop(write);
    drop(stdout);
    drop(stderr);
    assert!(fs::read(root.path().join("stdout")).unwrap().is_empty());
    assert!(fs::read(root.path().join("stderr")).unwrap().is_empty());
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), original);
    assert!(
        !root
            .path()
            .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
            .exists()
    );
    root.close().unwrap();
}

#[path = "../../../tests/support/report_message.rs"]
mod report_message;
#[test]
fn windows_distinct_console_events_force_cli_after_real_partial_report() {
    let root = root();
    let (input, golden) = report_message::large_report_message();
    let source = root.path().join("a.eml");
    fs::write(&source, &input).unwrap();
    let job = Job::new().unwrap();
    let (mut read, write) = anonymous_pipe().unwrap();
    let stderr = output(root.path(), "stderr");
    let environment = std::env::vars_os().collect::<Vec<_>>();
    let arguments = vec![
        "--format".into(),
        "json".into(),
        "--".into(),
        source.as_os_str().to_owned(),
    ];
    let process = job
        .launch(
            &ProcessSpec {
                executable: Path::new(CLI),
                arguments: &arguments,
                directory: root.path(),
                environment: &environment,
            },
            &write,
            &stderr,
        )
        .unwrap();
    drop(write);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut prefix = vec![0; 65536];
        let observed = read.read_exact(&mut prefix);
        let _ = sender.send((read, prefix, observed));
    });
    let (mut read, mut prefix, observed) = match receiver.recv_timeout(Duration::from_secs(15)) {
        Ok(value) => value,
        Err(error) => {
            job.terminate(125).unwrap();
            empty(&job);
            panic!("CLI did not reach actual report backpressure: {error}");
        }
    };
    reader.join().unwrap();
    observed.unwrap();
    assert!(String::from_utf8_lossy(&prefix).contains("\"removed\":["));
    assert!(process.exit_code().unwrap().is_none());
    assert_eq!(
        fs::read(
            root.path()
                .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
        )
        .unwrap(),
        golden
    );
    send(&process, ConsoleEvent::Interrupt, root.path(), "first");
    send(&process, ConsoleEvent::Break, root.path(), "second");
    assert_eq!(wait(&process), 130);
    empty(&job);
    read.read_to_end(&mut prefix).unwrap();
    assert!(!String::from_utf8_lossy(&prefix).contains("\"type\":\"summary\""));
    drop(process);
    drop(job);
    drop(stderr);
    drop(read);
    assert!(fs::read(root.path().join("stderr")).unwrap().is_empty());
    assert_eq!(fs::read(&source).unwrap(), input);
    assert_eq!(
        fs::read(
            root.path()
                .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
        )
        .unwrap(),
        golden
    );
    root.close().unwrap();
}
