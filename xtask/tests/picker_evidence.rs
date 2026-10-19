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
    child.wait_with_output().unwrap().status.success()
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
fn preparation() -> Value {
    let id = json!({"path":"/owned/probe.app","signature_display":"Mach-O thin (x86_64) Signature=adhoc",
        "files":{"Contents/Info.plist":{"sha256":"1".repeat(64),"byte_length":10},
            "Contents/MacOS/NativePanelDiagnostic":{"sha256":"2".repeat(64),"byte_length":100},
            "Contents/_CodeSignature/CodeResources":{"sha256":"3".repeat(64),"byte_length":20}}});
    json!({"stage":"observed","purpose":"picker_qualification_preparation","original_failure_remains_authoritative":true,"gui_uid":501,
        "target":"gui/501/com.apple.iconservices.iconservicesagent",
        "baseline":{"source":expected()["source"],"run":"12","attempt":"1","entitlements":null,
            "host":{"target":"x86_64-apple-darwin","process_os":"macos","process_architecture":"x86_64","kernel_machine":"x86_64","actual_os":"ProductVersion: 26.6.1\n"},
            "process":process(2),"identity":id},
        "unchanged_repeat":{"process":process(2),"identity":id,"identity_after":id},
        "bootout":{"program":"/bin/launchctl","arguments":["bootout","gui/501/com.apple.iconservices.iconservicesagent"],"process":process(0)},
        "intervention_probe":{"process":process(0),"identity":id,"identity_after":id}})
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
    qualification.as_object_mut().unwrap().extend(json!({"stage":"picker_qualification","mode":"normal","normal":normal,"modified":null,"product":product()}).as_object().unwrap().clone());
    assert!(admits("qualification($e)", &qualification));
    qualification["mode"] = json!("modified_host");
    assert!(!admits("qualification($e)", &qualification));
}

#[test]
fn modified_picker_requires_known_failure_same_run_probe_and_actual_product() {
    assert!(admits("known_failure($e)", &known_failure()));
    let mut unrelated = known_failure();
    unrelated["xctest_log"] = json!("Go To sheet did not close");
    assert!(!admits("known_failure($e)", &unrelated));
    let prep = preparation();
    assert!(admits("preparation($e.source)", &prep));
    let mut missing = prep.clone();
    missing["baseline"]
        .as_object_mut()
        .unwrap()
        .remove("identity");
    assert!(!admits("preparation($e.source)", &missing));
    let mut repeat_pass = prep.clone();
    repeat_pass["unchanged_repeat"]["process"]["exit_code"] = json!(0);
    assert!(!admits("preparation($e.source)", &repeat_pass));
    let mut modified = expected();
    modified.as_object_mut().unwrap().extend(json!({"stage":"picker_modified_qualification","normal":known_failure(),"preparation":prep,"product":product()}).as_object().unwrap().clone());
    let mut qualified = expected();
    qualified.as_object_mut().unwrap().extend(json!({"stage":"picker_qualification","mode":"modified_host","normal":known_failure(),"modified":modified,"product":product()}).as_object().unwrap().clone());
    assert!(admits("qualification($e)", &qualified));
    qualified["modified"]["preparation"]["baseline"]["run"] = json!("other-run");
    assert!(!admits("qualification($e)", &qualified));
    qualified["modified"]["preparation"]["baseline"]["run"] = json!("12");
    qualified["product"] = json!({"stage":"observed"});
    assert!(!admits("qualification($e)", &qualified));
}
