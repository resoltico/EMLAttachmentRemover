//! Hand-authored native-schema oracles; no native process is executed here.
use super::*;
use serde_json::json;

fn raw_report() -> (Value, Value) {
    let header = json!({"app_name":"iconservicesagent","bug_type":"309"});
    let report = json!({"version":2,"pid":42,"userID":501,
        "bootSessionUUID":"12345678-1234-1234-1234-123456789ABC","osVersion":{"build":"25G76"},
        "captureTime":"2026-10-21 00:00:00.0000 +0000","procPath":"/System/Library/CoreServices/iconservicesagent",
        "exception":{"signal":"SIGABRT"},"termination":{"namespace":"SIGNAL","code":6},"faultingThread":1,
        "threads":[{"triggered":false,"frames":[]},{"triggered":true,"frames":[
            {"imageIndex":0,"imageOffset":1},
            {"imageIndex":0,"symbol":"+[MTLLoader sliceIDForDevice:legacyDriverVersion:airntDriverVersion:]"},
            {"imageIndex":1,"symbol":"RB::FunctionLibrary::FunctionLibrary"}]}],
        "usedImages":[{"name":"Metal","path":"/System/Library/Frameworks/Metal.framework/Versions/A/Metal"},
            {"name":"RenderBox","path":"/System/Library/PrivateFrameworks/RenderBox.framework/Versions/A/RenderBox"}]});
    (header, report)
}
fn bytes(header: &Value, report: &Value) -> Vec<u8> {
    format!("{header}\n{report}\n").into_bytes()
}
#[test]
fn actual_schema_allows_unsymbolicated_frames_but_requires_real_critical_symbols() {
    let (header, mut raw) = raw_report();
    let observed = parse("iconservicesagent-fixture.ips", &bytes(&header, &raw)).unwrap();
    assert_eq!(observed.pid, 42);
    assert_eq!(observed.faulting_thread.index, 1);
    assert_eq!(observed.faulting_thread.frames[0].symbol, None);
    assert!(known(&observed, &super::super::tests::host(), 501));
    for index in [1, 2] {
        let mut missing = raw.clone();
        missing["threads"][1]["frames"][index]
            .as_object_mut()
            .unwrap()
            .remove("symbol");
        let observed = parse("iconservicesagent-fixture.ips", &bytes(&header, &missing)).unwrap();
        assert!(!known(&observed, &super::super::tests::host(), 501));
    }
    raw["threads"][1]["frames"][0]["symbol"] = json!(17);
    assert!(parse("iconservicesagent-fixture.ips", &bytes(&header, &raw)).is_err());
}
#[test]
fn partial_unknown_and_ambiguous_fault_references_are_not_known_reports() {
    let (header, report) = raw_report();
    for (path, value) in [
        ("/version", json!(3)),
        ("/faultingThread", json!(-1)),
        ("/faultingThread", json!(2)),
        ("/threads/1/triggered", json!(false)),
        ("/threads/1/frames/1/imageIndex", json!(99)),
        ("/threads/1/frames/1/imageIndex", json!(true)),
        ("/usedImages/0/path", json!(null)),
    ] {
        let mut wrong = report.clone();
        *wrong.pointer_mut(path).unwrap() = value;
        assert!(
            parse("iconservicesagent-fixture.ips", &bytes(&header, &wrong)).is_err(),
            "{path}"
        );
    }
    assert!(
        parse(
            "iconservicesagent-fixture.ips",
            header.to_string().as_bytes()
        )
        .is_err()
    );
    let mut trailing = bytes(&header, &report);
    trailing.extend_from_slice(b"{}");
    assert!(parse("iconservicesagent-fixture.ips", &trailing).is_err());
    assert!(
        parse(
            "iconservicesagent-fixture.ips",
            &vec![b' '; REPORT_LIMIT as usize + 1]
        )
        .is_err()
    );
}
#[test]
fn unrelated_boot_user_build_or_nonfault_signature_does_not_authorize_action() {
    let (header, report) = raw_report();
    for (path, value) in [
        ("/bootSessionUUID", json!("other-boot")),
        ("/userID", json!(502)),
        ("/osVersion/build", json!("other-build")),
        ("/procPath", json!("/other/iconservicesagent")),
        ("/exception/signal", json!("SIGSEGV")),
        ("/termination/code", json!(11)),
        ("/termination/namespace", json!("OTHER")),
        ("/usedImages/0/path", json!("/other/Metal")),
        ("/usedImages/1/path", json!("/other/RenderBox")),
    ] {
        let mut wrong = report.clone();
        *wrong.pointer_mut(path).unwrap() = value;
        let observed = parse("iconservicesagent-fixture.ips", &bytes(&header, &wrong)).unwrap();
        assert!(
            !known(&observed, &super::super::tests::host(), 501),
            "{path}"
        );
    }
    let mut wrong = report.clone();
    wrong["faultingThread"] = json!(0);
    wrong["threads"][0] = json!({"triggered":true,"frames":[{"imageIndex":0,"imageOffset":1}]});
    let observed = parse("iconservicesagent-fixture.ips", &bytes(&header, &wrong)).unwrap();
    assert!(!known(&observed, &super::super::tests::host(), 501));
}
#[test]
fn recognized_old_reports_do_not_replace_or_exclude_a_required_current_boot_fault() {
    let (header, raw) = raw_report();
    let current = parse("iconservicesagent-fixture.ips", &bytes(&header, &raw)).unwrap();
    let mut old = current.clone();
    old.boot_uuid = "other-boot".into();
    assert!(require_known(&[old.clone()], &super::super::tests::host(), 501).is_err());
    assert!(require_known(&[old, current], &super::super::tests::host(), 501).is_ok());
}
