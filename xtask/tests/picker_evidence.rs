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
        "archive":{"sha256":"b".repeat(64),"byte_length":100},"reference":{"sha256":"d".repeat(64),"byte_length":50},
        "picker_host":{"image_os":"macos26","image_version":"20260824.0517.1","product_version":"26.6.1","build_version":"25G76"}})
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
    "host":{"target":"x86_64-apple-darwin","process_os":"macos","process_architecture":"x86_64","kernel_machine":"x86_64","product_version":"26.6.1","actual_os":"ProductName: macOS\nProductVersion: 26.6.1\nBuildVersion: 25G76\n"},
    "verification":{"stage":"picker_compatibility","snapshot":expected()["archive"],"picker":{
        "source":expected()["source"],"case":"picker","test_method":"testOwnedNativePickerCompatibility","app":"/owned/app.app",
        "summary":{"totalTestCount":1,"passedTests":1,"failedTests":0,"skippedTests":0,"expectedFailures":0},
        "process":process(0),"lifetime_owner":owner(),"ownership_receipt":owner()["ownership_receipt"],
        "picker_observation":{"schema":1,"scenario":"picker","owned_pid":42,"message":"/owned/one.eml","folder":"/owned/folder-input"},
        "persistent_preferences":{"comparison":{"real_pickers_observed":true,"application_and_other_preferences_unchanged":true}}
    }}})
}
fn host() -> Value {
    json!({"target":"x86_64-apple-darwin","process_os":"macos","process_architecture":"x86_64","kernel_machine":"x86_64",
        "image_os":"macos26","image_version":"20260824.0517.1","product_version":"26.6.1","build_version":"25G76",
        "boot_uuid":"11111111-2222-3333-4444-555555555555"})
}
fn caller() -> Value {
    json!({"uid":501,"euid":501,"console_uid":501,"security_session":{"status":0,"security_session_id":9,"graphic_access":true}})
}
fn command(arguments: Value, exit: i32) -> Value {
    json!({"program":"/bin/launchctl","arguments":arguments,"process":process(exit),
        "stdout_path":"/owned/command.stdout","stderr_path":"/owned/command.stderr"})
}
fn disabled(value: Value) -> Value {
    let mut result = command(json!(["print-disabled", "gui/501"]), 0);
    result["override"] = value;
    result
}
fn absent() -> Value {
    let mut result = command(
        json!(["print", "gui/501/com.apple.iconservices.iconservicesagent"]),
        113,
    );
    result["selected_fields"] = json!({});
    result["absence_diagnostic"] = json!(
        "Could not find service \"com.apple.iconservices.iconservicesagent\" in domain for user gui: 501"
    );
    result
}
fn provisioned() -> Value {
    json!({"target":"gui/501/com.apple.iconservices.iconservicesagent","print":absent(),"disabled":disabled(json!(true))})
}
fn loaded() -> Value {
    let mut result = command(
        json!(["print", "gui/501/com.apple.iconservices.iconservicesagent"]),
        0,
    );
    result["absence_diagnostic"] = Value::Null;
    result["selected_fields"] = json!({"path":"/System/Library/LaunchAgents/com.apple.iconservices.iconservicesagent.plist",
        "program":"/System/Library/CoreServices/iconservicesagent","type":"LaunchAgent","domain":"gui/501 [100001]","state":"spawn scheduled"});
    json!({"target":"gui/501/com.apple.iconservices.iconservicesagent","print":result,"disabled":disabled(Value::Null)})
}
fn crash() -> Value {
    json!({"file_name":"iconservicesagent-native.ips","identity":{"sha256":"e".repeat(64),"byte_length":1000},
        "pid":700,"user_id":501,"boot_uuid":host()["boot_uuid"],"os_build":"25G76","capture_time":"2026-10-21 03:20:00+0000",
        "proc_path":"/System/Library/CoreServices/iconservicesagent","exception_signal":"SIGABRT","termination":{"namespace":"SIGNAL","code":6},
        "faulting_thread":{"index":0,"triggered":true,"frames":[
            {"symbol":"+[MTLLoader sliceIDForDevice:legacyDriverVersion:airntDriverVersion:]","image_name":"Metal","image_path":"/System/Library/Frameworks/Metal.framework/Versions/A/Metal"},
            {"symbol":"RBCreateDevice","image_name":"RenderBox","image_path":"/System/Library/PrivateFrameworks/RenderBox.framework/Versions/A/RenderBox"},
            {"symbol":null,"image_name":"libsystem_kernel.dylib","image_path":"/usr/lib/system/libsystem_kernel.dylib"}]}})
}
fn preparation() -> Value {
    let mut result = expected();
    result.as_object_mut().unwrap().extend(json!({"stage":"picker_host_preparation","host":host(),"caller":caller(),
        "service_before":loaded(),"crash_reports":[crash()],
        "bootout":command(json!(["bootout","gui/501/com.apple.iconservices.iconservicesagent"]),0),"bootout_absence":null,
        "disable":command(json!(["disable","gui/501/com.apple.iconservices.iconservicesagent"]),0),"service_after":provisioned()}).as_object().unwrap().clone());
    result
}
fn postverification() -> Value {
    let mut result = expected();
    result.as_object_mut().unwrap().extend(json!({"stage":"picker_host_verification","host":host(),"caller":caller(),"service":provisioned()}).as_object().unwrap().clone());
    result
}
fn qualification() -> Value {
    let mut result = expected();
    result.as_object_mut().unwrap().extend(
        json!({"stage":"picker_qualification","mode":"modified_host",
        "preparation":preparation(),"product":product(),"postverification":postverification()})
        .as_object()
        .unwrap()
        .clone(),
    );
    result
}
fn rejects(path: &str, value: Value) {
    let mut record = qualification();
    *record.pointer_mut(path).unwrap() = value;
    assert!(!admits("qualification($e)", &record), "accepted {path}");
}

#[test]
fn real_product_contract_remains_required_without_skips_or_forced_cleanup() {
    assert!(admits("product($e)", &product()));
    for (path, value) in [
        ("/product/verification/picker/case", json!("workflow")),
        (
            "/product/verification/picker/test_method",
            json!("readiness"),
        ),
        (
            "/product/verification/picker/summary/skippedTests",
            json!(1),
        ),
        (
            "/product/verification/picker/summary/expectedFailures",
            json!(1),
        ),
        (
            "/product/verification/picker/lifetime_owner/forced_app_cleanup",
            json!(true),
        ),
        (
            "/product/verification/picker/lifetime_owner/failure",
            json!("failure"),
        ),
        (
            "/product/verification/picker/persistent_preferences/comparison/application_and_other_preferences_unchanged",
            json!(false),
        ),
        ("/product/verification/picker/process/signal", json!(9)),
        (
            "/product/verification/snapshot/sha256",
            json!("0".repeat(64)),
        ),
    ] {
        rejects(path, value);
    }
}

#[test]
fn modified_host_requires_complete_bracketing_and_observed_not_assumed_state() {
    assert!(admits("qualification($e)", &qualification()));
    for prior in [Value::Null, json!(true), json!(false)] {
        let mut record = qualification();
        record["preparation"]["service_before"]["disabled"]["override"] = prior;
        assert!(admits("qualification($e)", &record));
    }
    let mut already_absent = qualification();
    already_absent["preparation"]["bootout"]["process"]["exit_code"] = json!(1);
    already_absent["preparation"]["bootout_absence"] = absent();
    assert!(admits("qualification($e)", &already_absent));
    already_absent["preparation"]["bootout_absence"]["process"]["exit_code"] = json!(1);
    assert!(!admits("qualification($e)", &already_absent));
    let mut older = qualification();
    let mut report = crash();
    report["boot_uuid"] = json!("different-old-boot");
    older["preparation"]["crash_reports"]
        .as_array_mut()
        .unwrap()
        .push(report);
    assert!(admits("qualification($e)", &older));
    for mode in ["normal", "unmodified_followup", "unknown"] {
        rejects("/mode", json!(mode));
    }
    for path in ["/preparation", "/product", "/postverification"] {
        rejects(path, Value::Null);
    }
    rejects("/stage", json!("picker_qualification_attempt"));
    rejects("/preparation/stage", json!("ready"));
    rejects("/postverification/stage", json!("picker_host_preparation"));
    for prefix in ["", "/preparation", "/postverification"] {
        for field in ["failure", "error"] {
            let mut failed = qualification();
            failed.pointer_mut(prefix).unwrap()[field] = json!("failed operation");
            assert!(!admits("qualification($e)", &failed));
        }
    }
}

#[test]
fn source_run_archive_policy_host_and_gui_context_cannot_be_substituted() {
    for prefix in ["", "/preparation", "/postverification"] {
        for field in [
            "source",
            "target",
            "run",
            "attempt",
            "workflow_ref",
            "workflow_sha",
            "archive",
            "reference",
            "picker_host",
        ] {
            rejects(&format!("{prefix}/{field}"), json!("unrelated"));
        }
    }
    for prefix in ["/preparation", "/postverification"] {
        for (field, value) in [
            ("image_os", json!("macos15")),
            ("image_version", json!("unknown")),
            ("build_version", json!("25G77")),
            ("product_version", json!("26.6.2")),
            ("process_architecture", json!("aarch64")),
        ] {
            rejects(&format!("{prefix}/host/{field}"), value);
        }
        for (field, value) in [
            ("uid", json!(0)),
            ("euid", json!(0)),
            ("console_uid", json!(502)),
        ] {
            rejects(&format!("{prefix}/caller/{field}"), value);
        }
        for (field, value) in [
            ("status", json!(1)),
            ("security_session_id", json!(0)),
            ("graphic_access", json!(false)),
        ] {
            rejects(&format!("{prefix}/caller/security_session/{field}"), value);
        }
    }
    rejects(
        "/postverification/host/boot_uuid",
        json!("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
    );
    rejects(
        "/postverification/caller/security_session/security_session_id",
        json!(10),
    );
    rejects(
        "/product/host/actual_os",
        json!("ProductVersion: 26.6.1\nBuildVersion: 25G77\n"),
    );
}

#[test]
fn exact_native_actions_and_postconditions_are_required() {
    for path in ["/preparation/bootout", "/preparation/disable"] {
        rejects(&format!("{path}/program"), json!("/usr/bin/true"));
        rejects(&format!("{path}/arguments"), json!(["other-service"]));
        for (field, value) in [
            ("cancelled", json!(true)),
            ("timed_out", json!(true)),
            ("signal", json!(9)),
            ("cleanup_complete", json!(false)),
            ("unexpected_survivors", json!(1)),
        ] {
            rejects(&format!("{path}/process/{field}"), value);
        }
    }
    rejects("/preparation/bootout/process/exit_code", json!(1));
    rejects("/preparation/disable/process/exit_code", json!(1));
    rejects(
        "/preparation/service_before/print/selected_fields/program",
        json!("/tmp/counterfeit"),
    );
    rejects(
        "/preparation/service_before/print/selected_fields/domain",
        json!("gui/502 [100001]"),
    );
    rejects(
        "/preparation/service_before/print/selected_fields/state",
        json!(""),
    );
    for prefix in ["/preparation/service_after", "/postverification/service"] {
        rejects(
            &format!("{prefix}/target"),
            json!("gui/502/com.apple.iconservices.iconservicesagent"),
        );
        rejects(&format!("{prefix}/print/process/exit_code"), json!(0));
        rejects(
            &format!("{prefix}/print/absence_diagnostic"),
            json!("Not found"),
        );
        rejects(
            &format!("{prefix}/print/selected_fields"),
            json!({"state":"running"}),
        );
        rejects(&format!("{prefix}/disabled/override"), json!(false));
        rejects(&format!("{prefix}/disabled/process/exit_code"), json!(1));
    }
}

#[test]
fn all_selected_crashes_require_recognized_schema_and_a_matching_fault() {
    for (field, value) in [
        ("capture_time", Value::Null),
        ("identity", Value::Null),
        ("file_name", json!("other.ips")),
        ("pid", json!(0)),
        ("user_id", json!(502)),
        ("boot_uuid", json!("old-boot")),
        ("os_build", json!("25G77")),
        ("proc_path", json!("/tmp/iconservicesagent")),
        ("exception_signal", json!("SIGSEGV")),
    ] {
        rejects(&format!("/preparation/crash_reports/0/{field}"), value);
    }
    rejects("/preparation/crash_reports", json!([]));
    rejects("/preparation/crash_reports/0/termination/code", json!(11));
    rejects(
        "/preparation/crash_reports/0/faulting_thread/triggered",
        json!(false),
    );
    for (path, value) in [
        ("0/symbol", Value::Null),
        ("0/symbol", json!(true)),
        ("0/image_path", json!("/tmp/Metal")),
        ("1/symbol", Value::Null),
        ("1/image_path", json!("/tmp/RenderBox")),
        ("2/symbol", json!(4)),
    ] {
        rejects(
            &format!("/preparation/crash_reports/0/faulting_thread/frames/{path}"),
            value,
        );
    }
    let mut partial = qualification();
    partial["preparation"]["crash_reports"]
        .as_array_mut()
        .unwrap()
        .push(json!({"boot_uuid":host()["boot_uuid"]}));
    assert!(!admits("qualification($e)", &partial));
    for path in [
        "/preparation/bootout/process",
        "/preparation/service_after/print/process",
        "/postverification/service/disabled/process",
    ] {
        rejects(path, json!({"event":"ready","session":1}));
    }
}
