//! Literal synthetic observations exercise admission; they are not native execution.
use super::*;
const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn id(n: u8) -> Value {
    let mut bytes = [0u8; 16];
    bytes[0] = n;
    json!({"volume":7,"file":bytes})
}
fn sig(n: u8, bytes: &[u8]) -> Value {
    json!({"identity":id(n),"size":bytes.len(),"modified_native":[101,0],"changed_native":[100,0]})
}
fn file(n: u8, bytes: &[u8]) -> Value {
    json!({"signature":sig(n,bytes),"parent":id(2),"bytes":bytes,"readonly":false})
}
fn path(name: &str) -> Value {
    json!(
        format!("\\\\?\\C:\\owned\\{name}")
            .encode_utf16()
            .collect::<Vec<_>>()
    )
}
fn held(source: &Value) -> Value {
    json!({"file":source,"write_open_succeeded":false,"write_open_native_code":32})
}
fn observed(op: &str, result: &str) -> Value {
    json!({"operation":op,"result":result,"signature":null,"parent":null,"source":null,"bytes_equal":null,"native_code":null,"sync":null,"namespace":null})
}
fn full(op: &str, result: &str, copy: &Value) -> Value {
    let mut s = observed(op, result);
    s["signature"] = copy["signature"].clone();
    s["parent"] = copy["parent"].clone();
    s
}
fn finish(v: Value) -> Value {
    let text = serde_json::to_string(&v).unwrap();
    json!({"identity":ByteIdentity::of(text.as_bytes()),"document":text})
}
/// All ten cases and their operation order are authored independently of CASES.
pub(crate) fn fixture(source: &str, epoch: u64, target: &str, profile: &str) -> Value {
    let architecture = if target.starts_with("aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let cases: [(&str, &str, &[&str], &str); 10] = [
        (
            "adapter-handoff-success",
            "eml_fs",
            &[
                "comparison_opened",
                "comparison_read",
                "readonly_flush",
                "comparison_closed",
                "sync_opened",
                "sync_read",
                "file_flush",
                "sync_closed",
            ],
            "synchronized",
        ),
        (
            "adapter-readonly-sync-denied",
            "eml_fs",
            &[
                "comparison_opened",
                "comparison_read",
                "comparison_closed",
                "sync_open_denied",
            ],
            "sync_access_denied",
        ),
        (
            "adapter-close-required",
            "eml_fs",
            &[
                "comparison_opened",
                "sync_open_denied",
                "comparison_closed",
                "sync_opened",
                "sync_closed",
            ],
            "checked_handoff",
        ),
        (
            "adapter-parent-retarget",
            "eml_fs",
            &["comparison_opened", "parent_retargeted", "binding_refused"],
            "parent_changed",
        ),
        (
            "core-readonly-equal",
            "execution",
            &["core_returned"],
            "EXISTING_SYNC_FAILED",
        ),
        (
            "core-readonly-unequal",
            "execution",
            &["core_returned"],
            "DESTINATION_EXISTS",
        ),
        (
            "core-sync-success",
            "eml_core",
            &[
                "comparison_matched",
                "comparison_closed",
                "sync_opened",
                "binding_matched",
                "recompared",
                "file_flush",
                "endpoint_flush",
                "requalified",
            ],
            "up_to_date",
        ),
        (
            "core-gap-same-id",
            "eml_core",
            &[
                "comparison_matched",
                "comparison_closed",
                "sync_opened",
                "binding_refused",
            ],
            "DESTINATION_EXISTS",
        ),
        (
            "core-gap-replacement",
            "eml_core",
            &[
                "comparison_matched",
                "comparison_closed",
                "sync_opened",
                "binding_refused",
            ],
            "ADDRESS_CHANGED",
        ),
        (
            "core-source-alias",
            "existing",
            &["comparison_refused"],
            "source_alias",
        ),
    ];
    let mut records = Vec::new();
    for (name, group, operations, outcome) in cases {
        let original = file(3, &[1, 2, 3]);
        let mut before = file(4, &[9, 8]);
        if name.contains("readonly") {
            before["readonly"] = json!(true);
        }
        if name == "core-source-alias" {
            before = original.clone();
        }
        let mut after = before.clone();
        if name == "core-gap-same-id" {
            after["bytes"] = json!([6, 5, 4]);
            after["signature"]["size"] = json!(3);
            after["signature"]["modified_native"] = json!([102, 0]);
        }
        if name == "core-gap-replacement" {
            after["signature"]["identity"] = id(5);
        }
        if name == "adapter-parent-retarget" {
            after["parent"] = id(6);
        }
        let has_source = name.starts_with("core-") || name == "adapter-handoff-success";
        let source_value = if has_source {
            original.clone()
        } else {
            Value::Null
        };
        let source_steps = name == "adapter-handoff-success"
            || name == "core-source-alias"
            || name.starts_with("core-gap-")
            || name == "core-sync-success";
        let mut steps = Vec::new();
        for op in operations {
            let mut s = match *op {
                "comparison_opened" => full(op, "succeeded", &before),
                "comparison_read" | "comparison_matched" => {
                    let mut v = full(op, "equal", &before);
                    v["bytes_equal"] = json!(true);
                    v
                }
                "sync_opened" | "binding_matched" => full(op, "succeeded", &after),
                "sync_read" | "recompared" => {
                    let mut v = full(op, "equal", &after);
                    v["bytes_equal"] = json!(true);
                    v
                }
                "requalified" => {
                    let mut v = full(op, "succeeded", &after);
                    v["bytes_equal"] = json!(true);
                    v
                }
                "parent_retargeted" => full(op, "changed", &after),
                "binding_refused" => {
                    if name == "adapter-parent-retarget" {
                        observed(op, "refused")
                    } else {
                        full(op, "refused", &after)
                    }
                }
                "comparison_refused" => full(op, "refused", &before),
                "core_returned" => {
                    let mut v = full(op, "refused", &after);
                    v["bytes_equal"] = json!(name == "core-readonly-equal");
                    v["native_code"] = if name == "core-readonly-equal" {
                        json!(5)
                    } else {
                        Value::Null
                    };
                    v["namespace"] = json!({"receipt":null,"required_edges":if name=="core-readonly-equal"{1}else{0},"completed_edges":0,"failed_edge_index":null});
                    v
                }
                "readonly_flush" => {
                    let mut v = observed(op, "failed");
                    v["native_code"] = json!(5);
                    v["sync"] =
                        json!({"method":"FlushFileBuffers","state":"Failed","native_code":5});
                    v
                }
                "sync_open_denied" => {
                    let mut v = observed(op, "failed");
                    v["native_code"] = json!(if name == "adapter-close-required" {
                        32
                    } else {
                        5
                    });
                    v
                }
                "file_flush" => {
                    let mut v = observed(op, "succeeded");
                    v["sync"] =
                        json!({"method":"FlushFileBuffers","state":"Succeeded","native_code":null});
                    v
                }
                "endpoint_flush" => {
                    let mut v = observed(op, "succeeded");
                    v["parent"] = after["parent"].clone();
                    v["namespace"] = json!({"receipt":{"method":"FlushFileBuffers","state":"Succeeded","native_code":null},"required_edges":1,"completed_edges":1,"failed_edge_index":null});
                    v
                }
                _ => observed(op, "succeeded"),
            };
            if source_steps {
                s["source"] = held(&original);
            }
            steps.push(s);
        }
        let pid = match group {
            "eml_fs" => 11,
            "eml_core" => 12,
            "execution" => 13,
            _ => 14,
        };
        records.push(finish(json!({"schema":"eml-windows-existing-sync-evidence","schema_version":1,"case":name,"architecture":architecture,"process_id":pid,"current_exe_utf16":path(&format!("{group}-0123456789abcdef.exe")),"fixture":{"root_utf16":path("fixture"),"identity":id(1),"filesystem":"NTFS"},"requested_parent_utf16":path("fixture"),"before":{"source":source_value,"copy":before},"steps":steps,"outcome":{"classification":outcome,"native_code":if name=="adapter-readonly-sync-denied"||name=="core-readonly-equal"{json!(5)}else{Value::Null}},"after":{"source":source_value,"copy":after}})));
    }
    let control = finish(
        json!({"native_exit_code":0,"timed_out":false,"cancelled":false,"cleanup_complete":true,"unexpected_survivors":0,"cleanup_error":null,"observation_error":null,"termination_error":null}),
    );
    let identity = ByteIdentity::of(b"actual immutable test image");
    let images:Vec<_>=["eml_fs","eml_core","execution","existing"].iter().map(|g|json!({"path_utf16":path(&format!("{g}-0123456789abcdef.exe")),"identity":identity})).collect();
    let names = [
        "held_readonly_file_must_be_checked_closed_before_fresh_sync_access",
        "native_existing_sync_access_keeps_identity_bytes_and_self_validation",
        "readonly_existing_file_is_readable_but_sync_access_is_refused_without_changes",
        "retargeted_parent_with_same_file_hardlink_is_a_changed_binding",
        "identical_readonly_copy_retains_equality_and_never_clears_its_attribute",
        "readable_different_readonly_copy_is_a_conflict_before_any_sync_access",
        "controlled_resync_gap_distinguishes_file_replacement_and_same_id_change",
        "hardlink_to_current_source_is_refused_before_byte_comparison",
    ];
    json!({"schema":"eml-windows-existing-sync-campaign","schema_version":1,"source":source,"source_epoch":epoch,"source_provenance":"git_commit","target":target,"profile":profile,"host":{"target":target,"process_os":"windows","process_architecture":architecture,"native_machine":if architecture=="x86_64"{0x8664}else{0xaa64},"emulated_process_machine":0,"actual_kernel":{"major":10,"minor":0,"build":20348},"compiler":format!("rustc pinned fixture\nhost: {target}\n")},"test_execution":{"discovery_identity":ByteIdentity::of(b"actual discovery"),"execution_identity":ByteIdentity::of(b"actual execution"),"control":control,"required_tests":names.iter().map(|n|json!({"name":n,"discovered":1,"executed":1})).collect::<Vec<_>>()},"images":images,"records":records})
}
fn valid(v: &Value) -> bool {
    validate(v, SOURCE, 123, "x86_64-pc-windows-gnullvm", "release").is_ok()
}
fn edit(v: &mut Value, index: usize, change: impl FnOnce(&mut Value)) {
    let mut r = retained(&v["records"][index], MAX_RECORD).unwrap();
    change(&mut r);
    v["records"][index] = finish(r);
}
#[test]
fn literal_native_sync_records_require_complete_ordered_full_native_observations() {
    let good = fixture(SOURCE, 123, "x86_64-pc-windows-gnullvm", "release");
    assert!(valid(&good));
    let arm = fixture(SOURCE, 123, "aarch64-pc-windows-gnullvm", "test");
    assert!(validate(&arm, SOURCE, 123, "aarch64-pc-windows-gnullvm", "test").is_ok());
    let mut bad = good.clone();
    bad["records"].as_array_mut().unwrap().pop();
    assert!(!valid(&bad));
    let mut bad = good.clone();
    bad["records"][1] = bad["records"][0].clone();
    assert!(!valid(&bad));
    for mutate in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
        let mut bad = good.clone();
        edit(&mut bad, 6, |r| match mutate {
            0 => r["steps"].as_array_mut().unwrap().swap(1, 2),
            1 => {
                r["steps"][3]["signature"]["changed_native"] = json!([100]);
            }
            2 => {
                r["steps"][2]["source"]["write_open_native_code"] = json!(5);
            }
            3 => {
                r["steps"][5]["sync"]["native_code"] = json!(5);
            }
            4 => {
                r["after"]["source"]["bytes"] = json!([8, 8, 8]);
            }
            5 => {
                r["steps"][1]["source"] = Value::Null;
            }
            6 => {
                r["steps"][6]["namespace"]["completed_edges"] = json!(0);
            }
            7 => {
                r["steps"][4]["bytes_equal"] = json!(false);
            }
            8 => {
                r["steps"][1].as_object_mut().unwrap().remove("signature");
            }
            9 => {
                r["steps"][2]["parent"]["file"] = json!([1, 2]);
            }
            10 => {
                r["fixture"]["filesystem"] = json!("server NTFS");
            }
            _ => {
                r["process_id"] = json!(0);
            }
        });
        assert!(!valid(&bad), "substitution {mutate}");
    }
}
#[test]
fn retained_native_records_reject_rehashable_data_causes_images_and_process_substitutions() {
    let good = fixture(SOURCE, 123, "x86_64-pc-windows-gnullvm", "release");
    for index in [1, 2, 3, 4, 5, 7, 8, 9] {
        let mut bad = good.clone();
        edit(&mut bad, index, |r| {
            r["outcome"]["native_code"] = json!(999);
        });
        assert!(!valid(&bad));
    }
    let mut bad = good.clone();
    edit(&mut bad, 7, |r| {
        r["after"]["copy"] = r["before"]["copy"].clone();
    });
    assert!(!valid(&bad));
    let mut bad = good.clone();
    edit(&mut bad, 8, |r| {
        r["after"]["copy"]["signature"]["identity"] =
            r["before"]["copy"]["signature"]["identity"].clone();
    });
    assert!(!valid(&bad));
    let mut bad = good.clone();
    edit(&mut bad, 3, |r| {
        r["after"]["copy"]["parent"] = r["before"]["copy"]["parent"].clone();
    });
    assert!(!valid(&bad));
    let mut bad = good.clone();
    edit(&mut bad, 1, |r| {
        r["process_id"] = json!(1234);
    });
    assert!(!valid(&bad));
    let mut bad = good.clone();
    edit(&mut bad, 0, |r| {
        r["current_exe_utf16"] = path("unlaunched-0123.exe");
    });
    assert!(!valid(&bad));
    let mut bad = good.clone();
    let unrelated = json!({"path_utf16":path("unrelated-0123.exe"),"identity":ByteIdentity::of(b"unrelated image")});
    bad["images"].as_array_mut().unwrap().push(unrelated);
    assert!(!valid(&bad));
    let mut bad = good.clone();
    bad["test_execution"]["required_tests"][0]["executed"] = json!(0);
    assert!(!valid(&bad));
    let mut bad = good.clone();
    bad["records"][0]["document"] = json!("{}");
    assert!(!valid(&bad));
    let mut bad = good.clone();
    bad["source"] = json!("b".repeat(40));
    assert!(!valid(&bad));
    let mut bad = good.clone();
    let document = bad["records"][0]["document"].as_str().unwrap().replacen(
        "{",
        "{\"case\":\"adapter-handoff-success\",",
        1,
    );
    bad["records"][0] =
        json!({"identity":ByteIdentity::of(document.as_bytes()),"document":document});
    assert!(!valid(&bad));
}

#[test]
fn default_compiler_host_checks_keep_actual_records_without_qualifying_a_shipping_target() {
    let mut local = fixture(SOURCE, 123, "x86_64-pc-windows-gnullvm", "test");
    local["target"] = Value::Null;
    local["host"]["target"] = json!("x86_64-pc-windows-msvc");
    local["host"]["compiler"] = json!("rustc pinned fixture\nhost: x86_64-pc-windows-msvc\n");
    assert!(validate_observations(&local, SOURCE, 123, None, "test").is_ok());
    local["source_provenance"] = json!("declared_archive");
    assert!(validate_observations(&local, SOURCE, 123, None, "test").is_ok());
    let mut attempted_promotion = local.clone();
    attempted_promotion["target"] = json!("x86_64-pc-windows-gnullvm");
    attempted_promotion["host"]["target"] = json!("x86_64-pc-windows-gnullvm");
    attempted_promotion["host"]["compiler"] =
        json!("rustc pinned fixture\nhost: x86_64-pc-windows-gnullvm\n");
    assert!(
        validate(
            &attempted_promotion,
            SOURCE,
            123,
            "x86_64-pc-windows-gnullvm",
            "test"
        )
        .is_err()
    );

    assert!(validate(&local, SOURCE, 123, "x86_64-pc-windows-gnullvm", "test").is_err());
    edit(&mut local, 6, |r| {
        r["steps"].as_array_mut().unwrap().swap(1, 2);
    });
    assert!(validate_observations(&local, SOURCE, 123, None, "test").is_err());
}
