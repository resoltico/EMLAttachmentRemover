//! Shared independent protocol cases through real Rust framing and reduction.
use eml_app::{FrameError, FrameSplitter, ProtocolFailure, RunReport};
impl From<FrameError> for Failure {
    fn from(error: FrameError) -> Self {
        Self::Framing(error)
    }
}
#[derive(Debug)]
enum Failure {
    Framing(FrameError),
    Protocol(ProtocolFailure),
}
fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/contract/protocol/{name}.jsonl"));
    std::fs::read_to_string(path)
        .unwrap()
        .replace("@VERSION@", "test-version")
        .into_bytes()
}
fn decode(bytes: &[u8], split: usize, exit: i32) -> Result<RunReport, Failure> {
    let mut report = RunReport::new("test-version");
    let mut framing = FrameSplitter::default();
    for chunk in [&bytes[..split], &bytes[split..]] {
        framing.append(chunk, |frame| {
            report.accept_frame(frame).map_err(Failure::Protocol)
        })?;
    }
    let tail = framing.finish();
    report.finish(Some(exit), tail).map_err(Failure::Protocol)?;
    Ok(report)
}
#[test]
fn all_shared_scenarios_pass_at_every_byte_boundary_with_explicit_exit_and_item_oracles() {
    let cases = [
        ("planned-create-unix", 0, 1),
        ("planned-create-windows", 0, 1),
        ("created", 0, 1),
        ("up-to-date", 0, 1),
        ("planned-existing", 0, 1),
        ("no-inputs", 1, 0),
        ("prepublication-failure", 1, 1),
        ("existing-sync-failure", 1, 1),
        ("published-with-error", 1, 1),
        ("unknown-publication", 1, 1),
        ("failed-root-admission", 1, 1),
        ("collection-limit", 1, 0),
        ("started-preedge-cancellation", 130, 1),
        ("postedge-cancellation", 130, 1),
        ("native-control-name", 1, 1),
        ("created-limited-sync", 0, 1),
        ("partial-namespace-observation", 1, 1),
        ("unattempted-required-edge", 1, 1),
        ("complete-cleanup-with-error", 1, 1),
        ("created-attachment", 0, 1),
        ("up-to-date-attachment", 0, 1),
        ("incomplete-item-failure", 1, 1),
    ];
    assert_eq!(cases.len(), 22);
    for (name, exit, items) in cases {
        let bytes = fixture(name);
        for boundary in 0..=bytes.len() {
            let report = decode(&bytes, boundary, exit)
                .unwrap_or_else(|error| panic!("{name} at {boundary}: {error:?}"));
            assert_eq!(report.items().len(), items);
        }
    }
}
#[test]
fn known_items_survive_malformed_later_records_and_export_unknown_summary() {
    let bytes = fixture("created-attachment");
    let mut frames = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty());
    let mut report = RunReport::new("test-version");
    report.accept_frame(frames.next().unwrap()).unwrap();
    report.accept_frame(frames.next().unwrap()).unwrap();
    assert_eq!(
        report.accept_frame(b"{\"type\":\"unknown\"}"),
        Err(ProtocolFailure::MalformedRecord)
    );
    assert_eq!(
        report.accept_frame(frames.next().unwrap()),
        Err(ProtocolFailure::MalformedRecord)
    );
    assert_eq!(report.items().len(), 1);
    assert_eq!(report.saved_bytes(0), Some(515));
    assert_eq!(report.items()[0].removed[0].bytes, 507);
    assert_eq!(
        report.finish(Some(0), false),
        Err(ProtocolFailure::MalformedRecord)
    );
    let exported: serde_json::Value =
        serde_json::from_slice(&report.technical_report().unwrap()).unwrap();
    assert!(exported["summary"].is_null());
    assert_eq!(exported["protocol_failure"], "malformed_record");
    assert!(
        !String::from_utf8(report.technical_report().unwrap())
            .unwrap()
            .contains("Hello Bob")
    );
}
#[test]
fn summary_exit_partial_tail_and_postsummary_records_cannot_change_accepted_items() {
    let bytes = fixture("created");
    assert!(matches!(
        decode(&bytes, 0, 1),
        Err(Failure::Protocol(ProtocolFailure::ExitDisagreement))
    ));
    let mut partial = bytes.clone();
    partial.extend_from_slice(b"unfinished");
    assert!(matches!(
        decode(&partial, 0, 0),
        Err(Failure::Protocol(ProtocolFailure::IncompleteTail))
    ));
    let mut extra = bytes.clone();
    extra.extend_from_slice(b"{}\n");
    assert!(matches!(
        decode(&extra, 0, 0),
        Err(Failure::Protocol(ProtocolFailure::MalformedRecord))
    ));
    let mut framing = FrameSplitter::default();
    let mut report = RunReport::new("test-version");
    framing
        .append::<FrameError>(&bytes, |frame| {
            report.accept_frame(frame).unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        report.finish(None, false),
        Err(ProtocolFailure::ExitDisagreement)
    );
    assert_eq!(report.items().len(), 1);
}
#[test]
fn changed_receipts_counts_closed_identity_and_index_order_are_rejected() {
    let frames: Vec<serde_json::Value> = String::from_utf8(fixture("created"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for pointer in [
        "/publication/candidate_identity/kind",
        "/publication/address",
        "/publication/file_sync/state",
        "/publication/namespace_sync/completed_edges",
        "/publication/cleanup/native_code",
    ] {
        let mut item = frames[1].clone();
        *item.pointer_mut(pointer).unwrap() = match pointer {
            "/publication/candidate_identity/kind" => serde_json::json!("unknown_identity"),
            "/publication/address" => serde_json::json!("changed"),
            "/publication/file_sync/state" => serde_json::json!("failed"),
            "/publication/namespace_sync/completed_edges" => serde_json::json!(0),
            _ => serde_json::json!(5),
        };
        let mut report = RunReport::new("test-version");
        report
            .accept_frame(&serde_json::to_vec(&frames[0]).unwrap())
            .unwrap();
        assert_eq!(
            report.accept_frame(&serde_json::to_vec(&item).unwrap()),
            Err(ProtocolFailure::InvalidReceipt),
            "{pointer}"
        );
        assert!(report.items().is_empty());
    }
    let mut report = RunReport::new("test-version");
    for frame in &frames[..2] {
        report
            .accept_frame(&serde_json::to_vec(frame).unwrap())
            .unwrap();
    }
    assert_eq!(
        report.accept_frame(&serde_json::to_vec(&frames[1]).unwrap()),
        Err(ProtocolFailure::InvalidOrder)
    );
    assert_eq!(report.items().len(), 1);
    let mut wrong = frames[2].clone();
    wrong["counts"]["created"] = serde_json::json!(0);
    let mut report = RunReport::new("test-version");
    for frame in &frames[..2] {
        report
            .accept_frame(&serde_json::to_vec(frame).unwrap())
            .unwrap();
    }
    assert_eq!(
        report.accept_frame(&serde_json::to_vec(&wrong).unwrap()),
        Err(ProtocolFailure::InvalidCounts)
    );
}
#[test]
fn a_framing_failure_is_observable_independently_of_a_protocol_failure() {
    let error = decode(b"\n", 0, 0).err().unwrap();
    assert!(matches!(error, Failure::Framing(FrameError::EmptyRecord)));
}

fn paths_item(
    source: (&str, &[u8], &str),
    destination: (&str, &[u8], &str),
) -> Vec<serde_json::Value> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut frames: Vec<serde_json::Value> = String::from_utf8(fixture("prepublication-failure"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for (key, (display, raw, encoding)) in [("source", source), ("destination", destination)] {
        frames[1][key] = serde_json::json!({"display":display,"native":{"encoding":encoding,"base64":STANDARD.encode(raw)}});
    }
    frames
}
fn accept_item(
    frames: &[serde_json::Value],
    mut report: RunReport,
) -> Result<RunReport, ProtocolFailure> {
    for frame in &frames[..2] {
        report.accept_frame(&serde_json::to_vec(frame).unwrap())?;
    }
    Ok(report)
}
#[test]
fn report_paths_accept_derived_lengths_and_lossy_native_representations() {
    let derived = vec![b'a'; 32_769];
    let frames = paths_item(("a", &derived, "unix-bytes"), ("b", b"b", "unix-bytes"));
    assert!(accept_item(&frames, RunReport::new("test-version")).is_ok());
    // A replacement scalar can represent a single invalid byte or its literal
    // three-byte UTF8 spelling. Both are valid producer observations.
    for raw in [b"\xff".as_slice(), "\u{fffd}".as_bytes()] {
        let frames = paths_item(("\u{fffd}", raw, "unix-bytes"), ("b", b"b", "unix-bytes"));
        assert!(accept_item(&frames, RunReport::new("test-version")).is_ok());
    }
    let frames = paths_item(
        ("\u{fffd}", &[0, 0xd8], "windows-utf16le"),
        ("b", &[b'b', 0], "windows-utf16le"),
    );
    assert!(accept_item(&frames, RunReport::new("test-version")).is_ok());
}
#[test]
fn report_path_budget_charges_each_operand_maximum_at_exact_boundary() {
    const LIMIT: usize = 8 * 1024 * 1024;
    // One native-heavy operand and one display-heavy operand: summing separate
    // native/display totals would incorrectly admit this one-byte overflow.
    let native = vec![b'a'; LIMIT / 2];
    for (extra, expected) in [(0, true), (1, false)] {
        let display = "b".repeat(LIMIT / 2 + extra);
        let frames = paths_item(("a", &native, "unix-bytes"), (&display, b"b", "unix-bytes"));
        let result = accept_item(&frames, RunReport::new("test-version"));
        assert_eq!(result.is_ok(), expected);
        if !expected {
            assert!(matches!(result, Err(ProtocolFailure::ResourceLimit)));
        }
    }
}
#[test]
fn desktop_worker_rejects_dry_run_and_foreign_native_operands() {
    let dry: serde_json::Value = serde_json::from_slice(
        fixture("planned-create-unix")
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        RunReport::for_native_worker("test-version")
            .accept_frame(&serde_json::to_vec(&dry).unwrap()),
        Err(ProtocolFailure::IncompatibleHelper)
    );
    let foreign = if cfg!(windows) {
        ("unix-bytes", b"a".as_slice())
    } else {
        ("windows-utf16le", b"a\0".as_slice())
    };
    let frames = paths_item(("a", foreign.1, foreign.0), ("b", foreign.1, foreign.0));
    assert!(matches!(
        accept_item(&frames, RunReport::for_native_worker("test-version")),
        Err(ProtocolFailure::IncompatibleHelper)
    ));
}
