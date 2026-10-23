use super::*;
fn platform(family: OsFamily) -> PlatformId {
    PlatformId::parse(match family {
        OsFamily::Linux => "linux-x86_64",
        OsFamily::Macos => "macos-x86_64",
        OsFamily::Windows => "windows-x86_64",
    })
    .unwrap()
}
#[test]
fn complete_independent_profile_obligations_reject_omissions_and_false_observations() {
    let image = ByteIdentity::of(b"owned shipping executable");
    for family in [OsFamily::Linux, OsFamily::Macos, OsFamily::Windows] {
        let complete = valid_fixture("owned-source", &image, family);
        validate(&complete, "owned-source", &image, platform(family)).unwrap();
        let mut omitted = complete.clone();
        omitted["observations"]["profiles"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(validate(&omitted, "owned-source", &image, platform(family)).is_err());
        let mut duplicate = complete.clone();
        let first = duplicate["observations"]["profiles"][0].clone();
        duplicate["observations"]["profiles"]
            .as_array_mut()
            .unwrap()
            .push(first);
        assert!(validate(&duplicate, "owned-source", &image, platform(family)).is_err());
        for pointer in [
            "/process/cleanup_complete",
            "/observations/profiles/0/behavior/passed",
        ] {
            let mut bad = complete.clone();
            *bad.pointer_mut(pointer).unwrap() = json!(false);
            assert!(validate(&bad, "owned-source", &image, platform(family)).is_err());
        }
        for (pointer, bad_value) in [
            ("/source", json!("other-source")),
            ("/observations/binary_sha256", json!("other-image")),
            (
                "/observations/profiles/0/status",
                json!("failed_or_unavailable"),
            ),
            ("/observations/profiles/0/exit_code", json!(48)),
            (
                "/observations/profiles/0/behavior/case_mode",
                json!("unobserved"),
            ),
            (
                "/observations/profiles/0/behavior/expected/0/token",
                json!("bad"),
            ),
            (
                "/observations/profiles/0/behavior/runs/2/statuses/0",
                json!("created"),
            ),
        ] {
            let mut bad = complete.clone();
            *bad.pointer_mut(pointer).unwrap() = bad_value;
            assert!(
                validate(&bad, "owned-source", &image, platform(family)).is_err(),
                "{pointer}"
            );
        }
    }
}

#[test]
fn existing_sync_refusal_cannot_hide_contradictory_operation_order_or_probe_failure() {
    let image = ByteIdentity::of(b"owned shipping executable");
    let baseline = valid_fixture("owned-source", &image, OsFamily::Linux);
    validate(&baseline, "owned-source", &image, platform(OsFamily::Linux)).unwrap();
    for contradiction in [
        "endpoint-after-file-failure",
        "endpoint-without-file-sync",
        "independent-io-failure",
    ] {
        let mut value = baseline.clone();
        let behavior = &mut value["observations"]["existing_profiles"][1]["behavior"];
        behavior["independent_sync_probe"]["file_errno"] = json!(95);
        let run = &mut behavior["runs"][0];
        run["outcome"] = json!("native_sync_refused");
        run["exit_code"] = json!(1);
        run["item"]["status"] = json!("failed");
        run["item"]["error"] = json!({"code":"EXISTING_SYNC_FAILED","phase":"existing"});
        run["item"]["existing_observation"]["file_sync"] =
            json!({"method":"fsync","state":"unsupported","native_code":95});
        match contradiction {
            "endpoint-after-file-failure" => {} // baseline retains a successful endpoint call
            "endpoint-without-file-sync" => {
                run["item"]["existing_observation"]["file_sync"] =
                    json!({"method":"not_requested","state":"not_attempted","native_code":null});
                run["item"]["existing_observation"]["namespace_sync"] = json!({"method":"fsync","state":"unsupported","native_code":95,"required_edges":1,"completed_edges":0,"failed_edge_index":0});
                behavior["independent_sync_probe"]["file_errno"] = Value::Null;
                behavior["independent_sync_probe"]["endpoint_errno"] = json!(95);
            }
            "independent-io-failure" => {
                run["item"]["existing_observation"]["namespace_sync"] = json!({"method":"not_requested","state":"not_attempted","native_code":null,"required_edges":1,"completed_edges":0,"failed_edge_index":null});
                behavior["independent_sync_probe"]["endpoint_errno"] = json!(5);
            }
            _ => unreachable!(),
        }
        assert!(
            validate(&value, "owned-source", &image, platform(OsFamily::Linux)).is_err(),
            "{contradiction}"
        );
    }
}

#[test]
fn overlay_unsupported_file_retains_unattempted_required_endpoint() {
    let image = ByteIdentity::of(b"owned shipping executable");
    let mut value = valid_fixture("owned-source", &image, OsFamily::Linux);
    let behavior = &mut value["observations"]["existing_profiles"][1]["behavior"];
    behavior["independent_sync_probe"]["file_errno"] = json!(95);
    let run = &mut behavior["runs"][0];
    run["outcome"] = json!("native_sync_refused");
    run["exit_code"] = json!(1);
    run["item"]["status"] = json!("failed");
    run["item"]["error"] = json!({"code":"EXISTING_SYNC_FAILED","phase":"existing"});
    run["item"]["existing_observation"]["file_sync"] =
        json!({"method":"fsync","state":"unsupported","native_code":95});
    run["item"]["existing_observation"]["namespace_sync"] = json!({"method":"not_requested","state":"not_attempted","native_code":null,"required_edges":1,"completed_edges":0,"failed_edge_index":null});
    validate(&value, "owned-source", &image, platform(OsFamily::Linux)).unwrap();
}

#[test]
fn qualified_existing_copy_cannot_retain_an_error_or_failed_native_cause() {
    let image = ByteIdentity::of(b"owned shipping executable");
    let baseline = valid_fixture("owned-source", &image, OsFamily::Linux);
    for (pointer, contradiction) in [
        (
            "/observations/existing_profiles/0/behavior/runs/0/item/error",
            json!({"code":"EXISTING_SYNC_FAILED","phase":"existing"}),
        ),
        (
            "/observations/existing_profiles/0/behavior/runs/0/item/existing_observation/file_sync/native_code",
            json!(5),
        ),
    ] {
        let mut value = baseline.clone();
        *value.pointer_mut(pointer).unwrap() = contradiction;
        assert!(
            validate(&value, "owned-source", &image, platform(OsFamily::Linux)).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn native_source_substitution_cannot_disagree_with_observed_original_selection() {
    let image = ByteIdentity::of(b"owned shipping executable");
    let mut value = valid_fixture("owned-source", &image, OsFamily::Linux);
    let behavior = &mut value["observations"]["profiles"][0]["behavior"];
    let substituted =
        json!({"encoding":"unix-bytes","base64":STANDARD.encode(b"/substituted/source")});
    behavior["expected"][0]["source_native"] = substituted.clone();
    for run in behavior["runs"].as_array_mut().unwrap() {
        run["destinations"][0]["source_native"] = substituted.clone();
    }
    assert!(validate(&value, "owned-source", &image, platform(OsFamily::Linux)).is_err());
}

#[test]
fn linux_profile_labels_cannot_substitute_for_actual_filesystem_or_settled_cleanup() {
    let image = ByteIdentity::of(b"owned shipping executable");
    let baseline = valid_fixture("owned-source", &image, OsFamily::Linux);
    validate(&baseline, "owned-source", &image, platform(OsFamily::Linux)).unwrap();
    for (pointer, contradiction) in [
        (
            "/observations/profiles/0/mount_record/filesystems/0/fstype",
            json!("overlay"),
        ),
        (
            "/observations/existing_profiles/1/mount_record/filesystems/0/fstype",
            json!("ext4"),
        ),
        (
            "/observations/profiles/0/cleanup/residual_mounts",
            json!(true),
        ),
        (
            "/observations/existing_profiles/1/cleanup/observation_complete",
            json!(false),
        ),
        (
            "/observations/existing_profiles/1/cleanup/forced_detach",
            json!(true),
        ),
    ] {
        let mut value = baseline.clone();
        *value.pointer_mut(pointer).unwrap() = contradiction;
        assert!(
            validate(&value, "owned-source", &image, platform(OsFamily::Linux)).is_err(),
            "{pointer}"
        );
    }
}

// These are admission-fixture mutations, not execution on a Windows filesystem.
#[test]
fn windows_vhd_case_alias_and_settlement_facts_are_required_on_both_native_cpus() {
    let image = ByteIdentity::of(b"owned shipping executable");
    for (target, host_architecture, machine, wrong_machine) in [
        ("windows-x86_64", "X64", 0x8664, 0xaa64),
        ("windows-aarch64", "Arm64", 0xaa64, 0x8664),
    ] {
        let target = PlatformId::parse(target).unwrap();
        let mut baseline = valid_fixture("owned-source", &image, OsFamily::Windows);
        baseline["observations"]["host"]["architecture"] = json!(host_architecture);
        baseline["observations"]["storage"]["native_cli_machine"] = json!(machine);
        validate(&baseline, "owned-source", &image, target).unwrap();
        for pointer in [
            "/observations/storage",
            "/observations/storage/physical_path",
            "/observations/storage/image",
            "/observations/storage/disk_number",
            "/observations/storage/disk_unique_id",
            "/observations/storage/label",
            "/observations/storage/native_cli_machine",
            "/observations/storage/lifetime",
            "/observations/storage/volume_serial",
            "/observations/storage/volume_root",
            "/observations/cleanup",
            "/observations/cleanup/observation_complete",
            "/observations/profiles/0/executed",
            "/observations/profiles/0/volume/case/case_sensitive",
            "/observations/profiles/0/short_root_check",
        ] {
            let mut bad = baseline.clone();
            remove_member(&mut bad, pointer);
            assert!(
                validate(&bad, "owned-source", &image, target).is_err(),
                "missing {pointer} on {target:?}"
            );
        }
        for (pointer, contradiction) in [
            (
                "/observations/storage/native_cli_machine",
                json!(wrong_machine),
            ),
            ("/observations/host/architecture", json!("other-native-CPU")),
            (
                "/observations/storage/lifetime",
                json!("permanent-lifetime"),
            ),
            ("/observations/storage/image", json!("existing-volume.vhd")),
            (
                "/observations/storage/image",
                json!("\\\\server\\share\\unowned.vhd"),
            ),
            ("/observations/storage/disk_number", json!(10)),
            ("/observations/storage/disk_unique_id", json!("")),
            (
                "/observations/storage/label",
                json!("existing-owner-volume"),
            ),
            ("/observations/storage/filesystem", json!("ReFS")),
            ("/observations/storage/drive_type", json!(4)),
            ("/observations/storage/global_8dot3_policy", json!(1)),
            (
                "/observations/storage/physical_path",
                json!("PhysicalDrive9"),
            ),
            ("/observations/binary_unchanged", json!(false)),
            ("/observations/cleanup/detached", json!(false)),
            ("/observations/cleanup/observation_complete", json!(false)),
            ("/observations/cleanup/forced_detach", json!(true)),
            ("/observations/profiles/0/executed", json!(false)),
            (
                "/observations/profiles/0/volume/case/case_sensitive",
                json!(true),
            ),
            (
                "/observations/profiles/1/volume/case/case_sensitive",
                json!(false),
            ),
            (
                "/observations/profiles/0/volume/short_root",
                json!("S:\\EMLQUA~1"),
            ),
            (
                "/observations/profiles/0/volume/short_root",
                json!("R:\\no-observed-alias"),
            ),
            ("/observations/profiles/0/volume/serial", json!("5678")),
            (
                "/observations/profiles/0/volume/label",
                json!("different-volume"),
            ),
        ] {
            let mut bad = baseline.clone();
            *bad.pointer_mut(pointer).unwrap() = contradiction;
            assert!(
                validate(&bad, "owned-source", &image, target).is_err(),
                "{pointer} on {target:?}"
            );
        }
        let mut unbound_physical = baseline.clone();
        unbound_physical["observations"]["storage"]["physical_path"] =
            json!("\\\\.\\PhysicalDrive4294967296");
        remove_member(&mut unbound_physical, "/observations/storage/disk_number");
        assert!(
            validate(&unbound_physical, "owned-source", &image, target).is_err(),
            "overflowing physical drive with missing disk number"
        );
        for field in ["failure", "observation_failure"] {
            let mut bad = baseline.clone();
            bad["observations"]["cleanup"][field] = json!("Actual detach observation failed");
            assert!(
                validate(&bad, "owned-source", &image, target).is_err(),
                "cleanup {field}"
            );
        }
    }
}

#[test]
fn windows_short_root_check_cannot_hide_writes_syncs_incomplete_stream_or_changed_originals() {
    let image = ByteIdentity::of(b"owned shipping executable");
    let baseline = valid_fixture("owned-source", &image, OsFamily::Windows);
    let target = platform(OsFamily::Windows);
    let prefix = "/observations/profiles/0/short_root_check";
    for (suffix, contradiction) in [
        ("namespace_unchanged", json!(false)),
        ("namespace_after_sha256", json!("e".repeat(64))),
        ("original_identity_after", json!("different-owned-original")),
        ("original_sha256_after", json!("f".repeat(64))),
        ("events/0/dry_run", json!(false)),
        ("events/0/collection_complete", json!(false)),
        ("events/0/total", json!(2)),
        ("events/1/index", json!(1)),
        ("events/1/status", json!("up_to_date")),
        ("events/1/plan", json!("create")),
        ("events/1/existing_observation/bytes_equal", json!(false)),
        ("events/1/existing_observation/stable", json!(false)),
        ("events/1/publication/state", json!("published")),
        ("events/1/publication/cleanup/state", json!("residual")),
        (
            "events/1/existing_observation/file_sync/state",
            json!("succeeded"),
        ),
        (
            "events/1/publication/file_sync/method",
            json!("flush_file_buffers"),
        ),
        (
            "events/1/publication/namespace_sync/required_edges",
            json!(1),
        ),
        (
            "events/1/existing_observation/namespace_sync/completed_edges",
            json!(1),
        ),
        ("events/2/exit_code", json!(1)),
        ("events/2/interrupted", json!(true)),
        ("events/2/collection_complete", json!(false)),
        ("events/2/counts/created", json!(1)),
    ] {
        let pointer = format!("{prefix}/{suffix}");
        let mut bad = baseline.clone();
        *bad.pointer_mut(&pointer).unwrap() = contradiction;
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "{pointer}"
        );
    }
    let mut reordered = baseline.clone();
    reordered["observations"]["profiles"][0]["short_root_check"]["events"]
        .as_array_mut()
        .unwrap()
        .swap(0, 2);
    assert!(validate(&reordered, "owned-source", &image, target).is_err());
    let mut extra = baseline.clone();
    extra["observations"]["profiles"][0]["short_root_check"]["events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"summary","exit_code":0}));
    assert!(validate(&extra, "owned-source", &image, target).is_err());
    for suffix in [
        "events/1/error",
        "events/2/run_error",
        "events/1/publication/file_sync/native_code",
        "events/1/publication/namespace_sync/native_code",
        "events/1/publication/namespace_sync/failed_edge_index",
        "events/1/existing_observation/file_sync/native_code",
        "events/1/existing_observation/namespace_sync/native_code",
        "events/1/existing_observation/namespace_sync/failed_edge_index",
    ] {
        let pointer = format!("{prefix}/{suffix}");
        let mut bad = baseline.clone();
        remove_member(&mut bad, &pointer);
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "missing required null {pointer}"
        );
    }
}

fn remove_member(value: &mut Value, pointer: &str) {
    let (parent, member) = pointer.rsplit_once('/').unwrap();
    assert!(
        value
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(member)
            .is_some(),
        "test must actually omit {pointer}"
    );
}

#[test]
fn windows_generated_case_and_short_spelling_records_are_complete_without_inheritance_assumptions()
{
    let image = ByteIdentity::of(b"owned shipping executable");
    let target = platform(OsFamily::Windows);
    let mut baseline = valid_fixture("owned-source", &image, OsFamily::Windows);
    // An observed generated directory may differ from its configured destination.
    // This fixture also records a file with a distinct short spelling and one with
    // no distinct alias; both must resolve to their own exact long-path identity.
    let sensitive = &mut baseline["observations"]["profiles"][1];
    sensitive["native_entries"][16]["case_sensitive"] = json!(false);
    sensitive["native_entries"][0]["short_path"] = json!("R:\\EMLQUA~1\\DESTIN~1\\COPY-1");
    assert_eq!(sensitive["volume"]["case"]["destination_before"], true);
    assert_eq!(sensitive["source_case_sensitive"], false);
    validate(&baseline, "owned-source", &image, target).unwrap();
    for pointer in [
        "/observations/profiles/1/native_entries",
        "/observations/profiles/1/native_entries/16/case_sensitive",
        "/observations/profiles/1/native_entries/0/short_identity",
        "/observations/profiles/1/native_entries/0/long_path",
        "/observations/profiles/1/source_case_sensitive",
        "/observations/profiles/1/volume/case/destination_before",
        "/observations/profiles/1/volume/case/destination_after",
    ] {
        let mut bad = baseline.clone();
        remove_member(&mut bad, pointer);
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "missing {pointer}"
        );
    }
    for (pointer, contradiction) in [
        (
            "/observations/profiles/1/native_entries/16/case_sensitive",
            Value::Null,
        ),
        (
            "/observations/profiles/1/native_entries/16/case_sensitive",
            json!("inherited"),
        ),
        (
            "/observations/profiles/1/native_entries/0/case_sensitive",
            json!(false),
        ),
        (
            "/observations/profiles/1/native_entries/0/short_identity",
            json!("00000000000004d2:ffffffffffffffffffffffffffffffff"),
        ),
        (
            "/observations/profiles/1/native_entries/0/short_path",
            json!("S:\\OTHER~1\\COPY-1"),
        ),
        (
            "/observations/profiles/1/native_entries/0/long_path",
            json!("R:\\outside\\copy-1"),
        ),
        (
            "/observations/profiles/1/native_entries/0/relative",
            json!("unexpected-copy"),
        ),
        (
            "/observations/profiles/1/source_case_sensitive",
            Value::Null,
        ),
        (
            "/observations/profiles/1/volume/case/destination_after",
            json!(false),
        ),
    ] {
        let mut bad = baseline.clone();
        *bad.pointer_mut(pointer).unwrap() = contradiction;
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "{pointer}"
        );
    }
    for mutation in ["missing", "duplicate", "merged-identity"] {
        let mut bad = baseline.clone();
        let entries = bad["observations"]["profiles"][1]["native_entries"]
            .as_array_mut()
            .unwrap();
        match mutation {
            "missing" => {
                entries.pop();
            }
            "duplicate" => {
                entries.push(entries[0].clone());
            }
            "merged-identity" => {
                entries[1]["long_identity"] = entries[0]["long_identity"].clone();
                entries[1]["short_identity"] = entries[0]["short_identity"].clone();
            }
            _ => unreachable!(),
        }
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn unsupported_linux_new_names_refuse_independently_of_unsafe_creation_modes() {
    let image = ByteIdentity::of(b"owned shipping executable");
    let target = platform(OsFamily::Linux);
    let complete = valid_fixture("owned-source", &image, OsFamily::Linux);
    validate(&complete, "owned-source", &image, target).unwrap();
    let base = "/observations/existing_profiles/1/behavior/unsupported_new_name";
    for (suffix, replacement) in [
        ("passed", json!(false)),
        ("profile", json!("ext4")),
        ("destination_mode", json!(511)),
        ("owner_uid", json!(1002)),
        ("runs/0/before/0/mode", json!(511)),
        ("runs/0/after/0/mtime_ns", json!(101)),
        ("runs/0/after/0/ctime_ns", json!(101)),
        (
            "runs/0/item/existing_observation",
            json!({"bytes_equal":true}),
        ),
        (
            "runs/0/item/publication/file_sync/state",
            json!("succeeded"),
        ),
        (
            "runs/1/item/publication/namespace_sync/required_edges",
            json!(1),
        ),
        ("runs/1/item/error/code", json!("DESTINATION_UNSAFE")),
        ("runs/1/exit_code", json!(0)),
        ("runs/1/mode", json!("check")),
    ] {
        let mut bad = complete.clone();
        let pointer = format!("{base}/{suffix}");
        *bad.pointer_mut(&pointer).unwrap() = replacement;
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "{pointer}"
        );
    }
    for suffix in [
        "",
        "/runs/0/item/existing_observation",
        "/runs/1/item/publication/file_sync/native_code",
    ] {
        let mut bad = complete.clone();
        remove_member(&mut bad, &format!("{base}{suffix}"));
        assert!(validate(&bad, "owned-source", &image, target).is_err());
    }
}

#[test]
fn windows_metadata_control_requires_actual_path_bound_timestamp_mutation_and_cleanup() {
    let image = ByteIdentity::of(b"owned shipping executable");
    let target = platform(OsFamily::Windows);
    let baseline = valid_fixture("owned-source", &image, OsFamily::Windows);
    let control = &baseline["observations"]["metadata_control"];
    // Literal independent oracle: .NET UTC ticks for these two calendar instants.
    assert_eq!(control["expected_before_ticks"], 637479219060000000u64);
    assert_eq!(control["expected_after_ticks"], 637819671670000000u64);
    // The enumeration can already predate SetLastWriteTimeUtc(2021). Its stale
    // cached value need not equal the authoritative native before observation.
    assert_eq!(control["cached_before_ticks"], 637134336000000000u64); // 2020-01-01Z
    assert_eq!(control["cached_after_ticks"], 637134336000000000u64);
    assert_eq!(control["root"], r"\\?\R:\EML metadata observation control");
    assert_eq!(
        control["directory"],
        r"\\?\R:\EML metadata observation control\directory"
    );
    validate(&baseline, "owned-source", &image, target).unwrap();
    for missing in [
        "/observations/metadata_control",
        "/observations/metadata_control/root",
        "/observations/metadata_control/cached_before_ticks",
        "/observations/metadata_control/cached_after_ticks",
        "/observations/metadata_control/directory",
        "/observations/metadata_control/native_before/Identity",
        "/observations/metadata_control/native_after/Attributes",
        "/observations/metadata_control/namespace_before",
        "/observations/metadata_control/namespace_after",
        "/observations/metadata_control/cleanup_complete",
    ] {
        let mut bad = baseline.clone();
        remove_member(&mut bad, missing);
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "missing {missing}"
        );
    }
    for (suffix, contradiction) in [
        ("passed", json!(false)),
        ("root", json!(r"\\?\S:\EML metadata observation control")),
        (
            "directory",
            json!(r"\\?\R:\EML metadata observation control\other"),
        ),
        ("expected_before_ticks", json!(637479219060000001u64)),
        ("expected_after_ticks", json!(637819671670000001u64)),
        ("cached_after_ticks", json!(637819671670000000u64)),
        ("cached_after_ticks", json!(637479219060000000u64)),
        ("cached_before_ticks", json!(0)),
        ("cached_before_ticks", json!(-1)),
        ("native_after/LastWriteTicks", json!(637479219060000000u64)),
        (
            "native_after/Identity",
            json!("00000000000004d2:0000000000000000000000000000aaaa"),
        ),
        ("native_after/Attributes", json!(32)),
        (
            "namespace_after/1/last_write_ticks",
            json!(637479219060000000u64),
        ),
        (
            "namespace_after/1/path",
            json!(r"\\?\R:\unrelated\directory"),
        ),
        (
            "namespace_after/1/identity",
            json!("00000000000004d2:0000000000000000000000000000aaaa"),
        ),
        ("namespace_after/1/attributes", json!("Directory")),
        ("namespace_after/1/bytes", json!(0)),
        ("namespace_after/1/sha256", json!("e".repeat(64))),
        ("namespace_change_detected", json!(false)),
        ("cleanup_complete", json!(false)),
    ] {
        let mut bad = baseline.clone();
        *bad.pointer_mut(&format!("/observations/metadata_control/{suffix}"))
            .unwrap() = contradiction;
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "contradictory {suffix}"
        );
    }
    // A success flag cannot replace the actual changed directory in a complete
    // namespace, nor can a substituted native volume/entry supply that proof.
    for mutation in 0..5 {
        let mut bad = baseline.clone();
        let control = &mut bad["observations"]["metadata_control"];
        match mutation {
            0 => control["namespace_after"] = control["namespace_before"].clone(),
            1 => {
                control["namespace_after"].as_array_mut().unwrap().pop();
            }
            2 => {
                control["namespace_after"][0] = control["namespace_after"][1].clone();
            }
            3 => {
                for phase in ["namespace_before", "namespace_after"] {
                    control[phase][0]["identity"] =
                        json!("000000000000beef:00000000000000000000000000001001");
                }
            }
            4 => {
                control["cached_before_ticks"] = json!(637819671670000000u64);
                control["cached_after_ticks"] = json!(637819671670000000u64);
            }
            _ => unreachable!(),
        }
        assert!(
            validate(&bad, "owned-source", &image, target).is_err(),
            "substitution {mutation}"
        );
    }
}
