//! Independent synthetic native observations; these tests do not claim native execution.
use super::*;
use serde_json::json;
use std::time::{Duration, UNIX_EPOCH};
const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LINUX: &str = "remove-eml-attachments-7.0.0-linux-aarch64.tar.gz";
const LINUX_X86: &str = "remove-eml-attachments-7.0.0-linux-x86_64.tar.gz";
const MAC_CLI: &str = "remove-eml-attachments-7.0.0-macos-x86_64.zip";
const MAC_APP: &str = "EML-Attachment-Remover-7.0.0-macos-x86_64.zip";
fn workflow() -> WorkflowIdentity {
    WorkflowIdentity {
        run: "12".into(),
        attempt: "1".into(),
        workflow_ref:
            "resoltico/EMLAttachmentRemover/.github/workflows/release.yml@refs/heads/main".into(),
        workflow_sha: "c".repeat(40),
    }
}
fn identity() -> ByteIdentity {
    ByteIdentity {
        byte_length: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
    }
}
fn frozen(value: Value) -> FrozenRecord {
    let contents = serde_json::to_string(&value).unwrap();
    FrozenRecord {
        identity: ByteIdentity::of(contents.as_bytes()),
        contents,
    }
}
fn edit(record: &mut FrozenRecord, change: impl FnOnce(&mut Value)) {
    let mut value = record.document().unwrap();
    change(&mut value);
    *record = frozen(value);
}
fn process() -> Value {
    json!({"exit_code":0,"signal":null,"timed_out":false,"cancelled":false,"unexpected_survivors":0,"cleanup_complete":true,"elapsed_seconds":0.1,"cleanup_seconds":0.0})
}
fn summary() -> Value {
    json!({"totalTestCount":1,"passedTests":1,"failedTests":0,"skippedTests":0,"expectedFailures":0})
}
fn comparison(pickers: bool) -> Value {
    json!({"whole_domain_unchanged":true,"application_and_other_preferences_unchanged":true,"real_pickers_observed":pickers,"permitted_system_changes":[]})
}
fn ownership(app: &str) -> Value {
    json!({"pid":42,"launch_date":1.0,"bundle_path":app,"pasteboard":"owned-private-board"})
}
fn owner(app: &str, originals: u32) -> Value {
    json!({"failure":null,"source_check_failure":null,"timed_out":false,"cancelled":false,"runner_reaped":true,"runner_status":0,"owned_app_terminated":true,"forced_app_cleanup":false,"originals_captured":originals,"originals_unchanged":true,"owned_pasteboard_released":true,"unexpected_application_pids":[],"ownership_receipt":ownership(app)})
}
fn host(target: &str, os: &str, arch: &str, major: u8) -> Value {
    json!({"target":target,"process_os":os,"process_architecture":arch,"kernel_machine":if os=="macos"&&arch=="aarch64"{"arm64"}else{arch},"product_version":format!("{major}.1"),"runner_image":"new-image","runner_image_version":"new-version","actual_os":format!("ProductVersion: {major}.1\nBuildVersion: TestBuild\n")})
}
// Independent required-case oracle; do not derive this from the production catalog.
const CONTROLLED_CASES: &[(&str, &str)] = &[
    ("active-quit", "testOwnedCoreActiveQuit"),
    ("force-stop", "testOwnedCoreForceStop"),
    ("published-error", "testOwnedCorePublishedError"),
    ("stop", "testOwnedCoreStop"),
    (
        "blocked-processing-preflight",
        "testOwnedBlockedProcessingPreflight",
    ),
    (
        "blocked-processing-readback",
        "testOwnedBlockedProcessingReadback",
    ),
    (
        "blocked-processing-stage-write",
        "testOwnedBlockedProcessingStageWrite",
    ),
    (
        "blocked-processing-file-sync",
        "testOwnedBlockedProcessingFileSync",
    ),
    (
        "blocked-processing-publication",
        "testOwnedBlockedProcessingPublication",
    ),
    (
        "blocked-processing-namespace-sync",
        "testOwnedBlockedProcessingNamespaceSync",
    ),
    (
        "blocked-export-preflight",
        "testOwnedBlockedExportPreflight",
    ),
    ("blocked-export-readback", "testOwnedBlockedExportReadback"),
    (
        "blocked-export-stage-write",
        "testOwnedBlockedExportStageWrite",
    ),
    ("blocked-export-file-sync", "testOwnedBlockedExportFileSync"),
    (
        "blocked-export-publication",
        "testOwnedBlockedExportPublication",
    ),
    (
        "blocked-export-namespace-sync",
        "testOwnedBlockedExportNamespaceSync",
    ),
    ("export-publication", "testOwnedExportPublication"),
    ("export-writer-block", "testOwnedExportWriterBlock"),
];
fn controlled_actor(name: &str) -> Value {
    let (operation, phase) = name
        .strip_prefix("blocked-")
        .and_then(|value| value.split_once('-'))
        .unwrap_or((
            "export",
            if name == "export-writer-block" {
                "request-body"
            } else {
                "after-publication"
            },
        ));
    let mut ui = json!({"case":name,"operation":operation,"phase":phase,"operation_id":6,"worker_pid":44,"run_id":if operation=="processing"{2}else{1}});
    let mut additional_worker = Value::Null;
    let mut matrix = Value::Null;
    let mut writer_document = Value::Null;
    let mut target_identity = Value::Null;
    if name.starts_with("blocked-") {
        for key in [
            "ui_responsive",
            "other_report_viewed",
            "first_stop_observed",
            "force_stop_observed",
        ] {
            ui[key] = true.into();
        }
        ui["source_reserved"] = (operation == "export").into();
        ui["queued_then_processing"] = (operation == "export").into();
        ui["ui_response_seconds"] = json!(0.5);
        ui["settled_text"] = if operation == "processing" {
            "Processing operation: 6; Worker exit None; settled true; reusable true"
        } else {
            "Report export operation 6 for run 1; settled: true; reusable: true"
        }
        .into();
        if name == "blocked-export-preflight" {
            for key in [
                "dual_quit",
                "native_panel_seen",
                "native_panel_cancelled",
                "quit_cancel_preserved_children",
                "app_quit_completed",
            ] {
                ui[key] = true.into();
            }
            ui["pending_requests"] = 8.into();
            ui["cosmetic_notice"] =
                "8 requests are already waiting. Remove a waiting run before adding another."
                    .into();
            ui["processing_operation_id"] = 7.into();
            ui["processing_run_id"] = 3.into();
            ui["processing_worker_pid"] = 45.into();
            additional_worker = json!({"operation_id":7,"run_id":3,"target":{"path":"/owned/processing-copy.eml","identity":null},"worker_pid":45,"worker_absent":true,"first_interrupt_micros":100,"second_interrupt_micros":10000100});
        }
    } else if name == "export-writer-block" {
        for key in [
            "writer_busy_observed",
            "ui_responsive",
            "other_report_viewed",
            "source_reserved",
            "first_stop_observed",
        ] {
            ui[key] = true.into();
        }
        ui["document_bytes"] = json!(2 * 1024 * 1024);
        ui["document_sha256"] = "a".repeat(64).into();
        writer_document = json!({"byte_length":2*1024*1024,"sha256":"a".repeat(64)});
    } else {
        ui["cases"] = json!([
            "ordinary",
            "occupied",
            "directory",
            "symlink",
            "racing-winner",
            "pre-edge-stop",
            "post-edge-stop",
            "post-edge-loss"
        ]);
        ui["no_replace_observed"] = true.into();
        ui["post_edge_bytes_preserved"] = true.into();
        ui["expected_bytes"] = 3.into();
        ui["expected_sha256"] = identity().sha256.into();
        ui["winner_sha256"] = ByteIdentity::of(b"winner").sha256.into();
        matrix = json!({"expected_document":identity(),"winner":ByteIdentity::of(b"winner"),"directory_unchanged":true,"symlink_unchanged":true,"pre_edge_absent":true,"controls":[{"case":"racing-winner","phase":"publication","operation_id":7,"worker_pid":47,"worker_absent":true,"first_interrupt_micros":null,"second_interrupt_micros":null},{"case":"pre-edge-stop","phase":"publication","operation_id":8,"worker_pid":48,"worker_absent":true,"first_interrupt_micros":100,"second_interrupt_micros":null},{"case":"post-edge-stop","phase":"namespace-sync","operation_id":9,"worker_pid":49,"worker_absent":true,"first_interrupt_micros":100,"second_interrupt_micros":null}]});
        target_identity = serde_json::to_value(identity()).unwrap();
    }
    if name.starts_with("blocked-") && matches!(phase, "readback" | "namespace-sync") {
        target_identity = if operation == "processing" {
            serde_json::to_value(ByteIdentity::of(include_bytes!(
                "../../../tests/contract/mime/golden/01-gmail-attachment.eml"
            )))
            .unwrap()
        } else {
            serde_json::to_value(identity()).unwrap()
        };
    }
    let contents = serde_json::to_string(&ui).unwrap();
    json!({"case":name,"operation":operation,"phase":phase,"operation_id":6,"worker_pid":44,"worker_absent":true,"additional_worker":additional_worker,"matrix":matrix,"writer_document":writer_document,"first_interrupt_micros":100,"second_interrupt_micros":if name=="export-writer-block" {Value::Null}else if name=="blocked-export-preflight"{json!(10000100)}else{json!(200)},"ui_receipt":{"identity":ByteIdentity::of(contents.as_bytes()),"contents":contents},"originals":[{"name":"one.eml","identity":identity()},{"name":"two.eml","identity":identity()},{"name":"folder-input/folder.eml","identity":identity()},{"name":"service-input/日本語-مرحبا-\u{202e}report.eml","identity":identity()}],"target":{"path":"/owned/report.json","identity":target_identity}})
}
fn desktop_case(name: &str, method: &str) -> Value {
    let controlled = CONTROLLED_CASES.iter().any(|(case, _)| *case == name);
    let app = if controlled {
        "/owned/Observed Operations.app"
    } else {
        "/owned/EML Attachment Remover.app"
    };
    let export_picker = name.starts_with("blocked-export-") || name.starts_with("export-");
    let preferences = if name == "workflow" {
        json!({"fresh_process_before":{},"fresh_process_after":{},"comparison":comparison(true),"argument_only_controls":comparison(false)})
    } else {
        json!({"before":{},"after":{},"comparison":comparison(export_picker)})
    };
    let mut value = json!({"case":name,"test_method":method,"source":SOURCE,"source_archive":identity(),"app":app,"process":process(),"lifetime_owner":owner(app,if name=="pagination"{69}else if name=="export-writer-block"{1028}else{4}),"summary":summary(),"persistent_preferences":preferences});
    match name {
        "workflow" => {
            value["ownership_receipt"] = ownership(app);
            value["picker_observation"] = json!({"schema":1,"scenario":"workflow","owned_pid":42,"message":"/owned/one.eml","folder":"/owned/folder-input"});
            value["lifetime_owner"]["missing_copy_observation"] = json!({"held":true,"restored_by_request":true,"copy_left_held":false,"restoration_failure":null});
        }
        "pagination" => {
            value["observation"] = json!({"owned_pid":42,"items":65,"removed_parts_in_first_item":65,"page_transitions":["parts-next","parts-previous","items-next","items-previous"],"outputs_match_independent_goldens":true})
        }
        "appearance" => {
            value["appearance"] = json!({"schema":1,"restored":true,"owned_pid":42,"settings_pid":43,"conditions":["light","dark","increased-contrast"],"native_product_observations":["Interface native appearance: light; increased contrast: false","Interface native appearance: dark; increased contrast: false","Interface native appearance: dark; increased contrast: true"],"original_selector":"Light","original_contrast":false,"original_reduce_transparency":true,"native_settings_observations":{"before":{"increase_contrast":false,"reduce_transparency":true},"increased_contrast":{"increase_contrast":true,"reduce_transparency":true},"restored":{"increase_contrast":false,"reduce_transparency":true}}});
            value["lifetime_owner"]["ownership_receipt"]["settings"] = json!({"pid":43,"launch_date":1.0,"bundle_path":"/System/Applications/System Settings.app"});
            value["lifetime_owner"]["owned_settings_terminated"] = true.into();
            value["lifetime_owner"]["unexpected_settings_application_pids"] = json!([]);
        }
        _ if name.starts_with("blocked-") || name.starts_with("export-") => {
            value["actor"] = controlled_actor(name);
        }
        _ => {
            value["actor"] = json!({"case":name,"worker_pid":44,"worker_absent":true,"independent_copy_golden":true,"first_interrupt_micros":if name=="published-error"{Value::Null}else{json!(100)},"second_interrupt_micros":match name {"active-quit"=>json!(10000100),"force-stop"=>json!(200),_=>Value::Null}})
        }
    }
    value
}
fn desktop(reference: &ByteIdentity) -> Value {
    let controls: Vec<_> = CONTROLLED_CASES
        .iter()
        .map(|(name, method)| desktop_case(name, method))
        .collect();
    json!({"stage":"desktop_qualification","source":SOURCE,"target":"x86_64-apple-darwin","workflow":workflow(),"host":host("x86_64-apple-darwin","macos","x86_64",15),"reference":reference,"verification":{"stage":"desktop_qualification","snapshot":identity(),"gui_interaction":{"workflow":desktop_case("workflow","testOwnedDesktopWorkflowAndNativeIngress"),"pagination":desktop_case("pagination","testOwnedPagination"),"appearance":desktop_case("appearance","testOwnedNativeAppearance"),"operation_controls":{"shipping_bundle_unchanged":true,"cases":controls}}}})
}
fn picker(reference: &ByteIdentity) -> (VerifiedPickerReceipt, FrozenRecord) {
    let binding = binding(
        SOURCE,
        "x86_64-apple-darwin",
        &workflow(),
        &identity(),
        reference,
    );
    let native = json!({"target":"x86_64-apple-darwin","process_os":"macos","process_architecture":"x86_64","kernel_machine":"x86_64","image_os":"new-image","image_version":"new-version","product_version":"26.1","build_version":"TestBuild","boot_uuid":"11111111-2222-3333-4444-555555555555"});
    let before = json!({"host":native,"caller":{"uid":501,"euid":501,"console_uid":501,"security_session":{"status":0,"security_session_id":9,"graphic_access":true}},"disabled_override":{"program":"/bin/launchctl","arguments":["print-disabled","gui/501"],"process":process(),"override":null}});
    let app = "/owned/EML Attachment Remover.app";
    let raw = frozen(
        json!({"stage":"picker_compatibility","source":SOURCE,"target":"x86_64-apple-darwin","workflow":workflow(),"host":host("x86_64-apple-darwin","macos","x86_64",26),"verification":{"stage":"picker_compatibility","snapshot":identity(),"picker":{"source":SOURCE,"source_archive":identity(),"case":"picker","test_method":"testOwnedNativePickerCompatibility","app":app,"summary":summary(),"process":process(),"lifetime_owner":owner(app,4),"ownership_receipt":ownership(app),"picker_observation":{"schema":1,"scenario":"picker","owned_pid":42,"message":"/owned/one.eml","folder":"/owned/folder-input"},"persistent_preferences":{"fresh_process_before":{},"fresh_process_after":{},"comparison":comparison(true),"argument_only_controls":comparison(false)}}}}),
    );
    // Facts are deliberately authored independently from project_product().
    let product = json!({"stage":"picker_compatibility","source":SOURCE,"target":"x86_64-apple-darwin","host":native,"snapshot":identity(),"raw_product_identity":raw.identity,"case":"picker","test_method":"testOwnedNativePickerCompatibility","app":app,"summary":{"total":1,"passed":1,"failed":0,"skipped":0,"expected_failures":0},"process":process(),"lifetime_owner":owner(app,4),"ownership_receipt":ownership(app),"picker_observation":{"schema":1,"scenario":"picker","owned_pid":42,"message":"/owned/one.eml","folder":"/owned/folder-input"},"persistent_preferences":{"fresh_process_before":{},"fresh_process_after":{},"comparison":comparison(true),"argument_only_controls":comparison(false)}});
    let id = json!({"byte_length":1000,"sha256":"e".repeat(64)});
    let probe_id = json!({"path":"/owned/NativePanelDiagnostic.app","files":{"info":id,"executable":id,"resources":id},"signature_display":"Signature=adhoc\n","scope":"synthetic data-only probe identity"});
    let receipt = json!({"mode":"unmodified_host","binding":binding,"before":before,"after":before,"panel":{"identity":probe_id,"identity_after":probe_id,"process":process(),"phases":["constructor entry","constructor returned","presentation returned","cancel retained panel; visible=true","callback response 0"],"observation":"healthy"},"product":product,"actions":[]});
    let root = tempfile::tempdir().unwrap();
    let verified = crate::picker_evidence::verify_receipt(
        root.path(),
        &BuildPlan::test_plan(),
        &serde_json::to_vec(&receipt).unwrap(),
        &binding,
        UNIX_EPOCH + Duration::from_secs(1792713600),
    )
    .unwrap();
    (verified, raw)
}
fn resource() -> Value {
    let fixture = |raw, body, wrappers| json!({"raw_bytes":raw,"body_bytes":body,"wrappers":wrappers,"sha256":"e".repeat(64),"path":"/owned/input.eml","expected":"/owned/output.eml"});
    let mut rows = Vec::new();
    for i in 1..=5 {
        for (name, time) in [("shallow", 0.1), ("deep", 0.3)] {
            rows.push(json!({"name":format!("{name}-{i}"),"fixture":name,"elapsed_seconds":time,"user_seconds":0.1,"system_seconds":0.0,"max_rss_kib":65536,"exit_code":0,"exact_existing_bytes_matched":true}));
        }
    }
    rows.push(json!({"name":"limit","fixture":"limit","elapsed_seconds":1.0,"user_seconds":0.5,"system_seconds":0.1,"max_rss_kib":600000,"exit_code":0,"exact_existing_bytes_matched":true}));
    json!({"source":SOURCE,"binary":identity(),"process":process(),"process_failure":null,"measurement_read_failure":null,"measurements":{"source":SOURCE,"binary_sha256":identity().sha256,"passed":true,"failure":null,"host":{"system":"Linux","machine":"x86_64"},"filesystem":"ext4","case_mode":"case-sensitive","fixtures":{"shallow":fixture(33554560u64,33554432u64,1),"deep":fixture(33558528u64,33554432u64,63),"limit":fixture(268435456u64,268431360u64,63)},"runs":rows,"medians":{"shallow_seconds":0.1,"deep_seconds":0.3,"maximum_deep_seconds":0.5},"rss_limit_bytes":637534208u64}})
}
fn fixture(extra_linux: bool) -> (tempfile::TempDir, BuildPlan, VerifiedInputs) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("archives");
    fs::create_dir(&dir).unwrap();
    let mut table: toml::Value = toml::from_str(include_str!("../../distribution.toml")).unwrap();
    table["targets"].as_array_mut().unwrap().retain(|row| {
        matches!(
            row["platform"].as_str(),
            Some("macos-x86_64" | "linux-aarch64")
        ) || (extra_linux && row["platform"].as_str() == Some("linux-x86_64"))
    });
    let plan = BuildPlan::parse(
        &toml::to_string(&table).unwrap(),
        include_str!("../../tools.toml"),
        "7.0.0".into(),
    )
    .unwrap();
    let mut products = BTreeMap::new();
    for (name, platform, target, os, arch) in [
        (
            LINUX,
            "linux-aarch64",
            "aarch64-unknown-linux-gnu",
            "linux",
            "aarch64",
        ),
        (
            MAC_CLI,
            "macos-x86_64",
            "x86_64-apple-darwin",
            "macos",
            "x86_64",
        ),
    ]
    .into_iter()
    .chain(extra_linux.then_some((
        LINUX_X86,
        "linux-x86_64",
        "x86_64-unknown-linux-gnu",
        "linux",
        "x86_64",
    ))) {
        let path = dir.join(name);
        fs::write(&path, b"abc").unwrap();
        let mut record = json!({"source":SOURCE,"target":target,"workflow":workflow(),"host":host(target,os,arch,26),"archive":{"identity":identity()},"cli_image":identity(),"qualification_failures":[]});
        record["name_profiles"] = crate::name_profiles::valid_fixture(
            SOURCE,
            &identity(),
            if os == "macos" {
                OsFamily::Macos
            } else {
                OsFamily::Linux
            },
        );
        if platform == "linux-x86_64" {
            record["preparation_resources"] = resource();
        }
        products.insert(
            name.into(),
            ProductInput {
                platform: platform.into(),
                role: ProductRole::Cli,
                archive: identity(),
                archive_path: path,
                producer: frozen(record),
                reference: None,
                desktop: None,
                picker: None,
                picker_product: None,
            },
        );
    }
    let reference = frozen(
        json!({"schema":2,"source":SOURCE,"target":"x86_64-apple-darwin","workflow":workflow(),"version":"7.0.0","root":"EML Attachment Remover.app","architecture":"x86_64"}),
    );
    let (picker, picker_product) = picker(&reference.identity);
    let consumer = frozen(desktop(&reference.identity));
    let path = dir.join(MAC_APP);
    fs::write(&path, b"abc").unwrap();
    products.insert(MAC_APP.into(),ProductInput{platform:"macos-x86_64".into(),role:ProductRole::DesktopApp,archive:identity(),archive_path:path,producer:frozen(json!({"source":SOURCE,"target":"x86_64-apple-darwin","workflow":workflow(),"host":host("x86_64-apple-darwin","macos","x86_64",26),"stage":"native_candidate","verification":{"stage":"native_candidate"},"identity":identity(),"reference":reference.identity})),reference:Some(reference),desktop:Some(consumer),picker:Some(picker),picker_product:Some(picker_product)});
    (
        root,
        plan,
        VerifiedInputs {
            schema: "eml-verified-release-inputs".into(),
            schema_version: 1,
            source: SOURCE.into(),
            version: "7.0.0".into(),
            workflow: workflow(),
            products: products.into(),
        },
    )
}
fn synchronize_picker_raw(product: &mut ProductInput) {
    let id = product.picker_product.as_ref().unwrap().identity.clone();
    let mut retained = serde_json::to_value(product.picker.as_ref().unwrap()).unwrap();
    retained["receipt"]["product"]["raw_product_identity"] = serde_json::to_value(&id).unwrap();
    let mut document: Value = serde_json::from_str(retained["document"].as_str().unwrap()).unwrap();
    document["product"]["raw_product_identity"] = serde_json::to_value(&id).unwrap();
    let text = serde_json::to_string(&document).unwrap();
    retained["identity"] = serde_json::to_value(ByteIdentity::of(text.as_bytes())).unwrap();
    retained["document"] = text.into();
    product.picker = Some(serde_json::from_value(retained).unwrap());
}
fn clone_inputs(value: &VerifiedInputs) -> VerifiedInputs {
    serde_json::from_value(serde_json::to_value(value).unwrap()).unwrap()
}
#[test]
fn composed_current_run_inputs_admit_variable_exact_sets_and_ordinary_picker_without_registry() {
    for extra in [false, true] {
        let (root, plan, value) = fixture(extra);
        value.validate(&plan, SOURCE, &workflow()).unwrap();

        value
            .recheck_pickers(root.path(), &plan, UNIX_EPOCH)
            .unwrap();
        assert!(!root.path().join("xtask/host-incidents.toml").exists());
    }
}
#[test]
fn missing_extra_duplicate_wrong_product_roles_and_stale_bindings_fail() {
    let (_, plan, good) = fixture(false);
    good.validate(&plan, SOURCE, &workflow()).unwrap();
    for case in [
        "missing",
        "extra",
        "platform",
        "role",
        "source",
        "version",
        "run",
        "attempt",
        "workflow",
        "archive",
        "reference",
        "qualification-failure",
    ] {
        let mut bad = clone_inputs(&good);
        match case {
            "missing" => {
                bad.products.remove(LINUX);
            }
            "extra" => {
                let extra = bad.products.get(LINUX).unwrap();
                let extra: ProductInput =
                    serde_json::from_value(serde_json::to_value(extra).unwrap()).unwrap();
                bad.products.insert("undeclared.zip".into(), extra);
            }
            "platform" => bad.products.get_mut(LINUX).unwrap().platform = "macos-x86_64".into(),
            "role" => bad.products.get_mut(LINUX).unwrap().role = ProductRole::DesktopApp,
            "source" => bad.source = "b".repeat(40),
            "version" => bad.version = "0.0.0".into(),
            "run" => edit(&mut bad.products.get_mut(LINUX).unwrap().producer, |v| {
                v["workflow"]["run"] = "13".into()
            }),
            "attempt" => edit(&mut bad.products.get_mut(LINUX).unwrap().producer, |v| {
                v["workflow"]["attempt"] = "2".into()
            }),
            "workflow" => edit(&mut bad.products.get_mut(LINUX).unwrap().producer, |v| {
                v["workflow"]["workflow_sha"] = "d".repeat(40).into()
            }),
            "archive" => bad.products.get_mut(LINUX).unwrap().archive.sha256 = "f".repeat(64),
            "qualification-failure" => edit(
                &mut bad.products.get_mut(LINUX).unwrap().producer,
                |value| value["qualification_failures"] = json!(["required naming gate failed"]),
            ),
            "reference" => {
                bad.products
                    .get_mut(MAC_APP)
                    .unwrap()
                    .reference
                    .as_mut()
                    .unwrap()
                    .contents
                    .push(' ');
            }
            _ => unreachable!(),
        }
        assert!(bad.validate(&plan, SOURCE, &workflow()).is_err(), "{case}");
    }
    let raw = serde_json::to_string(&good).unwrap();
    let product = serde_json::to_string(good.products.get(LINUX).unwrap()).unwrap();
    let duplicate = raw.replacen(
        "\"products\":{",
        &format!("\"products\":{{\"{LINUX}\":{product},"),
        1,
    );
    assert!(serde_json::from_str::<VerifiedInputs>(&duplicate).is_err());
}
#[test]
fn desktop_case_identity_owner_process_and_preference_omissions_fail_composed_admission() {
    let (_, plan, good) = fixture(false);
    for (pointer, replacement) in [
        (
            "/verification/gui_interaction/workflow/source",
            json!("stale"),
        ),
        (
            "/verification/gui_interaction/pagination/process/signal",
            json!(9),
        ),
        (
            "/verification/gui_interaction/appearance/lifetime_owner/forced_app_cleanup",
            json!(true),
        ),
        (
            "/verification/gui_interaction/workflow/lifetime_owner",
            Value::Null,
        ),
        (
            "/verification/gui_interaction/workflow/persistent_preferences/comparison/whole_domain_unchanged",
            json!(false),
        ),
        (
            "/verification/gui_interaction/operation_controls/cases/0/test_method",
            json!("unrelated"),
        ),
        (
            "/verification/gui_interaction/pagination/summary/expectedFailures",
            json!(1),
        ),
        (
            "/verification/gui_interaction/appearance/appearance/restored",
            json!(false),
        ),
        (
            "/verification/gui_interaction/operation_controls/cases/0/actor/worker_absent",
            json!(false),
        ),
        (
            "/verification/gui_interaction/pagination/observation/outputs_match_independent_goldens",
            json!(false),
        ),
    ] {
        let mut bad = clone_inputs(&good);
        edit(
            bad.products
                .get_mut(MAC_APP)
                .unwrap()
                .desktop
                .as_mut()
                .unwrap(),
            |v| *v.pointer_mut(pointer).unwrap() = replacement,
        );
        assert!(
            bad.validate(&plan, SOURCE, &workflow()).is_err(),
            "{pointer}"
        );
    }
    let mut bad = clone_inputs(&good);
    edit(
        bad.products
            .get_mut(MAC_APP)
            .unwrap()
            .desktop
            .as_mut()
            .unwrap(),
        |v| {
            v["verification"]["gui_interaction"]
                .as_object_mut()
                .unwrap()
                .remove("pagination");
        },
    );
    assert!(bad.validate(&plan, SOURCE, &workflow()).is_err());
}
#[test]
fn stale_raw_picker_workflows_and_duplicate_observations_cannot_be_laundered() {
    let (_, plan, good) = fixture(false);
    let mut bad = clone_inputs(&good);
    edit(
        bad.products
            .get_mut(MAC_APP)
            .unwrap()
            .picker_product
            .as_mut()
            .unwrap(),
        |v| v["workflow"]["run"] = "11".into(),
    );
    synchronize_picker_raw(bad.products.get_mut(MAC_APP).unwrap());
    assert!(bad.validate(&plan, SOURCE, &workflow()).is_err());
    let mut bad = clone_inputs(&good);
    let raw = bad
        .products
        .get_mut(MAC_APP)
        .unwrap()
        .picker_product
        .as_mut()
        .unwrap();
    raw.contents = raw.contents.replacen(
        "\"source\":",
        &format!("\"source\":\"{}\",\"source\":", "b".repeat(40)),
        1,
    );
    raw.identity = ByteIdentity::of(raw.contents.as_bytes());
    synchronize_picker_raw(bad.products.get_mut(MAC_APP).unwrap());
    assert!(bad.validate(&plan, SOURCE, &workflow()).is_err());
}
#[test]
fn failed_actual_picker_cannot_be_replaced_by_success_projection_even_with_consistent_hashes() {
    let (_, plan, good) = fixture(false);
    let mut bad = clone_inputs(&good);
    edit(
        bad.products
            .get_mut(MAC_APP)
            .unwrap()
            .picker_product
            .as_mut()
            .unwrap(),
        |value| {
            value["verification"]["picker"]["summary"]["passedTests"] = 0.into();
            value["verification"]["picker"]["summary"]["failedTests"] = 1.into();
        },
    );
    synchronize_picker_raw(bad.products.get_mut(MAC_APP).unwrap());
    assert!(bad.validate(&plan, SOURCE, &workflow()).is_err());
}
#[test]
fn linux_resource_failure_missing_rows_or_fabricated_metrics_cannot_qualify() {
    let (_, plan, good) = fixture(true);
    good.validate(&plan, SOURCE, &workflow()).unwrap();
    for (pointer, replacement) in [
        ("/preparation_resources", Value::Null),
        ("/preparation_resources/process/signal", json!(9)),
        ("/preparation_resources/measurements/passed", json!(false)),
        ("/preparation_resources/measurements/source", json!("stale")),
        (
            "/preparation_resources/measurements/binary_sha256",
            json!("f".repeat(64)),
        ),
        (
            "/preparation_resources/measurements/runs/0/exact_existing_bytes_matched",
            json!(false),
        ),
        (
            "/preparation_resources/measurements/runs/10/max_rss_kib",
            json!(700000),
        ),
        (
            "/preparation_resources/measurements/fixtures/limit/raw_bytes",
            json!(268435455),
        ),
        (
            "/preparation_resources/measurements/medians/deep_seconds",
            json!(0.1),
        ),
    ] {
        let mut bad = clone_inputs(&good);
        edit(
            &mut bad.products.get_mut(LINUX_X86).unwrap().producer,
            |v| *v.pointer_mut(pointer).unwrap() = replacement,
        );
        assert!(
            bad.validate(&plan, SOURCE, &workflow()).is_err(),
            "{pointer}"
        );
    }
    let mut bad = clone_inputs(&good);
    edit(
        &mut bad.products.get_mut(LINUX_X86).unwrap().producer,
        |v| {
            v["preparation_resources"]["measurements"]["runs"]
                .as_array_mut()
                .unwrap()
                .remove(0);
        },
    );
    assert!(bad.validate(&plan, SOURCE, &workflow()).is_err());
    let mut bad = clone_inputs(&good);
    edit(
        &mut bad.products.get_mut(LINUX_X86).unwrap().producer,
        |value| {
            let measurements = &mut value["preparation_resources"]["measurements"];
            for row in measurements["runs"].as_array_mut().unwrap() {
                if row["fixture"] == "deep" {
                    row["elapsed_seconds"] = 1.0.into();
                }
            }
            measurements["medians"]["deep_seconds"] = 1.0.into();
        },
    );
    assert!(
        bad.validate(&plan, SOURCE, &workflow()).is_err(),
        "honest slower deep median must fail independently of declared passed"
    );
}

#[test]
fn required_manifest_nullable_roles_and_duplicate_producer_observations_are_not_defaulted() {
    let (_, plan, good) = fixture(false);
    let base = serde_json::to_value(&good).unwrap();
    for field in ["reference", "desktop", "picker", "picker_product"] {
        let mut value = base.clone();
        value["products"][LINUX]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            serde_json::from_value::<VerifiedInputs>(value).is_err(),
            "missing {field}"
        );
    }
    let mut bad = clone_inputs(&good);
    let producer = &mut bad.products.get_mut(LINUX).unwrap().producer;
    producer.contents = producer.contents.replacen(
        "\"source\":",
        &format!("\"source\":\"{}\",\"source\":", "b".repeat(40)),
        1,
    );
    producer.identity = ByteIdentity::of(producer.contents.as_bytes());
    assert!(bad.validate(&plan, SOURCE, &workflow()).is_err());
}

#[test]
fn every_controlled_desktop_case_and_its_bound_raw_observations_are_required() {
    let (_, plan, good) = fixture(false);
    good.validate(&plan, SOURCE, &workflow()).unwrap();
    for (name, _) in CONTROLLED_CASES {
        let mut omitted = clone_inputs(&good);
        edit(
            omitted
                .products
                .get_mut(MAC_APP)
                .unwrap()
                .desktop
                .as_mut()
                .unwrap(),
            |value| {
                let cases = value["verification"]["gui_interaction"]["operation_controls"]["cases"]
                    .as_array_mut()
                    .unwrap();
                let previous = cases.len();
                cases.retain(|case| case["case"] != *name);
                assert_eq!(cases.len() + 1, previous);
            },
        );
        assert!(
            omitted.validate(&plan, SOURCE, &workflow()).is_err(),
            "missing {name}"
        );
    }
    for change in [
        "duplicate",
        "unexpected",
        "changed_raw",
        "wrong_operation",
        "wrong_phase",
        "wrong_run",
        "missing_response",
        "wrong_picker",
        "forced_cleanup",
    ] {
        let mut bad = clone_inputs(&good);
        edit(
            bad.products
                .get_mut(MAC_APP)
                .unwrap()
                .desktop
                .as_mut()
                .unwrap(),
            |value| {
                let cases = value["verification"]["gui_interaction"]["operation_controls"]["cases"]
                    .as_array_mut()
                    .unwrap();
                if change == "duplicate" {
                    cases.push(cases[4].clone());
                    return;
                }
                let case = &mut cases[4];
                if change == "unexpected" {
                    case["case"] = "unreviewed-case".into();
                    return;
                }
                if change == "forced_cleanup" {
                    case["lifetime_owner"]["forced_app_cleanup"] = true.into();
                    return;
                }
                if change == "wrong_picker" {
                    case["persistent_preferences"]["comparison"]["real_pickers_observed"] =
                        true.into();
                    return;
                }
                let receipt = &mut case["actor"]["ui_receipt"];
                let contents = receipt["contents"].as_str().unwrap();
                if change == "changed_raw" {
                    receipt["contents"] = format!("{contents} ").into();
                    return;
                }
                let mut document: Value = serde_json::from_str(contents).unwrap();
                match change {
                    "wrong_operation" => document["operation"] = "export".into(),
                    "wrong_phase" => document["phase"] = "file-sync".into(),
                    "wrong_run" => document["run_id"] = 0.into(),
                    "missing_response" => {
                        document
                            .as_object_mut()
                            .unwrap()
                            .remove("ui_response_seconds");
                    }
                    _ => unreachable!(),
                }
                let contents = serde_json::to_string(&document).unwrap();
                *receipt =
                    json!({"identity":ByteIdentity::of(contents.as_bytes()),"contents":contents});
            },
        );
        assert!(
            bad.validate(&plan, SOURCE, &workflow()).is_err(),
            "{change}"
        );
    }
}
