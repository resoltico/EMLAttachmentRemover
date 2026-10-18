//! Real reusable desktop supervisor and core-backed worker, without a GUI window.
#![cfg(target_os = "macos")]
use eml_app::{
    RequestSnapshot, RunQueue, WorkerCompletion, WorkerControl, WorkerFailure, WorkerObservation,
    WorkerRun,
};
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
const BIN: &str = env!("CARGO_BIN_EXE_eml-worker");
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/supervised-worker-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
#[cfg(feature = "native-fixtures")]
fn worker_fixture(root: &Path, mode: Option<&str>) -> PathBuf {
    let executable = root.join("worker-observation");
    fs::copy(env!("CARGO_BIN_EXE_worker-observation"), &executable).unwrap();
    fs::write(root.join("fixture-authorized"), b"owned transport fixture").unwrap();
    if let Some(mode) = mode {
        fs::write(root.join("fixture-mode"), mode).unwrap();
    }
    executable
}
fn admitted(paths: Vec<PathBuf>, destination: Option<PathBuf>) -> (RequestSnapshot, WorkerControl) {
    let mut queue = RunQueue::default();
    let id = queue
        .enqueue(RequestSnapshot::new(paths, destination).unwrap())
        .unwrap();
    let (actual, request) = queue.start_next().unwrap().unwrap();
    assert_eq!(id, actual);
    (request, WorkerControl::new(id))
}
fn finish(worker: &mut WorkerRun) -> WorkerCompletion {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let WorkerObservation::Settled(result) = worker.poll() {
            return result;
        }
        if Instant::now() >= deadline {
            worker
                .control()
                .confirm_shutdown(Instant::now() - Duration::from_secs(12))
                .unwrap();
            let cleanup = Instant::now() + Duration::from_secs(5);
            while Instant::now() < cleanup {
                if matches!(worker.poll(), WorkerObservation::Settled(_)) {
                    panic!(
                        "Owned supervisor exceeded execution allowance; settlement completed after Kill intent"
                    );
                }
                thread::sleep(Duration::from_millis(10));
            }
            eprintln!(
                "Owned supervisor settlement remains unverified; abort later tests and retain resources"
            );
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn real_supervisor_checks_identity_feeds_core_drains_and_preserves_complete_outcomes() {
    let root = root();
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    for expected in ["created", "up_to_date"] {
        let (request, control) = admitted(vec![source.clone()], None);
        let mut worker = WorkerRun::start(
            PathBuf::from(BIN),
            env!("CARGO_PKG_VERSION").into(),
            request,
            control,
        )
        .unwrap();
        let report = worker.report();
        let done = finish(&mut worker);
        assert!(
            matches!(worker.poll(), WorkerObservation::Settled(ref terminal) if terminal.exit_code == done.exit_code && terminal.failure == done.failure)
        );
        assert!(done.settled && done.reusable, "{done:?}");
        assert_eq!(done.failure, None);
        assert_eq!(done.exit_code, Some(0));
        assert!(done.input_may_have_been_sent);
        let bytes = report.export_bytes().unwrap();
        let exported: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(exported["items"][0]["status"], expected);
        assert_eq!(exported["summary"]["exit_code"], 0);
        assert!(exported["protocol_failure"].is_null());
        assert!(fs::read(&source).unwrap() == INPUT, "Original changed");
        assert!(
            fs::read(root.path().join("message.no-attachments.eml")).unwrap() == GOLDEN,
            "Independent copy bytes differ"
        );
    }
    root.close().unwrap();
}
#[test]
fn stopped_before_launch_does_not_start_even_an_unavailable_helper() {
    let root = root();
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let (request, control) = admitted(vec![source.clone()], None);
    control.request_stop().unwrap();
    let mut worker = WorkerRun::start(
        root.path().join("absent-helper"),
        env!("CARGO_PKG_VERSION").into(),
        request,
        control,
    )
    .unwrap();
    let done = finish(&mut worker);
    assert!(done.settled && done.reusable);
    assert_eq!(done.failure, None);
    assert!(!done.input_may_have_been_sent);
    assert_eq!(done.exit_code, None);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert!(fs::read(source).unwrap() == INPUT);
    root.close().unwrap();
}
#[test]
fn wrong_version_refuses_before_request_and_captured_invalid_destination_remains_a_core_refusal() {
    let root = root();
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let (request, control) = admitted(vec![source.clone()], None);
    let mut worker = WorkerRun::start(
        PathBuf::from(BIN),
        "0.0.0-incompatible".into(),
        request,
        control,
    )
    .unwrap();
    let done = finish(&mut worker);
    assert!(done.settled);
    assert!(!done.reusable);
    assert_eq!(done.failure, Some(WorkerFailure::Identity));
    assert!(!done.input_may_have_been_sent);
    let (request, control) = admitted(
        vec![source.clone()],
        Some(root.path().join("absent-destination")),
    );
    let mut worker = WorkerRun::start(
        PathBuf::from(BIN),
        env!("CARGO_PKG_VERSION").into(),
        request,
        control,
    )
    .unwrap();
    let done = finish(&mut worker);
    assert!(done.settled && done.reusable, "{done:?}");
    assert_eq!(done.exit_code, Some(2));
    assert!(done.input_may_have_been_sent);
    assert!(fs::read(source).unwrap() == INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    root.close().unwrap();
}

#[cfg(feature = "native-fixtures")]
#[test]
fn cancellation_before_ready_cannot_excuse_an_unexpected_partial_stdout_prefix() {
    let root = root();
    let fixture = worker_fixture(root.path(), None);
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let (request, control) = admitted(vec![source.clone()], None);
    let mut worker =
        WorkerRun::start(fixture, env!("CARGO_PKG_VERSION").into(), request, control).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.path().join("prefix-ready").exists() {
        if Instant::now() >= deadline {
            worker
                .control()
                .confirm_shutdown(Instant::now() - Duration::from_secs(12))
                .unwrap();
            let done = finish(&mut worker);
            panic!("Fixture never reached its output prefix: {done:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
    worker.control().request_stop().unwrap();
    let done = finish(&mut worker);
    assert!(done.settled, "{done:?}");
    assert!(!done.reusable, "{done:?}");
    assert_eq!(done.failure, Some(WorkerFailure::Transport));
    assert!(!done.input_may_have_been_sent);
    assert!(
        done.faults
            .iter()
            .any(|fault| fault.contains("before private readiness")),
        "{done:?}"
    );
    assert_eq!(fs::read(source).unwrap(), INPUT);
    root.close().unwrap();
}

#[cfg(feature = "native-fixtures")]
#[test]
fn pending_report_warning_closes_admission_and_preserves_identity_failure_after_settlement() {
    let root = root();
    let fixture = worker_fixture(root.path(), Some("wrong-identity"));
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let request = RequestSnapshot::new(vec![source.clone()], None).unwrap();
    let offered = request.clone();
    let mut session = eml_app::DesktopSession::new(fixture, env!("CARGO_PKG_VERSION").into());
    let id = session.enqueue(request).unwrap();
    assert!(session.start_next().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.path().join("prefix-ready").exists() {
        if Instant::now() >= deadline {
            session
                .control()
                .unwrap()
                .confirm_shutdown(Instant::now() - Duration::from_secs(12))
                .unwrap();
            let done = session_finish(&mut session);
            panic!("Fixture never emitted its wrong identity: {done:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let report = session.reports()[0].report();
    let (locked, wait_locked) = std::sync::mpsc::sync_channel(1);
    let (release, wait_release) = std::sync::mpsc::sync_channel(1);
    let inspector = thread::spawn(move || {
        report
            .inspect(|_| {
                locked.send(()).unwrap();
                wait_release.recv_timeout(Duration::from_secs(6)).unwrap();
            })
            .unwrap()
            .unwrap();
    });
    wait_locked.recv_timeout(Duration::from_secs(1)).unwrap();
    session.control().unwrap().request_stop().unwrap();
    let allowance = Instant::now() + Duration::from_secs(4);
    let mut warned = false;
    let mut admission_refused = false;
    while Instant::now() < allowance {
        match session.poll() {
            Some(WorkerObservation::Pending { warning: Some(_) }) => {
                warned = true;
                admission_refused = session.blocked()
                    && matches!(
                        session.enqueue(offered.clone()),
                        Err(eml_app::SessionError::Blocked)
                    );
                break;
            }
            Some(WorkerObservation::Settled(_)) => break,
            _ => thread::sleep(Duration::from_millis(5)),
        }
    }
    release.send(()).unwrap();
    inspector.join().unwrap();
    let done = session_finish(&mut session);
    assert!(
        warned,
        "No bounded warning during held final report: {done:?}"
    );
    assert!(done.settled && !done.reusable, "{done:?}");
    assert_eq!(done.failure, Some(WorkerFailure::Identity));
    assert!(!done.input_may_have_been_sent);
    assert!(matches!(
        session.poll(),
        Some(WorkerObservation::Settled(_))
    ));
    assert!(
        admission_refused,
        "Admission remained open during an unverified pending warning"
    );
    session.acknowledge_completion(id).unwrap();
    assert!(session.blocked());
    assert!(matches!(
        session.enqueue(offered),
        Err(eml_app::SessionError::Blocked)
    ));
    assert!(matches!(
        session.start_next(),
        Err(eml_app::SessionError::Blocked)
    ));
    let snapshot = session.reports()[0].export_snapshot().unwrap();
    let exported = root.path().join("failed-worker-report.json");
    let receipt = eml_app::export_report(&snapshot, &exported, &eml_core::RunControl::default());
    assert!(receipt.usable(), "{receipt:?}");
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(exported).unwrap()).unwrap();
    assert_eq!(saved["run_id"], 1);
    assert_eq!(saved["worker"]["failure"], "identity");
    assert_eq!(saved["worker"]["settled"], true);
    assert_eq!(saved["worker"]["reusable"], false);
    assert_eq!(saved["worker"]["input_may_have_been_sent"], false);
    assert_eq!(saved["summary"], serde_json::Value::Null);
    session.confirm_shutdown(Instant::now()).unwrap();
    assert!(session.worker_shutdown_settled());
    assert_eq!(fs::read(source).unwrap(), INPUT);
    root.close().unwrap();
}

#[cfg(feature = "native-fixtures")]
#[test]
fn missing_ready_and_first_interrupt_acknowledgment_end_as_failed_owned_cleanup() {
    for mode in ["no-ready", "no-first-ack"] {
        let root = root();
        let fixture = worker_fixture(root.path(), Some(mode));
        let source = root.path().join("message.eml");
        fs::write(&source, INPUT).unwrap();
        let (request, control) = admitted(vec![source.clone()], None);
        let mut worker =
            WorkerRun::start(fixture, env!("CARGO_PKG_VERSION").into(), request, control).unwrap();
        if mode == "no-first-ack" {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !root.path().join("fed-ready").exists() {
                if Instant::now() >= deadline {
                    worker
                        .control()
                        .confirm_shutdown(Instant::now() - Duration::from_secs(12))
                        .unwrap();
                    let done = finish(&mut worker);
                    panic!("Fixture never received the complete request: {done:?}");
                }
                thread::sleep(Duration::from_millis(5));
            }
            worker.control().request_stop().unwrap();
            worker.control().request_stop().unwrap();
        }
        let done = finish(&mut worker);
        assert!(done.settled && !done.reusable, "{mode}: {done:?}");
        assert_eq!(
            done.failure,
            Some(WorkerFailure::Transport),
            "{mode}: {done:?}"
        );
        assert_eq!(done.signal, Some(9), "{mode}: {done:?}");
        assert!(!done.first_interrupt_observed);
        if mode == "no-first-ack" {
            assert_eq!(
                done.interrupt_calls_succeeded, 1,
                "A second interrupt needs an actual F: {done:?}"
            );
            assert!(done.input_may_have_been_sent);
            assert!(
                done.faults
                    .iter()
                    .any(|fault| fault.contains("acknowledgment unavailable")),
                "{done:?}"
            );
        } else {
            assert_eq!(done.interrupt_calls_succeeded, 0);
            assert!(!done.input_may_have_been_sent);
            assert!(
                done.faults
                    .iter()
                    .any(|fault| fault.contains("readiness/feed allowance")),
                "{done:?}"
            );
        }
        assert_eq!(fs::read(source).unwrap(), INPUT);
        assert!(!root.path().join("message.no-attachments.eml").exists());
        root.close().unwrap();
    }
}

fn session_finish(session: &mut eml_app::DesktopSession) -> WorkerCompletion {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(WorkerObservation::Settled(done)) = session.poll() {
            return done;
        }
        if Instant::now() >= deadline {
            session
                .confirm_shutdown(Instant::now() - Duration::from_secs(12))
                .unwrap();
            let cleanup = Instant::now() + Duration::from_secs(5);
            while Instant::now() < cleanup {
                if matches!(session.poll(), Some(WorkerObservation::Settled(_))) {
                    panic!("Session execution exceeded allowance; cleanup completed");
                }
                thread::sleep(Duration::from_millis(5));
            }
            eprintln!(
                "Session ownership remains unverified; retaining resources and stopping later tests"
            );
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn desktop_session_waits_for_final_view_application_and_explicit_report_dismissal() {
    use eml_app::{DesktopSession, QueueError, SessionError};
    let root = root();
    let mut session = DesktopSession::new(PathBuf::from(BIN), env!("CARGO_PKG_VERSION").into());
    let mut ids = Vec::new();
    for name in ["first", "second", "third"] {
        let source = root.path().join(format!("{name}.eml"));
        fs::write(&source, INPUT).unwrap();
        ids.push(
            session
                .enqueue(RequestSnapshot::new(vec![source], None).unwrap())
                .unwrap(),
        );
    }
    assert!(session.start_next().unwrap());
    let done = session_finish(&mut session);
    assert!(done.settled && done.reusable, "{done:?}");
    assert_eq!(session.active(), Some(ids[0]));
    assert!(
        !session.start_next().unwrap(),
        "Final rendering acknowledgment is required before the next launch"
    );
    assert!(!root.path().join("second.no-attachments.eml").exists());
    assert!(matches!(
        session.acknowledge_completion(ids[1]),
        Err(SessionError::InactiveRun)
    ));
    assert!(
        !session.dismiss(ids[0]),
        "Active reservation must remain retained"
    );
    session.acknowledge_completion(ids[0]).unwrap();
    assert!(session.start_next().unwrap());
    assert!(
        session.control_for(ids[0]).is_none(),
        "A stale Force Stop confirmation must not control the next run"
    );
    assert_eq!(session.control_for(ids[1]).unwrap().id(), ids[1]);
    assert!(matches!(
        session.acknowledge_completion(ids[0]),
        Err(SessionError::InactiveRun)
    ));
    let done = session_finish(&mut session);
    assert!(done.settled && done.reusable, "{done:?}");
    session.acknowledge_completion(ids[1]).unwrap();
    assert!(matches!(
        session.start_next(),
        Err(SessionError::Queue(QueueError::ReportSpace))
    ));
    assert_eq!(session.waiting(), 1);
    assert_eq!(session.reports().len(), 2);
    assert!(!root.path().join("third.no-attachments.eml").exists());
    let exporting = session.reports()[0].report();
    assert!(
        !session.dismiss(ids[0]),
        "An outstanding export/read handle retains its report reservation"
    );
    assert!(matches!(
        session.start_next(),
        Err(SessionError::Queue(QueueError::ReportSpace))
    ));
    drop(exporting);
    assert!(session.dismiss(ids[0]));
    assert!(session.start_next().unwrap());
    let done = session_finish(&mut session);
    assert!(done.settled && done.reusable, "{done:?}");
    session.acknowledge_completion(ids[2]).unwrap();
    assert_eq!(session.waiting(), 0);
    assert!(!session.blocked());
    session.confirm_shutdown(Instant::now()).unwrap();
    assert!(session.worker_shutdown_settled());
    let request = RequestSnapshot::new(vec![root.path().join("first.eml")], None).unwrap();
    assert!(matches!(
        session.enqueue(request),
        Err(SessionError::Closing)
    ));
    for name in ["first", "second", "third"] {
        assert_eq!(
            fs::read(root.path().join(format!("{name}.eml"))).unwrap(),
            INPUT
        );
        assert_eq!(
            fs::read(root.path().join(format!("{name}.no-attachments.eml"))).unwrap(),
            GOLDEN
        );
    }
    root.close().unwrap();
}

#[cfg(feature = "native-fixtures")]
#[test]
fn confirmed_shutdown_cancels_waiting_and_retains_actual_worker_until_final_ack() {
    use eml_app::{DesktopSession, SessionError};
    let root = root();
    let fixture = worker_fixture(root.path(), Some("no-first-ack"));
    let mut session = DesktopSession::new(fixture, env!("CARGO_PKG_VERSION").into());
    let mut ids = Vec::new();
    for name in ["active", "queued"] {
        let source = root.path().join(format!("{name}.eml"));
        fs::write(&source, INPUT).unwrap();
        ids.push(
            session
                .enqueue(RequestSnapshot::new(vec![source], None).unwrap())
                .unwrap(),
        );
    }
    assert!(session.start_next().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.path().join("fed-ready").exists() {
        if Instant::now() >= deadline {
            session
                .confirm_shutdown(Instant::now() - Duration::from_secs(12))
                .unwrap();
            let done = session_finish(&mut session);
            panic!("Owned fixture never received request: {done:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let token = session.control_for(ids[0]).unwrap().clone();
    let start = Instant::now();
    session.confirm_shutdown(start).unwrap();
    let waiting_cancelled = session.waiting() == 0;
    let active_retained = session.active() == Some(ids[0]) && !session.worker_shutdown_settled();
    let request = RequestSnapshot::new(vec![root.path().join("queued.eml")], None).unwrap();
    let admission_closed = matches!(session.enqueue(request), Err(SessionError::Closing));
    let launch_closed = matches!(session.start_next(), Err(SessionError::Closing));
    session
        .confirm_shutdown(start + Duration::from_secs(11))
        .unwrap();
    // Exercise the policy clock separately from real wall-clock acceptance:
    // repeated confirmation must not replace its original deadline.
    let original_deadline = token.intent(start + Duration::from_secs(12)).unwrap();
    let done = session_finish(&mut session);
    assert!(done.settled, "{done:?}");
    assert!(waiting_cancelled && active_retained && admission_closed && launch_closed);
    assert!(done.input_may_have_been_sent && !done.reusable);
    assert_eq!(done.exit_code, None);
    assert_eq!(done.signal, Some(9));
    assert_eq!(done.failure, Some(WorkerFailure::Transport));
    assert_eq!(done.interrupt_calls_succeeded, 1);
    assert!(!done.first_interrupt_observed);
    let retained = session.reports()[0].completion().unwrap();
    assert_eq!(retained.failure, done.failure);
    assert_eq!(retained.signal, done.signal);
    assert_eq!(
        retained.interrupt_calls_succeeded,
        done.interrupt_calls_succeeded
    );
    assert!(original_deadline.kill_due && original_deadline.interrupts == 2);
    assert_eq!(session.active(), Some(ids[0]));
    assert!(
        !session.worker_shutdown_settled(),
        "Final view-model acknowledgment remains required after reap"
    );
    assert!(!session.dismiss(ids[0]));
    session.acknowledge_completion(ids[0]).unwrap();
    assert!(session.worker_shutdown_settled());
    assert!(session.control_for(ids[1]).is_none());
    assert!(matches!(session.start_next(), Err(SessionError::Closing)));
    for name in ["active", "queued"] {
        assert_eq!(
            fs::read(root.path().join(format!("{name}.eml"))).unwrap(),
            INPUT
        );
        assert!(
            !root
                .path()
                .join(format!("{name}.no-attachments.eml"))
                .exists()
        );
    }
    root.close().unwrap();
}

#[test]
fn explicit_report_export_reuses_protected_publication_and_never_replaces_original_or_report() {
    use eml_app::export_report;
    let root = root();
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let (request, control) = admitted(vec![source.clone()], None);
    let mut worker = WorkerRun::start(
        PathBuf::from(BIN),
        env!("CARGO_PKG_VERSION").into(),
        request,
        control,
    )
    .unwrap();

    let done = finish(&mut worker);
    assert!(done.settled && done.reusable, "{done:?}");
    let report = worker.export_snapshot().expect("actually settled export");
    let destination = root.path().join("report.json");
    let output = destination.clone();
    let retained = report.clone();
    let first =
        thread::spawn(move || export_report(&retained, &output, &eml_core::RunControl::default()))
            .join()
            .unwrap();
    assert!(first.usable(), "{first:?}");
    assert_eq!(first.publication, eml_fs::PublicationState::Published);
    assert_eq!(first.final_file_verified, Some(true));
    assert_eq!(first.cleanup.state, eml_fs::CleanupState::Complete);
    let bytes = fs::read(&destination).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["run_id"], 1);
    assert_eq!(parsed["worker"]["exit_code"], 0);
    assert_eq!(parsed["worker"]["signal"], serde_json::Value::Null);
    assert_eq!(parsed["worker"]["failure"], serde_json::Value::Null);
    assert_eq!(parsed["worker"]["settled"], true);
    assert_eq!(parsed["items"][0]["status"], "created");
    assert_eq!(parsed["summary"]["counts"]["created"], 1);
    assert!(
        !String::from_utf8(bytes.clone())
            .unwrap()
            .contains("Hello Bob")
    );
    for output in [destination.clone(), source.clone()] {
        let retained = report.clone();
        let refused = thread::spawn(move || {
            export_report(&retained, &output, &eml_core::RunControl::default())
        })
        .join()
        .unwrap();
        assert!(!refused.usable(), "{refused:?}");
        assert_eq!(refused.publication, eml_fs::PublicationState::NotPublished);
        assert_eq!(refused.cleanup.state, eml_fs::CleanupState::Complete);
        assert!(!refused.failures.is_empty());
    }
    let retained = report.clone();
    let cancelled_path = root.path().join("cancelled.json");
    let output = cancelled_path.clone();
    let stopped = eml_core::RunControl::default();
    stopped.request_stop();
    let cancelled = thread::spawn(move || export_report(&retained, &output, &stopped))
        .join()
        .unwrap();
    assert!(!cancelled.usable());
    assert_eq!(
        cancelled.publication,
        eml_fs::PublicationState::NotPublished
    );
    assert_eq!(cancelled.cleanup.state, eml_fs::CleanupState::NotNeeded);
    assert!(!cancelled_path.exists());
    assert_eq!(fs::read(&destination).unwrap(), bytes);
    assert_eq!(fs::read(source).unwrap(), INPUT);
    assert_eq!(
        fs::read(root.path().join("message.no-attachments.eml")).unwrap(),
        GOLDEN
    );
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        3,
        "Owned stages must be removed"
    );
    root.close().unwrap();
}
