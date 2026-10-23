//! Real Unix session/group lifetime probes. Windows bounded campaigns are unsupported.
#![cfg(unix)]
use std::{fs, path::Path, process::Command, time::Duration};
use xtask::{archive, process::Controller};
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
    let controller = controller(root.path());
    let result = controller
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
    assert!(!controller.admits_work());
    assert!(
        controller
            .run(
                worker(root.path(), "marker"),
                &root.path().join("later.log"),
                Duration::from_secs(5)
            )
            .is_err()
    );
    assert!(
        controller
            .capture(&mut worker(root.path(), "marker"))
            .is_err()
    );
    assert!(!root.path().join("later.log").exists());
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

#[test]
fn cooperative_deadline_cleans_external_resource_but_cannot_qualify_exit_zero() {
    let _scenario = scenario();
    let root = scratch();
    let controller = controller(root.path());
    let result = controller
        .run_with_cooperative_stop(
            worker(root.path(), "cooperative-resource"),
            &root.path().join("worker.log"),
            Duration::from_secs(3),
            Duration::from_secs(1),
        )
        .unwrap();
    assert!(result.timed_out && !result.cancelled && result.status.success());
    assert!(result.cleanup_complete && !result.success());
    assert!(root.path().join("cooperative-stop-observed").is_file());
    assert!(!root.path().join("external-resource").exists());
    assert!(!controller.admits_work());
    assert!(controller.ownership_settled());
    controller.finish_helper().unwrap();
    assert!(!controller.admits_work());
}

#[test]
fn cooperative_cancellation_closes_admission_even_after_clean_exit_zero() {
    let _scenario = scenario();
    let root = scratch();
    let controller = controller(root.path());
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            wait_file(&root.path().join("worker-ready"));
            controller.request_stop();
        });
        controller
            .run_with_cooperative_stop(
                worker(root.path(), "cooperative-resource"),
                &root.path().join("worker.log"),
                Duration::from_secs(10),
                Duration::from_secs(1),
            )
            .unwrap()
    });
    assert!(
        result.cancelled && result.status.success() && result.cleanup_complete && !result.success()
    );
    assert!(!controller.admits_work());
    assert!(controller.ownership_settled());
    controller.finish_helper().unwrap();
    assert!(!controller.admits_work());
    assert!(root.path().join("cooperative-stop-observed").is_file());
    assert!(!root.path().join("external-resource").exists());
}

#[test]
fn cooperative_control_eof_allows_cleanup_before_pinned_group_termination() {
    let _scenario = scenario();
    let root = scratch();
    let (mut child, writer, mut reader) = raw_session_with_cooperative_stop(
        root.path(),
        "cooperative-resource",
        Path::new("/bin/ps"),
        10_000,
        1000,
    );
    wait_file(&root.path().join("worker-ready"));
    drop(writer);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let result: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(result["event"], "finished");
    assert_eq!(result["cancelled"], true);
    assert_eq!(result["cleanup_complete"], true);
    assert!(child.wait().unwrap().success());
    assert!(root.path().join("cooperative-stop-observed").is_file());
    assert!(!root.path().join("external-resource").exists());
}

#[test]
fn ignored_cooperative_stop_still_uses_bounded_pinned_hard_cleanup() {
    let _scenario = scenario();
    let root = scratch();
    let result = controller(root.path())
        .run_with_cooperative_stop(
            worker(root.path(), "ignore-term"),
            &root.path().join("worker.log"),
            Duration::from_secs(3),
            Duration::from_millis(200),
        )
        .unwrap();
    assert!(
        result.timed_out
            && result.cleanup_complete
            && !result.status.success()
            && !result.success()
    );
    worker_dead(root.path());
    assert!(!root.path().join("invoked").exists());
}

#[test]
fn excessive_cooperative_grace_is_rejected_before_launch() {
    let _scenario = scenario();
    let root = scratch();
    for grace in [
        Duration::ZERO,
        xtask::process::COOPERATIVE_STOP_LIMIT + Duration::from_millis(1),
    ] {
        assert!(
            controller(root.path())
                .run_with_cooperative_stop(
                    worker(root.path(), "marker"),
                    &root.path().join("worker.log"),
                    Duration::from_secs(3),
                    grace
                )
                .is_err()
        );
    }
    assert!(!root.path().join("worker.log").exists());
    assert!(!root.path().join("worker-group").exists());
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
        assert!(
            output.stderr.is_empty()
                && (output.status.success()
                    || output.status.code() == Some(1) && output.stdout.is_empty()),
            "Owned worker absence observer failed: {output:?}"
        );
        let state = String::from_utf8(output.stdout).unwrap();
        assert!(
            began.elapsed() < xtask::process::CLEANUP_GRACE,
            "Owned nested worker {pid} absence was not confirmed within cleanup grace; observed state {state:?}"
        );
        if state.trim().is_empty() || state.trim_start().starts_with('Z') {
            return;
        }
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
    assert!(!controller.admits_work());
    assert!(
        controller
            .run(
                worker(root.path(), "marker"),
                &root.path().join("later.log"),
                Duration::from_secs(5)
            )
            .is_err()
    );
    assert!(!root.path().join("later.log").exists());
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
fn separate_output_retains_nonzero_status_and_exact_streams() {
    let _scenario = scenario();
    let root = scratch();
    let controller = controller(root.path());
    let log = root.path().join("streams.log");
    let result = controller
        .run_separate_output(
            worker(root.path(), "streams-exit"),
            &log,
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(result.status.code(), Some(23));
    assert!(!result.success());
    assert!(!result.timed_out);
    assert!(!result.cancelled);
    assert_eq!(result.unexpected_survivors, 0);
    assert!(result.cleanup_complete);
    assert_eq!(fs::read(log).unwrap(), b"owned stdout\n");
    assert_eq!(
        fs::read(root.path().join("streams.stderr")).unwrap(),
        b"owned stderr\n"
    );
    assert!(controller.admits_work());
    let failure = result
        .require_success(&root.path().join("streams.log"))
        .unwrap_err();
    let failure = failure
        .downcast::<xtask::process::CommandFailure>()
        .unwrap();
    assert_eq!(failure.process["exit_code"], 23);
    assert_eq!(failure.process["cleanup_complete"], true);
    assert!(
        controller
            .run(
                Command::new("/usr/bin/true"),
                &root.path().join("later.log"),
                Duration::from_secs(5)
            )
            .unwrap()
            .success()
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
#[allow(
    dead_code,
    unreachable_pub,
    reason = "The private fixture module includes the exact production controller source to exercise its crate-private kernel pin boundary with the real built helper; public production entry points become unreachable and unrelated entry points are unused in this compilation unit. Exposing private kernel hooks as shipping API or copying their logic would weaken that boundary."
)]
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
    raw_session_with_cooperative_stop(root, mode, inspector, deadline_ms, 0)
}
fn raw_session_with_cooperative_stop(
    root: &Path,
    mode: &str,
    inspector: &Path,
    deadline_ms: u64,
    cooperative_stop_ms: u64,
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
        "cwd":root.as_os_str().as_bytes(),"env":[],"log":root.join("raw.log").as_os_str().as_bytes(),"deadline_ms":deadline_ms,"cooperative_stop_ms":cooperative_stop_ms,
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

fn inspection_controller(root: &Path, script: &str) -> controller_source::Controller {
    let inspector = root.join("inspect");
    fs::write(&inspector, script).unwrap();
    fs::set_permissions(&inspector, fs::Permissions::from_mode(0o700)).unwrap();
    let mut controller =
        controller_source::Controller::new(root, Path::new(env!("CARGO_BIN_EXE_xtask"))).unwrap();
    controller.inspector = inspector;
    controller
}

#[test]
fn admission_inspects_only_supervisor_then_cleanup_inspects_full_session() {
    let _scenario = scenario();
    let root = scratch();
    let controller = inspection_controller(
        root.path(),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.args\"\nexec /bin/ps \"$@\"\n",
    );
    let result = controller
        .run(
            Command::new("/usr/bin/true"),
            &root.path().join("worker.log"),
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(result.success());
    let ready: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("worker.started.json")).unwrap())
            .unwrap();
    let args = fs::read_to_string(root.path().join("inspect.args")).unwrap();
    let lines = args.lines().collect::<Vec<_>>();
    assert_eq!(
        lines[0],
        format!("-p {} -o pid=,pgid=,state=,ucomm=", ready["session"])
    );
    assert!(lines.len() > 1);
    assert!(
        lines[1..]
            .iter()
            .all(|line| *line == "-axo pid=,pgid=,state=,ucomm=")
    );
}

#[test]
fn delayed_admission_inspection_is_reaped_recorded_and_closes_admission() {
    let _scenario = scenario();
    let root = scratch();
    let controller = inspection_controller(
        root.path(),
        "#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$0.pid\"\nexec /bin/sleep 4\n",
    );
    let error = controller
        .run(
            worker(root.path(), "marker"),
            &root.path().join("worker.log"),
            Duration::from_secs(5),
        )
        .err()
        .expect("A delayed inspector cannot admit a worker");
    assert!(
        error
            .to_string()
            .contains("inspection failed before worker launch")
    );
    assert!(error.to_string().contains("exceeded its remaining budget"));
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("worker.control.json")).unwrap())
            .unwrap();
    assert_eq!(record["event"], "inspection_failed");
    assert!(record["elapsed_ms"].as_u64().unwrap() >= 2000);
    assert!(record.get("raw_status").is_none());
    assert!(record.get("cleanup_complete").is_none());
    let pid: i32 = fs::read_to_string(root.path().join("inspect.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        rustix::process::getpgid(rustix::process::Pid::from_raw(pid)).unwrap_err(),
        rustix::io::Errno::SRCH
    );
    assert!(!root.path().join("worker-group").exists());
    assert!(!root.path().join("invoked").exists());
    assert!(!controller.admits_work());
}

#[test]
fn malformed_admission_inspection_refuses_worker_with_terminal_evidence() {
    let _scenario = scenario();
    let root = scratch();
    let controller = inspection_controller(root.path(), "#!/bin/sh\nprintf 'malformed row\\n'\n");
    let error = controller
        .run(
            worker(root.path(), "marker"),
            &root.path().join("worker.log"),
            Duration::from_secs(5),
        )
        .err()
        .expect("Malformed inspection cannot admit a worker");
    assert!(
        error
            .to_string()
            .contains("Malformed numeric process inspection")
    );
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("worker.control.json")).unwrap())
            .unwrap();
    assert_eq!(record["event"], "inspection_failed");
    assert!(!controller.admits_work());
    assert!(!root.path().join("worker-group").exists());
    assert!(!root.path().join("invoked").exists());
}

#[test]
fn unsupported_full_census_refuses_cleanup_after_valid_self_inspection() {
    let _scenario = scenario();
    let root = scratch();
    let controller = inspection_controller(
        root.path(),
        "#!/bin/sh\nif [ \"$1\" = -axo ]; then exit 1; fi\nexec /bin/ps \"$@\"\n",
    );
    let error = controller
        .run(
            worker(root.path(), "marker"),
            &root.path().join("worker.log"),
            Duration::from_secs(5),
        )
        .err()
        .expect("A successful worker cannot qualify without the full census");
    assert!(
        error.to_string().contains("Supervisor cleanup failed:"),
        "{error}"
    );
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("worker.control.json")).unwrap())
            .unwrap();
    assert_eq!(record["event"], "finished");
    assert_eq!(record["raw_status"], 0);
    assert_eq!(record["cleanup_complete"], false);
    assert!(root.path().join("invoked").exists());
    assert!(!controller.admits_work());
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
    let request = serde_json::json!({"program":b"/usr/bin/true","args":[],"cwd":root.path().as_os_str().as_bytes(),"env":[],"log":root.path().join("waitable.log").as_os_str().as_bytes(),"deadline_ms":5000,"cooperative_stop_ms":0,"inspector":b"/bin/ps","capture":false});
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

#[test]
fn settled_helper_copy_is_removed_but_identity_and_process_receipts_remain() {
    let _scenario = scenario();
    let root = scratch();
    let controller = controller(root.path());
    let helper_directory = fs::read_dir(root.path().join("target/process-control"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("helper-")
        })
        .unwrap();
    let identity: archive::ByteIdentity =
        serde_json::from_slice(&fs::read(helper_directory.join("helper-identity.json")).unwrap())
            .unwrap();
    assert!(identity.byte_length > 0);
    let log = root.path().join("worker.log");
    assert!(
        controller
            .run(Command::new("/usr/bin/true"), &log, Duration::from_secs(5))
            .unwrap()
            .success()
    );
    controller.finish_helper().unwrap();
    assert!(!helper_directory.join("xtask-helper").exists());
    let cleanup: serde_json::Value =
        serde_json::from_slice(&fs::read(helper_directory.join("cleanup.json")).unwrap()).unwrap();
    assert_eq!(cleanup["helper_removed"], true);
    assert_eq!(cleanup["identity"], serde_json::to_value(identity).unwrap());
    assert!(log.is_file());
    assert!(
        controller
            .run(
                Command::new("/usr/bin/true"),
                &root.path().join("late.log"),
                Duration::from_secs(5)
            )
            .is_err()
    );
}

#[test]
fn clipboard_abort_waits_for_complete_ack_and_rejects_malformed_bytes() {
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    let _scenario = scenario();
    let root = scratch();
    let finished = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let publication = scope.spawn(|| {
            let began = std::time::Instant::now();
            while !root.path().join("panel-path-1.request").is_file() {
                assert!(began.elapsed() < Duration::from_secs(8));
                std::thread::sleep(Duration::from_millis(10));
            }
            let mut marker = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.path().join("panel-path-1.ready"))
                .unwrap();
            std::thread::sleep(Duration::from_millis(150));
            assert!(
                !finished.load(Ordering::Relaxed),
                "An empty marker is not readiness"
            );
            marker.write_all(b"owned panel").unwrap();
            std::thread::sleep(Duration::from_millis(150));
            assert!(
                !finished.load(Ordering::Relaxed),
                "A partial marker is not readiness"
            );
            marker.write_all(b" path ready 1\n").unwrap();
        });
        let result = controller(root.path())
            .run(
                worker(root.path(), "clipboard-abort"),
                &root.path().join("clipboard-partial.log"),
                Duration::from_secs(10),
            )
            .unwrap();
        finished.store(true, Ordering::Relaxed);
        publication.join().unwrap();
        assert_eq!(result.status.signal(), Some(6));
        assert!(result.cleanup_complete && result.unexpected_survivors == 0);
        assert!(!result.timed_out && !result.cancelled);
    });
    for bytes in [
        b"wrong".as_slice(),
        b"owned panel path ready 1\nextra".as_slice(),
    ] {
        let root = scratch();
        fs::write(root.path().join("panel-path-1.ready"), bytes).unwrap();
        let result = controller(root.path())
            .run(
                worker(root.path(), "clipboard-abort"),
                &root.path().join("clipboard-malformed.log"),
                Duration::from_secs(10),
            )
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(result.cleanup_complete && result.unexpected_survivors == 0);
        assert!(!result.timed_out && !result.cancelled);
    }
}
