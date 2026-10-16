//! Real Unix session/group lifetime probes. Windows bounded campaigns are unsupported.
#![cfg(unix)]
use std::{fs, path::Path, process::Command, time::Duration};
use xtask::process::Controller;
// Scenarios share the global process census and finite observation budget even
// with separate directories/sessions. Isolate outer scenarios, preserving all
// concurrency inside each real process-lifetime/negative-control scenario.
static PROCESS_SCENARIOS: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn scenario() -> std::sync::MutexGuard<'static, ()> {
    // A failed test remains failed; poisoning must not hide independent later
    // scenario evidence. Locals/resources drop before this first-declared guard.
    PROCESS_SCENARIOS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
struct ProbeRoot(Option<tempfile::TempDir>);
impl ProbeRoot {
    fn path(&self) -> &Path {
        self.0.as_ref().unwrap().path()
    }
}
impl Drop for ProbeRoot {
    fn drop(&mut self) {
        if std::thread::panicking()
            && let Some(root) = self.0.take()
        {
            let path = root.keep();
            let _ = writeln!(
                std::io::stderr(),
                "Retained owned failure evidence: {}",
                path.display()
            );
        }
    }
}
fn scratch() -> ProbeRoot {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/process-tests");
    fs::create_dir_all(&parent).unwrap();
    ProbeRoot(Some(tempfile::tempdir_in(parent).unwrap()))
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
fn normal_singleton_completes_with_real_status_and_cleanup() {
    let _scenario = scenario();
    let root = scratch();
    let mut command = Command::new("/usr/bin/true");
    command.current_dir(root.path());
    let result = controller(root.path())
        .run(
            command,
            &root.path().join("worker.log"),
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(result.success());
    assert!(result.cleanup_complete);
}
#[test]
fn ordinary_exit_does_not_leave_background_marker_writer() {
    let _scenario = scenario();
    let root = scratch();
    let result = controller(root.path())
        .run(
            worker(root.path(), "background"),
            &root.path().join("worker.log"),
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(result.status.success());
    assert!(result.unexpected_survivors > 0);
    assert!(!result.success());
    std::thread::sleep(Duration::from_secs(1));
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn deadline_kills_nested_group_that_ignores_term() {
    let _scenario = scenario();
    let root = scratch();
    let result = controller(root.path())
        .run(
            worker(root.path(), "nested-wait"),
            &root.path().join("worker.log"),
            Duration::from_millis(300),
        )
        .unwrap();
    assert!(result.timed_out);
    assert!(!result.success());
    assert!(result.cleanup_complete);
    worker_dead(root.path());
    std::thread::sleep(Duration::from_secs(1));
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn overdue_observed_success_cannot_qualify() {
    let _scenario = scenario();
    let root = scratch();
    let result = controller(root.path())
        .run(
            Command::new("/usr/bin/true"),
            &root.path().join("worker.log"),
            Duration::from_millis(1),
        )
        .unwrap();
    assert!(result.timed_out);
    assert!(!result.success());
}
fn worker_dead(root: &Path) {
    let path = root.join("worker-ready");
    assert!(
        path.is_file(),
        "Expected the nested worker to have actually started"
    );
    let pid: i32 = fs::read_to_string(path).unwrap().parse().unwrap();
    assert!(pid > 0);
    let began = std::time::Instant::now();
    loop {
        let output = Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "state="])
            .output()
            .unwrap();
        let state = String::from_utf8(output.stdout).unwrap();
        if state.trim().is_empty() || state.trim_start().starts_with('Z') {
            return;
        }
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "Owned nested worker {pid} still live: {state}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_file(path: &Path) {
    let began = std::time::Instant::now();
    while !path.is_file() {
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "{}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn parent_sigterm_uses_control_eof_and_leaves_no_marker_writer() {
    let _scenario = scenario();
    let root = scratch();
    let mut parent = Command::new(env!("CARGO_BIN_EXE_process-probe"))
        .arg("supervise")
        .arg(root.path())
        .arg(env!("CARGO_BIN_EXE_xtask"))
        .spawn()
        .unwrap();
    wait_file(&root.path().join("worker-ready"));
    rustix::process::kill_process(
        rustix::process::Pid::from_child(&parent),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    assert!(!parent.wait().unwrap().success());
    worker_dead(root.path());
    std::thread::sleep(Duration::from_secs(1));
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn parent_sigkill_closes_its_only_control_writer() {
    let _scenario = scenario();
    let root = scratch();
    let mut parent = Command::new(env!("CARGO_BIN_EXE_process-probe"))
        .arg("supervise")
        .arg(root.path())
        .arg(env!("CARGO_BIN_EXE_xtask"))
        .spawn()
        .unwrap();
    wait_file(&root.path().join("worker-ready"));
    rustix::process::kill_process(
        rustix::process::Pid::from_child(&parent),
        rustix::process::Signal::KILL,
    )
    .unwrap();
    assert!(!parent.wait().unwrap().success());
    worker_dead(root.path());
    std::thread::sleep(Duration::from_secs(1));
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn startup_failure_never_starts_the_requested_worker() {
    let _scenario = scenario();
    let root = scratch();
    let controller = Controller::new(root.path(), Path::new("/usr/bin/false")).unwrap();
    assert!(
        controller
            .run(
                worker(root.path(), "marker"),
                &root.path().join("worker.log"),
                Duration::from_secs(5)
            )
            .is_err()
    );
    assert!(!root.path().join("worker-group").exists());
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn setup_capture_does_not_mix_worker_stderr_into_json() {
    let _scenario = scenario();
    let root = scratch();
    let controller = controller(root.path());
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf '{\"ok\":true}'; printf diagnostic >&2"]);
    let bytes = controller.capture(&mut command).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["ok"],
        true
    );
}
#[test]
fn parent_sigint_cancels_and_reaps_its_nested_worker() {
    let _scenario = scenario();
    let root = scratch();
    let mut parent = Command::new(env!("CARGO_BIN_EXE_process-probe"))
        .arg("supervise")
        .arg(root.path())
        .arg(env!("CARGO_BIN_EXE_xtask"))
        .spawn()
        .unwrap();
    wait_file(&root.path().join("worker-ready"));
    rustix::process::kill_process(
        rustix::process::Pid::from_child(&parent),
        rustix::process::Signal::INT,
    )
    .unwrap();
    assert!(!parent.wait().unwrap().success());
    worker_dead(root.path());
    assert!(!root.path().join("invoked").exists());
}

// Include the exact controller source to exercise its crate-private kernel pin
// boundary with the real built helper, rather than an unrelated stand-in.
#[allow(dead_code)]
#[path = "../src/process/mod.rs"]
mod controller_source;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    process::{Child, ChildStdin, ChildStdout, Stdio},
};
fn raw_session(
    root: &Path,
    mode: &str,
    inspector: &Path,
    deadline_ms: u64,
) -> (Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("--eml-process-session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["session"],
        child.id()
    );
    let mut writer = child.stdin.take().unwrap();
    let request = serde_json::json!({
        "program":Path::new(env!("CARGO_BIN_EXE_process-probe")).as_os_str().as_bytes(),"args":[mode.as_bytes(),root.as_os_str().as_bytes()],
        "cwd":root.as_os_str().as_bytes(),"env":[],"log":root.join("raw.log").as_os_str().as_bytes(),"deadline_ms":deadline_ms,
        "inspector":inspector.as_os_str().as_bytes(),"capture":false});
    writer
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    writer.write_all(b"\n").unwrap();
    (child, writer, reader)
}
#[test]
fn failed_enumeration_cannot_launch_a_worker_or_qualify_cleanup() {
    let _scenario = scenario();
    let root = scratch();
    let script = root.path().join("inspect");
    fs::write(&script, b"#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let (mut helper, writer, _reader) = raw_session(root.path(), "marker", &script, 5000);
    drop(writer);
    assert!(!helper.wait().unwrap().success());
    assert!(!root.path().join("worker-group").exists());
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn a_group_in_another_owned_session_is_not_signal_authority() {
    let _scenario = scenario();
    let root = scratch();
    let (mut helper, writer, mut reader) =
        raw_session(root.path(), "marker", Path::new("/bin/ps"), 5000);
    wait_file(&root.path().join("worker-group"));
    let group: i32 = fs::read_to_string(root.path().join("worker-group"))
        .unwrap()
        .parse()
        .unwrap();
    let error = controller_source::unix::pin(
        Path::new(env!("CARGO_BIN_EXE_xtask")),
        rustix::process::getsid(None).unwrap(),
        rustix::process::Pid::from_raw(group),
        std::time::Instant::now() + Duration::from_secs(2),
    )
    .err()
    .expect("Cross-session join unexpectedly succeeded");
    assert_eq!(
        error
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::raw_os_error),
        Some(rustix::io::Errno::PERM.raw_os_error())
    );
    let mut result = String::new();
    reader.read_line(&mut result).unwrap();
    let recorded = fs::write(root.path().join("raw.control.json"), &result);
    drop(writer);
    let helper_status = helper.wait();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap()["cleanup_complete"],
        true,
        "{result}; recording={recorded:?}; helper={helper_status:?}"
    );
    assert!(
        helper_status.is_ok_and(|status| status.success()),
        "{result}"
    );
    assert!(
        recorded.is_ok(),
        "Could not retain received response: {result}; {recorded:?}"
    );
    assert!(root.path().join("invoked").exists());
}
#[test]
fn unreaped_dead_pin_still_anchors_group_through_last_signal() {
    let _scenario = scenario();
    let session = rustix::process::getsid(None).unwrap();
    let helper = Path::new(env!("CARGO_BIN_EXE_xtask"));
    let mut first = controller_source::unix::pin(
        helper,
        session,
        None,
        std::time::Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    rustix::process::kill_process(
        rustix::process::Pid::from_child(&first.child),
        rustix::process::Signal::KILL,
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(30));
    let mut second = controller_source::unix::pin(
        helper,
        session,
        Some(first.id),
        std::time::Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    rustix::process::kill_process_group(first.id, rustix::process::Signal::KILL).unwrap();
    assert!(!first.child.wait().unwrap().success());
    assert!(!second.child.wait().unwrap().success());
}
#[test]
fn pin_survives_term_then_eof_cleans_its_own_group() {
    let _scenario = scenario();
    let session = rustix::process::getsid(None).unwrap();
    let mut held = controller_source::unix::pin(
        Path::new(env!("CARGO_BIN_EXE_xtask")),
        session,
        None,
        std::time::Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    rustix::process::kill_process(
        rustix::process::Pid::from_child(&held.child),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(30));
    let output = Command::new("/bin/ps")
        .args(["-p", &held.child.id().to_string(), "-o", "state="])
        .output()
        .unwrap();
    let state = String::from_utf8(output.stdout).unwrap();
    assert!(!state.trim().is_empty() && !state.trim_start().starts_with('Z'));
    drop(held.child.stdin.take());
    assert!(!held.child.wait().unwrap().success());
}
#[test]
fn supervisor_death_during_frozen_nested_groups_uses_eof_hup_cont_cascade() {
    let _scenario = scenario();
    let root = scratch();
    let script = root.path().join("inspect");
    fs::write(&script,b"#!/bin/sh\nn=0\nif [ -f \"$0.count\" ]; then read -r n < \"$0.count\"; fi\nn=$((n+1))\nprintf '%s\\n' \"$n\" > \"$0.count\"\nif [ \"$n\" -ge 3 ]; then : > \"$0.frozen\"; sleep 0.3; fi\nexec /bin/ps \"$@\"\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let (mut helper, writer, _reader) = raw_session(root.path(), "nested-wait", &script, 300);
    wait_file(&root.path().join("worker-ready"));
    wait_file(&root.path().join("inspect.frozen"));
    rustix::process::kill_process(
        rustix::process::Pid::from_child(&helper),
        rustix::process::Signal::KILL,
    )
    .unwrap();
    drop(writer);
    assert!(!helper.wait().unwrap().success());
    worker_dead(root.path());
    std::thread::sleep(Duration::from_millis(400));
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn group_survives_original_leader_reaping_only_until_owned_cleanup() {
    let _scenario = scenario();
    let root = scratch();
    let result = controller(root.path())
        .run(
            worker(root.path(), "nested-leader-exit"),
            &root.path().join("worker.log"),
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(result.status.success());
    assert!(result.unexpected_survivors > 0);
    assert!(!result.success());
    std::thread::sleep(Duration::from_secs(1));
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn group_created_after_root_freeze_is_discovered_and_terminated() {
    let _scenario = scenario();
    let root = scratch();
    let script = root.path().join("inspect");
    fs::write(&script,b"#!/bin/sh\nn=0\nif [ -f \"$0.count\" ]; then read -r n < \"$0.count\"; fi\nn=$((n+1))\nprintf '%s\\n' \"$n\" > \"$0.count\"\nif [ \"$n\" -eq 2 ]; then : > \"$0.fork-now\"; sleep 0.1; fi\nexec /bin/ps \"$@\"\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let (mut helper, writer, mut reader) = raw_session(root.path(), "nested-race", &script, 300);
    wait_file(&root.path().join("first-ready"));
    let mut result = String::new();
    reader.read_line(&mut result).unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["timed_out"], true);
    assert_eq!(result["cleanup_complete"], true);
    drop(writer);
    assert!(helper.wait().unwrap().success());
    worker_dead(root.path());
    assert!(!root.path().join("invoked").exists());
}
#[test]
fn inherited_child_signal_policy_requires_waitable_result_or_refusal() {
    let _scenario = scenario();
    let root = scratch();
    let mut child = Command::new("/bin/sh")
        .args([
            "-c",
            "trap '' CHLD; exec \"$1\" --eml-process-session",
            "waitability-probe",
            env!("CARGO_BIN_EXE_xtask"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut writer = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    if line.is_empty() {
        drop(writer);
        assert!(!child.wait().unwrap().success());
        assert!(!root.path().join("worker-group").exists());
        return;
    }
    // Some platforms/launch paths normalize the inherited disposition or keep
    // children waitable. Readiness is acceptable only with real complete waits.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["event"],
        "ready"
    );
    let request = serde_json::json!({"program":b"/usr/bin/true","args":[],"cwd":root.path().as_os_str().as_bytes(),"env":[],"log":root.path().join("waitable.log").as_os_str().as_bytes(),"deadline_ms":5000,"inspector":b"/bin/ps","capture":false});
    writer
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    writer.write_all(b"\n").unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    let result: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(result["raw_status"], 0);
    assert_eq!(result["cleanup_complete"], true);
    drop(writer);
    assert!(child.wait().unwrap().success());
}
#[test]
fn pin_startup_uses_remaining_absolute_budget_not_a_fresh_timeout() {
    let _scenario = scenario();
    let session = rustix::process::getsid(None).unwrap();
    let result = controller_source::unix::pin(
        Path::new(env!("CARGO_BIN_EXE_process-probe")),
        session,
        None,
        std::time::Instant::now() + Duration::from_millis(20),
    );
    assert!(
        result.is_err(),
        "A 300ms delayed ACK exceeded its 20ms remaining budget"
    );
    let expired = controller_source::unix::pin(
        Path::new(env!("CARGO_BIN_EXE_xtask")),
        session,
        None,
        std::time::Instant::now() - Duration::from_millis(1),
    );
    assert!(expired.is_err());
}
