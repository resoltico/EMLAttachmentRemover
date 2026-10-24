//! Independent synthetic native observations; these tests do not claim native execution.
use super::*;
use serde_json::json;
use std::time::{Duration, UNIX_EPOCH};
const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LINUX: &str = "emlattachmentremover-7.0.0-linux-aarch64.tar.gz";
const LINUX_X86: &str = "emlattachmentremover-7.0.0-linux-x86_64.tar.gz";
const MAC_CLI: &str = "emlattachmentremover-7.0.0-macos-x86_64.tar.gz";
const MAC_APP: &str = "emlattachmentremover-gui-7.0.0-macos-x86_64.zip";
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
    json!({"failure":null,"source_check_failure":null,"fixture_filesystem":crate::picker_evidence::fixture_filesystem_test_value(),"fixture_output_relay":crate::picker_evidence::fixture_output_relay_test_value(),"fixture_actors":crate::picker_evidence::fixture_actors::test_value("picker"),"panel_path_clipboard":crate::picker_evidence::panel_path_clipboard_test_value(true),"timed_out":false,"cancelled":false,"runner_reaped":true,"runner_status":0,"owned_app_terminated":true,"forced_app_cleanup":false,"originals_captured":originals,"originals_unchanged":true,"owned_pasteboard_released":true,"unexpected_application_pids":[],"ownership_receipt":ownership(app)})
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
        matrix = json!({"expected_document":identity(),"expected_document_contents":"abc","winner":ByteIdentity::of(b"winner"),"directory_unchanged":true,"symlink_unchanged":true,"pre_edge_absent":true,"controls":[{"case":"racing-winner","phase":"publication","operation_id":90,"worker_pid":140,"worker_absent":true,"first_interrupt_micros":null,"second_interrupt_micros":null},{"case":"pre-edge-stop","phase":"publication","operation_id":91,"worker_pid":141,"worker_absent":true,"first_interrupt_micros":100,"second_interrupt_micros":null},{"case":"post-edge-stop","phase":"namespace-sync","operation_id":92,"worker_pid":142,"worker_absent":true,"first_interrupt_micros":100,"second_interrupt_micros":null}]});
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
fn bind_actor_payload(actors: &mut Value, kind: &str, contents: String) {
    use base64::Engine as _;
    for record in actors["records"].as_array_mut().unwrap() {
        let mut ack: Value = serde_json::from_str(record["document"].as_str().unwrap()).unwrap();
        if ack["actor"] == kind {
            ack["outputs"][0]["bytes"] =
                serde_json::to_value(ByteIdentity::of(contents.as_bytes())).unwrap();
            let line = format!(
                "EML_FIXTURE_ACTOR {} {} {}\n",
                ack["sequence"].as_u64().unwrap(),
                kind,
                base64::engine::general_purpose::STANDARD.encode(contents.as_bytes())
            );
            ack["request_sha256"] = ByteIdentity::of(line.as_bytes()).sha256.into();
            let document = serde_json::to_string(&ack).unwrap();
            record["identity"] =
                serde_json::to_value(ByteIdentity::of(document.as_bytes())).unwrap();
            record["document"] = document.into();
        }
    }
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
    value["lifetime_owner"]["fixture_actors"] =
        crate::picker_evidence::fixture_actors::test_value(name);
    value["lifetime_owner"]["panel_path_clipboard"] =
        crate::picker_evidence::panel_path_clipboard_test_value(
            name == "workflow" || export_picker,
        );
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
            let ui_identity = value["actor"]["ui_receipt"]["contents"]
                .as_str()
                .unwrap()
                .to_owned();
            bind_actor_payload(
                &mut value["lifetime_owner"]["fixture_actors"],
                "retain-child-observation",
                ui_identity,
            );
            if name == "export-publication" {
                let identity = value["actor"]["matrix"]["expected_document_contents"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                bind_actor_payload(
                    &mut value["lifetime_owner"]["fixture_actors"],
                    "retain-independent-report",
                    identity,
                );
            }
        }
        _ => {
            value["actor"] = json!({"case":name,"worker_pid":44,"worker_absent":true,"independent_copy_golden":true,"first_interrupt_micros":if name=="published-error"{Value::Null}else{json!(100)},"second_interrupt_micros":match name {"active-quit"=>json!(10000100),"force-stop"=>json!(200),_=>Value::Null}})
        }
    }
    value
}
fn timestamps() -> Value {
    json!({"source_epoch":1609459201,"zip_epoch_seconds":1609459200,"precision":"UTC DOS two-second floor","entries":{
        "":{"kind":"directory","mtime_seconds":1609459200,"mtime_nanoseconds":0},
        "Contents":{"kind":"directory","mtime_seconds":1609459200,"mtime_nanoseconds":0},
        "Contents/Info.plist":{"kind":"file","mtime_seconds":1609459200,"mtime_nanoseconds":0},
        "Contents/MacOS":{"kind":"directory","mtime_seconds":1609459200,"mtime_nanoseconds":0},
        "Contents/MacOS/EMLAttachmentRemover":{"kind":"file","mtime_seconds":1609459200,"mtime_nanoseconds":0}
    }})
}
fn desktop(reference: &ByteIdentity) -> Value {
    let controls: Vec<_> = CONTROLLED_CASES
        .iter()
        .map(|(name, method)| desktop_case(name, method))
        .collect();
    json!({"stage":"desktop_qualification","source":SOURCE,"source_epoch":1609459201,"source_provenance":"git_commit","target":"x86_64-apple-darwin","workflow":workflow(),"host":host("x86_64-apple-darwin","macos","x86_64",15),"reference":reference,"verification":{"source_epoch":1609459201,"source_provenance":"git_commit","bundle_timestamps":timestamps(),"stage":"desktop_qualification","snapshot":identity(),"gui_interaction":{"workflow":desktop_case("workflow","testOwnedDesktopWorkflowAndNativeIngress"),"pagination":desktop_case("pagination","testOwnedPagination"),"appearance":desktop_case("appearance","testOwnedNativeAppearance"),"operation_controls":{"shipping_bundle_unchanged":true,"cases":controls}}}})
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
        json!({"stage":"picker_compatibility","source":SOURCE,"source_epoch":1609459201,"source_provenance":"git_commit","target":"x86_64-apple-darwin","workflow":workflow(),"host":host("x86_64-apple-darwin","macos","x86_64",26),"verification":{"source_epoch":1609459201,"source_provenance":"git_commit","bundle_timestamps":timestamps(),"stage":"picker_compatibility","snapshot":identity(),"picker":{"source":SOURCE,"source_archive":identity(),"case":"picker","test_method":"testOwnedNativePickerCompatibility","app":app,"summary":summary(),"process":process(),"lifetime_owner":owner(app,4),"ownership_receipt":ownership(app),"picker_observation":{"schema":1,"scenario":"picker","owned_pid":42,"message":"/owned/one.eml","folder":"/owned/folder-input"},"persistent_preferences":{"fresh_process_before":{},"fresh_process_after":{},"comparison":comparison(true),"argument_only_controls":comparison(false)}}}}),
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
    let fixture = |name: &str, raw, body, wrappers| json!({"path":format!("/owned/measurement/{name}.eml"),"raw_bytes":raw,"body_bytes":body,"wrappers":wrappers,"sha256":"e".repeat(64),"expected":format!("/owned/measurement/{name}.no-attachments.eml")});
    let mut rows = Vec::new();
    for i in 1..=5 {
        for (name, time) in [("shallow", 0.1), ("deep", 0.3)] {
            rows.push(json!({"name":format!("{name}-{i}"),"fixture":name,"elapsed_seconds":time,"user_seconds":0.1,"system_seconds":0.0,"max_rss_kib":65536,"exit_code":0,"exact_existing_bytes_matched":true}));
        }
    }
    rows.push(json!({"name":"limit","fixture":"limit","elapsed_seconds":1.0,"user_seconds":0.5,"system_seconds":0.1,"max_rss_kib":600000,"exit_code":0,"exact_existing_bytes_matched":true}));
    json!({"source":SOURCE,"binary":identity(),"process":process(),"process_failure":null,"measurement_read_failure":null,"measurements":{"source":SOURCE,"binary_sha256":identity().sha256,"passed":true,"failure":null,"host":{"system":"Linux","machine":"x86_64"},"filesystem":"ext2/ext3","filesystem_mount":{"filesystems":[{"target":"/owned","source":"/dev/loop7","fstype":"ext4","options":"rw,relatime"}]},"case_mode":"case-sensitive","fixtures":{"shallow":fixture("shallow",33554560u64,33554432u64,1),"deep":fixture("deep",33558528u64,33554432u64,63),"limit":fixture("limit",268435456u64,268431360u64,63)},"runs":rows,"medians":{"shallow_seconds":0.1,"deep_seconds":0.3,"maximum_deep_seconds":0.5},"rss_limit_bytes":637534208u64}})
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
        crate::distribution::PackageSource::fixture(),
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
        let mut record = json!({"source":SOURCE,"source_epoch":1609459201,"source_provenance":"git_commit","target":target,"workflow":workflow(),"host":host(target,os,arch,26),"archive":{"identity":identity()},"cli_image":identity(),"qualification_failures":[]});
        record["release_tests"] = json!({"native_publication":crate::publication_evidence::test_fixture(SOURCE,1609459201,target,os,arch,"release")});
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
        json!({"schema":2,"source":SOURCE,"source_epoch":1609459201,"source_provenance":"git_commit","target":"x86_64-apple-darwin","workflow":workflow(),"version":"7.0.0","root":"EML Attachment Remover.app","architecture":"x86_64","files":{"Contents/Info.plist":[identity(),420],"Contents/MacOS/EMLAttachmentRemover":[identity(),493]}}),
    );
    let (picker, picker_product) = picker(&reference.identity);
    let consumer = frozen(desktop(&reference.identity));
    let path = dir.join(MAC_APP);
    fs::write(&path, b"abc").unwrap();
    products.insert(MAC_APP.into(),ProductInput{platform:"macos-x86_64".into(),role:ProductRole::DesktopApp,archive:identity(),archive_path:path,producer:frozen(json!({"source":SOURCE,"source_epoch":1609459201,"source_provenance":"git_commit","target":"x86_64-apple-darwin","workflow":workflow(),"host":host("x86_64-apple-darwin","macos","x86_64",26),"stage":"native_candidate","release_tests":{"native_publication":crate::publication_evidence::test_fixture(SOURCE,1609459201,"x86_64-apple-darwin","macos","x86_64","release")},"verification":{"stage":"native_candidate","source_epoch":1609459201,"source_provenance":"git_commit","bundle_timestamps":timestamps()},"identity":identity(),"reference":reference.identity})),reference:Some(reference),desktop:Some(consumer),picker:Some(picker),picker_product:Some(picker_product)});
    (
        root,
        plan,
        VerifiedInputs {
            schema: "eml-verified-release-inputs".into(),
            schema_version: 2,
            source: SOURCE.into(),
            source_epoch: PackageEpoch::new(1_609_459_201).unwrap(),
            source_provenance: "git_commit".into(),
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
        value.validate(&plan, &workflow()).unwrap();

        value
            .recheck_pickers(root.path(), &plan, UNIX_EPOCH)
            .unwrap();
        assert!(!root.path().join("xtask/host-incidents.toml").exists());
    }
}
#[test]
fn missing_extra_duplicate_wrong_product_roles_and_stale_bindings_fail() {
    let (_, plan, good) = fixture(false);
    good.validate(&plan, &workflow()).unwrap();
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
        assert!(bad.validate(&plan, &workflow()).is_err(), "{case}");
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
        assert!(bad.validate(&plan, &workflow()).is_err(), "{pointer}");
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
    assert!(bad.validate(&plan, &workflow()).is_err());
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
    assert!(bad.validate(&plan, &workflow()).is_err());
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
    assert!(bad.validate(&plan, &workflow()).is_err());
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
    assert!(bad.validate(&plan, &workflow()).is_err());
}
#[test]
fn linux_resource_failure_missing_rows_or_fabricated_metrics_cannot_qualify() {
    let (_, plan, good) = fixture(true);
    good.validate(&plan, &workflow()).unwrap();
    for (pointer, replacement) in [
        ("/preparation_resources", Value::Null),
        ("/preparation_resources/process/signal", json!(9)),
        ("/preparation_resources/measurements/passed", json!(false)),
        (
            "/preparation_resources/measurements/filesystem_mount",
            Value::Null,
        ),
        (
            "/preparation_resources/measurements/filesystem_mount/filesystems",
            json!([]),
        ),
        (
            "/preparation_resources/measurements/filesystem_mount/filesystems/0/fstype",
            json!(""),
        ),
        (
            "/preparation_resources/measurements/filesystem_mount/filesystems/0/source",
            Value::Null,
        ),
        (
            "/preparation_resources/measurements/filesystem_mount/filesystems/0/target",
            json!("/another-volume"),
        ),
        (
            "/preparation_resources/measurements/fixtures/limit/path",
            json!("relative.eml"),
        ),
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
        assert!(bad.validate(&plan, &workflow()).is_err(), "{pointer}");
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
    assert!(bad.validate(&plan, &workflow()).is_err());
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
        bad.validate(&plan, &workflow()).is_err(),
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
    assert!(bad.validate(&plan, &workflow()).is_err());
}

#[test]
fn every_controlled_desktop_case_and_its_bound_raw_observations_are_required() {
    let (_, plan, good) = fixture(false);
    good.validate(&plan, &workflow()).unwrap();
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
            omitted.validate(&plan, &workflow()).is_err(),
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
        assert!(bad.validate(&plan, &workflow()).is_err(), "{change}");
    }
}

#[test]
fn manifest_epoch_provenance_and_schema_are_required_and_match_the_held_source() {
    let (_, plan, good) = fixture(false);
    for field in ["source_epoch", "source_provenance"] {
        let mut missing = serde_json::to_value(&good).unwrap();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<VerifiedInputs>(missing).is_err(),
            "{field}"
        );
    }
    for (field, replacement) in [
        ("source_epoch", json!(1609459200)),
        ("source_epoch", json!(0)),
        ("source_epoch", Value::Null),
        ("source_provenance", json!("declared_archive")),
        ("source_provenance", Value::Null),
        ("schema_version", json!(1)),
    ] {
        let mut value = serde_json::to_value(&good).unwrap();
        value[field] = replacement;
        if let Ok(bad) = serde_json::from_value::<VerifiedInputs>(value) {
            assert!(bad.validate(&plan, &workflow()).is_err(), "{field}");
        }
    }
}

#[test]
fn every_product_and_app_consumer_requires_exact_source_epoch_and_git_provenance() {
    let (_, plan, good) = fixture(false);
    for lane in ["cli", "app", "desktop", "picker", "reference"] {
        for field in ["source_epoch", "source_provenance"] {
            for replacement in [
                None,
                Some(Value::Null),
                Some(if field == "source_epoch" {
                    json!(1609459200)
                } else {
                    json!("declared_archive")
                }),
            ] {
                let mut bad = clone_inputs(&good);
                let product = bad
                    .products
                    .get_mut(if lane == "cli" { LINUX } else { MAC_APP })
                    .unwrap();
                let record = match lane {
                    "cli" | "app" => &mut product.producer,
                    "desktop" => product.desktop.as_mut().unwrap(),
                    "picker" => product.picker_product.as_mut().unwrap(),
                    "reference" => product.reference.as_mut().unwrap(),
                    _ => unreachable!(),
                };
                edit(record, |v| {
                    if let Some(value) = replacement {
                        v[field] = value;
                    } else {
                        v.as_object_mut().unwrap().remove(field);
                    }
                });
                if lane == "reference" {
                    let identity = product.reference.as_ref().unwrap().identity.clone();
                    edit(&mut product.producer, |v| v["reference"] = json!(identity));
                }
                let error = bad.validate(&plan, &workflow()).unwrap_err().to_string();
                assert!(
                    error.contains("epoch") || error.contains("invalid type"),
                    "{lane}/{field}: {error}"
                );
            }
        }
    }
}

#[test]
fn app_timestamp_observations_cover_the_whole_reference_tree_at_exact_zip_precision() {
    let (_, plan, good) = fixture(false);
    for lane in ["app", "desktop", "picker"] {
        for pointer in [
            "/verification/source_epoch",
            "/verification/source_provenance",
            "/verification/bundle_timestamps",
            "/verification/bundle_timestamps/source_epoch",
            "/verification/bundle_timestamps/zip_epoch_seconds",
            "/verification/bundle_timestamps/precision",
            "/verification/bundle_timestamps/entries/",
            "/verification/bundle_timestamps/entries/Contents",
            "/verification/bundle_timestamps/entries/Contents~1Info.plist/mtime_seconds",
            "/verification/bundle_timestamps/entries/Contents~1MacOS~1EMLAttachmentRemover/mtime_nanoseconds",
        ] {
            let mut bad = clone_inputs(&good);
            let product = bad.products.get_mut(MAC_APP).unwrap();
            let record = match lane {
                "app" => &mut product.producer,
                "desktop" => product.desktop.as_mut().unwrap(),
                "picker" => product.picker_product.as_mut().unwrap(),
                _ => unreachable!(),
            };
            edit(record, |v| *v.pointer_mut(pointer).unwrap() = Value::Null);
            assert!(bad.validate(&plan, &workflow()).is_err(), "{lane}{pointer}");
        }
    }
    let reference = good
        .products
        .get(MAC_APP)
        .unwrap()
        .reference
        .as_ref()
        .unwrap()
        .document()
        .unwrap();
    let base = json!({"source_epoch":1609459201,"source_provenance":"git_commit","bundle_timestamps":timestamps()});
    for (pointer, wrong) in [
        ("/bundle_timestamps/source_epoch", json!(1609459200)),
        ("/bundle_timestamps/zip_epoch_seconds", json!(1609459202)),
        ("/bundle_timestamps/precision", json!("UTC nearest second")),
        (
            "/bundle_timestamps/entries/Contents~1Info.plist/kind",
            json!("directory"),
        ),
        (
            "/bundle_timestamps/entries/Contents~1Info.plist/mtime_seconds",
            json!(1609459201),
        ),
        (
            "/bundle_timestamps/entries/Contents~1Info.plist/mtime_nanoseconds",
            json!(1),
        ),
    ] {
        let mut wrong_record = base.clone();
        *wrong_record.pointer_mut(pointer).unwrap() = wrong;
        assert!(
            bundle_timestamps(&wrong_record, &reference, &plan).is_err(),
            "{pointer}"
        );
    }
    for target in ["header", "row", "entry"] {
        let mut extra = base.clone();
        match target {
            "header" => extra["bundle_timestamps"]["unexpected"] = true.into(),
            "row" => extra["bundle_timestamps"]["entries"]["Contents"]["unexpected"] = true.into(),
            "entry" => {
                extra["bundle_timestamps"]["entries"]["unlisted"] =
                    json!({"kind":"file","mtime_seconds":1609459200,"mtime_nanoseconds":0})
            }
            _ => unreachable!(),
        }
        assert!(
            bundle_timestamps(&extra, &reference, &plan).is_err(),
            "{target}"
        );
    }
    for path in [
        "../outside",
        "Contents/Info.plist/child",
        "contents/other",
        "Contents/",
    ] {
        let mut bad_reference = reference.clone();
        bad_reference["files"][path] = json!([identity(), 420]);
        assert!(
            bundle_timestamps(&base, &bad_reference, &plan).is_err(),
            "{path}"
        );
    }
}

#[test]
fn every_literal_panel_case_requires_used_restored_clipboard_evidence() {
    let (_, plan, good) = fixture(false);
    good.validate(&plan, &workflow()).unwrap();
    // This list is an independent authored-method obligation, not a projection
    // of the production native-case catalog.
    for name in [
        "workflow",
        "blocked-export-preflight",
        "blocked-export-readback",
        "blocked-export-stage-write",
        "blocked-export-file-sync",
        "blocked-export-publication",
        "blocked-export-namespace-sync",
        "export-publication",
        "export-writer-block",
    ] {
        for failure in ["unused", "lost", "settled_only", "missing"] {
            let mut bad = clone_inputs(&good);
            edit(
                bad.products
                    .get_mut(MAC_APP)
                    .unwrap()
                    .desktop
                    .as_mut()
                    .unwrap(),
                |value| {
                    let interaction = &mut value["verification"]["gui_interaction"];
                    let case = if name == "workflow" {
                        &mut interaction["workflow"]
                    } else {
                        interaction["operation_controls"]["cases"]
                            .as_array_mut()
                            .unwrap()
                            .iter_mut()
                            .find(|v| v["case"] == name)
                            .unwrap()
                    };
                    let owner = &mut case["lifetime_owner"];
                    match failure {
                        "unused" => {
                            owner["panel_path_clipboard"] =
                                json!({"state":"unused","operations":[],"failure":null})
                        }
                        "lost" => {
                            owner["panel_path_clipboard"]["operations"][0]["state"] =
                                json!("ownership_lost")
                        }
                        "settled_only" => {
                            owner["panel_path_clipboard"]["operations"][0]["restore_phase"] =
                                json!("runner_settled")
                        }
                        "missing" => {
                            owner
                                .as_object_mut()
                                .unwrap()
                                .remove("panel_path_clipboard");
                        }
                        _ => unreachable!(),
                    }
                },
            );
            assert!(
                bad.validate(&plan, &workflow()).is_err(),
                "{name}: {failure}"
            );
        }
    }
}

fn literal_windows_query_selection() -> Value {
    json!([
        {"name":"windows::name_domain::tests::native_locality_query_retains_actual_completion_fields","discovered":1,"executed":1},
        {"name":"windows::name_domain::tests::native_failed_wait_retains_concrete_query_owners_until_event_and_thread_settle","discovered":1,"executed":1},
        {"name":"windows::name_domain::tests::only_exact_complete_local_disk_success_qualifies","discovered":1,"executed":1}
    ])
}
#[test]
fn windows_release_admission_requires_retained_native_sync_records_not_only_test_success() {
    let plan = BuildPlan::test_plan();
    let name = "emlattachmentremover-7.0.0-windows-x86_64.zip";
    let target = "x86_64-pc-windows-gnullvm";
    let mut record = json!({"source":SOURCE,"source_epoch":plan.source_epoch(),"source_provenance":"git_commit","target":target,"workflow":workflow(),"host":host(target,"windows","x86_64",26),"archive":{"identity":identity()},"cli_image":identity(),"qualification_failures":[],"name_profiles":crate::name_profiles::valid_fixture(SOURCE,&identity(),OsFamily::Windows),"release_tests":{"windows_existing_sync":crate::windows_sync_evidence::test_fixture(SOURCE,u64::from(plan.source_epoch().seconds()),target,"release"),"ntfs_query":crate::ntfs_query_evidence::test_summary("x86_64"),"ntfs_query_test_cases":literal_windows_query_selection(),"native_publication":crate::publication_evidence::test_fixture(SOURCE,u64::from(plan.source_epoch().seconds()),target,"windows","x86_64","release")}});
    let product = |value: Value| ProductInput {
        platform: "windows-x86_64".into(),
        role: ProductRole::Cli,
        archive: identity(),
        archive_path: PathBuf::from(name),
        producer: frozen(value),
        reference: None,
        desktop: None,
        picker: None,
        picker_product: None,
    };
    assert!(check_product(&product(record.clone()), name, &plan, SOURCE, &workflow()).is_ok());
    let mut no_query = record.clone();
    no_query["release_tests"]
        .as_object_mut()
        .unwrap()
        .remove("ntfs_query");
    assert!(check_product(&product(no_query), name, &plan, SOURCE, &workflow()).is_err());
    let mut no_query_execution = record.clone();
    no_query_execution["release_tests"]["ntfs_query_test_cases"][0]["executed"] =
        serde_json::json!(0);
    assert!(
        check_product(
            &product(no_query_execution),
            name,
            &plan,
            SOURCE,
            &workflow()
        )
        .is_err()
    );
    record["release_tests"]
        .as_object_mut()
        .unwrap()
        .remove("windows_existing_sync");
    assert!(check_product(&product(record), name, &plan, SOURCE, &workflow()).is_err());
    let mut record = product(json!({}));
    record.producer = frozen(
        json!({"source":SOURCE,"source_epoch":plan.source_epoch(),"source_provenance":"git_commit","target":target,"workflow":workflow(),"host":host(target,"windows","x86_64",26),"archive":{"identity":identity()},"cli_image":identity(),"qualification_failures":[],"name_profiles":crate::name_profiles::valid_fixture(SOURCE,&identity(),OsFamily::Windows),"release_tests":{"windows_existing_sync":crate::windows_sync_evidence::test_fixture(SOURCE,u64::from(plan.source_epoch().seconds()),target,"test"),"ntfs_query":crate::ntfs_query_evidence::test_summary("x86_64"),"ntfs_query_test_cases":literal_windows_query_selection(),"native_publication":crate::publication_evidence::test_fixture(SOURCE,u64::from(plan.source_epoch().seconds()),target,"windows","x86_64","release")}}),
    );
    assert!(check_product(&record, name, &plan, SOURCE, &workflow()).is_err());
}

#[test]
fn every_product_requires_actual_publication_records_and_rechecks_rehashed_edge_facts() {
    let (_, plan, value) = fixture(false);
    for name in [LINUX, MAC_CLI, MAC_APP] {
        let mut bad = clone_inputs(&value);
        edit(&mut bad.products.get_mut(name).unwrap().producer, |v| {
            v["release_tests"]
                .as_object_mut()
                .unwrap()
                .remove("native_publication");
        });
        assert!(bad.validate(&plan, &workflow()).is_err(), "missing {name}");
        let mut bad = clone_inputs(&value);
        edit(&mut bad.products.get_mut(name).unwrap().producer, |v| {
            let record = &mut v["release_tests"]["native_publication"]["records"][2];
            let mut raw: Value =
                serde_json::from_str(record["document"].as_str().unwrap()).unwrap();
            raw["native_edge"]["succeeded"] = json!(true);
            let text = serde_json::to_string(&raw).unwrap();
            record["identity"] = serde_json::to_value(ByteIdentity::of(text.as_bytes())).unwrap();
            record["document"] = json!(text);
        });
        assert!(
            bad.validate(&plan, &workflow()).is_err(),
            "changed native edge {name}"
        );
    }
}
