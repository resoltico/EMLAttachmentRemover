//! Real native object/console lifetime checks; no mocked process or Job API.
#![cfg(all(windows, feature = "native-fixtures"))]
use eml_windows::{Job, Process, ProcessSpec};
#[path = "../../../tests/support/owned_scratch.rs"]
mod scratch;
use std::{
    ffi::OsString,
    fs,
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};
const FIXTURE: &str = env!("CARGO_BIN_EXE_job-observation");
fn scratch() -> scratch::OwnedScratch {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/windows-job-tests");
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
            began.elapsed() < Duration::from_secs(5),
            "Owned native child did not exit"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn empty(job: &Job) {
    let began = Instant::now();
    while job.active_processes().unwrap() != 0 {
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "Owned job members remain active"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
struct ControlChild(Child);
impl Drop for ControlChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[test]
fn native_job_observes_actual_exit_and_separate_streams() {
    let root = scratch();
    let job = Job::new().unwrap();
    let arguments = vec!["streams".into(), root.path().as_os_str().to_owned()];
    let environment = std::env::vars_os().collect::<Vec<_>>();
    let stdout = output(root.path(), "stdout");
    let stderr = output(root.path(), "stderr");
    let process = job
        .launch(
            &ProcessSpec {
                executable: Path::new(FIXTURE),
                arguments: &arguments,
                directory: root.path(),
                environment: &environment,
            },
            &stdout,
            &stderr,
        )
        .unwrap();
    assert_eq!(wait(&process), 23);
    empty(&job);
    drop(process);
    drop(job);
    drop(stdout);
    drop(stderr);
    assert_eq!(
        fs::read(root.path().join("stdout")).unwrap(),
        b"owned stdout\n"
    );
    assert_eq!(
        fs::read(root.path().join("stderr")).unwrap(),
        b"owned stderr\n"
    );
    root.close().unwrap();
}
#[test]
fn native_job_contains_a_descendant_after_primary_exit_and_terminates_only_owned_members() {
    let root = scratch();
    let job = Job::new().unwrap();
    let arguments = vec!["background".into(), root.path().as_os_str().to_owned()];
    let environment = std::env::vars_os().collect::<Vec<_>>();
    let stdout = output(root.path(), "stdout");
    let stderr = output(root.path(), "stderr");
    let process = job
        .launch(
            &ProcessSpec {
                executable: Path::new(FIXTURE),
                arguments: &arguments,
                directory: root.path(),
                environment: &environment,
            },
            &stdout,
            &stderr,
        )
        .unwrap();
    assert_eq!(wait(&process), 0);
    assert!(job.active_processes().unwrap() > 0);
    job.terminate(130).unwrap();
    empty(&job);
    drop(process);
    drop(job);
    drop(stdout);
    drop(stderr);
    std::thread::sleep(Duration::from_millis(1700));
    assert!(!root.path().join("invoked").exists());
    // A real out-of-job positive control must produce the marker; the oracle
    // would catch a containment/termination defect rather than merely executing.
    let mut control = ControlChild(
        Command::new(FIXTURE)
            .arg("delayed-write")
            .arg(root.path())
            .spawn()
            .unwrap(),
    );
    assert!(control.0.wait().unwrap().success());
    assert_eq!(
        fs::read(root.path().join("invoked")).unwrap(),
        b"owned native child actually ran"
    );
    drop(control);
    root.close().unwrap();
}
#[test]
fn native_job_kernel_closes_members_when_owner_exits_without_rust_drop() {
    let root = scratch();
    let mut owner = ControlChild(
        Command::new(FIXTURE)
            .arg("parent-exit")
            .arg(root.path())
            .spawn()
            .unwrap(),
    );
    assert!(owner.0.wait().unwrap().success());
    assert!(root.path().join("parent-ready").exists());
    std::thread::sleep(Duration::from_millis(1700));
    assert!(!root.path().join("invoked").exists());
    drop(owner);
    root.close().unwrap();
}
#[test]
fn native_process_arguments_preserve_quotes_backslashes_and_native_surrogates() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let root = scratch();
    let job = Job::new().unwrap();
    let values = vec![
        OsString::from(""),
        OsString::from("a b\"c\\"),
        OsString::from_wide(&[0xd800, 0x61, 0xdc00]),
    ];
    let mut arguments = vec!["echo".into(), root.path().as_os_str().to_owned()];
    arguments.extend(values.clone());
    let environment = std::env::vars_os().collect::<Vec<_>>();
    let stdout = output(root.path(), "stdout");
    let stderr = output(root.path(), "stderr");
    let process = job
        .launch(
            &ProcessSpec {
                executable: Path::new(FIXTURE),
                arguments: &arguments,
                directory: root.path(),
                environment: &environment,
            },
            &stdout,
            &stderr,
        )
        .unwrap();
    assert_eq!(wait(&process), 0);
    empty(&job);
    drop(process);
    drop(job);
    drop(stdout);
    drop(stderr);
    let expected = values
        .iter()
        .map(|value| format!("{:?}\n", value.encode_wide().collect::<Vec<_>>()))
        .collect::<String>();
    assert_eq!(
        fs::read_to_string(root.path().join("stdout")).unwrap(),
        expected
    );
    root.close().unwrap();
}

#[test]
fn native_job_refuses_nul_arguments_and_duplicate_environment_before_launch() {
    use std::os::windows::ffi::OsStringExt;
    let root = scratch();
    let job = Job::new().unwrap();
    let stdout = output(root.path(), "stdout");
    let stderr = output(root.path(), "stderr");
    let mut arguments = vec![
        "streams".into(),
        root.path().as_os_str().to_owned(),
        OsString::from_wide(&[0x61, 0]),
    ];
    let environment = std::env::vars_os().collect::<Vec<_>>();
    let error = job
        .launch(
            &ProcessSpec {
                executable: Path::new(FIXTURE),
                arguments: &arguments,
                directory: root.path(),
                environment: &environment,
            },
            &stdout,
            &stderr,
        )
        .err()
        .expect("NUL argument accepted");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(job.active_processes().unwrap(), 0);
    arguments.pop();
    let environment = vec![
        ("PATH".into(), "first".into()),
        ("Path".into(), "second".into()),
    ];
    let error = job
        .launch(
            &ProcessSpec {
                executable: Path::new(FIXTURE),
                arguments: &arguments,
                directory: root.path(),
                environment: &environment,
            },
            &stdout,
            &stderr,
        )
        .err()
        .expect("Duplicate environment accepted");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(job.active_processes().unwrap(), 0);
    drop(job);
    drop(stdout);
    drop(stderr);
    root.close().unwrap();
}
