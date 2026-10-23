//! Independent data oracles. These tests never claim native GUI execution.
use super::*;
use serde_json::{Value, json};
use std::{
    fs,
    time::{Duration, UNIX_EPOCH},
};
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1792713600)
}
fn expected() -> Value {
    json!({"source":"a".repeat(40),"target":"x86_64-apple-darwin","run":"12","attempt":"1","workflow_ref":"resoltico/EMLAttachmentRemover/.github/workflows/ci.yml@refs/pull/1/merge","workflow_sha":"c".repeat(40),"archive":{"sha256":"b".repeat(64),"byte_length":100},"reference":{"sha256":"d".repeat(64),"byte_length":50}})
}
fn process(exit: i32) -> Value {
    json!({"exit_code":exit,"signal":null,"cancelled":false,"timed_out":false,"cleanup_complete":true,"unexpected_survivors":0,"elapsed_seconds":0.1,"cleanup_seconds":0.0})
}
fn host() -> Value {
    json!({"target":"x86_64-apple-darwin","process_os":"macos","process_architecture":"x86_64","kernel_machine":"x86_64","image_os":"macos26","image_version":"20260824.0517.1","product_version":"26.6.1","build_version":"25G76","boot_uuid":"11111111-2222-3333-4444-555555555555"})
}
fn caller() -> Value {
    json!({"uid":501,"euid":501,"console_uid":501,"security_session":{"status":0,"security_session_id":9,"graphic_access":true}})
}
fn command(arguments: Value, exit: i32) -> Value {
    json!({"program":"/bin/launchctl","arguments":arguments,"process":process(exit),"stdout_path":"/owned/command.stdout","stderr_path":"/owned/command.stderr","guard":{"wall_at":1792713600u64,"monotonic_elapsed_ns":1}})
}
fn disabled(value: Value) -> Value {
    json!({"program":"/bin/launchctl","arguments":["print-disabled","gui/501"],"process":process(0),"override":value})
}
fn snapshot() -> Value {
    json!({"host":host(),"caller":caller(),"disabled_override":disabled(Value::Null)})
}
fn identity() -> Value {
    json!({"sha256":"e".repeat(64),"byte_length":1000})
}
fn panel(healthy: bool) -> Value {
    let id = json!({"path":"/owned/NativePanelDiagnostic.app","files":{"info":identity(),"executable":identity(),"resources":identity()},"signature_display":"Signature=adhoc\n","scope":"actual signed probe"});
    json!({"identity":id,"identity_after":id,"process":process(if healthy{0}else{2}),"phases":if healthy {json!(["constructor entry","constructor returned","presentation returned","cancel retained panel; visible=true","callback response 0"])} else {json!(["constructor entry","constructor returned","presentation returned","cancel retained panel; visible=false","callback deadline exceeded"])},"observation":if healthy{"healthy"} else {"known_failure"}})
}
fn ownership() -> Value {
    json!({"pid":42,"launch_date":1.0,"bundle_path":"/owned/EML Attachment Remover.app","pasteboard":"owned-private-board"})
}
fn comparison(pickers: bool) -> Value {
    json!({"whole_domain_unchanged":true,"application_and_other_preferences_unchanged":true,"real_pickers_observed":pickers,"permitted_system_changes":[]})
}
fn product() -> Value {
    json!({"stage":"picker_compatibility","source":expected()["source"],"target":expected()["target"],"host":host(),"snapshot":expected()["archive"],"raw_product_identity":identity(),"case":"picker","test_method":"testOwnedNativePickerCompatibility","app":"/owned/EML Attachment Remover.app","summary":{"total":1,"passed":1,"failed":0,"skipped":0,"expected_failures":0},"process":process(0),"ownership_receipt":ownership(),"lifetime_owner":{"failure":null,"source_check_failure":null,"timed_out":false,"cancelled":false,"runner_reaped":true,"runner_status":0,"owned_app_terminated":true,"forced_app_cleanup":false,"originals_captured":4,"originals_unchanged":true,"owned_pasteboard_released":true,"unexpected_application_pids":[],"ownership_receipt":ownership()},"picker_observation":{"schema":1,"scenario":"picker","owned_pid":42,"message":"/owned/one.eml","folder":"/owned/folder-input"},"persistent_preferences":{"fresh_process_before":{},"fresh_process_after":{},"comparison":comparison(true),"argument_only_controls":comparison(false)}})
}
fn ordinary() -> Value {
    json!({"mode":"unmodified_host","binding":expected(),"before":snapshot(),"panel":panel(true),"product":product(),"after":snapshot(),"actions":[]})
}
fn incident_host() -> Value {
    json!({"image_os":"macos26","image_version":"20260824.0517.1","product_version":"26.6.1","build_version":"25G76"})
}
fn absent() -> Value {
    json!({"program":"/bin/launchctl","arguments":["print","gui/501/com.apple.iconservices.iconservicesagent"],"process":process(113),"selected_fields":{},"absence_diagnostic":"Could not find service \"com.apple.iconservices.iconservicesagent\" in domain for user gui: 501"})
}
fn provisioned() -> Value {
    json!({"target":"gui/501/com.apple.iconservices.iconservicesagent","print":absent(),"disabled":disabled(json!(true))})
}
fn loaded() -> Value {
    json!({"target":"gui/501/com.apple.iconservices.iconservicesagent","print":{"program":"/bin/launchctl","arguments":["print","gui/501/com.apple.iconservices.iconservicesagent"],"process":process(0),"absence_diagnostic":null,"selected_fields":{"path":"/System/Library/LaunchAgents/com.apple.iconservices.iconservicesagent.plist","program":"/System/Library/CoreServices/iconservicesagent","type":"LaunchAgent","domain":"gui/501 [100001]","state":"spawn scheduled"}},"disabled":disabled(Value::Null)})
}
fn crash() -> Value {
    json!({"file_name":"iconservicesagent-native.ips","identity":identity(),"pid":700,"user_id":501,"boot_uuid":host()["boot_uuid"],"os_build":"25G76","capture_time":"2026-10-23 03:20:00+0000","proc_path":"/System/Library/CoreServices/iconservicesagent","exception_signal":"SIGABRT","termination":{"namespace":"SIGNAL","code":6},"faulting_thread":{"index":0,"triggered":true,"frames":[{"symbol":"+[MTLLoader sliceIDForDevice:legacyDriverVersion:airntDriverVersion:]","image_name":"Metal","image_path":"/System/Library/Frameworks/Metal.framework/Versions/A/Metal"},{"symbol":"RBCreateDevice","image_name":"RenderBox","image_path":"/System/Library/PrivateFrameworks/RenderBox.framework/Versions/A/RenderBox"},{"symbol":null,"image_name":"libsystem_kernel.dylib","image_path":"/usr/lib/system/libsystem_kernel.dylib"}]}})
}
fn preparation() -> Value {
    json!({"stage":"picker_host_preparation","binding":expected(),"picker_host":incident_host(),"host":host(),"caller":caller(),"service_before":loaded(),"crash_reports":[crash()],"bootout":command(json!(["bootout","gui/501/com.apple.iconservices.iconservicesagent"]),0),"bootout_absence":null,"disable":command(json!(["disable","gui/501/com.apple.iconservices.iconservicesagent"]),0),"service_after":provisioned()})
}
fn post() -> Value {
    json!({"stage":"picker_host_verification","binding":expected(),"picker_host":incident_host(),"host":host(),"caller":caller(),"service":provisioned()})
}
fn modified(root: &Path) -> Value {
    let registry = Registry::load(root).unwrap();
    let record = &registry.records()[0];
    json!({"mode":"incident_modified_host","binding":expected(),"failed_panel":panel(false),"admission":{"identity":{"id":record.id,"policy_sha256":registry.digest(),"source":expected()["source"]},"policy":record,"admitted_at":1792713600u64,"admitted_subsec_ns":0,"action_deadline_wall":1792714200u64},"preparation":preparation(),"recovered_panel":panel(true),"product":product(),"postverification":post()})
}
fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("xtask")).unwrap();
    fs::write(
        root.path().join("xtask/host-incidents.toml"),
        include_str!("../../host-incidents.toml"),
    )
    .unwrap();
    root
}
fn admits(root: &Path, value: &Value) -> bool {
    verify_receipt(
        root,
        &BuildPlan::test_plan(),
        &serde_json::to_vec(value).unwrap(),
        &serde_json::from_value(expected()).unwrap(),
        now(),
    )
    .is_ok()
}
fn rejects(root: &Path, base: &Value, path: &str, value: Value) {
    let mut record = base.clone();
    *record
        .pointer_mut(path)
        .unwrap_or_else(|| panic!("Missing independent negative-control field {path}")) = value;
    assert!(!admits(root, &record), "Accepted {path}");
}

#[test]
fn ordinary_new_images_never_need_incident_registry_or_loaded_service() {
    let root = tempfile::tempdir().unwrap();
    let base = ordinary();
    assert!(admits(root.path(), &base));
    fs::create_dir(root.path().join("xtask")).unwrap();
    fs::write(
        root.path().join("xtask/host-incidents.toml"),
        "malformed irrelevant registry",
    )
    .unwrap();
    assert!(admits(root.path(), &base));
    let mut future = base.clone();
    for prefix in ["/before/host", "/after/host", "/product/host"] {
        future.pointer_mut(prefix).unwrap()["image_version"] = json!("future-image");
        future.pointer_mut(prefix).unwrap()["product_version"] = json!("26.99");
        future.pointer_mut(prefix).unwrap()["build_version"] = json!("future-build");
    }
    assert!(admits(root.path(), &future));
    for prior in [Value::Null, json!(false)] {
        let mut record = base.clone();
        record["before"]["disabled_override"]["override"] = prior;
        assert!(admits(root.path(), &record));
    }
    for prefix in ["/before", "/after"] {
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/disabled_override/override"),
            json!(true),
        );
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/disabled_override/process/exit_code"),
            json!(1),
        );
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/caller/uid"),
            json!(0),
        );
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/host/process_architecture"),
            json!("aarch64"),
        );
    }
    rejects(
        root.path(),
        &base,
        "/actions",
        json!([command(
            json!([
                "disable",
                "gui/501/com.apple.iconservices.iconservicesagent"
            ]),
            0
        )]),
    );
    rejects(
        root.path(),
        &base,
        "/after/host/boot_uuid",
        json!("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
    );
    rejects(
        root.path(),
        &base,
        "/after/caller/security_session/security_session_id",
        json!(10),
    );
}
#[test]
fn actual_product_case_counts_owner_preferences_and_frozen_bytes_are_independent_obligations() {
    let root = root();
    for base in [ordinary(), modified(root.path())] {
        assert!(admits(root.path(), &base));
        for (path, value) in [
            ("stage", json!("native_candidate")),
            ("case", json!("workflow")),
            ("test_method", json!("readiness")),
            ("summary/skipped", json!(1)),
            ("summary/expected_failures", json!(1)),
            ("summary/total", json!(0)),
            ("lifetime_owner/forced_app_cleanup", json!(true)),
            ("lifetime_owner/failure", json!("failure")),
            ("lifetime_owner/source_check_failure", json!("changed")),
            ("lifetime_owner/originals_unchanged", json!(false)),
            (
                "persistent_preferences/comparison/application_and_other_preferences_unchanged",
                json!(false),
            ),
            ("process/signal", json!(9)),
            ("snapshot/sha256", json!("0".repeat(64))),
            ("ownership_receipt/pid", json!(43)),
            ("picker_observation/owned_pid", json!(44)),
        ] {
            rejects(root.path(), &base, &format!("/product/{path}"), value);
        }
    }
}
#[test]
fn exact_binding_host_boot_and_graphical_caller_cannot_be_substituted() {
    let root = root();
    let base = modified(root.path());
    for prefix in [
        "/binding",
        "/preparation/binding",
        "/postverification/binding",
    ] {
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
            rejects(
                root.path(),
                &base,
                &format!("{prefix}/{field}"),
                json!("unrelated"),
            );
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
            rejects(root.path(), &base, &format!("{prefix}/host/{field}"), value);
        }
        for (field, value) in [
            ("uid", json!(0)),
            ("euid", json!(0)),
            ("console_uid", json!(502)),
        ] {
            rejects(
                root.path(),
                &base,
                &format!("{prefix}/caller/{field}"),
                value,
            );
        }
        for (field, value) in [
            ("status", json!(1)),
            ("security_session_id", json!(0)),
            ("graphic_access", json!(false)),
        ] {
            rejects(
                root.path(),
                &base,
                &format!("{prefix}/caller/security_session/{field}"),
                value,
            );
        }
    }
}
#[test]
fn modified_route_requires_exact_actions_service_readback_and_matching_crashes() {
    let root = root();
    let base = modified(root.path());
    assert!(admits(root.path(), &base));
    for path in ["/preparation/bootout", "/preparation/disable"] {
        rejects(
            root.path(),
            &base,
            &format!("{path}/program"),
            json!("/usr/bin/true"),
        );
        rejects(
            root.path(),
            &base,
            &format!("{path}/arguments"),
            json!(["other-service"]),
        );
        for (field, value) in [
            ("cancelled", json!(true)),
            ("timed_out", json!(true)),
            ("signal", json!(9)),
            ("cleanup_complete", json!(false)),
            ("unexpected_survivors", json!(1)),
        ] {
            rejects(
                root.path(),
                &base,
                &format!("{path}/process/{field}"),
                value,
            );
        }
    }
    rejects(
        root.path(),
        &base,
        "/preparation/service_before/disabled/override",
        json!(true),
    );
    rejects(
        root.path(),
        &base,
        "/preparation/disable/process/exit_code",
        json!(1),
    );
    rejects(
        root.path(),
        &base,
        "/preparation/bootout/process/exit_code",
        json!(1),
    );
    for prefix in ["/preparation/service_after", "/postverification/service"] {
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/target"),
            json!("gui/502/com.apple.iconservices.iconservicesagent"),
        );
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/print/process/exit_code"),
            json!(0),
        );
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/disabled/override"),
            json!(false),
        );
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/print/absence_diagnostic"),
            json!("not found"),
        );
        rejects(
            root.path(),
            &base,
            &format!("{prefix}/print/selected_fields"),
            json!({"state":"running"}),
        );
    }
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
        rejects(
            root.path(),
            &base,
            &format!("/preparation/crash_reports/0/{field}"),
            value,
        );
    }
    rejects(root.path(), &base, "/preparation/crash_reports", json!([]));
    rejects(
        root.path(),
        &base,
        "/preparation/crash_reports/0/termination/code",
        json!(11),
    );
    rejects(
        root.path(),
        &base,
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
            root.path(),
            &base,
            &format!("/preparation/crash_reports/0/faulting_thread/frames/{path}"),
            value,
        );
    }
    let mut partial = base.clone();
    partial["preparation"]["crash_reports"]
        .as_array_mut()
        .unwrap()
        .push(json!({"boot_uuid":host()["boot_uuid"]}));
    assert!(!admits(root.path(), &partial));
    let mut already_absent = base.clone();
    already_absent["preparation"]["bootout"]["process"]["exit_code"] = json!(1);
    already_absent["preparation"]["bootout_absence"] = absent();
    assert!(admits(root.path(), &already_absent));
}
#[test]
fn objects_required_nullable_facts_duplicates_unknowns_and_old_modes_are_rejected() {
    let root = root();
    for base in [ordinary(), modified(root.path())] {
        for mode in ["normal", "modified_host", "unmodified_followup", "unknown"] {
            rejects(root.path(), &base, "/mode", json!(mode));
        }
        let mut unknown = base.clone();
        unknown["failure"] = json!(null);
        assert!(!admits(root.path(), &unknown));
        for path in [
            "/binding",
            "/binding/archive",
            "/product",
            "/product/process",
            "/product/lifetime_owner",
            "/product/ownership_receipt",
            "/product/picker_observation",
            "/product/persistent_preferences/comparison",
        ] {
            let mut array = base.clone();
            let object = array.pointer_mut(path).unwrap();
            *object = Value::Array(object.as_object().unwrap().values().cloned().collect());
            assert!(!admits(root.path(), &array), "Accepted array {path}");
            let mut unknown = base.clone();
            unknown.pointer_mut(path).unwrap()["unexpected"] = json!(false);
            assert!(
                !admits(root.path(), &unknown),
                "Accepted unknown field {path}"
            );
        }
        let text = serde_json::to_string(&base).unwrap();
        let duplicate = text.replacen("\"signal\":null", "\"signal\":null,\"signal\":null", 1);
        assert_ne!(text, duplicate);
        assert!(
            verify_receipt(
                root.path(),
                &BuildPlan::test_plan(),
                duplicate.as_bytes(),
                &serde_json::from_value(expected()).unwrap(),
                now()
            )
            .is_err()
        );
        let duplicate = text.replacen("\"mode\":", "\"mode\":\"unmodified_host\",\"mode\":", 1);
        assert!(
            verify_receipt(
                root.path(),
                &BuildPlan::test_plan(),
                duplicate.as_bytes(),
                &serde_json::from_value(expected()).unwrap(),
                now()
            )
            .is_err()
        );
    }
    let base = ordinary();
    for path in ["/before/disabled_override", "/after/disabled_override"] {
        let mut missing = base.clone();
        missing
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("override");
        assert!(!admits(root.path(), &missing));
    }
    let mut missing = base;
    missing.as_object_mut().unwrap().remove("actions");
    assert!(!admits(root.path(), &missing));
}
#[test]
fn release_rechecks_original_receipt_identity_and_exclusive_incident_expiry() {
    let root = root();
    let expected: QualificationBinding = serde_json::from_value(expected()).unwrap();
    let plan = BuildPlan::test_plan();
    let raw = serde_json::to_vec_pretty(&modified(root.path())).unwrap();
    let verified = verify_receipt(root.path(), &plan, &raw, &expected, now()).unwrap();
    assert_eq!(verified.identity(), &ByteIdentity::of(&raw));
    assert!(recheck_release(root.path(), &plan, &verified, &expected, now()).is_ok());
    assert!(
        recheck_release(
            root.path(),
            &plan,
            &verified,
            &expected,
            UNIX_EPOCH + Duration::from_secs(1795219200)
        )
        .is_err()
    );
    let mut changed = verified.clone();
    changed.document.push(' ');
    assert!(recheck_release(root.path(), &plan, &changed, &expected, now()).is_err());
    let policy = root.path().join("xtask/host-incidents.toml");
    let old = fs::read_to_string(&policy).unwrap();
    fs::write(&policy, old.replace("1795219200", "1795219199")).unwrap();
    assert!(recheck_release(root.path(), &plan, &verified, &expected, now()).is_err());
    let ordinary = verify_receipt(
        root.path(),
        &plan,
        &serde_json::to_vec(&ordinary()).unwrap(),
        &expected,
        now(),
    )
    .unwrap();
    fs::remove_file(policy).unwrap();
    assert!(recheck_release(root.path(), &plan, &ordinary, &expected, UNIX_EPOCH).is_ok());
}
#[test]
fn preference_comparison_cannot_launder_application_changes_or_omitted_controls() {
    let root = root();
    let base = ordinary();
    let mut changed = base.clone();
    changed["product"]["persistent_preferences"]["fresh_process_after"]["copyDestinationBytes"] =
        json!({"type":"string","value":"different"});
    assert!(!admits(root.path(), &changed));
    for (path, value) in [
        ("comparison/real_pickers_observed", json!(false)),
        ("comparison/whole_domain_unchanged", json!(false)),
        ("argument_only_controls/real_pickers_observed", json!(true)),
        (
            "argument_only_controls/whole_domain_unchanged",
            json!(false),
        ),
    ] {
        rejects(
            root.path(),
            &base,
            &format!("/product/persistent_preferences/{path}"),
            value,
        );
    }
    let mut os = base.clone();
    os["product"]["persistent_preferences"]["fresh_process_after"]["NSOSPLastRootDirectory"] =
        json!({"type":"data","base64":"AAH/"});
    os["product"]["persistent_preferences"]["comparison"] = json!({"whole_domain_unchanged":false,"application_and_other_preferences_unchanged":true,"real_pickers_observed":true,"permitted_system_changes":[{"key":"NSOSPLastRootDirectory","before_present":false,"native_type":"data"}]});
    assert!(admits(root.path(), &os));
    os["product"]["persistent_preferences"]["fresh_process_after"]["NSOSPLastRootDirectory"] =
        json!({"type":"integer","representation":"i","value":"1"});
    assert!(!admits(root.path(), &os));
}
#[test]
fn action_guards_retain_one_unexpired_wall_and_monotonic_admission() {
    let root = root();
    let base = modified(root.path());
    rejects(
        root.path(),
        &base,
        "/preparation/bootout/guard/wall_at",
        json!(1792627199u64),
    );
    rejects(
        root.path(),
        &base,
        "/preparation/disable/guard/wall_at",
        json!(1795219200u64),
    );
    rejects(
        root.path(),
        &base,
        "/preparation/bootout/guard/monotonic_elapsed_ns",
        json!(600_000_000_000u64),
    );
    rejects(
        root.path(),
        &base,
        "/preparation/disable/guard/monotonic_elapsed_ns",
        json!(600_000_000_000u64),
    );
    let mut reversed = base.clone();
    reversed["preparation"]["bootout"]["guard"]["monotonic_elapsed_ns"] = json!(10);
    reversed["preparation"]["disable"]["guard"]["monotonic_elapsed_ns"] = json!(9);
    assert!(!admits(root.path(), &reversed));
    let mut rollback = base;
    rollback["preparation"]["disable"]["guard"]["wall_at"] = json!(1792700000u64);
    assert!(
        admits(root.path(), &rollback),
        "Wall rollback within review interval cannot reset the monotonic deadline"
    );
}
#[test]
fn producer_projection_preserves_actual_facts_without_defaults_and_raw_identity() {
    let p = product();
    let mut actual_host = host();
    actual_host["runner_image"] = json!("macos26");
    actual_host["runner_image_version"] = json!("20260824.0517.1");
    actual_host["actual_os"] =
        json!("ProductName: macOS\nProductVersion: 26.6.1\nBuildVersion: 25G76\n");
    let mut picker = p.clone();
    picker["source"] = expected()["source"].clone();
    picker["source_archive"] = expected()["archive"].clone();
    picker["summary"] = json!({"totalTestCount":1,"passedTests":1,"failedTests":0,"skippedTests":0,"expectedFailures":0});
    let raw = json!({"stage":"picker_compatibility","source":expected()["source"],"target":expected()["target"],"host":actual_host,"verification":{"stage":"picker_compatibility","snapshot":expected()["archive"],"picker":picker}});
    let binding: QualificationBinding = serde_json::from_value(expected()).unwrap();
    let host: Host = serde_json::from_value(host()).unwrap();
    let bytes = serde_json::to_vec(&raw).unwrap();
    let projected = project_product(&bytes, &binding, &host).unwrap();
    assert_eq!(projected.raw_product_identity, ByteIdentity::of(&bytes));
    assert_eq!(projected.picker_observation.owned_pid, 42);
    assert_eq!(projected.summary.total, 1);
    let root = tempfile::tempdir().unwrap();
    let mut qualification = ordinary();
    qualification["product"] = serde_json::to_value(&projected).unwrap();
    let verified = verify_receipt(
        root.path(),
        &BuildPlan::test_plan(),
        &serde_json::to_vec(&qualification).unwrap(),
        &binding,
        now(),
    )
    .unwrap();
    verified.check_raw_product(&bytes, &binding).unwrap();
    let mut failed = raw.clone();
    failed["verification"]["picker"]["process"]["exit_code"] = json!(1);
    let failed = serde_json::to_vec(&failed).unwrap();
    let mut identity_only = verified.clone();
    if let PickerQualification::UnmodifiedHost(receipt) = &mut identity_only.receipt {
        receipt.product.raw_product_identity = ByteIdentity::of(&failed);
    }
    assert!(
        identity_only.check_raw_product(&failed, &binding).is_err(),
        "Matching only a changed raw hash must not promote a failed raw product"
    );
    let duplicate = String::from_utf8(bytes.clone()).unwrap().replacen(
        "\"expectedFailures\":0",
        "\"expectedFailures\":1,\"expectedFailures\":0",
        1,
    );
    assert!(
        project_product(duplicate.as_bytes(), &binding, &host).is_err(),
        "Duplicate raw observations cannot be silently collapsed"
    );
    let mut missing = raw.clone();
    missing["verification"]["picker"]["summary"]
        .as_object_mut()
        .unwrap()
        .remove("expectedFailures");
    assert!(project_product(&serde_json::to_vec(&missing).unwrap(), &binding, &host).is_err());
    let mut wrong_archive = raw.clone();
    wrong_archive["verification"]["picker"]["source_archive"]["sha256"] = json!("0".repeat(64));
    assert!(
        project_product(
            &serde_json::to_vec(&wrong_archive).unwrap(),
            &binding,
            &host
        )
        .is_err()
    );
    let mut wrong = raw;
    wrong["host"]["actual_os"] = json!("ProductVersion: 26.6.1\nBuildVersion: 25G77\n");
    assert!(project_product(&serde_json::to_vec(&wrong).unwrap(), &binding, &host).is_err());
    let plan = BuildPlan::test_plan();
    validate_snapshot(
        &plan,
        &binding,
        &serde_json::from_value(snapshot()).unwrap(),
    )
    .unwrap();
    validate_panel(&serde_json::from_value(panel(true)).unwrap(), true).unwrap();
    assert!(validate_panel(&serde_json::from_value(panel(false)).unwrap(), true).is_err());
}
#[test]
fn fractional_admission_uses_exact_remaining_lifetime_and_rejects_missing_fraction() {
    let root = root();
    let mut value = modified(root.path());
    value["admission"]["admitted_at"] = json!(1795219150u64);
    value["admission"]["admitted_subsec_ns"] = json!(900_000_000u32);
    value["admission"]["action_deadline_wall"] = json!(1795219200u64);
    for name in ["bootout", "disable"] {
        value["preparation"][name]["guard"]["wall_at"] = json!(1795219151u64);
    }
    value["preparation"]["disable"]["guard"]["monotonic_elapsed_ns"] = json!(49_099_999_999u64);
    let check = |value: &Value| {
        verify_receipt(
            root.path(),
            &BuildPlan::test_plan(),
            &serde_json::to_vec(value).unwrap(),
            &serde_json::from_value(expected()).unwrap(),
            UNIX_EPOCH + Duration::from_secs(1795219199),
        )
    };
    assert!(check(&value).is_ok());
    value["preparation"]["disable"]["guard"]["monotonic_elapsed_ns"] = json!(49_100_000_000u64);
    assert!(check(&value).is_err());
    value["preparation"]["disable"]["guard"]["monotonic_elapsed_ns"] = json!(1);
    value["admission"]["admitted_subsec_ns"] = json!(1_000_000_000u32);
    assert!(check(&value).is_err());
    value["admission"]
        .as_object_mut()
        .unwrap()
        .remove("admitted_subsec_ns");
    assert!(check(&value).is_err());
}
#[test]
fn bundle_root_is_the_published_artifact_root_not_the_executable_name() {
    let root = tempfile::tempdir().unwrap();
    let mut wrong = ordinary();
    for path in [
        "/product/app",
        "/product/ownership_receipt/bundle_path",
        "/product/lifetime_owner/ownership_receipt/bundle_path",
    ] {
        *wrong.pointer_mut(path).unwrap() = json!("/owned/EMLAttachmentRemover.app");
    }
    assert!(
        !admits(root.path(), &wrong),
        "Coherent process identities must still belong to the plan's actual app bundle root"
    );
}
