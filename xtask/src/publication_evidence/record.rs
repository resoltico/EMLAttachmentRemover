//! Closed actual-publication fixture records, separate from source/process binding.
use crate::Result;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) const FILES: [&str; 5] = [
    "core-stop-before.json",
    "core-stop-after.json",
    "native-racing-winner.json",
    "native-parent-binding.json",
    "controlled-cleanup-cause.json",
];
const COMMON_TESTS: [&str; 2] = [
    "run::tests::controlled_stop_before_and_after_the_real_edge_preserves_actual_effect",
    "run::tests::stage_checkpoint_origin_is_distinct_from_a_native_interrupted_error",
];
const UNIX_TESTS: [&str; 4] = [
    COMMON_TESTS[0],
    COMMON_TESTS[1],
    "candidate::tests::actual_storage_private_modes_sync_and_no_replace_preserve_winner",
    "candidate::tests::descriptor_publication_survives_destination_and_stage_name_substitution",
];
const WINDOWS_TESTS: [&str; 4] = [
    COMMON_TESTS[0],
    COMMON_TESTS[1],
    "windows::publication::tests::native_rename_conflict_and_preedge_name_refusals_only_clean_owned_staging",
    "windows::publication::tests::native_no_replace_edge_keeps_complete_same_object_and_release_retains_final_file",
];
pub(super) fn required_tests(os: &str) -> &'static [&'static str] {
    match os {
        "windows" => &WINDOWS_TESTS,
        "linux" | "macos" => &UNIX_TESTS,
        _ => &[],
    }
}
const SOURCE: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn fields(v: &Value, names: &[&str]) -> Result<()> {
    let object = v
        .as_object()
        .ok_or("Publication evidence field is not an object")?;
    require(
        object.len() == names.len() && names.iter().all(|n| object.contains_key(*n)),
        "Publication evidence field set differs",
    )
}
fn bytes(v: &Value) -> Result<Vec<u8>> {
    let a = v
        .as_array()
        .ok_or("Publication byte oracle is not an array")?;
    require(
        a.len() <= 4096,
        "Publication fixture byte oracle exceeds bound",
    )?;
    a.iter()
        .map(|x| {
            x.as_u64()
                .and_then(|x| u8::try_from(x).ok())
                .ok_or_else(|| "Invalid publication byte".into())
        })
        .collect()
}
fn code(v: &Value) -> Result<Option<i64>> {
    if v.is_null() {
        Ok(None)
    } else {
        v.as_i64()
            .filter(|x| *x > 0)
            .map(Some)
            .ok_or_else(|| "Invalid native publication cause".into())
    }
}
fn identity(v: &Value, os: &str) -> Result<()> {
    if os == "windows" {
        fields(v, &["volume", "file"])?;
        require(v["volume"].as_u64().is_some(), "Invalid native volume")?;
        let a = v["file"].as_array().ok_or("Invalid full native file ID")?;
        require(
            a.len() == 16
                && a.iter().all(|n| n.as_u64().is_some_and(|n| n <= 255))
                && a.iter().any(|n| n.as_u64() != Some(0)),
            "Invalid full native file ID",
        )
    } else {
        fields(v, &["device", "inode"])?;
        require(
            v["device"].as_u64().is_some() && v["inode"].as_u64().is_some_and(|n| n > 0),
            "Invalid native device/inode",
        )
    }
}
fn domain<'a>(v: &'a Value, os: &str) -> &'a Value {
    &v[if os == "windows" { "volume" } else { "device" }]
}
fn signature(v: &Value, os: &str) -> Result<()> {
    fields(
        v,
        &["identity", "size", "modified_native", "changed_native"],
    )?;
    identity(&v["identity"], os)?;
    require(v["size"].as_u64().is_some(), "Invalid signature size")?;
    for field in ["modified_native", "changed_native"] {
        let a = v[field]
            .as_array()
            .ok_or("Missing full native signature pair")?;
        require(
            a.len() == 2 && a.iter().all(|v| v.as_i64().is_some()),
            "Incomplete native signature pair",
        )?;
        require(
            if os == "windows" {
                a[1] == 0
            } else {
                a[1].as_i64()
                    .is_some_and(|n| (0..1_000_000_000).contains(&n))
            },
            "Malformed backend native signature precision",
        )?;
    }
    Ok(())
}
fn units(v: &Value, os: &str) -> Result<Vec<u16>> {
    let a = v.as_array().ok_or("Native component is not an array")?;
    require(
        !a.is_empty() && a.len() <= 32768,
        "Invalid native component size",
    )?;
    let values: Vec<u16> = a
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|v| u16::try_from(v).ok())
                .filter(|n| os == "windows" || *n <= 255)
                .ok_or_else(|| "Invalid native component unit".into())
        })
        .collect::<Result<_>>()?;
    require(
        !values
            .iter()
            .any(|n| matches!(*n, 0 | 47) || (os == "windows" && *n == 92))
            && values != [46]
            && values != [46, 46],
        "Invalid relative native component",
    )?;
    Ok(values)
}
fn cleanup(v: &Value, os: &str, state: &str) -> Result<()> {
    fields(v, &["state", "residual_name", "native_code"])?;
    require(v["state"] == state, "Publication cleanup state differs")?;
    code(&v["native_code"])?;
    if !v["residual_name"].is_null() {
        units(&v["residual_name"], os)?;
    }
    if matches!(state, "Complete" | "NotNeeded") {
        require(
            v["residual_name"].is_null() && v["native_code"].is_null(),
            "Successful cleanup retained unresolved facts",
        )?;
    }
    Ok(())
}
fn sync(v: &Value, os: &str, file: bool) -> Result<()> {
    fields(v, &["method", "state", "native_code"])?;
    require(
        v["state"] == "Succeeded",
        "Publication synchronization did not succeed",
    )?;
    let limited = os == "macos" && file && v["method"] == "FsyncFullfsyncUnsupported";
    require(
        v["method"]
            == if os == "windows" {
                "FlushFileBuffers"
            } else if os == "macos" && file && !limited {
                "FsyncFullfsync"
            } else if limited {
                "FsyncFullfsyncUnsupported"
            } else {
                "Fsync"
            },
        "Publication synchronization method differs",
    )?;
    require(
        if limited {
            matches!(code(&v["native_code"])?, Some(22 | 45 | 102))
        } else {
            v["native_code"].is_null()
        },
        "Publication synchronization cause differs",
    )
}
fn native_edge(v: &Value, os: &str, succeeded: bool) -> Result<()> {
    fields(v, &["primitive", "succeeded", "raw_return", "native_code"])?;
    require(
        v["primitive"]
            == match os {
                "linux" => "linkat",
                "macos" => "renameat_with_noreplace",
                _ => "SetFileInformationByHandle_FileRenameInfo",
            }
            && v["succeeded"] == succeeded,
        "Native publication primitive/result differs",
    )?;
    if os == "windows" {
        let raw = v["raw_return"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or("Invalid actual native BOOL")?;
        require(
            (raw != 0) == succeeded,
            "Native BOOL contradicts publication",
        )?;
    } else {
        require(
            v["raw_return"].is_null(),
            "Rustix result fabricated raw return",
        )?;
    }
    let actual = code(&v["native_code"])?;
    require(
        if succeeded {
            actual.is_none()
        } else if os == "windows" {
            matches!(actual, Some(80 | 183))
        } else {
            actual == Some(17)
        },
        "Native edge did not retain the actual expected collision/success",
    )
}
fn file(v: &Value, os: &str, expected: &[u8]) -> Result<()> {
    fields(v, &["signature", "bytes"])?;
    signature(&v["signature"], os)?;
    require(
        bytes(&v["bytes"])? == expected
            && v["signature"]["size"].as_u64() == Some(expected.len() as u64),
        "Actual publication bytes/size differ from independent oracle",
    )
}
fn inventory(v: &Value, os: &str) -> Result<()> {
    let entries = v
        .as_array()
        .ok_or("Publication inventory is not an array")?;
    require(
        entries.len() <= 64,
        "Finite fixture inventory exceeds bound",
    )?;
    let mut previous = None;
    let mut dirs = BTreeSet::new();
    let mut observed_ids = Vec::new();
    for entry in entries {
        fields(entry, &["path", "kind", "identity", "signature"])?;
        let components = entry["path"]
            .as_array()
            .ok_or("Inventory native path is not an array")?;
        require(
            !components.is_empty() && components.len() <= 8,
            "Invalid fixture relative depth",
        )?;
        let path: Vec<Vec<u16>> = components
            .iter()
            .map(|p| units(p, os))
            .collect::<Result<_>>()?;
        require(
            previous.as_ref().is_none_or(|p| p < &path),
            "Publication inventory is duplicated or unsorted",
        )?;
        if path.len() > 1 {
            require(
                dirs.contains(&path[..path.len() - 1]),
                "Publication inventory lacks an ordinary parent",
            )?;
        }
        identity(&entry["identity"], os)?;
        let directory = entry["kind"] == "Directory";
        require(
            !observed_ids.iter().any(|(id, was_directory)| {
                *id == &entry["identity"] && (directory || *was_directory)
            }),
            "Ordinary directory identity is aliased in the fixture inventory",
        )?;
        observed_ids.push((&entry["identity"], directory));
        match entry["kind"].as_str() {
            Some("Directory") => {
                require(
                    entry["signature"].is_null(),
                    "Directory gained a file signature",
                )?;
                dirs.insert(path.clone());
            }
            Some("File") => {
                signature(&entry["signature"], os)?;
                require(
                    entry["signature"]["identity"] == entry["identity"],
                    "Inventory full identity disagrees",
                )?;
            }
            _ => return Err("Publication inventory contains a nonordinary entry".into()),
        }
        previous = Some(path);
    }
    Ok(())
}
fn path_eq(v: &Value, parts: &[&str]) -> bool {
    let Some(a) = v.as_array() else {
        return false;
    };
    a.len() == parts.len() && a.iter().zip(parts).all(|(a, b)| a == &json!(b.as_bytes()))
}
fn inventory_file<'a>(v: &'a Value, parts: &[&str]) -> Result<&'a Value> {
    v.as_array()
        .and_then(|a| a.iter().find(|e| path_eq(&e["path"], parts)))
        .filter(|e| e["kind"] == "File")
        .ok_or_else(|| "Expected exact publication inventory file is missing".into())
}
fn core(v: &Value, os: &str, after: bool) -> Result<()> {
    fields(
        &v["source"],
        &[
            "expected",
            "before",
            "after",
            "signature_before",
            "signature_after",
        ],
    )?;
    for f in ["expected", "before", "after"] {
        require(
            bytes(&v["source"][f])? == SOURCE,
            "Original source differs from independent fixture",
        )?;
    }
    for f in ["signature_before", "signature_after"] {
        signature(&v["source"][f], os)?;
        require(
            v["source"][f]["size"].as_u64() == Some(SOURCE.len() as u64),
            "Source signature size differs",
        )?;
    }
    require(
        v["source"]["signature_before"] == v["source"]["signature_after"],
        "Original full source signature changed",
    )?;
    require(
        bytes(&v["candidate_expected"])? == GOLDEN,
        "Core candidate oracle differs",
    )?;
    let phases = if after {
        vec![
            "Preflight",
            "StageWrite",
            "FileSync",
            "Publication",
            "AfterPublication",
            "NamespaceSync",
        ]
    } else {
        vec!["Preflight", "StageWrite", "FileSync", "Publication"]
    };
    require(
        v["engine_phases"] == json!(phases),
        "Engine phase order differs (not a syscall count)",
    )?;
    let r = &v["receipt"];
    fields(
        r,
        &[
            "classification",
            "error_code",
            "error_phase",
            "native_code",
            "effect",
            "candidate_identity",
            "address",
            "file_sync",
            "namespace",
            "cleanup",
        ],
    )?;
    identity(&r["candidate_identity"], os)?;
    require(
        r["candidate_identity"] != v["source"]["signature_before"]["identity"]
            && domain(&r["candidate_identity"], os)
                == domain(&v["source"]["signature_before"]["identity"], os),
        "Staged candidate aliases original or leaves the owned filesystem",
    )?;
    require(
        r["native_code"].is_null(),
        "Controlled stop gained native error",
    )?;
    sync(&r["file_sync"], os, true)?;
    let ns = &r["namespace"];
    fields(
        ns,
        &[
            "receipt",
            "required_edges",
            "completed_edges",
            "failed_edge_index",
        ],
    )?;
    require(
        ns["required_edges"] == 1
            && ns["completed_edges"] == u64::from(after)
            && ns["failed_edge_index"].is_null(),
        "Core namespace debt differs",
    )?;
    let before_file = inventory_file(&v["inventory_before"], &["a.eml"])?;
    require(
        v["inventory_before"].as_array().unwrap().len() == 1
            && before_file["signature"] == v["source"]["signature_before"],
        "Initial source inventory differs",
    )?;
    let after_source = inventory_file(&v["inventory_after"], &["a.eml"])?;
    require(
        after_source == before_file,
        "Final source inventory changed",
    )?;
    if after {
        require(
            r["classification"] == "Created"
                && r["effect"] == "Published"
                && r["address"] == "Verified"
                && r["error_code"].is_null()
                && r["error_phase"].is_null(),
            "Post-edge stop lost published qualification",
        )?;
        sync(&ns["receipt"], os, false)?;
        cleanup(
            &r["cleanup"],
            os,
            if os == "windows" {
                "NotNeeded"
            } else {
                "Complete"
            },
        )?;
        file(&v["final"], os, GOLDEN)?;
        require(
            v["final"]["signature"]["identity"] == r["candidate_identity"]
                && r["candidate_identity"] != v["source"]["signature_before"]["identity"],
            "Final candidate identity differs or aliases original",
        )?;
        let entry = inventory_file(
            &v["inventory_after"],
            &["a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml"],
        )?;
        require(
            v["inventory_after"].as_array().unwrap().len() == 2
                && entry["signature"] == v["final"]["signature"],
            "Published inventory differs",
        )?;
    } else {
        require(
            r["classification"] == "Failed"
                && r["error_code"] == "Interrupted"
                && r["error_phase"] == "Publish"
                && r["effect"] == "NotPublished"
                && r["address"] == "NotChecked"
                && v["final"].is_null()
                && ns["receipt"].is_null()
                && v["inventory_before"] == v["inventory_after"],
            "Pre-edge stop gained publication or lost actual failure",
        )?;
        cleanup(&r["cleanup"], os, "Complete")?;
    }
    Ok(())
}
fn race(v: &Value, os: &str) -> Result<()> {
    require(
        v["events"]
            == json!([
                "stage_returned",
                "winner_created",
                "native_publication_returned",
                "cleanup_returned"
            ]),
        "Native winner event order differs",
    )?;
    identity(&v["candidate_identity"], os)?;
    require(
        bytes(&v["candidate_expected"])? == b"synthetic loser"
            && bytes(&v["candidate_actual"])? == b"synthetic loser",
        "Native candidate independent byte oracle differs",
    )?;
    file(&v["winner_before"], os, b"synthetic winner")?;
    file(&v["winner_after"], os, b"synthetic winner")?;
    require(
        domain(&v["candidate_identity"], os)
            == domain(&v["winner_before"]["signature"]["identity"], os),
        "Candidate and winner are not on the same native filesystem",
    )?;
    require(
        v["winner_before"] == v["winner_after"]
            && v["candidate_identity"] != v["winner_before"]["signature"]["identity"]
            && v["publication"] == "NotPublished",
        "Native collision replaced winner or aliases candidate",
    )?;
    sync(&v["file_sync"], os, true)?;
    native_edge(&v["native_edge"], os, false)?;
    cleanup(&v["cleanup"], os, "Complete")?;
    let before = inventory_file(&v["inventory_before"], &["winner.eml"])?;
    let after = inventory_file(&v["inventory_after"], &["winner.eml"])?;
    require(
        before == after
            && after["signature"] == v["winner_after"]["signature"]
            && v["inventory_after"].as_array().unwrap().len() == 1,
        "Winner inventory changed or retained staging",
    )?;
    let staged: Vec<_> = v["inventory_before"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["identity"] == v["candidate_identity"])
        .collect();
    require(
        staged.len() == 1
            && staged[0]["kind"] == "File"
            && staged[0]["signature"]["size"] == 15
            && v["inventory_before"].as_array().unwrap().len()
                == if os == "windows" { 2 } else { 3 },
        "Initial inventory omits actual staged candidate or adds unrelated entries",
    )
}
fn parent(v: &Value, os: &str) -> Result<()> {
    for f in [
        "parent_before",
        "held_parent_after",
        "requested_parent_after",
        "candidate_identity",
    ] {
        identity(&v[f], os)?;
    }
    require(
        domain(&v["candidate_identity"], os) == domain(&v["parent_before"], os)
            && domain(&v["requested_parent_after"], os) == domain(&v["parent_before"], os),
        "Parent interference left the owned filesystem",
    )?;
    require(
        v["parent_before"] == v["held_parent_after"],
        "Held parent identity changed",
    )?;
    sync(&v["file_sync"], os, true)?;
    require(
        v["namespace_sync"].is_null() && v["publication"] == "Published",
        "Native parent control fabricated namespace sync or lost publication",
    )?;
    native_edge(&v["native_edge"], os, true)?;
    let before_entries = v["inventory_before"].as_array().unwrap();
    let staged: Vec<_> = before_entries
        .iter()
        .filter(|e| e["identity"] == v["candidate_identity"])
        .collect();
    require(
        staged.len() == 1
            && staged[0]["kind"] == "File"
            && staged[0]["signature"]["size"].as_u64()
                == Some(bytes(&v["candidate_actual"])?.len() as u64),
        "Parent fixture does not identify its actual staged file",
    )?;
    if os == "windows" {
        require(
            before_entries.len() == 1,
            "Pinned parent fixture gained unrelated initial entries",
        )?;
        require(
            v["events"]
                == json!([
                    "stage_returned",
                    "parent_rename_denied",
                    "native_publication_returned",
                    "cleanup_returned"
                ])
                && code(&v["actor_native_code"])? == Some(32)
                && v["actor_changed_address"] == false
                && v["requested_address_verified"] == true
                && v["parent_before"] == v["requested_parent_after"]
                && v["substitute_expected"].is_null()
                && v["substitute_after"].is_null(),
            "Native pinned parent did not retain actual sharing refusal",
        )?;
        let expected = b"Subject: synthetic\r\n\r\ncomplete verified body\r\n";
        require(
            bytes(&v["candidate_expected"])? == expected
                && bytes(&v["candidate_actual"])? == expected,
            "Pinned-parent candidate differs",
        )?;
        cleanup(&v["cleanup"], os, "NotNeeded")?;
        let entry = inventory_file(&v["inventory_after"], &["result.eml"])?;
        require(
            v["inventory_after"].as_array().unwrap().len() == 1
                && entry["identity"] == v["candidate_identity"]
                && entry["signature"]["size"].as_u64() == Some(expected.len() as u64),
            "Pinned-parent final inventory differs",
        )?;
    } else {
        require(
            v["events"]
                == json!([
                    "stage_returned",
                    "parent_renamed",
                    "replacement_parent_created",
                    "stage_renamed",
                    "replacement_stage_created",
                    "substitute_written",
                    "native_publication_returned",
                    "cleanup_returned"
                ])
                && v["actor_native_code"].is_null()
                && v["actor_changed_address"] == true
                && v["requested_address_verified"] == false
                && v["parent_before"] != v["requested_parent_after"],
            "Descriptor parent substitution facts differ",
        )?;
        require(
            bytes(&v["candidate_expected"])? == b"held verified bytes"
                && bytes(&v["candidate_actual"])? == b"held verified bytes"
                && bytes(&v["substitute_expected"])? == b"fake bytes"
                && bytes(&v["substitute_after"])? == b"fake bytes",
            "Descriptor candidate/substitute independent bytes differ",
        )?;
        cleanup(&v["cleanup"], os, "Unknown")?;
        require(
            before_entries.len() == 3
                && before_entries.iter().any(|e| {
                    path_eq(&e["path"], &["destination"]) && e["identity"] == v["parent_before"]
                }),
            "Initial descriptor parent inventory differs",
        )?;
        let initial_path = staged[0]["path"].as_array().unwrap();
        require(
            initial_path.len() == 3
                && initial_path[0] == json!(b"destination")
                && initial_path[2] == json!(b"candidate"),
            "Initial descriptor candidate path differs",
        )?;
        let stage_name = &initial_path[1];
        let stage = before_entries
            .iter()
            .find(|e| e["path"] == json!([b"destination", stage_name]))
            .ok_or("Initial owned stage directory is missing")?;
        require(
            stage["kind"] == "Directory",
            "Initial stage is not ordinary directory",
        )?;
        let candidate = inventory_file(&v["inventory_after"], &["moved", "copy"])?;
        require(
            candidate["identity"] == v["candidate_identity"]
                && candidate["signature"]["size"] == 19,
            "Descriptor final candidate differs",
        )?;
        let inv = v["inventory_after"].as_array().unwrap();
        require(
            inv.len() == 6
                && inv.iter().any(|e| {
                    path_eq(&e["path"], &["destination"])
                        && e["identity"] == v["requested_parent_after"]
                })
                && inv.iter().any(|e| {
                    path_eq(&e["path"], &["moved"]) && e["identity"] == v["held_parent_after"]
                })
                && inv.iter().any(|e| {
                    path_eq(&e["path"], &["moved", "held-stage"]) && e["kind"] == "Directory"
                }),
            "Descriptor substitution inventory lacks expected physical parents",
        )?;
        let fake: Vec<_> = inv
            .iter()
            .filter(|e| e["kind"] == "File" && !path_eq(&e["path"], &["moved", "copy"]))
            .collect();
        let held = inv
            .iter()
            .find(|e| path_eq(&e["path"], &["moved", "held-stage"]))
            .ok_or("Held renamed stage is missing")?;
        require(
            held["identity"] == stage["identity"],
            "Held stage identity changed",
        )?;
        require(
            fake.len() == 1
                && fake[0]["path"] == json!([b"moved", stage_name, b"candidate"])
                && fake[0]["signature"]["size"] == 10
                && fake[0]["identity"] != v["candidate_identity"],
            "Substitute inventory ownership was lost",
        )?;
    }
    require(
        v["inventory_before"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["identity"] == v["candidate_identity"]),
        "Initial inventory lacks staged candidate",
    )
}
fn controlled(v: &Value, os: &str) -> Result<()> {
    let cases = v["cases"]
        .as_array()
        .ok_or("Controlled cleanup inputs are not an array")?;
    require(cases.len() == 2, "Controlled cleanup input count differs")?;
    for (i, c) in cases.iter().enumerate() {
        fields(
            c,
            &[
                "checkpoint_stopped",
                "original_error_kind",
                "translated_code",
                "translated_phase",
                "file_sync",
                "cleanup",
            ],
        )?;
        require(
            c["checkpoint_stopped"] == (i == 1)
                && c["original_error_kind"] == "Interrupted"
                && c["translated_code"] == if i == 1 { "Interrupted" } else { "WriteFailed" }
                && c["translated_phase"] == "Write"
                && c["file_sync"]
                    == json!({"method":"Fsync","state":"Succeeded","native_code":null}),
            "Controlled primary cause translation differs",
        )?;
        cleanup(&c["cleanup"], os, "Residual")?;
        require(
            c["cleanup"]["native_code"] == 5
                && c["cleanup"]["residual_name"] == json!(b".owned-stage"),
            "Controlled cleanup cause was overwritten",
        )?;
    }
    Ok(())
}
/// Validate one bounded decoded record; the caller rejects duplicate JSON fields.
pub(super) fn validate_record(name: &str, v: &Value, os: &str, architecture: &str) -> Result<()> {
    require(
        matches!(os, "linux" | "macos" | "windows") && matches!(architecture, "x86_64" | "aarch64"),
        "Unknown native publication platform",
    )?;
    let extra: &[&str] = match name {
        "core-stop-before.json" | "core-stop-after.json" => &[
            "engine_phases",
            "source",
            "candidate_expected",
            "final",
            "receipt",
            "inventory_before",
            "inventory_after",
        ],
        "native-racing-winner.json" => &[
            "events",
            "candidate_identity",
            "candidate_expected",
            "candidate_actual",
            "file_sync",
            "native_edge",
            "publication",
            "cleanup",
            "winner_before",
            "winner_after",
            "inventory_before",
            "inventory_after",
        ],
        "native-parent-binding.json" => &[
            "events",
            "parent_before",
            "held_parent_after",
            "requested_parent_after",
            "actor_native_code",
            "actor_changed_address",
            "requested_address_verified",
            "candidate_identity",
            "candidate_expected",
            "candidate_actual",
            "native_edge",
            "publication",
            "cleanup",
            "inventory_before",
            "inventory_after",
            "substitute_expected",
            "substitute_after",
            "file_sync",
            "namespace_sync",
        ],
        "controlled-cleanup-cause.json" => &["cases"],
        _ => return Err("Unexpected native publication record".into()),
    };
    let mut names = vec![
        "schema",
        "case",
        "origin",
        "process_os",
        "process_architecture",
    ];
    names.extend_from_slice(extra);
    fields(v, &names)?;
    require(
        v["schema"] == 1
            && v["case"] == name.strip_suffix(".json").unwrap()
            && v["process_os"] == os
            && v["process_architecture"] == architecture,
        "Native publication record context differs",
    )?;
    let origin = match name {
        "core-stop-before.json" | "core-stop-after.json" => "actual_core_with_controlled_stop",
        "native-racing-winner.json" => "actual_native_publication",
        "native-parent-binding.json" => "actual_native_parent_interference",
        _ => "controlled_stage_failure_translation",
    };
    require(
        v["origin"] == origin,
        "Native and controlled publication origins were conflated",
    )?;
    if name != "controlled-cleanup-cause.json" {
        inventory(&v["inventory_before"], os)?;
        inventory(&v["inventory_after"], os)?;
    }
    match name {
        "core-stop-before.json" => core(v, os, false),
        "core-stop-after.json" => core(v, os, true),
        "native-racing-winner.json" => race(v, os),
        "native-parent-binding.json" => parent(v, os),
        _ => controlled(v, os),
    }
}
#[cfg(test)]
pub(super) fn test_records(os: &str, arch: &str) -> Vec<(&'static str, Value)> {
    FILES
        .into_iter()
        .map(|name| (name, tests::fixture(name, os, arch)))
        .collect()
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
