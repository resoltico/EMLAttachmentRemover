//! Data-only admission controls; these fixtures are not native GUI evidence.
#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn expected() -> Value {
    json!({"source":"a".repeat(40),"target":"x86_64-apple-darwin","run":"12","attempt":"1",
        "workflow_ref":"owner/repo/.github/workflows/ci.yml@refs/pull/1/merge","workflow_sha":"c".repeat(40),
        "archive":{"sha256":"b".repeat(64),"byte_length":100},"reference":{"sha256":"d".repeat(64),"byte_length":50}})
}
fn admits(filter: &str, value: &Value) -> bool {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut child = Command::new("jq")
        .current_dir(root)
        .args(["-e", "-L", ".github/scripts", "--argjson", "e"])
        .arg(expected().to_string())
        .arg(format!("include \"picker-evidence\"; {filter}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(value.to_string().as_bytes());
    let output = child.wait_with_output().unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "jq evaluation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.status.success()
}
fn process(exit: i32) -> Value {
    json!({"exit_code":exit,"signal":null,"cancelled":false,"timed_out":false,"cleanup_complete":true,"unexpected_survivors":0})
}
fn owner() -> Value {
    json!({"failure":null,"source_check_failure":null,"timed_out":false,"cancelled":false,"runner_reaped":true,"runner_status":0,
        "owned_app_terminated":true,"forced_app_cleanup":false,"originals_captured":4,"originals_unchanged":true,
        "owned_pasteboard_released":true,"unexpected_application_pids":[],
        "ownership_receipt":{"pid":42,"launch_date":1.0,"bundle_path":"/owned/app.app"}})
}
fn product() -> Value {
    json!({"stage":"picker_compatibility","source":expected()["source"],"target":expected()["target"],
    "host":{"target":"x86_64-apple-darwin","process_os":"macos","process_architecture":"x86_64","kernel_machine":"x86_64","product_version":"26.6.1"},
    "verification":{"stage":"picker_compatibility","snapshot":expected()["archive"],"picker":{
        "source":expected()["source"],"case":"picker","test_method":"testOwnedNativePickerCompatibility","app":"/owned/app.app",
        "summary":{"totalTestCount":1,"passedTests":1,"failedTests":0,"skippedTests":0,"expectedFailures":0},
        "process":process(0),"lifetime_owner":owner(),"ownership_receipt":owner()["ownership_receipt"],
        "picker_observation":{"schema":1,"scenario":"picker","owned_pid":42,"message":"/owned/one.eml","folder":"/owned/folder-input"},
        "persistent_preferences":{"comparison":{"real_pickers_observed":true,"application_and_other_preferences_unchanged":true}}
    }}})
}
fn known_failure() -> Value {
    let mut result = expected();
    let mut failed_owner = owner();
    failed_owner["failure"] = json!("runner failed");
    failed_owner["runner_status"] = json!(65);
    failed_owner["native_picker_diagnostic_settled"] = json!(true);
    failed_owner["native_picker_owned_process_log"] = json!({"pid":42});
    failed_owner["fixture_output_relay"] =
        json!({"enabled":true,"eof_observed":true,"drain_within_deadline":true,"failure":null});
    result.as_object_mut().unwrap().extend(json!({"stage":"picker_normal_observation","outcome":"known_panel_failure","original_exit_code":1,
        "process":process(1),"owner":failed_owner,
        "xctest_log":"error: -[DesktopInteraction.DesktopInteractionTests testOwnedNativePickerCompatibility] : failed - Native input panel did not appear",
        "owned_log":"Advance to configuration phase semaphore timed out"}).as_object().unwrap().clone());
    result
}
fn probe(healthy: bool) -> Value {
    let id = json!({"path":"/owned/probe.app","signature_display":"Mach-O thin (x86_64) Signature=adhoc",
        "files":{"Contents/Info.plist":{"sha256":"1".repeat(64),"byte_length":10},
            "Contents/MacOS/NativePanelDiagnostic":{"sha256":"2".repeat(64),"byte_length":100},
            "Contents/_CodeSignature/CodeResources":{"sha256":"3".repeat(64),"byte_length":20}}});
    json!({"identity":id,"identity_after":id,"process":process(if healthy {0} else {2}),
        "observation":if healthy {"healthy"} else {"known_panel_failure"},
        "phases":["constructor entry","constructor returned","presentation returned",
            if healthy {"cancel retained panel; visible=true"} else {"cancel retained panel; visible=false"},
            if healthy {"callback response 0"} else {"callback deadline exceeded"}]})
}
fn preparation(outcome: &str, healthy_baseline: bool) -> Value {
    let mut baseline = probe(healthy_baseline);
    baseline.as_object_mut().unwrap().extend(json!({"source":expected()["source"],"run":"12","attempt":"1","entitlements":null,
        "host":{"target":"x86_64-apple-darwin","process_os":"macos","process_architecture":"x86_64","kernel_machine":"x86_64","actual_os":"ProductVersion: 26.6.1\n"}}).as_object().unwrap().clone());
    let mut prep = json!({"stage":"observed","purpose":"picker_qualification_preparation","outcome":outcome,
        "original_failure_remains_authoritative":true,"gui_uid":501,"baseline":baseline});
    if !healthy_baseline {
        prep["unchanged_repeat"] = probe(outcome == "unmodified");
    }
    if outcome == "service_intervention" {
        prep["target"] = json!("gui/501/com.apple.iconservices.iconservicesagent");
        prep["bootout"] = json!({"program":"/bin/launchctl","arguments":["bootout","gui/501/com.apple.iconservices.iconservicesagent"],"process":process(0)});
        prep["intervention_probe"] = probe(true);
    }
    prep
}
fn followup(prep: Value) -> Value {
    let mut record = expected();
    record.as_object_mut().unwrap().extend(json!({"stage":"picker_followup_qualification","normal":known_failure(),"preparation":prep,"product":product()}).as_object().unwrap().clone());
    record
}
fn qualification(prep: Value) -> Value {
    let mode = if prep["outcome"] == "unmodified" {
        "unmodified_followup"
    } else {
        "modified_host"
    };
    let mut record = expected();
    record.as_object_mut().unwrap().extend(json!({"stage":"picker_qualification","mode":mode,"normal":known_failure(),"followup":followup(prep),"product":product()}).as_object().unwrap().clone());
    record
}

#[test]
fn normal_picker_requires_real_case_identity_and_complete_owner_facts() {
    let p = product();
    assert!(admits("product($e)", &p));
    for field in ["case", "test_method"] {
        let mut wrong = p.clone();
        wrong["verification"]["picker"][field] = json!("workflow");
        assert!(!admits("product($e)", &wrong));
    }
    let mut skipped = p.clone();
    skipped["verification"]["picker"]["summary"]["skippedTests"] = json!(1);
    assert!(!admits("product($e)", &skipped));
    let mut forced = p.clone();
    forced["verification"]["picker"]["lifetime_owner"]["forced_app_cleanup"] = json!(true);
    assert!(!admits("product($e)", &forced));
    let mut missing = p;
    missing["verification"]["picker"]["lifetime_owner"]
        .as_object_mut()
        .unwrap()
        .remove("failure");
    assert!(!admits("product($e)", &missing));
    let mut normal = expected();
    normal.as_object_mut().unwrap().extend(json!({"stage":"picker_normal_observation","outcome":"normal_pass","original_exit_code":0,"product":product()}).as_object().unwrap().clone());
    let mut qualification = expected();
    qualification.as_object_mut().unwrap().extend(json!({"stage":"picker_qualification","mode":"normal","normal":normal,"followup":null,"product":product()}).as_object().unwrap().clone());
    assert!(admits("qualification($e)", &qualification));
    qualification["mode"] = json!("modified_host");
    assert!(!admits("qualification($e)", &qualification));
}

#[test]
fn followup_requires_original_failure_same_run_and_actual_product() {
    assert!(admits("known_failure($e)", &known_failure()));
    let mut unrelated = known_failure();
    unrelated["xctest_log"] = json!("Go To sheet did not close");
    assert!(!admits("known_failure($e)", &unrelated));
    for prep in [
        preparation("unmodified", true),
        preparation("unmodified", false),
        preparation("service_intervention", false),
    ] {
        let qualified = qualification(prep);
        assert!(admits("qualification($e)", &qualified));
        for path in [
            "/normal",
            "/followup/normal",
            "/product",
            "/followup/product",
        ] {
            let mut missing = qualified.clone();
            *missing.pointer_mut(path).unwrap() = Value::Null;
            assert!(
                !admits("qualification($e)", &missing),
                "accepted missing {path}"
            );
        }
        for field in [
            "source",
            "target",
            "run",
            "attempt",
            "workflow_ref",
            "workflow_sha",
            "archive",
            "reference",
        ] {
            for prefix in ["", "/normal", "/followup", "/followup/normal"] {
                let mut wrong = qualified.clone();
                wrong.pointer_mut(prefix).unwrap()[field] = json!("wrong identity");
                assert!(
                    !admits("qualification($e)", &wrong),
                    "accepted {prefix}/{field}"
                );
            }
        }
        for path in [
            "/followup/preparation/baseline/run",
            "/followup/preparation/baseline/attempt",
            "/followup/preparation/baseline/source",
        ] {
            let mut wrong = qualified.clone();
            *wrong.pointer_mut(path).unwrap() = json!("wrong");
            assert!(!admits("qualification($e)", &wrong));
        }
        for mode in [
            "normal",
            "unknown",
            if qualified["mode"] == "modified_host" {
                "unmodified_followup"
            } else {
                "modified_host"
            },
        ] {
            let mut wrong = qualified.clone();
            wrong["mode"] = json!(mode);
            assert!(!admits("qualification($e)", &wrong));
        }
        let mut forged = qualified;
        forged["normal"]["outcome"] = json!("normal_pass");
        forged["normal"]["original_exit_code"] = json!(0);
        forged["normal"]["product"] = product();
        forged["followup"]["normal"] = forged["normal"].clone();
        assert!(!admits("qualification($e)", &forged));
    }
}

#[test]
fn preparation_rejects_partial_changed_unknown_or_unclean_probe_evidence() {
    for prep in [
        preparation("unmodified", true),
        preparation("unmodified", false),
        preparation("service_intervention", false),
    ] {
        assert!(admits("preparation($e.source)", &prep));
        for key in ["baseline", "unchanged_repeat", "intervention_probe"] {
            if prep.get(key).is_none() {
                continue;
            }
            for field in [
                "identity",
                "identity_after",
                "observation",
                "phases",
                "process",
            ] {
                let mut missing = prep.clone();
                missing[key].as_object_mut().unwrap().remove(field);
                assert!(
                    !admits("preparation($e.source)", &missing),
                    "accepted missing {key}/{field}"
                );
            }
            for field in ["identity", "identity_after"] {
                let mut changed = prep.clone();
                changed[key][field]["path"] = json!("/changed/app.app");
                assert!(!admits("preparation($e.source)", &changed));
            }
            for field in [
                "cancelled",
                "timed_out",
                "cleanup_complete",
                "unexpected_survivors",
                "signal",
                "exit_code",
            ] {
                let mut wrong = prep.clone();
                wrong[key]["process"][field] = match field {
                    "cancelled" | "timed_out" => json!(true),
                    "cleanup_complete" => json!(false),
                    "unexpected_survivors" => json!(1),
                    "signal" => json!(9),
                    _ => json!(99),
                };
                assert!(
                    !admits("preparation($e.source)", &wrong),
                    "accepted {key}/{field}"
                );
            }
            let mut unknown = prep.clone();
            unknown[key]["observation"] = json!("unknown");
            assert!(!admits("preparation($e.source)", &unknown));
            let mut wrong_phases = prep.clone();
            wrong_phases[key]["phases"][4] = json!("callback response 1");
            assert!(!admits("preparation($e.source)", &wrong_phases));
        }
        for file in [
            "Contents/Info.plist",
            "Contents/MacOS/NativePanelDiagnostic",
            "Contents/_CodeSignature/CodeResources",
        ] {
            let mut partial = prep.clone();
            partial["baseline"]["identity"]["files"]
                .as_object_mut()
                .unwrap()
                .remove(file);
            partial["baseline"]["identity_after"] = partial["baseline"]["identity"].clone();
            assert!(!admits("preparation($e.source)", &partial));
        }
    }
}

#[test]
fn unmodified_preparation_forbids_intervention_and_extra_repeat() {
    for healthy in [false, true] {
        let prep = preparation("unmodified", healthy);
        for key in [
            "target",
            "service_before",
            "service_after",
            "bootout",
            "intervention_probe",
        ] {
            let mut hidden = prep.clone();
            hidden[key] = Value::Null;
            assert!(
                !admits("preparation($e.source)", &hidden),
                "accepted intervention field {key}"
            );
        }
    }
    let mut extra = preparation("unmodified", true);
    extra["unchanged_repeat"] = probe(true);
    assert!(!admits("preparation($e.source)", &extra));
    let mut failed = preparation("unmodified", false);
    failed["unchanged_repeat"] = probe(false);
    assert!(!admits("preparation($e.source)", &failed));
    let mut premature = preparation("service_intervention", false);
    premature["unchanged_repeat"] = probe(true);
    assert!(!admits("preparation($e.source)", &premature));
}

#[test]
fn intervention_preparation_requires_targeted_successful_service_action() {
    let prep = preparation("service_intervention", false);
    for path in ["/bootout", "/intervention_probe", "/unchanged_repeat"] {
        let mut missing = prep.clone();
        *missing.pointer_mut(path).unwrap() = Value::Null;
        assert!(!admits("preparation($e.source)", &missing));
    }
    for (path, value) in [
        (
            "/target",
            json!("gui/502/com.apple.iconservices.iconservicesagent"),
        ),
        ("/gui_uid", json!(0)),
        ("/bootout/program", json!("/usr/bin/true")),
        ("/bootout/arguments", json!(["bootout", "other-service"])),
        ("/bootout/process/exit_code", json!(1)),
        ("/bootout/process/signal", json!(9)),
        ("/bootout/process/cleanup_complete", json!(false)),
    ] {
        let mut wrong = prep.clone();
        *wrong.pointer_mut(path).unwrap() = value;
        assert!(!admits("preparation($e.source)", &wrong), "accepted {path}");
    }
}
