//! Actual Windows verification-controller process/Job observations.
#![cfg(windows)]
use std::{fs, path::Path, process::Command, time::Duration};
use xtask::process::Controller;
#[path = "../../tests/support/owned_scratch.rs"]
mod scratch;
fn scratch() -> scratch::OwnedScratch {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/windows-controller-tests");
    fs::create_dir_all(&parent).unwrap();
    scratch::OwnedScratch::new(&parent).unwrap()
}
fn controller(root: &Path) -> Controller {
    Controller::new(root, Path::new(env!("CARGO_BIN_EXE_xtask"))).unwrap()
}
fn worker(root: &Path, mode: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_process-probe"));
    command.arg(mode).arg(root);
    command
}
#[test]
fn windows_controller_retains_actual_nonzero_exit_and_separate_streams() {
    let root = scratch();
    let controller = controller(root.path());
    let log = root.path().join("output.log");
    let result = controller
        .run_separate_output(
            worker(root.path(), "streams-exit"),
            &log,
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(result.status.code(), Some(23));
    assert!(result.cleanup_complete);
    assert!(!result.success());
    assert!(!result.timed_out && !result.cancelled);
    assert_eq!(result.unexpected_survivors, 0);
    assert_eq!(fs::read(&log).unwrap(), b"owned stdout\n");
    assert_eq!(
        fs::read(log.with_extension("stderr")).unwrap(),
        b"owned stderr\n"
    );
    root.close().unwrap();
}
#[test]
fn windows_controller_rejects_and_settles_live_descendants_after_primary_exit() {
    let root = scratch();
    let controller = controller(root.path());
    let result = controller
        .run(
            worker(root.path(), "background"),
            &root.path().join("output.log"),
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(result.status.success());
    assert!(result.unexpected_survivors > 0);
    assert!(result.cleanup_complete);
    assert!(!result.success());
    assert!(!controller.admits_work());
    std::thread::sleep(Duration::from_millis(1000));
    assert!(!root.path().join("invoked").exists());
    root.close().unwrap();
}
#[test]
fn windows_controller_deadline_terminates_its_actual_private_job_and_closes_admission() {
    let root = scratch();
    let controller = controller(root.path());
    let result = controller
        .run(
            worker(root.path(), "ignore-term"),
            &root.path().join("output.log"),
            Duration::from_secs(1),
        )
        .unwrap();
    assert!(
        root.path().join("worker-ready").exists(),
        "Fixture never installed its real handler"
    );
    assert!(result.timed_out && result.cleanup_complete);
    assert!(!result.cancelled);
    assert!(!result.success());
    assert!(!controller.admits_work());
    assert!(!root.path().join("invoked").exists());
    root.close().unwrap();
}

#[test]
fn windows_controller_evidence_collision_preserves_existing_bytes() {
    let root = scratch();
    let controller = controller(root.path());
    let log = root.path().join("output.log");
    fs::write(
        log.with_extension("control.json"),
        b"owned existing evidence",
    )
    .unwrap();
    let error = controller
        .run(
            worker(root.path(), "streams-exit"),
            &log,
            Duration::from_secs(5),
        )
        .err()
        .expect("Existing sidecar must refuse replacement");
    assert!(error.to_string().contains("evidence write failed"));
    assert_eq!(
        fs::read(log.with_extension("control.json")).unwrap(),
        b"owned existing evidence"
    );
    assert!(!controller.admits_work());
    root.close().unwrap();
}
