//! Stub CLI processes exercise the production std::process supervisor and protocol reducer.
#![cfg(unix)]
use eml_app::{
    RequestSnapshot, WorkerCompletion, WorkerFailure, WorkerRun, WorkerState, bundled_cli,
    export_report,
};
use eml_core::RunMode;
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::ExitStatusExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/contract/protocol/{name}.jsonl")),
    )
    .unwrap()
    .replace("@VERSION@", eml_core::product::VERSION)
}
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn print(value: &str) -> String {
    format!("printf '%s' {}\n", quoted(value))
}
fn script(root: &Path, body: &str) -> PathBuf {
    let path = root.join("fixture-cli");
    let code = format!(
        "#!/bin/sh\n[ \"$#\" -eq 4 ] && [ \"$1\" = --format ] && [ \"$2\" = json ] && [ \"$3\" = --request ] && [ \"$4\" = - ] || exit 91\n{body}"
    );
    fs::write(&path, code).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn request(root: &Path, mode: RunMode) -> RequestSnapshot {
    RequestSnapshot::new(vec![root.join("message.eml")], None, mode).unwrap()
}
fn finish(run: &mut WorkerRun) -> WorkerCompletion {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(completion) = run.poll().unwrap() {
            return completion;
        }
        assert!(Instant::now() < deadline, "Owned fixture did not settle");
        thread::sleep(Duration::from_millis(2));
    }
}
fn wait_start(run: &mut WorkerRun) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while run.report().with_report(|report| report.start().is_some()) != Some(true) {
        assert!(
            run.poll().unwrap().is_none(),
            "Fixture exited before its start"
        );
        assert!(Instant::now() < deadline, "Fixture did not emit start");
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn completed_and_failed_runs_retain_records_exit_status_and_exact_jsonl() {
    for (name, code, state) in [
        ("created", 0, WorkerState::Completed),
        (
            "prepublication-failure",
            1,
            WorkerState::CompletedWithFailures,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let bytes = fixture(name);
        let cli = script(
            root.path(),
            &format!("cat >/dev/null\n{}exit {code}\n", print(&bytes)),
        );
        let mut run =
            WorkerRun::start(&cli, &request(root.path(), RunMode::Create), root.path()).unwrap();
        let completion = finish(&mut run);
        assert_eq!(completion.state, state);
        assert_eq!(completion.exit.code(), Some(code));
        assert_eq!(completion.failure, None);
        assert_eq!(completion.jsonl.as_ref(), bytes.as_bytes());
        assert_eq!(
            completion.report.with_report(|report| report.items().len()),
            Some(1)
        );
        assert!(
            completion
                .report
                .with_report(|report| report.summary().is_some())
                .unwrap()
        );
        run.stop().unwrap();
        run.force_stop().unwrap();
        assert!(run.poll().unwrap().is_none());
    }
}
#[test]
fn stop_is_cooperative_and_force_stop_is_sigkill_with_an_incomplete_report() {
    let records = fixture("started-preedge-cancellation");
    let (start, rest) = records.split_once('\n').unwrap();
    for forced in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let trap = if forced {
            "trap '' INT\n".to_owned()
        } else {
            format!("trap {} INT\n", quoted(&(print(rest) + "exit 130")))
        };
        let cli = script(
            root.path(),
            &format!(
                "{trap}cat >/dev/null\n{}while :; do :; done\n",
                print(&(start.to_owned() + "\n"))
            ),
        );
        let mut run =
            WorkerRun::start(&cli, &request(root.path(), RunMode::Create), root.path()).unwrap();
        wait_start(&mut run);
        if forced {
            run.force_stop().unwrap();
        } else {
            run.stop().unwrap();
            run.stop().unwrap();
        }
        let completion = finish(&mut run);
        if forced {
            assert_eq!(completion.exit.signal(), Some(9));
            assert_eq!(completion.state, WorkerState::ForceStopped);
            assert!(
                !completion
                    .report
                    .with_report(|r| r.summary().is_some())
                    .unwrap()
            );
        } else {
            assert_eq!(completion.exit.code(), Some(130));
            assert_eq!(completion.state, WorkerState::Stopped);
            assert!(
                completion
                    .report
                    .with_report(|r| r.summary().unwrap().interrupted)
                    .unwrap()
            );
        }
    }
}
#[test]
fn before_start_interruption_and_request_refusal_keep_their_actual_exit_codes() {
    for (code, state) in [
        (130, WorkerState::StoppedBeforeStart),
        (2, WorkerState::RequestRefused),
    ] {
        let root = tempfile::tempdir().unwrap();
        let cli = script(
            root.path(),
            &format!("cat >/dev/null\nprintf 'request detail' >&2\nexit {code}\n"),
        );
        let mut run =
            WorkerRun::start(&cli, &request(root.path(), RunMode::Create), root.path()).unwrap();
        let completion = finish(&mut run);
        assert_eq!(completion.state, state);
        assert_eq!(completion.exit.code(), Some(code));
        assert_eq!(completion.stderr, b"request detail");
        assert_eq!(
            completion.report.with_report(|r| r.start().is_none()),
            Some(true)
        );
    }
}
#[test]
fn sigint_before_start_is_observed_as_a_signalled_stop_and_reaped() {
    let root = tempfile::tempdir().unwrap();
    let ready = root.path().join("before-start-ready");
    let cli = script(
        root.path(),
        &format!(
            "trap - INT\ncat >/dev/null\n: >{}\nwhile :; do :; done\n",
            quoted(ready.to_str().unwrap())
        ),
    );
    let mut run =
        WorkerRun::start(&cli, &request(root.path(), RunMode::Create), root.path()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "Fixture did not reach its pre-start boundary"
        );
        thread::sleep(Duration::from_millis(2));
    }
    run.stop().unwrap();
    let completion = finish(&mut run);
    assert_eq!(completion.exit.signal(), Some(2));
    assert_eq!(completion.state, WorkerState::StoppedBeforeStart);
    assert_eq!(
        completion
            .report
            .with_report(|report| report.start().is_none()),
        Some(true)
    );
}

#[test]
fn identity_malformed_records_and_eof_without_summary_are_rejected_and_reaped() {
    let start = fixture("created").lines().next().unwrap().to_owned() + "\n";
    for (body, expected) in [
        (
            print(&start.replace("emlattachmentremover", "wrong-cli")),
            WorkerFailure::Identity,
        ),
        (print("{}\n"), WorkerFailure::Identity),
        (
            print(&start.replace(eml_core::product::VERSION, "0.0.0")),
            WorkerFailure::Identity,
        ),
        (
            print(&start) + &print("{broken}\n"),
            WorkerFailure::Protocol(eml_app::ProtocolFailure::MalformedRecord),
        ),
        (
            print(&start) + "exec 1>&-\n",
            WorkerFailure::Protocol(eml_app::ProtocolFailure::MissingSummary),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let cli = script(
            root.path(),
            &("cat >/dev/null\n".to_owned() + &body + "while :; do :; done\n"),
        );
        let mut run =
            WorkerRun::start(&cli, &request(root.path(), RunMode::Create), root.path()).unwrap();
        let completion = finish(&mut run);
        assert_eq!(completion.state, WorkerState::Incomplete);
        assert_eq!(completion.failure, Some(expected));
        assert_eq!(completion.exit.signal(), Some(9));
    }
}
#[test]
fn checking_then_creating_sends_two_fresh_requests_and_export_never_replaces() {
    let root = tempfile::tempdir().unwrap();
    for (mode, name) in [
        (RunMode::DryRun, "planned-create-unix"),
        (RunMode::Create, "created"),
    ] {
        let capture = root.path().join("received-request.json");
        let bytes = fixture(name);
        let cli = script(
            root.path(),
            &format!(
                "cat >{}\n{}exit 0\n",
                quoted(capture.to_str().unwrap()),
                print(&bytes)
            ),
        );
        let mut run = WorkerRun::start(&cli, &request(root.path(), mode), root.path()).unwrap();
        let completion = finish(&mut run);
        assert_eq!(completion.state, WorkerState::Completed);
        let encoded: serde_json::Value =
            serde_json::from_slice(&fs::read(capture).unwrap()).unwrap();
        assert_eq!(encoded["dry_run"], mode == RunMode::DryRun);
        let export = root.path().join(format!("{name}.jsonl"));
        export_report(&export, &completion.jsonl).unwrap();
        assert_eq!(fs::read(&export).unwrap(), bytes.as_bytes());
        assert_eq!(
            export_report(&export, b"replacement").unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&export).unwrap(), bytes.as_bytes());
    }
    assert_eq!(
        bundled_cli(Path::new("/App/Contents/MacOS/EMLAttachmentRemover")).unwrap(),
        Path::new("/App/Contents/Helpers/emlattachmentremover")
    );
    assert!(bundled_cli(Path::new("/")).is_err());
}
#[test]
fn stderr_is_drained_with_a_fixed_retention_bound() {
    let root = tempfile::tempdir().unwrap();
    let bytes = fixture("created");
    let cli = script(
        root.path(),
        &format!(
            "cat >/dev/null\ni=0\nwhile [ \"$i\" -lt 20000 ]; do printf 'warning\\n' >&2; i=$((i+1)); done\n{}exit 0\n",
            print(&bytes)
        ),
    );
    let mut run =
        WorkerRun::start(&cli, &request(root.path(), RunMode::Create), root.path()).unwrap();
    let completion = finish(&mut run);
    assert_eq!(completion.state, WorkerState::Completed);
    assert_eq!(completion.stderr.len(), eml_app::MAX_STDERR_BYTES);
    assert!(completion.stderr_truncated);
}
