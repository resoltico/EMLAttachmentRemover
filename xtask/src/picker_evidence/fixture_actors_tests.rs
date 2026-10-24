use super::fixture_actors::*;
use super::*;
use serde_json::{Value, json};

fn check(value: &Value, scenario: &str) -> Result<()> {
    check_fixture_actors(value, &fixture_filesystem_test_value(), scenario)
}
fn change_ack(value: &mut Value, index: usize, change: impl FnOnce(&mut Value)) {
    let record = &mut value["records"][index];
    let mut ack: Value = serde_json::from_str(record["document"].as_str().unwrap()).unwrap();
    change(&mut ack);
    let document = serde_json::to_string(&ack).unwrap();
    record["identity"] = serde_json::to_value(ByteIdentity::of(document.as_bytes())).unwrap();
    record["document"] = document.into();
}

#[test]
fn literal_case_sequences_require_owned_closed_effects_and_settlement() {
    for case in [
        "workflow",
        "picker",
        "owner-control",
        "pagination",
        "appearance",
        "stop",
        "force-stop",
        "active-quit",
        "published-error",
        "blocked-processing-preflight",
        "blocked-processing-readback",
        "blocked-processing-stage-write",
        "blocked-processing-file-sync",
        "blocked-processing-publication",
        "blocked-processing-namespace-sync",
        "blocked-export-preflight",
        "blocked-export-readback",
        "blocked-export-stage-write",
        "blocked-export-file-sync",
        "blocked-export-publication",
        "blocked-export-namespace-sync",
        "export-publication",
        "export-writer-block",
    ] {
        check(&test_value(case), case).unwrap();
    }
    let good = test_value("workflow");
    for field in [
        "state",
        "scenario",
        "actor_uid",
        "root",
        "root_address_verified",
        "descriptor_closed",
        "generation",
        "records",
        "unexecuted",
        "failure",
    ] {
        let mut missing = good.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(check(&missing, "workflow").is_err(), "missing {field}");
    }
    for (path, value) in [
        ("/root/inode", json!(999)),
        ("/actor_uid", json!(502)),
        ("/root_address_verified", json!(false)),
        ("/descriptor_closed", json!(false)),
        ("/generation", json!(1)),
        ("/state", json!("failed")),
        ("/scenario", json!("picker")),
        (
            "/unexecuted",
            json!([{"sequence":2,"actor":"occupy-export","request_sha256":"a".repeat(64),"reason":"relay ended"}]),
        ),
        ("/records/0/ack_published", json!(false)),
        ("/records/0/ack_pending_cleanup/state", json!("unknown")),
        ("/records/0/identity/sha256", json!("a".repeat(64))),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(path).unwrap() = value;
        assert!(check(&bad, "workflow").is_err(), "{path}");
    }
    assert!(check(&test_value("picker"), "workflow").is_err());
}

#[test]
fn symlink_permissions_are_observed_while_type_and_fixed_target_are_required() {
    for mode in [0o120755, 0o120777] {
        let mut value = test_value("export-publication");
        change_ack(&mut value, 3, |ack| {
            ack["outputs"][0]["identity"]["mode"] = json!(mode)
        });
        check(&value, "export-publication").unwrap();
    }
    for (path, value) in [
        ("/outputs/0/identity/mode", json!(0o100755)),
        (
            "/outputs/0/symlink_target",
            json!("/outside/occupied-report.json"),
        ),
    ] {
        let mut bad = test_value("export-publication");
        change_ack(&mut bad, 3, |ack| *ack.pointer_mut(path).unwrap() = value);
        assert!(check(&bad, "export-publication").is_err());
    }
}

#[test]
fn rehashed_acknowledgements_cannot_replace_independent_bytes_or_generation_facts() {
    let good = test_value("export-publication");
    for (index, path, value) in [
        (
            1,
            "/outputs/0/bytes",
            json!(ByteIdentity::of(b"forged sentinel")),
        ),
        (
            4,
            "/outputs/0/bytes",
            json!(ByteIdentity::of(b"export:after-publication\n")),
        ),
        (5, "/worker/operation_id", json!(0)),
        (6, "/worker/operation_id", json!(777)),
        (6, "/worker/pid", json!(777)),
        (1, "/request_sha256", json!("a".repeat(64))),
        (7, "/worker/pid_absent", json!(false)),
        (7, "/worker/absence_errno", json!(1)),
        (7, "/outputs/0/identity/inode", json!(999)),
        (7, "/outputs/1/relative_path", json!("another-control")),
        (8, "/generation_before", json!(0)),
        (9, "/worker/control_identity/inode", json!(999)),
        (10, "/generation_after", json!(1)),
        (11, "/outputs/0/relative_path", json!("../outside")),
        (12, "/worker/phase", json!("publication")),
        (13, "/pending_cleanup/state", json!("unknown")),
        (15, "/scenario", json!("workflow")),
        (15, "/state", json!("failed")),
    ] {
        let mut bad = good.clone();
        change_ack(&mut bad, index, |ack| {
            *ack.pointer_mut(path).unwrap() = value
        });
        assert!(check(&bad, "export-publication").is_err(), "{index}:{path}");
    }
    let mut alias = test_value("workflow");
    change_ack(&mut alias, 0, |ack| {
        ack["source"]["before"]["bytes"] = json!(ByteIdentity::of(b"forged original"));
        ack["source"]["after"]["bytes"] = ack["source"]["before"]["bytes"].clone();
    });
    assert!(check(&alias, "workflow").is_err());
    let mut bad = good.clone();
    bad["records"].as_array_mut().unwrap().swap(5, 6);
    assert!(check(&bad, "export-publication").is_err());
    let mut bad = good;
    bad["records"].as_array_mut().unwrap().pop();
    assert!(check(&bad, "export-publication").is_err());
}

#[test]
fn acknowledgement_objects_reject_duplicates_arrays_omissions_and_unbound_payloads() {
    let good = test_value("workflow");
    let raw = good["records"][0]["document"].as_str().unwrap();
    let ack: Value = serde_json::from_str(raw).unwrap();
    for field in ack.as_object().unwrap().keys() {
        let mut bad = good.clone();
        change_ack(&mut bad, 0, |v| {
            v.as_object_mut().unwrap().remove(field);
        });
        assert!(check(&bad, "workflow").is_err(), "{field}");
    }
    for path in [
        "/root",
        "/outputs/0",
        "/outputs/0/identity",
        "/outputs/0/bytes",
        "/source",
        "/source/before",
        "/source/after/identity",
        "/pending_cleanup",
    ] {
        let mut bad = good.clone();
        change_ack(&mut bad, 0, |v| *v.pointer_mut(path).unwrap() = json!([]));
        assert!(check(&bad, "workflow").is_err(), "{path}");
    }
    let mut duplicate = good.clone();
    let document = raw.replacen("{", "{\"schema\":1,", 1);
    duplicate["records"][0]["identity"] = json!(ByteIdentity::of(document.as_bytes()));
    duplicate["records"][0]["document"] = document.into();
    assert!(check(&duplicate, "workflow").is_err());
    let mut oversized = good;
    oversized["records"][0]["document"] = json!(" ".repeat(16385));
    assert!(check(&oversized, "workflow").is_err());
    let retained: FixtureActors =
        serde_json::from_value(test_value("blocked-processing-preflight")).unwrap();
    assert!(
        retained
            .require_output(
                "retain-child-observation",
                &ByteIdentity::of(b"unrelated receipt")
            )
            .is_err()
    );
    let mut owner = json!({"fixture_filesystem":fixture_filesystem_test_value(),"fixture_output_relay":fixture_output_relay_test_value(),"fixture_actors":test_value("picker"),"panel_path_clipboard":panel_path_clipboard_test_value(true),"failure":null,"source_check_failure":null,"timed_out":false,"cancelled":false,"runner_reaped":true,"runner_status":0,"owned_app_terminated":true,"forced_app_cleanup":false,"originals_captured":4,"originals_unchanged":true,"owned_pasteboard_released":true,"unexpected_application_pids":[],"ownership_receipt":{"pid":42,"launch_date":1.0,"bundle_path":"/owned/app","pasteboard":"private"}});
    project_lifetime_owner(&owner).unwrap();
    owner.as_object_mut().unwrap().remove("fixture_actors");
    assert!(project_lifetime_owner(&owner).is_err());
}

#[test]
fn common_owner_relay_requires_actual_eof_drain_and_no_failure() {
    let good = fixture_output_relay_test_value();
    check_fixture_output_relay(&good).unwrap();
    for (path, value) in [
        ("/enabled", json!(false)),
        ("/eof_observed", json!(false)),
        ("/drain_within_deadline", Value::Null),
        ("/drain_within_deadline", json!(false)),
        ("/failure", json!("malformed request")),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(path).unwrap() = value;
        assert!(check_fixture_output_relay(&bad).is_err(), "{path}");
    }
    for field in [
        "enabled",
        "eof_observed",
        "drain_within_deadline",
        "failure",
    ] {
        let mut bad = good.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(check_fixture_output_relay(&bad).is_err(), "missing {field}");
    }
}

#[test]
fn baseline_worker_observations_need_no_payload_but_required_actor_payloads_cannot_be_omitted() {
    for case in ["stop", "force-stop", "active-quit", "published-error"] {
        let good = test_value(case);
        check(&good, case).unwrap();
        let owner: FixtureActors = serde_json::from_value(good.clone()).unwrap();
        // Independent shape of finish()'s existing native worker observation:
        // these scenarios never publish a child UI or report payload actor.
        let worker = json!({"case":case,"worker_pid":1234,"worker_absent":true,"first_interrupt_micros":1,"second_interrupt_micros":null,"independent_copy_golden":true});
        owner.check_payloads(&worker).unwrap();
        let mut bad = good.clone();
        bad["state"] = json!("complete");
        assert!(check(&bad, case).is_err());
        let mut bad = good;
        bad["records"] = test_value("workflow")["records"].clone();
        assert!(check(&bad, case).is_err());
    }
    for case in [
        "blocked-processing-preflight",
        "blocked-processing-readback",
        "blocked-processing-stage-write",
        "blocked-processing-file-sync",
        "blocked-processing-publication",
        "blocked-processing-namespace-sync",
        "blocked-export-preflight",
        "blocked-export-readback",
        "blocked-export-stage-write",
        "blocked-export-file-sync",
        "blocked-export-publication",
        "blocked-export-namespace-sync",
        "export-writer-block",
    ] {
        let value = test_value(case);
        check(&value, case).unwrap();
        let owner: FixtureActors = serde_json::from_value(value).unwrap();
        owner
            .check_payloads(&json!({"ui_receipt":{"contents":"independent observation bytes"}}))
            .unwrap();
        assert!(
            owner.check_payloads(&json!({"worker_pid":1234})).is_err(),
            "missing {case}"
        );
        assert!(
            owner
                .check_payloads(&json!({"ui_receipt":{"contents":"wrong bytes"}}))
                .is_err(),
            "changed {case}"
        );
    }
    let mut unknown: FixtureActors = serde_json::from_value(test_value("stop")).unwrap();
    unknown.scenario = "foreign".into();
    assert!(unknown.check_payloads(&json!({})).is_err());
}

#[test]
fn exact_payload_requests_and_actual_owner_size_cannot_be_replaced_by_digests() {
    let good = test_value("blocked-processing-preflight");
    let actual: FixtureActors = serde_json::from_value(good.clone()).unwrap();
    let document = json!({"ui_receipt":{"contents":"independent observation bytes"}});
    actual.check_payloads(&document).unwrap();
    let mut changed = good;
    change_ack(&mut changed, 1, |ack| {
        ack["request_sha256"] = json!("a".repeat(64))
    });
    check(&changed, "blocked-processing-preflight").unwrap();
    let changed: FixtureActors = serde_json::from_value(changed).unwrap();
    assert!(changed.check_payloads(&document).is_err());
    let mut owner = json!({"fixture_filesystem":fixture_filesystem_test_value(),"fixture_output_relay":fixture_output_relay_test_value(),"fixture_actors":test_value("picker"),"panel_path_clipboard":panel_path_clipboard_test_value(true),"failure":null,"source_check_failure":null,"timed_out":false,"cancelled":false,"runner_reaped":true,"runner_status":0,"owned_app_terminated":true,"forced_app_cleanup":false,"originals_captured":4,"originals_unchanged":true,"owned_pasteboard_released":true,"unexpected_application_pids":[],"ownership_receipt":{"pid":42,"launch_date":1.0,"bundle_path":"/owned/app","pasteboard":"private"}});
    project_lifetime_owner(&owner).unwrap();
    owner["unprojected_observation"] = json!("x".repeat(MAX_OWNER_BYTES));
    assert!(project_lifetime_owner(&owner).is_err());
}

#[test]
fn internally_consistent_rehashed_workers_must_match_independent_native_matrix() {
    let matrix = json!({"ui_receipt":{"contents":"independent observation bytes"},"matrix":{"expected_document":ByteIdentity::of(b"abc"),"expected_document_contents":"abc","controls":[{"case":"racing-winner","operation_id":90,"worker_pid":140,"phase":"publication","worker_absent":true},{"case":"pre-edge-stop","operation_id":91,"worker_pid":141,"phase":"publication","worker_absent":true},{"case":"post-edge-stop","operation_id":92,"worker_pid":142,"phase":"namespace-sync","worker_absent":true}]}});
    let good = test_value("export-publication");
    let valid: FixtureActors = serde_json::from_value(good.clone()).unwrap();
    valid.check_payloads(&matrix).unwrap();
    for (parent, field) in [
        ("", "ui_receipt"),
        ("matrix", "expected_document_contents"),
        ("matrix", "expected_document"),
        ("matrix", "controls"),
    ] {
        let mut missing = matrix.clone();
        let object = if parent.is_empty() {
            &mut missing
        } else {
            &mut missing[parent]
        };
        object.as_object_mut().unwrap().remove(field);
        assert!(
            valid.check_payloads(&missing).is_err(),
            "missing {parent}/{field}"
        );
    }
    for (path, value) in [
        ("/ui_receipt/contents", json!("wrong UI")),
        ("/matrix/expected_document_contents", json!("wrong report")),
    ] {
        let mut changed = matrix.clone();
        *changed.pointer_mut(path).unwrap() = value;
        assert!(valid.check_payloads(&changed).is_err(), "{path}");
    }
    let mut root_alias = good.clone();
    for index in [5, 6, 7] {
        change_ack(&mut root_alias, index, |ack| {
            ack["worker"]["control_identity"] = ack["root"].clone();
            if index == 7 {
                ack["outputs"][0]["identity"] = ack["root"].clone();
            }
        });
    }
    assert!(check(&root_alias, "export-publication").is_err());
    for change_operation in [false, true] {
        let mut forged = good.clone();
        for (index, kind, operand) in [
            (5, "race-export-winner", "777"),
            (6, "release-operation", "777"),
            (7, "archive-control", "racing-winner:777"),
        ] {
            change_ack(&mut forged, index, |ack| {
                ack["worker"]["pid"] = json!(777);
                if change_operation {
                    ack["worker"]["operation_id"] = json!(777);
                    ack["request_sha256"] = json!(
                        ByteIdentity::of(
                            format!("EML_FIXTURE_ACTOR {} {kind} {operand}\n", index + 1)
                                .as_bytes()
                        )
                        .sha256
                    );
                }
            });
        }
        check(&forged, "export-publication").unwrap();
        let forged: FixtureActors = serde_json::from_value(forged).unwrap();
        assert!(forged.check_payloads(&matrix).is_err());
    }
}

#[test]
fn native_owner_bytes_reject_duplicates_before_value_projection() {
    owner_document(br#"{"fixture_actors":{"state":"unused"}}"#).unwrap();
    for bytes in [
        br#"{"fixture_actors":{},"fixture_actors":{}}"#.as_slice(),
        br#"{"fixture_actors":{"state":"unused","state":"complete"}}"#.as_slice(),
        br#"[]"#.as_slice(),
        br#"{"fixture_output_relay":{"enabled":true,"enabled":false}}"#.as_slice(),
    ] {
        assert!(owner_document(bytes).is_err());
    }
    assert!(owner_document(&vec![b' '; MAX_OWNER_BYTES + 1]).is_err());
}
