//! Actual owned-worker completions survive private session storage without loss.
#![cfg(unix)]
use eml_app::{RequestSnapshot, StoredReport, WorkerCompletion, WorkerRun};
use eml_core::RunMode;
use std::{
    fs,
    path::Path,
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
fn run(bytes: &str, exit: i32, mode: RunMode) -> WorkerCompletion {
    let root = tempfile::tempdir().unwrap();
    let quoted = bytes.replace('\'', "'\\''");
    fs::write(
        root.path().join("fixture.sh"),
        format!("#!/bin/sh\ncat >/dev/null\nprintf '%s' '{quoted}'\nprintf '%s' 'retained diagnostic' >&2\nexit {exit}\n"),
    )
    .unwrap();
    let request = RequestSnapshot::new(vec![root.path().join("a.eml")], None, mode).unwrap();
    let mut worker = WorkerRun::start(
        Path::new(env!("CARGO_BIN_EXE_eml-app-cli-fixture")),
        &request,
        root.path(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(completion) = worker.poll().unwrap() {
            return completion;
        }
        assert!(
            Instant::now() < deadline,
            "owned storage fixture did not settle"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn observations(completion: &WorkerCompletion) -> serde_json::Value {
    completion
        .report
        .with_report(|report| {
            serde_json::json!({
                "start": report.start(), "items": report.items(), "summary": report.summary(),
                "failure": report.failure(), "unreported": report.unreported_item_count()
            })
        })
        .unwrap()
}
#[test]
fn actual_complete_refused_partial_and_malformed_streams_round_trip_exactly() {
    let created = fixture("created");
    let start = created.lines().next().unwrap();
    let partial = created.lines().take(2).collect::<Vec<_>>().join("\n") + "\n";
    for (bytes, exit, mode) in [
        (created.clone(), 0, RunMode::Create),
        (fixture("prepublication-failure"), 1, RunMode::Create),
        (fixture("planned-create-unix"), 0, RunMode::DryRun),
        (partial, 1, RunMode::Create),
        (format!("{start}\n{{}}\n"), 1, RunMode::Create),
        (format!("{start}\n\n"), 1, RunMode::Create),
        (created.clone() + "unfinished-tail", 0, RunMode::Create),
        (String::new(), 2, RunMode::Create),
        (created, 0, RunMode::DryRun),
    ] {
        let completion = run(&bytes, exit, mode);
        let expected = observations(&completion);
        let stored = StoredReport::write(&completion, mode).unwrap();
        assert_eq!(stored.state(), completion.state);
        assert_eq!(stored.failure(), completion.failure);
        let loaded = stored.load().unwrap();
        assert_eq!(loaded.jsonl, completion.jsonl);
        assert_eq!(loaded.stderr, completion.stderr);
        assert_eq!(loaded.stderr_truncated, completion.stderr_truncated);
        assert_eq!(loaded.exit, completion.exit);
        assert_eq!(loaded.state, completion.state);
        assert_eq!(loaded.failure, completion.failure);
        assert_eq!(observations(&loaded), expected);
        drop(completion);
        assert_eq!(observations(&stored.load().unwrap()), expected);
    }
}
#[test]
fn non_replayable_observations_remain_in_memory_instead_of_being_evicted() {
    let mut completion = run(&fixture("created"), 0, RunMode::Create);
    let expected = observations(&completion);
    // A reader panic can lose its raw buffer after previously accepted events.
    completion.jsonl = Vec::new().into();
    assert!(StoredReport::write(&completion, RunMode::Create).is_err());
    assert_eq!(observations(&completion), expected);
}
