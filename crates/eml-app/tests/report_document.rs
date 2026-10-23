//! Strict versioned metadata report validation and public-contract separation.
use eml_app::{RequestSnapshot, RunQueue, RunReport, WorkerCompletion};
fn completion() -> WorkerCompletion {
    WorkerCompletion {
        operation_id: eml_protocol::desktop::OperationId::new(1).unwrap(),
        exit_code: Some(0),
        signal: None,
        input_may_have_been_sent: true,
        settled: true,
        reusable: true,
        failure: None,
        faults: Vec::new(),
        stderr: Vec::new(),
        stderr_truncated: false,
        interrupt_calls_succeeded: 0,
        first_interrupt_observed: false,
    }
}
fn document(fixture: Option<&str>) -> Vec<u8> {
    let mut report = RunReport::new(eml_core::product::VERSION);
    if let Some(name) = fixture {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../tests/contract/protocol/{name}.jsonl")),
        )
        .unwrap()
        .replace("@VERSION@", eml_core::product::VERSION);
        for frame in text.lines() {
            report.accept_frame(frame.as_bytes()).unwrap();
        }
        report.finish(Some(0), false).unwrap();
    }
    let path = std::env::current_dir().unwrap().join("absent.eml");
    let mut queue = RunQueue::default();
    let id = queue
        .enqueue(RequestSnapshot::new(vec![path], None).unwrap())
        .unwrap();
    let mut bytes = Vec::new();
    report
        .write_document(id, &completion(), &mut bytes)
        .unwrap();
    bytes
}
#[test]
fn empty_launch_failure_and_complete_accepted_reports_are_exportable() {
    for fixture in [None, Some("created-attachment")] {
        let bytes = document(fixture);
        eml_app::report_document::validate(&bytes).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("Hello Bob"));
    }
}
#[test]
fn strict_report_rejects_nested_unknown_duplicate_missing_and_array_fields() {
    let bytes = document(Some("created-attachment"));
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for pointer in [
        "",
        "/start",
        "/items/0",
        "/items/0/source",
        "/items/0/source/native",
        "/items/0/publication",
        "/items/0/publication/cleanup",
        "/items/0/removed/0",
        "/summary",
        "/summary/counts",
        "/worker",
    ] {
        let mut changed = original.clone();
        changed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown_report_field".into(), true.into());
        assert!(
            eml_app::report_document::validate(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "unknown {pointer}"
        );
    }
    for pointer in [
        "/items/0/publication/candidate_identity",
        "/items/0/publication/file_sync",
        "/items/0/publication/namespace_sync",
    ] {
        let mut changed = original.clone();
        if let Some(map) = changed.pointer_mut(pointer).and_then(|v| v.as_object_mut()) {
            map.insert("unknown_nested".into(), true.into());
            assert!(
                eml_app::report_document::validate(&serde_json::to_vec(&changed).unwrap()).is_err(),
                "nullable nested {pointer}"
            );
        }
    }
    for field in [
        "schema",
        "schema_version",
        "version",
        "build",
        "run_id",
        "start",
        "items",
        "summary",
        "protocol_failure",
        "worker",
        "terminal",
    ] {
        let mut changed = original.clone();
        changed.as_object_mut().unwrap().remove(field);
        assert!(
            eml_app::report_document::validate(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "missing {field}"
        );
    }
    let text = String::from_utf8(bytes).unwrap();
    for bad in [
        text.replacen(
            "\"schema\":",
            "\"schema\":\"eml-technical-report\",\"schema\":",
            1,
        ),
        format!("{text} null"),
        text.replace("\"signal\":null,", ""),
        text.replace("\"settled\":true", "\"settled\":false"),
    ] {
        assert!(
            eml_app::report_document::validate(bad.as_bytes()).is_err(),
            "{bad}"
        );
    }
}
#[test]
fn public_event_decoder_retains_additive_fields_while_report_is_strict() {
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/contract/protocol/created-attachment.jsonl"),
    )
    .unwrap()
    .replace("@VERSION@", eml_core::product::VERSION);
    let mut report = RunReport::new(eml_core::product::VERSION);
    for line in raw.lines() {
        let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
        value["additive"] = true.into();
        report
            .accept_frame(&serde_json::to_vec(&value).unwrap())
            .unwrap();
    }
    report.finish(Some(0), false).unwrap();
}

#[test]
fn terminal_kind_never_elevates_missing_protocol_or_process_facts() {
    let empty: serde_json::Value = serde_json::from_slice(&document(None)).unwrap();
    let complete: serde_json::Value =
        serde_json::from_slice(&document(Some("created-attachment"))).unwrap();
    let mut value = empty;
    value["terminal"] = "completed".into();
    assert!(eml_app::report_document::validate(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut value = complete.clone();
    value["protocol_failure"] = "malformed_record".into();
    assert!(eml_app::report_document::validate(&serde_json::to_vec(&value).unwrap()).is_err());
    for (pointer, replacement) in [
        ("/worker/signal", serde_json::json!(9)),
        ("/worker/failure", serde_json::json!("transport")),
        ("/worker/failure", serde_json::json!("unverified")),
        ("/terminal", serde_json::json!({"completed":null})),
    ] {
        let mut value = complete.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            eml_app::report_document::validate(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{pointer}"
        );
    }
    let mut value = complete.clone();
    value.as_object_mut().unwrap().remove("terminal");
    assert!(eml_app::report_document::validate(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut value = complete;
    value["worker"]["failure"] = "transport".into();
    value["terminal"] = "transport_failed".into();
    eml_app::report_document::validate(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(value["items"][0]["status"], "created");
}
#[test]
fn lifecycle_escaped_diagnostics_and_all_required_nulls_are_preserved() {
    let mut value: serde_json::Value = serde_json::from_slice(&document(None)).unwrap();
    value["worker"]["faults"] = serde_json::json!(["line\nwith \\\"quotes\\\""]);
    eml_app::report_document::validate(&serde_json::to_vec(&value).unwrap()).unwrap();
    for key in [
        "operation_id",
        "exit_code",
        "signal",
        "input_may_have_been_sent",
        "settled",
        "reusable",
        "failure",
        "faults",
        "stderr",
        "stderr_truncated",
        "interrupt_calls_succeeded",
        "first_interrupt_observed",
    ] {
        let mut missing = value.clone();
        missing["worker"].as_object_mut().unwrap().remove(key);
        assert!(
            eml_app::report_document::validate(&serde_json::to_vec(&missing).unwrap()).is_err(),
            "missing {key}"
        );
    }
    value["worker"]["faults"] = serde_json::json!(vec!["x"; 33]);
    assert!(eml_app::report_document::validate(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn nullable_error_and_nested_record_arrays_cannot_bypass_strict_structure() {
    let original: serde_json::Value =
        serde_json::from_slice(&document(Some("created-attachment"))).unwrap();
    let mut with_error = original.clone();
    with_error["items"][0]["error"] = serde_json::json!({"code":"READ_FAILED","phase":"read","detail":"bounded failure","section":null});
    eml_app::report_document::validate(&serde_json::to_vec(&with_error).unwrap()).unwrap();
    with_error["items"][0]["error"]["ignored"] = true.into();
    assert!(eml_app::report_document::validate(&serde_json::to_vec(&with_error).unwrap()).is_err());
    for pointer in [
        "/items/0/source/native",
        "/items/0/publication/candidate_identity",
        "/items/0/publication/namespace_sync",
        "/items/0/publication/cleanup",
        "/items/0/removed/0",
    ] {
        let mut changed = original.clone();
        let object = changed.pointer_mut(pointer).unwrap();
        let values = object
            .as_object()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        *object = values.into();
        assert!(
            eml_app::report_document::validate(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "{pointer}"
        );
    }
    let text = String::from_utf8(serde_json::to_vec(&original).unwrap()).unwrap();
    let bad = text.replacen(
        "\"required_edges\":1",
        "\"required_edges\":1,\"required_edges\":1",
        1,
    );
    assert!(eml_app::report_document::validate(bad.as_bytes()).is_err());
}
