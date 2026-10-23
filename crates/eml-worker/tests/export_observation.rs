//! Actual export worker boundaries with an independently authored complete byte oracle.
#![cfg(all(target_os = "macos", feature = "native-fixtures"))]
use eml_macos::{ChildExit, ChildState, StopSignal, spawn_with_control_pipe};
use eml_protocol::desktop::{
    BodyWriter, ExportReceipt, Header, Operation, OperationId, decode_receipt,
};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
#[path = "../../../tests/support/native_child.rs"]
mod native_child;
use native_child::{Settle, drain, terminal};
const BIN: &str = env!("CARGO_BIN_EXE_eml-worker");
// This literal is the entire admitted document, including its byte ordering and
// terminal/lifecycle facts. No production report writer supplies the oracle.
const DOCUMENT: &[u8] = br#"{"schema":"eml-technical-report","schema_version":1,"version":"7.0.0","build":36,"run_id":1,"terminal":"launch_failed","start":null,"items":[],"summary":null,"protocol_failure":null,"worker":{"operation_id":7,"exit_code":null,"signal":null,"input_may_have_been_sent":false,"settled":true,"reusable":false,"failure":"launch","faults":["Independent native export fixture"],"stderr":[],"stderr_truncated":false,"interrupt_calls_succeeded":0,"first_interrupt_observed":false}}"#;
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/export-worker-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
struct OperationChild {
    child: Settle,
    output: mpsc::Receiver<Vec<u8>>,
    errors: mpsc::Receiver<Vec<u8>>,
    control: mpsc::Receiver<Vec<u8>>,
    observation: Option<PathBuf>,
    pid: i32,
    operation_id: OperationId,
}
fn launch(destination: &Path, observation: Option<&Path>) -> OperationChild {
    eml_app::report_document::validate(DOCUMENT).unwrap();
    launch_operation(
        Operation::Export {
            destination: destination.to_owned(),
        },
        DOCUMENT,
        OperationId::new(91).unwrap(),
        observation,
    )
}
fn launch_operation(
    operation: Operation,
    bytes: &[u8],
    operation_id: OperationId,
    observation: Option<&Path>,
) -> OperationChild {
    let marker_directory = observation.map(|path| {
        if fs::read(path.join("observation")).is_ok_and(|bytes| bytes == b"dual-quit\n") {
            path.join(match operation {
                Operation::Processing => "processing",
                Operation::Export { .. } => "export",
            })
        } else {
            path.to_owned()
        }
    });
    let mut arguments = Vec::<OsString>::new();
    let executable = if let Some(path) = observation {
        let mut environment = OsString::from("EML_OPERATION_OBSERVATION=");
        environment.push(path);
        arguments.extend([environment, OsString::from(BIN)]);
        Path::new("/usr/bin/env")
    } else {
        Path::new(BIN)
    };
    arguments.push("--desktop-operation".into());
    let refs: Vec<_> = arguments.iter().map(OsString::as_os_str).collect();
    let result = spawn_with_control_pipe(executable, &refs).unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let owner = result.child.unwrap();
    let pid = owner.observation_pid().unwrap().unwrap();
    let child = OperationChild {
        child: Settle(owner),
        observation: marker_directory,
        operation_id,
        pid,
        output: drain(result.pipes.output),
        errors: drain(result.pipes.errors),
        control: drain(result.pipes.control.unwrap()),
    };
    let header = Header {
        operation_id,
        operation,
    };
    let mut writer = BodyWriter::new(result.pipes.input, &header).unwrap();
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap();
    child
}
fn observation(root: &Path, phase: &str) -> PathBuf {
    let directory = root.join("observation");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let directory = directory.canonicalize().unwrap();
    fs::write(directory.join("observation"), format!("export:{phase}\n")).unwrap();
    directory
}
fn wait_marker(child: &OperationChild, directory: &Path, name: &str) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(bytes) = fs::read(directory.join(name)) {
            return bytes;
        }
        assert_eq!(
            child.child.0.poll().unwrap(),
            ChildState::Running,
            "Worker exited before actual boundary {name}"
        );
        assert!(
            Instant::now() < deadline,
            "Actual boundary {name} was not reached"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn ready(child: &OperationChild, directory: &Path, phase: &str) {
    assert_eq!(
        wait_marker(child, directory, "ready"),
        format!("operation=export phase={phase} index=none operation_id=91\n").as_bytes()
    );
    assert_eq!(
        fs::read_to_string(directory.join("worker-pid"))
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap(),
        child.child.0.observation_pid().unwrap().unwrap()
    );
}
fn release(directory: &Path) {
    let pending = directory.join("owned-release");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .unwrap();
    file.write_all(b"release\n").unwrap();
    file.sync_all().unwrap();
    drop(file);
    fs::hard_link(&pending, directory.join("release")).unwrap();
    fs::remove_file(pending).unwrap();
}
fn finish_raw(child: OperationChild) -> (ChildExit, Vec<u8>, Vec<u8>) {
    let status = terminal(&child.child.0);
    let output = child.output.recv_timeout(Duration::from_secs(5)).unwrap();
    let errors = child.errors.recv_timeout(Duration::from_secs(5)).unwrap();
    let control = child.control.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut stream = eml_protocol::worker_control::WorkerControlStream::default();
    stream.append(&control).unwrap();
    stream.finish().unwrap();
    assert_eq!(control.first(), Some(&eml_protocol::worker_control::READY));
    if !errors.is_empty() {
        let markers: Vec<_> = child
            .observation
            .iter()
            .flat_map(|directory| {
                [
                    "observation",
                    "worker-pid",
                    "ready",
                    "first-interrupt",
                    "second-interrupt",
                    "release",
                ]
                .map(|name| (name, fs::read(directory.join(name)).ok()))
            })
            .collect();
        eprintln!(
            "Actual operation worker pid={} operation_id={} status={status:?} control={control:?} stdout={} stderr={} markers={markers:?}",
            child.pid,
            child.operation_id.get(),
            String::from_utf8_lossy(&output),
            String::from_utf8_lossy(&errors)
        );
    }
    (status, output, errors)
}
fn finish(child: OperationChild) -> (ChildExit, Vec<u8>) {
    let (status, output, errors) = finish_raw(child);
    assert!(
        errors.is_empty(),
        "Actual worker stderr: {}",
        String::from_utf8_lossy(&errors)
    );
    (status, output)
}

fn receipt(child: OperationChild, code: i32) -> ExportReceipt {
    let (status, bytes) = finish(child);
    assert_eq!(
        status,
        ChildExit {
            code: Some(code),
            signal: None
        }
    );
    let receipt = decode_receipt(&bytes, OperationId::new(91).unwrap()).unwrap();
    assert_eq!(receipt.operation_id.get(), 91);
    receipt
}
#[test]
fn exact_independent_report_bytes_and_all_existing_terminal_kinds_never_replace() {
    let root = root();
    let destination = root.path().join("report.json");
    let done = receipt(launch(&destination, None), 0);
    assert_eq!(done.publication, "published");
    assert_eq!(fs::read(&destination).unwrap(), DOCUMENT);
    let metadata = fs::metadata(&destination).unwrap();
    let identity = (metadata.dev(), metadata.ino());
    fs::create_dir(root.path().join("directory.json")).unwrap();
    std::os::unix::fs::symlink("report.json", root.path().join("link.json")).unwrap();
    std::os::unix::fs::symlink("missing", root.path().join("dangling.json")).unwrap();
    for name in [
        "report.json",
        "directory.json",
        "link.json",
        "dangling.json",
    ] {
        let done = receipt(launch(&root.path().join(name), None), 1);
        assert_eq!(done.publication, "not_published");
        assert!(done.failures.iter().any(|f| f.stage == "publication"));
    }
    let after = fs::metadata(&destination).unwrap();
    assert_eq!((after.dev(), after.ino()), identity);
    assert_eq!(fs::read(&destination).unwrap(), DOCUMENT);
    assert!(root.path().join("link.json").is_symlink());
    assert!(root.path().join("dangling.json").is_symlink());
    let missing = root.path().join("missing-parent/report.json");
    let failed = receipt(launch(&missing, None), 1);
    assert_eq!(failed.publication, "not_published");
    assert!(failed.failures.iter().any(|f| f.stage == "destination"));
    assert!(!missing.exists());
}
#[test]
fn racing_winner_and_pre_edge_stop_preserve_actual_no_replace_and_cleanup_facts() {
    for stopped in [false, true] {
        let root = root();
        let directory = observation(root.path(), "publication");
        let destination = root.path().join("report.json");
        let child = launch(&destination, Some(&directory));
        ready(&child, &directory, "publication");
        if stopped {
            assert!(child.child.0.signal(StopSignal::Interrupt).unwrap());
            wait_marker(&child, &directory, "first-interrupt");
        } else {
            let mut winner = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .unwrap();
            winner.write_all(b"independent racing winner").unwrap();
            winner.sync_all().unwrap();
        }
        release(&directory);
        let done = receipt(child, if stopped { 130 } else { 1 });
        assert_eq!(done.publication, "not_published");
        assert_eq!(done.cleanup.state, eml_protocol::records::Cleanup::Complete);
        if stopped {
            assert!(!destination.exists());
        } else {
            assert_eq!(
                fs::read(&destination).unwrap(),
                b"independent racing winner"
            );
        }
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".eml-")
        }));
    }
}
#[test]
fn actual_post_publication_readback_and_loss_never_authorize_filename_deletion() {
    for phase in ["readback", "after-publication"] {
        let root = root();
        let directory = observation(root.path(), phase);
        let destination = root.path().join("report.json");
        let child = launch(&destination, Some(&directory));
        ready(&child, &directory, phase);
        assert_eq!(fs::read(&destination).unwrap(), DOCUMENT);
        if phase == "readback" {
            release(&directory);
            let done = receipt(child, 0);
            assert_eq!(done.publication, "published");
            assert_eq!(done.final_file_verified.value(), Some(&true));
        } else {
            assert!(child.child.0.signal(StopSignal::Kill).unwrap());
            let (status, bytes) = finish(child);
            assert_eq!(
                status,
                ChildExit {
                    code: None,
                    signal: Some(9)
                }
            );
            assert!(bytes.is_empty());
            assert!(
                decode_receipt(&bytes, OperationId::new(91).unwrap()).is_err(),
                "Loss cannot produce a fabricated absence/publication receipt"
            );
        }
        assert_eq!(
            fs::read(&destination).unwrap(),
            DOCUMENT,
            "No filename-only rollback after actual publication/loss"
        );
    }
}

#[test]
fn preflight_stage_write_file_sync_and_namespace_sync_are_actual_owned_boundaries() {
    for phase in ["preflight", "stage-write", "file-sync", "namespace-sync"] {
        let root = root();
        let directory = observation(root.path(), phase);
        let destination = root.path().join("report.json");
        let child = launch(&destination, Some(&directory));
        ready(&child, &directory, phase);
        assert_eq!(destination.exists(), phase == "namespace-sync");
        if phase == "namespace-sync" {
            // Stop after publication cannot turn the completed effect into absence.
            assert_eq!(fs::read(&destination).unwrap(), DOCUMENT);
            assert!(child.child.0.signal(StopSignal::Interrupt).unwrap());
            wait_marker(&child, &directory, "first-interrupt");
        }
        release(&directory);
        let done = receipt(child, 0);
        assert!(done.qualified());
        assert_eq!(fs::read(&destination).unwrap(), DOCUMENT);
    }
    let root = root();
    let directory = observation(root.path(), "readback");
    let destination = root.path().join("occupied.json");
    fs::write(&destination, b"independent occupied bytes").unwrap();
    let done = receipt(launch(&destination, Some(&directory)), 1);
    assert_eq!(done.publication, "not_published");
    assert!(
        !directory.join("ready").exists(),
        "An occupied winner must never be read for export qualification"
    );
    assert_eq!(
        fs::read(&destination).unwrap(),
        b"independent occupied bytes"
    );
}

#[test]
fn selected_export_header_can_pause_its_reader_while_native_stop_settles_the_process() {
    let root = root();
    let directory = observation(root.path(), "request-body");
    let destination = root.path().join("report.json");
    let child = launch(&destination, Some(&directory));
    ready(&child, &directory, "request-body");
    assert!(!destination.exists());
    assert!(child.child.0.signal(StopSignal::Interrupt).unwrap());
    // The main control loop can settle while the request-reader actor remains
    // held. Process termination closes that reader; no release marker is sent.
    let (status, bytes) = finish(child);
    assert_eq!(
        status,
        ChildExit {
            code: Some(130),
            signal: None
        }
    );
    assert!(bytes.is_empty());
    assert!(!destination.exists());
    assert!(!directory.join("release").exists());
}

#[test]
fn actual_post_edge_parent_replacement_keeps_published_bytes_and_reports_address_loss() {
    let root = root();
    let directory = observation(root.path(), "after-publication");
    let parent = root.path().join("export-parent");
    let moved = root.path().join("moved-parent");
    fs::create_dir(&parent).unwrap();
    let destination = parent.join("report.json");
    let child = launch(&destination, Some(&directory));
    ready(&child, &directory, "after-publication");
    assert_eq!(fs::read(&destination).unwrap(), DOCUMENT);
    fs::rename(&parent, &moved).unwrap();
    fs::create_dir(&parent).unwrap();
    release(&directory);
    let done = receipt(child, 1);
    assert_eq!(done.publication, "published");
    assert_eq!(done.address_verified.value(), Some(&false));
    assert!(!done.qualified());
    assert!(!destination.exists());
    assert_eq!(fs::read(moved.join("report.json")).unwrap(), DOCUMENT);
    assert!(fs::read_dir(&parent).unwrap().next().is_none());
}

#[test]
fn incomplete_release_marker_is_refused_without_target_io_or_fabricated_receipt() {
    let root = root();
    let directory = observation(root.path(), "preflight");
    let destination = root.path().join("report.json");
    let child = launch(&destination, Some(&directory));
    ready(&child, &directory, "preflight");
    // Intentionally violate the complete-marker publication contract. The real
    // adapter must refuse; it must not wait for a producer to finish mutating it.
    fs::write(directory.join("release"), b"rel").unwrap();
    let (status, output, errors) = finish_raw(child);
    assert_eq!(
        status,
        ChildExit {
            code: Some(70),
            signal: None
        }
    );
    assert!(output.is_empty());
    assert!(String::from_utf8_lossy(&errors).contains("Malformed operation release marker"));
    assert!(!destination.exists());
}

#[test]
fn dual_quit_recipe_owns_two_actual_lanes_with_distinct_ids_until_both_settle() {
    let root = root();
    let control = root.path().join("control");
    fs::create_dir(&control).unwrap();
    fs::set_permissions(&control, fs::Permissions::from_mode(0o700)).unwrap();
    let control = control.canonicalize().unwrap();
    for (name, recipe) in [
        ("processing", b"processing:preflight:0\n".as_slice()),
        ("export", b"export:preflight\n".as_slice()),
    ] {
        let directory = control.join(name);
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(directory.join("observation"), recipe).unwrap();
    }
    fs::write(control.join("observation"), b"dual-quit\n").unwrap();
    let destination = root.path().join("report.json");
    let export = launch(&destination, Some(&control));
    ready(&export, &control.join("export"), "preflight");
    let source = root.path().join("message.eml");
    let original = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    fs::write(&source, original).unwrap();
    let request = eml_app::RequestSnapshot::new(vec![source.clone()], None).unwrap();
    let processing = launch_operation(
        Operation::Processing,
        request.encoded(),
        OperationId::new(92).unwrap(),
        Some(&control),
    );
    assert_eq!(
        wait_marker(&processing, &control.join("processing"), "ready"),
        b"operation=processing phase=preflight index=0 operation_id=92\n"
    );
    assert_ne!(export.pid, processing.pid);
    assert!(!control.join("worker-pid").exists());
    assert!(!destination.exists());
    assert!(export.child.0.signal(StopSignal::Interrupt).unwrap());
    assert!(processing.child.0.signal(StopSignal::Interrupt).unwrap());
    wait_marker(&export, &control.join("export"), "first-interrupt");
    wait_marker(&processing, &control.join("processing"), "first-interrupt");
    release(&control.join("export"));
    release(&control.join("processing"));
    let exported = receipt(export, 130);
    assert_eq!(exported.publication, "not_published");
    let (status, output) = finish(processing);
    assert_eq!(
        status,
        ChildExit {
            code: Some(130),
            signal: None
        }
    );
    let events: Vec<serde_json::Value> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(events.last().unwrap()["exit_code"], 130);
    assert_eq!(fs::read(&source).unwrap(), original);
    assert!(!destination.exists());
}
