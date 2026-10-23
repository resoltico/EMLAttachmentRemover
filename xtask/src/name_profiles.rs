//! Finite native filesystem naming observations against one frozen CLI image.
use crate::{
    Result,
    cli_build::FrozenCli,
    controller,
    distribution::{Architecture, OsFamily, PlatformId},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path, process::Command, time::Duration};
use xtask::archive::ByteIdentity;

pub(crate) fn validate(
    value: &Value,
    source: &str,
    binary: &ByteIdentity,
    platform: PlatformId,
) -> Result<()> {
    let family = platform.family();
    let process: crate::picker_evidence::ProcessObservation =
        serde_json::from_value(value["process"].clone())?;
    let observation = &value["observations"];
    let expected: &[&str] = match family {
        OsFamily::Linux => &[
            "ext2",
            "ext3",
            "ext4",
            "ext4-casefold",
            "tmpfs",
            "xfs",
            "xfs-ascii-ci",
            "btrfs",
        ],
        OsFamily::Macos => &[
            "apfs-case-insensitive",
            "apfs-case-sensitive",
            "hfs-case-insensitive",
            "hfs-case-sensitive",
        ],
        OsFamily::Windows => &["ntfs-case-insensitive-8dot3", "ntfs-case-sensitive-8dot3"],
    };
    if value["source"] != source
        || serde_json::from_value::<ByteIdentity>(value["binary"].clone())? != *binary
        || process.clean()? != 0
        || !explicit_null(value, "process_failure")
        || !explicit_null(value, "observation_read_failure")
        || observation["schema"] != 1
        || observation["passed"] != true
        || observation["binary_sha256"] != binary.sha256
    {
        return Err(
            "Native naming profiles lack successful exact-source/binary/process binding".into(),
        );
    }
    if family == OsFamily::Windows {
        validate_windows_metadata_control(observation)?;
    }
    let profiles = observation["profiles"]
        .as_array()
        .ok_or("Native profile observations are absent")?;
    let mut observed = std::collections::BTreeSet::new();
    for profile in profiles {
        let name = profile["profile"]
            .as_str()
            .ok_or("Native profile name is missing")?;
        let behavior = &profile["behavior"];
        let case_mode = match name {
            "ext4-casefold"
            | "xfs-ascii-ci"
            | "apfs-case-insensitive"
            | "hfs-case-insensitive"
            | "ntfs-case-insensitive-8dot3" => "case-insensitive",
            _ => "case-sensitive",
        };
        if !expected.contains(&name)
            || !observed.insert(name)
            || profile["status"] != "passed"
            || profile["exit_code"] != 0
            || behavior["schema"] != 1
            || behavior["passed"] != true
            || !explicit_null(behavior, "failure")
            || behavior["binary_sha256"] != binary.sha256
            || behavior["case_mode"] != case_mode
        {
            return Err(
                "Native profile is duplicated, unqualified or bound to another binary".into(),
            );
        }
        if family == OsFamily::Macos {
            let filesystem = if name.starts_with("apfs-") {
                "apfs"
            } else {
                "hfs"
            };
            if profile["volume"]["ownership_enabled"] != true
                || profile["cleanup"]["detached"] != true
                || profile["cleanup"]["owned_image_removed"] != true
                || profile["cleanup"]["forced_detach"] != false
                || profile
                    .get("cleanup_failure")
                    .is_some_and(|value| !value.is_null())
                || profile["volume"]["filesystem_type"] != filesystem
                || profile["volume"]["case"]["case_sensitive"] != (case_mode == "case-sensitive")
                || !profile["volume"]["device"]
                    .as_str()
                    .is_some_and(|device| device.starts_with("/dev/disk") && device.len() > 9)
            {
                return Err("Native macOS profile lacks exact volume/case observations and settled owned-image cleanup".into());
            }
        }
        if family == OsFamily::Linux {
            validate_linux_mount(profile, name)?;
        }
        if family == OsFamily::Windows {
            validate_windows_profile(
                observation,
                profile,
                case_mode == "case-sensitive",
                platform.architecture(),
            )?;
        }
        let runs = behavior["runs"]
            .as_array()
            .ok_or("Native profile has no actual CLI observations")?;
        let cases = behavior["expected"]
            .as_array()
            .ok_or("Native profile has no independent byte/name oracles")?;
        if cases.len() != 16
            || behavior["source_identities"]
                .as_object()
                .is_none_or(|map| map.len() != 16)
            || behavior["entries"].as_array().is_none_or(Vec::is_empty)
            || runs.len() != 4
        {
            return Err("Native profile fixture or execution selection is incomplete".into());
        }
        let tokens: std::collections::BTreeSet<_> =
            cases.iter().map(|case| case["token"].as_str()).collect();
        if tokens.len() != cases.len() || tokens.contains(&None) {
            return Err("Native profile independent leaf oracles are not distinct".into());
        }
        if tokens.iter().flatten().any(|token| {
            token.len() != 26
                || !token
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(&byte))
        }) {
            return Err("Native profile route token oracle is malformed".into());
        }
        validate_observed_names(behavior, family)?;
        for (run, (mode, status)) in runs.iter().zip([
            ("check", "planned"),
            ("create", "created"),
            ("repeat", "up_to_date"),
            ("create", "created"),
        ]) {
            if run["mode"] != mode
                || run["exit_code"] != 0
                || run["items"] != 16
                || run["statuses"].as_array().is_none_or(|statuses| {
                    statuses.len() != 16 || statuses.iter().any(|item| item != status)
                })
            {
                return Err(
                    "Native profile Check/Create/repeat/fresh selection did not complete".into(),
                );
            }
        }
    }
    if observed != expected.iter().copied().collect() {
        return Err("Required native filesystem profiles remain unqualified".into());
    }
    if family == OsFamily::Linux {
        validate_existing_profiles(observation, binary)?;
    }
    Ok(())
}

fn explicit_null(value: &Value, field: &str) -> bool {
    value.get(field) == Some(&Value::Null)
}

fn sha256(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        text.len() == 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
/// An actual owned-directory timestamp mutation must be visible to the very
/// namespace oracle used around Check. Enumerated FileSystemInfo cache values
/// remain stale; they are evidence of the rejected oracle, not an admission source.
fn validate_windows_metadata_control(observation: &Value) -> Result<()> {
    // Independently specified UTC instants, in .NET ticks since 0001-01-01.
    const BEFORE: u64 = 637_479_219_060_000_000; // 2021-02-03T04:05:06Z
    const AFTER: u64 = 637_819_671_670_000_000; // 2022-03-04T05:06:07Z
    let volume = observation["storage"]["volume_root"]
        .as_str()
        .ok_or("Metadata control has no owned volume binding")?;
    if volume.len() != 3 || !volume.as_bytes()[0].is_ascii_alphabetic() || &volume[1..] != ":\\" {
        return Err("Metadata control volume is not an exact local drive root".into());
    }
    let root = format!("\\\\?\\{volume}EML metadata observation control");
    let directory = format!("{root}\\directory");
    let control = &observation["metadata_control"];
    let cached = control["cached_before_ticks"]
        .as_u64()
        .filter(|ticks| *ticks > 0)
        .ok_or("Metadata control cached timestamp is absent or nonpositive")?;
    if control["passed"] != true
        || control["root"] != root
        || control["directory"] != directory
        || control["expected_before_ticks"] != BEFORE
        || control["expected_after_ticks"] != AFTER
        || control["cached_after_ticks"] != cached
        || cached == AFTER
        || control["namespace_change_detected"] != true
        || control["cleanup_complete"] != true
    {
        return Err("Native metadata control lacks exact instants, stale-cache contrast, path binding or cleanup".into());
    }
    let before = control["namespace_before"]
        .as_array()
        .ok_or("Metadata control before namespace is absent")?;
    let after = control["namespace_after"]
        .as_array()
        .ok_or("Metadata control after namespace is absent")?;
    if before.len() != 2 || after.len() != 2 || before == after {
        return Err("Metadata control must compare two complete distinct owned namespaces".into());
    }
    let mut identities = std::collections::BTreeSet::new();
    for path in [&root, &directory] {
        let find = |entries: &[Value]| -> Result<Value> {
            let entries: Vec<_> = entries
                .iter()
                .filter(|entry| entry["path"] == *path)
                .collect();
            if entries.len() != 1 {
                return Err("Metadata control namespace path is missing or duplicated".into());
            }
            Ok(entries[0].clone())
        };
        let first = find(before)?;
        let last = find(after)?;
        let id = first["identity"]
            .as_str()
            .ok_or("Metadata control native identity is absent")?;
        let identity_shape = id.split_once(':').is_some_and(|(volume, file)| {
            volume.len() == 16
                && file.len() == 32
                && volume
                    .bytes()
                    .chain(file.bytes())
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        });
        let attributes = first["attributes"]
            .as_u64()
            .filter(|value| *value <= u64::from(u32::MAX))
            .ok_or("Metadata control attributes are not native numeric flags")?;
        if !identity_shape
            || !identities.insert(id.to_owned())
            || attributes & 16 == 0
            || attributes & 1024 != 0
            || first["identity"] != last["identity"]
            || first["attributes"] != last["attributes"]
            || !explicit_null(&first, "bytes")
            || !explicit_null(&last, "bytes")
            || !explicit_null(&first, "sha256")
            || !explicit_null(&last, "sha256")
            || !first["last_write_ticks"]
                .as_u64()
                .is_some_and(|ticks| ticks > 0)
            || !last["last_write_ticks"]
                .as_u64()
                .is_some_and(|ticks| ticks > 0)
        {
            return Err(
                "Metadata control directory identity, attributes or empty namespace facts differ"
                    .into(),
            );
        }
        if path == &directory {
            if first["last_write_ticks"] != BEFORE || last["last_write_ticks"] != AFTER {
                return Err("Same namespace oracle did not observe the independently chosen timestamp change".into());
            }
            for (name, ticks) in [("native_before", BEFORE), ("native_after", AFTER)] {
                let native = &control[name];
                if native["Identity"] != id
                    || native["Attributes"] != attributes
                    || native["LastWriteTicks"] != ticks
                {
                    return Err(
                        "Held-handle metadata disagrees with the actual namespace oracle".into(),
                    );
                }
            }
        }
    }
    if identities
        .iter()
        .map(|id| &id[..16])
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != 1
    {
        return Err("Metadata control directories are not on the same owned volume".into());
    }
    Ok(())
}

fn validate_windows_profile(
    observation: &Value,
    profile: &Value,
    sensitive: bool,
    architecture: Architecture,
) -> Result<()> {
    let storage = &observation["storage"];
    let cleanup = &observation["cleanup"];
    let machine = match architecture {
        Architecture::X86_64 => 0x8664,
        Architecture::Aarch64 => 0xaa64,
    };
    let host_architecture = match architecture {
        Architecture::X86_64 => "X64",
        Architecture::Aarch64 => "Arm64",
    };
    let physical_number = storage["physical_path"]
        .as_str()
        .and_then(|path| path.strip_prefix("\\\\.\\PhysicalDrive"))
        .filter(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|number| number.parse::<u32>().ok());
    let image = storage["image"]
        .as_str()
        .ok_or("Owned VHD image path is missing")?;
    let local_image = image.strip_prefix("\\\\?\\").unwrap_or(image);
    if observation.get("failure") != Some(&Value::Null)
        || observation["host"]["architecture"] != host_architecture
        || observation["binary_unchanged"] != true
        || storage["filesystem"] != "NTFS"
        || storage["drive_type"] != 3
        || storage["max_component"] != 255
        || storage["lifetime"] != "kernel-handle-owned"
        || storage["native_cli_machine"] != machine
        || physical_number.is_none()
        || physical_number.map(u64::from) != storage["disk_number"].as_u64()
        || !storage["disk_unique_id"].as_str().is_some_and(|id| {
            !id.is_empty() && id.len() <= 1024 && !id.chars().any(char::is_control)
        })
        || !storage["label"].as_str().is_some_and(|label| {
            label.strip_prefix("EML-").is_some_and(|id| {
                id.len() == 12
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
        })
        || local_image.len() < 8
        || !local_image.as_bytes()[0].is_ascii_alphabetic()
        || &local_image.as_bytes()[1..3] != b":\\"
        || !local_image.ends_with(".vhd")
        || ![0, 2]
            .iter()
            .any(|policy| storage["global_8dot3_policy"] == *policy)
        || !storage["physical_path"].as_str().is_some_and(|path| {
            path.strip_prefix("\\\\.\\PhysicalDrive")
                .is_some_and(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                })
        })
        || cleanup["detached"] != true
        || cleanup["observation_complete"] != true
        || cleanup["forced_detach"] != false
        || ["failure", "observation_failure"]
            .iter()
            .any(|key| cleanup.get(*key).is_some_and(|value| !value.is_null()))
    {
        return Err("Native Windows naming lacks bound local NTFS, actual CPU or settled kernel-owned VHD evidence".into());
    }
    let volume = &profile["volume"];
    let root = volume["root"]
        .as_str()
        .ok_or("Owned Windows root is absent")?;
    let short = volume["short_root"]
        .as_str()
        .ok_or("Actual Windows short-root alias is absent")?;
    if profile["executed"] != true
        || profile.get("failure") != Some(&Value::Null)
        || volume["filesystem_type"] != "ntfs"
        || volume["case"]["case_sensitive"] != sensitive
        || volume["case"]["destination_before"] != sensitive
        || volume["case"]["destination_after"] != sensitive
        || !profile["source_case_sensitive"].is_boolean()
        || volume["drive_type"] != 3
        || volume["global_8dot3_policy"] != storage["global_8dot3_policy"]
        || volume["serial"] != storage["volume_serial"]
        || volume["label"] != storage["label"]
        || !volume["serial"].as_str().is_some_and(|serial| {
            !serial.is_empty()
                && serial.len() <= 10
                && serial.bytes().all(|byte| byte.is_ascii_digit())
        })
        || root == short
        || !short.contains('~')
        || !storage["volume_root"].as_str().is_some_and(|prefix| {
            prefix.len() == 3
                && prefix.as_bytes()[0].is_ascii_alphabetic()
                && prefix.ends_with(":\\")
                && root.starts_with(prefix)
                && short.starts_with(prefix)
        })
    {
        return Err(
            "Windows profile lacks actual per-directory case mode or same-volume 8.3 root alias"
                .into(),
        );
    }
    validate_windows_entries(profile, root, &storage["volume_root"])?;
    let check = &profile["short_root_check"];
    let events = check["events"]
        .as_array()
        .ok_or("Actual short-root CLI Check is absent")?;
    if events.len() != 3
        || events[0]["type"] != "start"
        || events[0]["protocol"] != 1
        || events[0]["dry_run"] != true
        || events[0]["total"] != 1
        || events[0]["collection_complete"] != true
        || events[1]["type"] != "item"
        || events[1]["index"] != 0
        || events[1]["status"] != "planned"
        || events[1]["plan"] != "verify_existing"
        || !explicit_null(&events[1], "error")
        || events[1]["existing_observation"]["bytes_equal"] != true
        || events[1]["existing_observation"]["stable"] != true
        || events[1]["publication"]["state"] != "not_published"
        || events[1]["publication"]["cleanup"]["state"] != "not_needed"
        || events[2]["type"] != "summary"
        || events[2]["exit_code"] != 0
        || events[2]["interrupted"] != false
        || events[2]["collection_complete"] != true
        || !explicit_null(&events[2], "run_error")
        || events[2]["counts"]
            != json!({"created":0,"up_to_date":0,"planned":1,"failed":0,"not_processed":0})
        || check["namespace_unchanged"] != true
        || !sha256(&check["namespace_before_sha256"])
        || check["namespace_before_sha256"] != check["namespace_after_sha256"]
        || !sha256(&check["original_sha256_before"])
        || check["original_sha256_before"] != check["original_sha256_after"]
        || !check["original_identity_before"]
            .as_str()
            .is_some_and(|identity| !identity.is_empty() && identity.len() < 128)
        || check["original_identity_before"] != check["original_identity_after"]
    {
        return Err("Windows short-root Check is incomplete or changed independent original/namespace facts".into());
    }
    for observation in [
        &events[1]["publication"],
        &events[1]["existing_observation"],
    ] {
        if observation["file_sync"]["method"] != "not_requested"
            || observation["file_sync"]["state"] != "not_attempted"
            || !explicit_null(&observation["file_sync"], "native_code")
            || observation["namespace_sync"]["method"] != "not_requested"
            || observation["namespace_sync"]["state"] != "not_attempted"
            || !explicit_null(&observation["namespace_sync"], "native_code")
            || observation["namespace_sync"]["required_edges"] != 0
            || observation["namespace_sync"]["completed_edges"] != 0
            || !explicit_null(&observation["namespace_sync"], "failed_edge_index")
        {
            return Err("Read-only short-root Check requested native synchronization".into());
        }
    }
    Ok(())
}

fn validate_windows_entries(profile: &Value, root: &str, volume: &Value) -> Result<()> {
    use std::collections::{BTreeMap, BTreeSet};
    let expected = profile["behavior"]["entries"]
        .as_array()
        .ok_or("Common Windows namespace observations are absent")?;
    let entries = profile["native_entries"]
        .as_array()
        .ok_or("Native generated long/short entry observations are absent")?;
    let volume = volume.as_str().ok_or("Observed Windows volume is absent")?;
    let mut expected_names = BTreeMap::new();
    for entry in expected {
        let name = entry["relative"]
            .as_str()
            .ok_or("Common relative entry is absent")?;
        let directory = entry["directory"]
            .as_bool()
            .ok_or("Common entry type is absent")?;
        if expected_names.insert(name, directory).is_some() {
            return Err("Common namespace contains a duplicate entry".into());
        }
    }
    let mut names = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for entry in entries {
        let relative = entry["relative"]
            .as_str()
            .ok_or("Native relative entry is absent")?;
        let directory = entry["directory"]
            .as_bool()
            .ok_or("Native entry type is absent")?;
        let long = entry["long_path"]
            .as_str()
            .ok_or("Native long spelling is absent")?;
        let short = entry["short_path"]
            .as_str()
            .ok_or("Native short spelling is absent")?;
        let long = long.strip_prefix("\\\\?\\").unwrap_or(long);
        let short = short.strip_prefix("\\\\?\\").unwrap_or(short);
        let identity = entry["long_identity"]
            .as_str()
            .ok_or("Native entry identity is absent")?;
        let identity_shape = identity.split_once(':').is_some_and(|(volume, file)| {
            volume.len() == 16
                && file.len() == 32
                && volume
                    .bytes()
                    .chain(file.bytes())
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        });
        if relative.is_empty()
            || relative.contains([':', '\0', '/'])
            || relative
                .split('\\')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || names.insert(relative, directory).is_some()
            || long != format!("{root}\\destination\\{relative}")
            || !short.starts_with(volume)
            || short.contains('\0')
            || !identity_shape
            || entry["short_identity"] != identity
            || !identities.insert(identity)
            || (directory && !entry["case_sensitive"].is_boolean())
            || (!directory && !explicit_null(entry, "case_sensitive"))
        {
            return Err(
                "Native generated case/long/short spelling or identity proof is inconsistent"
                    .into(),
            );
        }
    }
    if names != expected_names {
        return Err(
            "Native generated entry set differs from the independent common namespace".into(),
        );
    }
    Ok(())
}

fn native_bytes(value: &Value, family: OsFamily) -> Result<Vec<u8>> {
    let map = value
        .as_object()
        .ok_or("Native profile path is not an object")?;
    if map.len() != 2
        || value["encoding"]
            != if family == OsFamily::Windows {
                "windows-utf16le"
            } else {
                "unix-bytes"
            }
    {
        return Err("Native profile path encoding differs from actual platform".into());
    }
    let text = value["base64"]
        .as_str()
        .ok_or("Native profile path has no encoded units")?;
    if text.len() > 4 * eml_core::MAX_PATH_UNITS {
        return Err("Native profile path exceeds bound".into());
    }
    let bytes = STANDARD.decode(text)?;
    if bytes.len() > eml_core::MAX_PATH_UNITS * if family == OsFamily::Windows { 2 } else { 1 }
        || bytes.is_empty()
        || STANDARD.encode(&bytes) != text
        || (family == OsFamily::Windows
            && (bytes.len() % 2 != 0 || bytes.as_chunks::<2>().0.contains(&[0, 0])))
        || (family != OsFamily::Windows && bytes.contains(&0))
    {
        return Err("Native profile path units are malformed".into());
    }
    Ok(bytes)
}
fn identities(value: &Value) -> Result<std::collections::BTreeSet<(u64, u64)>> {
    let rows = value.as_array().ok_or("Native identity set is absent")?;
    let mut set = std::collections::BTreeSet::new();
    for row in rows {
        let pair = row
            .as_array()
            .filter(|row| row.len() == 2)
            .ok_or("Native identity is malformed")?;
        if !set.insert((
            pair[0].as_u64().ok_or("Native device is missing")?,
            pair[1].as_u64().ok_or("Native file ID is missing")?,
        )) {
            return Err("Native identity set contains duplicates".into());
        }
    }
    Ok(set)
}
fn validate_observed_names(behavior: &Value, family: OsFamily) -> Result<()> {
    let hash = behavior["source_bytes_sha256"]
        .as_str()
        .filter(|text| {
            text.len() == 64
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        .ok_or("Independent source hash is malformed")?;
    let mut expected = std::collections::BTreeMap::new();
    for case in behavior["expected"]
        .as_array()
        .ok_or("Independent native paths are absent")?
    {
        let source = case["source"]
            .as_str()
            .ok_or("Independent source spelling is missing")?;
        let source_units: Vec<u8> = if family == OsFamily::Windows {
            source.encode_utf16().flat_map(u16::to_le_bytes).collect()
        } else {
            source.as_bytes().to_vec()
        };
        if native_bytes(&case["source_native"], family)? != source_units
            || behavior["source_identities"]
                .get(source)
                .and_then(Value::as_array)
                .is_none_or(|identity| {
                    identity.len() != 7 || identity.iter().any(|unit| unit.as_u64().is_none())
                })
        {
            return Err("Native source differs from the observed original selection".into());
        }
        if case["sha256"] != hash
            || expected
                .insert(
                    native_bytes(&case["source_native"], family)?,
                    (
                        native_bytes(&case["destination_native"], family)?,
                        native_bytes(&case["fresh_destination_native"], family)?,
                    ),
                )
                .is_some()
        {
            return Err(
                "Independent source/destination oracles are duplicated or inconsistent".into(),
            );
        }
    }
    for (index, run) in behavior["runs"]
        .as_array()
        .ok_or("Native runs are absent")?
        .iter()
        .enumerate()
    {
        let destinations = run["destinations"]
            .as_array()
            .ok_or("Actual native destination observations are absent")?;
        let mut seen = std::collections::BTreeSet::new();
        for destination in destinations {
            let source = native_bytes(&destination["source_native"], family)?;
            let pair = expected
                .get(&source)
                .ok_or("Actual native source is outside independent selection")?;
            let target = if index == 3 { &pair.1 } else { &pair.0 };
            if !seen.insert(source)
                || native_bytes(&destination["independent_expected"], family)? != *target
                || native_bytes(&destination["actual_destination"], family)? != *target
            {
                return Err(
                    "Actual destination differs from independent complete-route oracle".into(),
                );
            }
        }
        if seen.len() != 16 {
            return Err("Actual native destination selection is incomplete".into());
        }
        if index != 0 {
            let namespace = &run["namespace"];
            let files = identities(&namespace["expected_file_identities"])?;
            let directories = identities(&namespace["expected_directory_identities"])?;
            if files.len() != 16
                || directories.is_empty()
                || files != identities(&namespace["actual_file_identities"])?
                || directories != identities(&namespace["actual_directory_identities"])?
                || !files.is_disjoint(&directories)
            {
                return Err(
                    "Actual namespace has aliases, missing entries or residual directories".into(),
                );
            }
            let entries = namespace["entries"]
                .as_array()
                .ok_or("Actual native entries are absent")?;
            let mut actual_files = std::collections::BTreeSet::new();
            let mut actual_directories = std::collections::BTreeSet::new();
            for entry in entries {
                let id = (
                    entry["device"].as_u64().ok_or("Entry device is absent")?,
                    entry["inode"].as_u64().ok_or("Entry identity is absent")?,
                );
                if entry["directory"] == true {
                    if !actual_directories.insert(id) {
                        return Err("Native directory entry is duplicated".into());
                    }
                } else if entry["directory"] != false
                    || entry["sha256"] != hash
                    || !actual_files.insert(id)
                {
                    return Err(
                        "Native leaf bytes or identity differ from independent oracle".into(),
                    );
                }
            }
            if actual_files != files || actual_directories != directories {
                return Err("Native entry inventory contradicts exact identity sets".into());
            }
        }
    }
    Ok(())
}

fn validate_unsupported_new_name(value: &Value) -> Result<()> {
    let runs = value["runs"]
        .as_array()
        .ok_or("Unsupported new-name controls are absent")?;
    let uid = value["owner_uid"]
        .as_u64()
        .ok_or("Unsupported new-name owner is absent")?;
    if value["profile"] != "overlay"
        || value["passed"] != true
        || value["destination_mode"] != 0o700
        || runs.len() != 2
    {
        return Err("Unsupported new-name controlled selection differs".into());
    }
    for (run, mode) in runs.iter().zip(["check", "create"]) {
        let before = run["before"]
            .as_array()
            .ok_or("New-name before inventory is absent")?;
        let item = &run["item"];
        let publication = &item["publication"];
        if run["mode"] != mode
            || run["exit_code"] != 1
            || before.len() != 1
            || run["before"] != run["after"]
            || before[0]["name"] != "."
            || before[0]["mode"] != 0o700
            || before[0]["kind"] != 0o040000
            || before[0]["uid"].as_u64() != Some(uid)
            || before[0]["device"].as_u64().is_none()
            || before[0]["mtime_ns"].as_u64().is_none()
            || before[0]["ctime_ns"].as_u64().is_none()
            || before[0]["inode"].as_u64().is_none_or(|n| n == 0)
            || !explicit_null(&before[0], "sha256")
            || item["status"] != "failed"
            || item["error"]["code"] != "DESTINATION_UNSUPPORTED"
            || item["error"]["phase"] != "admission"
            || !explicit_null(item, "existing_observation")
            || publication["state"] != "not_published"
            || publication["file_sync"]
                != json!({"method":"not_requested","state":"not_attempted","native_code":null})
            || publication["namespace_sync"]
                != json!({"method":"not_requested","state":"not_attempted","native_code":null,"required_edges":0,"completed_edges":0,"failed_edge_index":null})
            || publication["cleanup"]
                != json!({"state":"not_needed","residual_name":null,"native_code":null})
        {
            return Err("Unsupported-profile Check/Create changed or lacked a safe owned namespace, or attempted storage effects".into());
        }
    }
    Ok(())
}

fn validate_existing_profiles(observation: &Value, binary: &ByteIdentity) -> Result<()> {
    let profiles = observation["existing_profiles"]
        .as_array()
        .ok_or("Existing-only native profiles are absent")?;
    let mut names = std::collections::BTreeSet::new();
    for profile in profiles {
        let name = profile["profile"]
            .as_str()
            .ok_or("Existing-only profile name is absent")?;
        let ordinary = name == "ordinary-ext4-existing-only";
        let behavior = &profile["behavior"];
        validate_linux_mount(profile, name)?;
        if !["ordinary-ext4-existing-only", "overlay-existing-only"].contains(&name)
            || !names.insert(name)
            || profile["status"] != "passed"
            || profile["exit_code"] != 0
            || behavior["schema"] != 1
            || behavior["passed"] != true
            || !explicit_null(behavior, "failure")
            || behavior["binary_sha256"] != binary.sha256
            || behavior["creation_policy_refused"] != true
            || behavior["require_sync_success"] != ordinary
        {
            return Err(
                "Existing-only native profile is missing, duplicated or unqualified".into(),
            );
        }
        if ordinary {
            if !explicit_null(behavior, "unsupported_new_name") {
                return Err(
                    "Ordinary existing profile has an unexpected unsupported-name claim".into(),
                );
            }
        } else {
            validate_unsupported_new_name(&behavior["unsupported_new_name"])?;
        }
        let runs = behavior["runs"]
            .as_array()
            .ok_or("Existing-only native cases are absent")?;
        if runs.len() != 4 {
            return Err("Existing-only native selection is incomplete".into());
        }
        for (run, case) in
            runs.iter()
                .zip(["matching", "unequal", "missing-route", "replaced-route"])
        {
            let item = &run["item"];
            if run["case"] != case
                || run["inventory_unchanged"] != true
                || run["before"].as_array().is_none_or(Vec::is_empty)
                || run["before"] != run["after"]
                || item["publication"]["state"] != "not_published"
                || item["publication"]["file_sync"]["state"] != "not_attempted"
                || item["publication"]["namespace_sync"]["required_edges"] != 0
                || item["publication"]["cleanup"]["state"] != "not_needed"
            {
                return Err(
                    "Existing-only native case changed namespace or attempted publication".into(),
                );
            }
            let existing = &item["existing_observation"];
            if case == "matching" {
                if existing["bytes_equal"] != true || existing["stable"] != true {
                    return Err("Existing-only comparison was not established".into());
                }
                if run["outcome"] == "qualified" {
                    if run["exit_code"] != 0
                        || item["status"] != "up_to_date"
                        || existing["address"] != "verified"
                        || existing["file_sync"]["method"] != "fsync"
                        || existing["file_sync"]["state"] != "succeeded"
                        || !explicit_null(&existing["file_sync"], "native_code")
                        || !explicit_null(item, "error")
                        || existing["namespace_sync"]["method"] != "fsync"
                        || existing["namespace_sync"]["state"] != "succeeded"
                        || !explicit_null(&existing["namespace_sync"], "native_code")
                        || existing["namespace_sync"]["required_edges"] != 1
                        || existing["namespace_sync"]["completed_edges"] != 1
                        || !explicit_null(&existing["namespace_sync"], "failed_edge_index")
                    {
                        return Err(
                            "Existing-only success lacks actual file/endpoint receipts".into()
                        );
                    }
                } else {
                    if ordinary
                        || run["outcome"] != "native_sync_refused"
                        || run["exit_code"] != 1
                        || item["status"] != "failed"
                        || item["error"]["code"] != "EXISTING_SYNC_FAILED"
                    {
                        return Err("Unexpected native existing synchronization failure".into());
                    }
                    let file = &existing["file_sync"];
                    let namespace = &existing["namespace_sync"];
                    let file_failed = ["failed", "unsupported"]
                        .iter()
                        .any(|state| file["state"] == *state);
                    if namespace["required_edges"] != 1
                        || namespace["completed_edges"] != 0
                        || (file_failed
                            && (namespace["state"] != "not_attempted"
                                || namespace["method"] != "not_requested"
                                || !explicit_null(namespace, "native_code")
                                || !explicit_null(namespace, "failed_edge_index")))
                        || (!file_failed
                            && (file["method"] != "fsync"
                                || file["state"] != "succeeded"
                                || !explicit_null(file, "native_code")
                                || namespace["failed_edge_index"] != 0))
                    {
                        return Err("Existing-only sync receipts contradict actual file-then-endpoint order".into());
                    }
                    for probe in [
                        &behavior["independent_sync_probe"]["file_errno"],
                        &behavior["independent_sync_probe"]["endpoint_errno"],
                    ] {
                        if !probe.is_null() && ![22, 38, 95].iter().any(|errno| *probe == *errno) {
                            return Err(
                                "Independent native probe reported an unrelated I/O failure".into(),
                            );
                        }
                    }
                    let mut unsupported = 0;
                    for (receipt, probe) in [
                        (
                            &existing["file_sync"],
                            &behavior["independent_sync_probe"]["file_errno"],
                        ),
                        (
                            &existing["namespace_sync"],
                            &behavior["independent_sync_probe"]["endpoint_errno"],
                        ),
                    ] {
                        if ["failed", "unsupported"]
                            .iter()
                            .any(|state| receipt["state"] == *state)
                        {
                            if receipt["method"] != "fsync"
                                || receipt["native_code"] != *probe
                                || ![22, 38, 95].iter().any(|errno| *probe == *errno)
                            {
                                return Err("Native sync refusal differs from independent supported-error probe".into());
                            }
                            unsupported += 1;
                        }
                    }
                    if unsupported == 0 {
                        return Err(
                            "Native sync refusal has no actual unsupported operation".into()
                        );
                    }
                }
            } else {
                if run["outcome"] != "refused_without_sync"
                    || run["exit_code"] != 1
                    || item["status"] != "failed"
                {
                    return Err("Existing-only refusal was not established".into());
                }
                let code = match case {
                    "unequal" | "replaced-route" => "DESTINATION_EXISTS",
                    _ => "DESTINATION_UNSUPPORTED",
                };
                if item["error"]["code"] != code
                    || (case != "unequal" && item["error"]["phase"] != "admission")
                    || (case == "unequal" && existing["bytes_equal"] != false)
                {
                    return Err("Existing-only refusal lost its actual boundary cause".into());
                }
                if !existing.is_null()
                    && (existing["file_sync"]["state"] != "not_attempted"
                        || existing["namespace_sync"]["state"] != "not_attempted"
                        || existing["namespace_sync"]["required_edges"] != 0)
                {
                    return Err("Unequal or missing existing copy requested sync authority".into());
                }
            }
        }
    }
    if names
        != std::collections::BTreeSet::from([
            "ordinary-ext4-existing-only",
            "overlay-existing-only",
        ])
    {
        return Err("Existing-only native profile coverage is incomplete".into());
    }
    Ok(())
}

fn validate_linux_mount(profile: &Value, name: &str) -> Result<()> {
    let expected = match name {
        "ext2" => "ext2",
        "ext3" => "ext3",
        "ext4" | "ext4-casefold" | "ordinary-ext4-existing-only" => "ext4",
        "tmpfs" => "tmpfs",
        "xfs" | "xfs-ascii-ci" => "xfs",
        "btrfs" => "btrfs",
        "overlay-existing-only" => "overlay",
        _ => return Err("Unknown mounted native profile".into()),
    };
    let mounts = profile["mount_record"]["filesystems"]
        .as_array()
        .ok_or("Actual mounted native filesystem observation is absent")?;
    if mounts.len() != 1
        || mounts[0]["fstype"] != expected
        || !mounts[0]["target"]
            .as_str()
            .is_some_and(|value| value.starts_with('/'))
        || !profile["filesystem_observation"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
        || profile["cleanup"]["detached"] != true
        || profile["cleanup"]["residual_mounts"] != false
        || profile["cleanup"]["observation_complete"] != true
        || profile["cleanup"]["forced_detach"] != false
        || profile["cleanup"]["operation_exit_code"] != 0
    {
        return Err("Mounted native profile or actual cleanup observations are incomplete".into());
    }
    Ok(())
}

pub(crate) fn run(root: &Path, cli: &FrozenCli, out: &Path, platform: PlatformId) -> Result<Value> {
    let family = platform.family();
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let owner = xtask::archive::create_artifact_directory(&out)?;
    let binary = owner.path().join(if cfg!(windows) {
        "remove-eml-attachments.exe"
    } else {
        "remove-eml-attachments"
    });
    let mut image = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&binary)?;
    image.write_all(cli.bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        image.set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    image.sync_all()?;
    drop(image);
    if ByteIdentity::of(&fs::read(&binary)?) != *cli.identity() {
        return Err("Native profile executable differs from frozen shipping bytes".into());
    }
    let mut command = match family {
        OsFamily::Linux => {
            let mut command = Command::new("bash");
            command.arg(root.join("tools/qualify-linux-name-profiles.sh"));
            command
        }
        OsFamily::Macos => {
            let mut command = Command::new("python3");
            command.arg(root.join("tools/qualify_macos_name_profiles.py"));
            command
        }
        OsFamily::Windows => {
            let mut command = Command::new("pwsh");
            command
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
                .arg(root.join("tools/qualify_windows_name_profiles.ps1"));
            command
        }
    };
    command
        .current_dir(root)
        .arg(&binary)
        .arg(out.join("profiles"));
    let log = out.join("campaign.log");
    let process = controller()?.run(command, &log, Duration::from_secs(1200));
    let observations = (|| -> Result<Value> {
        crate::picker_evidence::observation_document(&fs::read(out.join("profiles/result.json"))?)
    })();
    let source = crate::git(root, &["rev-parse", "HEAD"])?;
    let result = json!({"source":source,"binary":cli.identity(),"process":process.as_ref().ok().map(|p|p.evidence()),"process_failure":process.as_ref().err().map(ToString::to_string),"observations":observations.as_ref().ok(),"observation_read_failure":observations.as_ref().err().map(ToString::to_string)});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    process?.require_success(&log)?;
    observations?;
    if ByteIdentity::of(&fs::read(&binary)?) != *cli.identity() {
        return Err("Native profile executable changed during observation".into());
    }
    validate(&result, &source, cli.identity(), platform)?;
    Ok(result)
}

#[cfg(test)]
pub(crate) fn valid_fixture(source: &str, binary: &ByteIdentity, family: OsFamily) -> Value {
    // Independently authored platform obligations; never generated from validate's selection.
    let profiles: &[(&str, &str)] = match family {
        OsFamily::Linux => &[
            ("ext2", "case-sensitive"),
            ("ext3", "case-sensitive"),
            ("ext4", "case-sensitive"),
            ("ext4-casefold", "case-insensitive"),
            ("tmpfs", "case-sensitive"),
            ("xfs", "case-sensitive"),
            ("xfs-ascii-ci", "case-insensitive"),
            ("btrfs", "case-sensitive"),
        ],
        OsFamily::Macos => &[
            ("apfs-case-insensitive", "case-insensitive"),
            ("apfs-case-sensitive", "case-sensitive"),
            ("hfs-case-insensitive", "case-insensitive"),
            ("hfs-case-sensitive", "case-sensitive"),
        ],
        OsFamily::Windows => &[
            ("ntfs-case-insensitive-8dot3", "case-insensitive"),
            ("ntfs-case-sensitive-8dot3", "case-sensitive"),
        ],
    };
    let encoding = if family == OsFamily::Windows {
        "windows-utf16le"
    } else {
        "unix-bytes"
    };
    let native = |text: &str| {
        let bytes: Vec<u8> = if family == OsFamily::Windows {
            text.encode_utf16().flat_map(u16::to_le_bytes).collect()
        } else {
            text.as_bytes().to_vec()
        };
        json!({"encoding":encoding,"base64":STANDARD.encode(bytes)})
    };
    let hash = "09074090b5e9c7cc758587507c6c84e279f29d69fb199995349b61417c17b85c";
    let cases: Vec<_> = (b'a'..=b'p').map(|byte| {
        let source = format!("/owned/source-{}", byte as char);
        let target = format!("/owned/copy-{}", byte as char);
        let fresh = format!("/owned/fresh/copy-{}", byte as char);
        json!({"source":source,"target":target,"token":format!("{}{}", "a".repeat(25), byte as char),"source_native":native(&source),"destination_native":native(&target),"fresh_destination_native":native(&fresh),"sha256":hash})
    }).collect();
    let mut entries:Vec<_> = (1..=16).map(|index|json!({"relative":format!("copy-{index}"),"device":1,"inode":index,"directory":false,"sha256":hash})).collect();
    entries.push(
        json!({"relative":"owned-mirror","device":1,"inode":100,"directory":true,"sha256":null}),
    );
    let ids: Vec<_> = (1..=16).map(|index| json!([1, index])).collect();
    let namespace = json!({"expected_file_identities":ids,"actual_file_identities":ids,"expected_directory_identities":[[1,100]],"actual_directory_identities":[[1,100]],"entries":entries});
    let originals: serde_json::Map<String, Value> = cases
        .iter()
        .map(|case| {
            (
                case["source"].as_str().unwrap().to_owned(),
                json!([1, 2, 3, 4, 448, 501, 20]),
            )
        })
        .collect();
    let mut value = json!({"source":source,"binary":binary,"process":{"exit_code":0,"signal":null,"timed_out":false,"cancelled":false,"unexpected_survivors":0,"cleanup_complete":true,"elapsed_seconds":1.0,"cleanup_seconds":0.1},"process_failure":null,"observation_read_failure":null,"observations":{"schema":1,"binary_sha256":binary.sha256,"passed":true,"profiles":profiles.iter().map(|(profile,mode)|json!({"profile":profile,"status":"passed","exit_code":0,"behavior":{"schema":1,"passed":true,"failure":null,"binary_sha256":binary.sha256,"case_mode":mode,"expected":cases,"source_identities":originals,"entries":[{"relative":"owned native entry"}],"runs":[{"mode":"check","exit_code":0,"items":16,"statuses":vec!["planned";16]},{"mode":"create","exit_code":0,"items":16,"statuses":vec!["created";16]},{"mode":"repeat","exit_code":0,"items":16,"statuses":vec!["up_to_date";16]},{"mode":"create","exit_code":0,"items":16,"statuses":vec!["created";16]}]}})).collect::<Vec<_>>()}});
    for profile in value["observations"]["profiles"].as_array_mut().unwrap() {
        let behavior = &mut profile["behavior"];
        behavior["source_bytes_sha256"] = json!(hash);
        behavior["entries"] = entries.clone().into();
        for (index, run) in behavior["runs"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            run["destinations"]=cases.iter().map(|case| { let expected=if index==3 { &case["fresh_destination_native"] } else { &case["destination_native"] }; json!({"source_native":case["source_native"],"independent_expected":expected,"actual_destination":expected}) }).collect::<Vec<_>>().into();
            if index != 0 {
                run["namespace"] = namespace.clone();
            }
        }
    }
    if family == OsFamily::Macos {
        for profile in value["observations"]["profiles"].as_array_mut().unwrap() {
            let name = profile["profile"].as_str().unwrap();
            let filesystem = if name.starts_with("apfs-") {
                "apfs"
            } else {
                "hfs"
            };
            let sensitive = profile["behavior"]["case_mode"] == "case-sensitive";
            profile["cleanup"] =
                json!({"detached":true,"owned_image_removed":true,"forced_detach":false});
            profile["volume"] = json!({"filesystem_type":filesystem,"ownership_enabled":true,"case":{"case_sensitive":sensitive},"device":"/dev/disk99s1"});
        }
    }
    if family == OsFamily::Linux {
        value["observations"]["existing_profiles"] = existing_profile_fixture(binary);
        for key in ["profiles", "existing_profiles"] {
            for profile in value["observations"][key].as_array_mut().unwrap() {
                let name = profile["profile"].as_str().unwrap();
                let fs = match name {
                    "ext2" => "ext2",
                    "ext3" => "ext3",
                    "ext4" | "ext4-casefold" | "ordinary-ext4-existing-only" => "ext4",
                    "tmpfs" => "tmpfs",
                    "xfs" | "xfs-ascii-ci" => "xfs",
                    "btrfs" => "btrfs",
                    "overlay-existing-only" => "overlay",
                    _ => unreachable!(),
                };
                profile["mount_record"] =
                    json!({"filesystems":[{"fstype":fs,"target":"/owned/native-mount/copies"}]});
                profile["filesystem_observation"] = json!("Independent native filesystem facts");
                profile["cleanup"] = json!({"detached":true,"residual_mounts":false,"observation_complete":true,"forced_detach":false,"operation_exit_code":0});
            }
        }
    }
    if family == OsFamily::Windows {
        value["observations"]["failure"] = Value::Null;
        value["observations"]["binary_unchanged"] = json!(true);
        value["observations"]["storage"] = json!({"filesystem":"NTFS","drive_type":3,"max_component":255,"lifetime":"kernel-handle-owned","native_cli_machine":0x8664,"global_8dot3_policy":2,"image":"C:\\owned\\owned-name-volume.vhd","disk_number":9,"disk_unique_id":"independent-owned-vhd-id","label":"EML-0123456789ab","physical_path":"\\\\.\\PhysicalDrive9","volume_serial":"1234","volume_root":"R:\\"});
        value["observations"]["cleanup"] =
            json!({"detached":true,"observation_complete":true,"forced_detach":false});
        value["observations"]["host"] = json!({"architecture":"X64"});
        let root = r"\\?\R:\EML metadata observation control";
        let directory = format!("{root}\\directory");
        let root_identity = "00000000000004d2:00000000000000000000000000001001";
        let child_identity = "00000000000004d2:00000000000000000000000000001002";
        let before = json!([
            {"path":root,"attributes":16,"last_write_ticks":637479219060000000u64,"identity":root_identity,"bytes":null,"sha256":null},
            {"path":directory,"attributes":16,"last_write_ticks":637479219060000000u64,"identity":child_identity,"bytes":null,"sha256":null}
        ]);
        let mut after = before.clone();
        after[1]["last_write_ticks"] = json!(637819671670000000u64);
        value["observations"]["metadata_control"] = json!({
            "passed":true,"root":root,"directory":directory,
            "expected_before_ticks":637479219060000000u64,"expected_after_ticks":637819671670000000u64,
            "cached_before_ticks":637134336000000000u64,"cached_after_ticks":637134336000000000u64,
            "native_before":{"Identity":child_identity,"Attributes":16,"LastWriteTicks":637479219060000000u64},
            "native_after":{"Identity":child_identity,"Attributes":16,"LastWriteTicks":637819671670000000u64},
            "namespace_before":before,"namespace_after":after,"namespace_change_detected":true,"cleanup_complete":true
        });
        let unattempted = json!({"file_sync":{"method":"not_requested","state":"not_attempted","native_code":null},"namespace_sync":{"method":"not_requested","state":"not_attempted","native_code":null,"required_edges":0,"completed_edges":0,"failed_edge_index":null}});
        let mut publication = unattempted.clone();
        publication["state"] = json!("not_published");
        publication["cleanup"] = json!({"state":"not_needed"});
        let mut existing = unattempted;
        existing["bytes_equal"] = json!(true);
        existing["stable"] = json!(true);
        for profile in value["observations"]["profiles"].as_array_mut().unwrap() {
            profile["executed"] = json!(true);
            profile["failure"] = Value::Null;
            profile["volume"] = json!({"filesystem_type":"ntfs","case":{"case_sensitive":profile["behavior"]["case_mode"]=="case-sensitive","destination_before":profile["behavior"]["case_mode"]=="case-sensitive","destination_after":profile["behavior"]["case_mode"]=="case-sensitive"},"root":"R:\\EML qualification root","short_root":"R:\\EMLQUA~1","serial":"1234","label":"EML-0123456789ab","drive_type":3,"global_8dot3_policy":2});
            profile["source_case_sensitive"] = json!(false);
            profile["native_entries"] = profile["behavior"]["entries"].as_array().unwrap().iter().enumerate().map(|(index, entry)| {
                let relative = entry["relative"].as_str().unwrap();
                let long = format!("R:\\EML qualification root\\destination\\{relative}");
                let identity = format!("00000000000004d2:{:032x}", index + 1);
                json!({"relative":relative,"directory":entry["directory"],"case_sensitive":if entry["directory"]==true { json!(index % 2 == 0) } else { Value::Null },"long_path":long,"short_path":long,"long_identity":identity,"short_identity":identity})
            }).collect::<Vec<_>>().into();
            profile["short_root_check"] = json!({"original_identity_before":"independent-native-file-id","original_identity_after":"independent-native-file-id","original_sha256_before":hash,"original_sha256_after":hash,"namespace_unchanged":true,"namespace_before_sha256":hash,"namespace_after_sha256":hash,"events":[{"type":"start","protocol":1,"dry_run":true,"total":1,"collection_complete":true},{"type":"item","index":0,"status":"planned","plan":"verify_existing","error":null,"existing_observation":existing,"publication":publication},{"type":"summary","exit_code":0,"interrupted":false,"collection_complete":true,"run_error":null,"counts":{"created":0,"up_to_date":0,"planned":1,"failed":0,"not_processed":0}}]});
        }
    }
    value
}

#[cfg(test)]
fn existing_profile_fixture(binary: &ByteIdentity) -> Value {
    let mut runs = Vec::new();
    for (case, outcome, exit_code, code) in [
        ("matching", "qualified", 0, None),
        (
            "unequal",
            "refused_without_sync",
            1,
            Some("DESTINATION_EXISTS"),
        ),
        (
            "missing-route",
            "refused_without_sync",
            1,
            Some("DESTINATION_UNSUPPORTED"),
        ),
        (
            "replaced-route",
            "refused_without_sync",
            1,
            Some("DESTINATION_EXISTS"),
        ),
    ] {
        let existing = match case {
            "matching" => {
                json!({"bytes_equal":true,"stable":true,"address":"verified","file_sync":{"method":"fsync","state":"succeeded","native_code":null},"namespace_sync":{"method":"fsync","state":"succeeded","native_code":null,"required_edges":1,"completed_edges":1,"failed_edge_index":null}})
            }
            "unequal" => {
                json!({"bytes_equal":false,"file_sync":{"method":"not_requested","state":"not_attempted","native_code":null},"namespace_sync":{"method":"not_requested","state":"not_attempted","native_code":null,"required_edges":0,"completed_edges":0,"failed_edge_index":null}})
            }
            _ => Value::Null,
        };
        runs.push(json!({"case":case,"outcome":outcome,"exit_code":exit_code,"inventory_unchanged":true,"before":[{"name":".","mode":511}],"after":[{"name":".","mode":511}],"item":{"status":if exit_code==0 {"up_to_date"}else{"failed"},"error":code.map(|code|json!({"code":code,"phase":if case=="unequal"{"existing"}else{"admission"}})),"existing_observation":existing,"publication":{"state":"not_published","file_sync":{"state":"not_attempted"},"namespace_sync":{"required_edges":0},"cleanup":{"state":"not_needed"}}}}));
    }
    let inventory = json!([{"name":".","device":2,"inode":9,"mode":448,"kind":16384,"uid":1001,"gid":1001,"mtime_ns":100,"ctime_ns":100,"sha256":null}]);
    let refusal_item = json!({"status":"failed","error":{"code":"DESTINATION_UNSUPPORTED","phase":"admission"},"existing_observation":null,"publication":{"state":"not_published","file_sync":{"method":"not_requested","state":"not_attempted","native_code":null},"namespace_sync":{"method":"not_requested","state":"not_attempted","native_code":null,"required_edges":0,"completed_edges":0,"failed_edge_index":null},"cleanup":{"state":"not_needed","residual_name":null,"native_code":null}}});
    let unsupported = json!({"profile":"overlay","destination_mode":448,"owner_uid":1001,"passed":true,"runs":[{"mode":"check","exit_code":1,"before":inventory,"after":inventory,"item":refusal_item},{"mode":"create","exit_code":1,"before":inventory,"after":inventory,"item":refusal_item}]});
    json!([{"profile":"ordinary-ext4-existing-only","status":"passed","exit_code":0,"behavior":{"schema":1,"binary_sha256":binary.sha256,"passed":true,"failure":null,"creation_policy_refused":true,"require_sync_success":true,"unsupported_new_name":null,"independent_sync_probe":{"file_errno":null,"endpoint_errno":null},"runs":runs}},{"profile":"overlay-existing-only","status":"passed","exit_code":0,"behavior":{"schema":1,"binary_sha256":binary.sha256,"passed":true,"failure":null,"creation_policy_refused":true,"require_sync_success":false,"unsupported_new_name":unsupported,"independent_sync_probe":{"file_errno":null,"endpoint_errno":null},"runs":runs}}])
}

#[cfg(test)]
mod tests;
