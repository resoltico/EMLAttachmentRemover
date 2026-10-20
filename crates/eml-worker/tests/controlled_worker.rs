//! Actual private GUI worker, independent control pipe and same-signal Force Stop.
#![cfg(target_os = "macos")]
use eml_macos::{ChildExit, ChildState, StopSignal, spawn_with_control_pipe};
use eml_protocol::worker_control::{FIRST_INTERRUPT, READY, WorkerControlStream};
use std::{
    ffi::OsStr,
    fs,
    io::{Read, Write},
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};
#[path = "../../../tests/support/native_child.rs"]
mod native_child;
#[path = "../../../tests/support/report_message.rs"]
mod report_message;
use native_child::{Settle, drain, terminal};
const BIN: &str = env!("CARGO_BIN_EXE_eml-worker");
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/controlled-worker-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
enum Control {
    Byte(u8),
    Eof,
}
fn controls(mut reader: std::io::PipeReader) -> mpsc::Receiver<Control> {
    let (send, receive) = mpsc::sync_channel(3);
    thread::spawn(move || {
        let mut stream = WorkerControlStream::default();
        let mut byte = [0];
        loop {
            let count = reader.read(&mut byte).unwrap();
            if count == 0 {
                stream.finish().unwrap();
                drop(send.send(Control::Eof));
                return;
            }
            stream.append(&byte).unwrap();
            if send.send(Control::Byte(byte[0])).is_err() {
                return;
            }
        }
    });
    receive
}
#[test]
fn readiness_precedes_request_and_stop_reaps_without_processing_held_open_input() {
    let result =
        spawn_with_control_pipe(Path::new(BIN), &[OsStr::new("--desktop-control")]).unwrap();
    let child = result.child.map(Settle);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let child = child.unwrap();
    let control = controls(result.pipes.control.unwrap());
    let output = drain(result.pipes.output);
    let errors = drain(result.pipes.errors);
    assert!(matches!(
        control.recv_timeout(Duration::from_secs(10)).unwrap(),
        Control::Byte(READY)
    ));
    assert_eq!(child.0.poll().unwrap(), ChildState::Running);
    assert!(child.0.signal(StopSignal::Interrupt).unwrap());
    assert_eq!(
        terminal(&child.0),
        ChildExit {
            code: Some(130),
            signal: None
        }
    );
    // Core may observe Stop and exit before its signal thread writes F. Actual
    // normal130 plus settled drains is valid; do not invent an acknowledgment.
    match control.recv_timeout(Duration::from_secs(2)).unwrap() {
        Control::Byte(FIRST_INTERRUPT) => assert!(matches!(
            control.recv_timeout(Duration::from_secs(2)).unwrap(),
            Control::Eof
        )),
        Control::Eof => {}
        _ => panic!("Unexpected control sequence"),
    }
    drop(result.pipes.input);
    assert!(
        output
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    assert!(
        errors
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    assert!(!child.0.signal(StopSignal::Kill).unwrap());
}
#[test]
fn acknowledged_first_int_then_same_second_int_forces_after_real_publication_and_partial_output() {
    let root = root();
    let (input, golden) = report_message::large_report_message();
    let source = root.path().join("message.eml");
    fs::write(&source, &input).unwrap();
    let request = eml_app::RequestSnapshot::new(vec![source.clone()], None).unwrap();
    let result =
        spawn_with_control_pipe(Path::new(BIN), &[OsStr::new("--desktop-control")]).unwrap();
    let child = result.child.map(Settle);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let child = child.unwrap();
    let control = controls(result.pipes.control.unwrap());
    let errors = drain(result.pipes.errors);
    assert!(matches!(
        control.recv_timeout(Duration::from_secs(10)).unwrap(),
        Control::Byte(READY)
    ));
    let mut writer = result.pipes.input;
    let request = request.encoded().to_vec();
    let (sent, complete) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = writer.write_all(&request);
        drop(writer);
        drop(sent.send(result));
    });
    let mut stdout = result.pipes.output;
    let (sent, prefix) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = vec![0; 65536];
        let result = stdout.read_exact(&mut bytes);
        drop(sent.send((stdout, bytes, result)));
    });
    let (stdout, mut prefix, read) = prefix.recv_timeout(Duration::from_secs(15)).unwrap();
    read.unwrap();
    complete
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&prefix).contains("\"removed\":["));
    assert!(
        fs::read(root.path().join("message.no-attachments.eml")).unwrap() == golden,
        "Published copy differs from independent retained-byte oracle"
    );
    assert!(fs::read(&source).unwrap() == input, "Original changed");
    assert_eq!(child.0.poll().unwrap(), ChildState::Running);
    assert!(child.0.signal(StopSignal::Interrupt).unwrap());
    assert!(matches!(
        control.recv_timeout(Duration::from_secs(2)).unwrap(),
        Control::Byte(FIRST_INTERRUPT)
    ));
    assert_eq!(child.0.poll().unwrap(), ChildState::Running);
    assert!(child.0.signal(StopSignal::Interrupt).unwrap());
    // Keep stdout held throughout terminal observation: normal processing cannot
    // finish its large event while this real pipe remains backpressured.
    assert_eq!(
        terminal(&child.0),
        ChildExit {
            code: Some(130),
            signal: None
        }
    );
    assert!(matches!(
        control.recv_timeout(Duration::from_secs(2)).unwrap(),
        Control::Eof
    ));
    prefix.extend(drain(stdout).recv_timeout(Duration::from_secs(2)).unwrap());
    assert!(!prefix.ends_with(b"\n"));
    assert!(!String::from_utf8_lossy(&prefix).contains("\"type\":\"summary\""));
    assert!(
        errors
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    assert!(!child.0.signal(StopSignal::Kill).unwrap());
    root.close().unwrap();
}
