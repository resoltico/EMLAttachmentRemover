//! Admission of the ten finite native Windows existing-copy synchronization controls.
use crate::{Result, picker_evidence::observation_document};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use xtask::archive::ByteIdentity;

const MAX_RECORD: usize = 256 * 1024;
const CASES: [(&str, &[&str], &str, &str); 10] = [
    (
        "adapter-handoff-success",
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
        "eml_fs",
    ),
    (
        "adapter-readonly-sync-denied",
        &[
            "comparison_opened",
            "comparison_read",
            "comparison_closed",
            "sync_open_denied",
        ],
        "sync_access_denied",
        "eml_fs",
    ),
    (
        "adapter-close-required",
        &[
            "comparison_opened",
            "sync_open_denied",
            "comparison_closed",
            "sync_opened",
            "sync_closed",
        ],
        "checked_handoff",
        "eml_fs",
    ),
    (
        "adapter-parent-retarget",
        &["comparison_opened", "parent_retargeted", "binding_refused"],
        "parent_changed",
        "eml_fs",
    ),
    (
        "core-readonly-equal",
        &["core_returned"],
        "EXISTING_SYNC_FAILED",
        "execution",
    ),
    (
        "core-readonly-unequal",
        &["core_returned"],
        "DESTINATION_EXISTS",
        "execution",
    ),
    (
        "core-sync-success",
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
        "eml_core",
    ),
    (
        "core-gap-same-id",
        &[
            "comparison_matched",
            "comparison_closed",
            "sync_opened",
            "binding_refused",
        ],
        "DESTINATION_EXISTS",
        "eml_core",
    ),
    (
        "core-gap-replacement",
        &[
            "comparison_matched",
            "comparison_closed",
            "sync_opened",
            "binding_refused",
        ],
        "ADDRESS_CHANGED",
        "eml_core",
    ),
    (
        "core-source-alias",
        &["comparison_refused"],
        "source_alias",
        "existing",
    ),
];
fn require(ok: bool, why: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(why.into()) }
}
fn fields(v: &Value, keys: &[&str]) -> Result<()> {
    let object = v
        .as_object()
        .ok_or("Windows sync observation is not an object")?;
    require(
        object.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == keys.iter().copied().collect(),
        "Windows sync observation fields differ",
    )
}
fn uint(v: &Value) -> Result<u64> {
    v.as_u64()
        .ok_or_else(|| "Windows sync observation requires an unsigned integer".into())
}
fn nullable_code(v: &Value) -> Result<Option<i64>> {
    if v.is_null() {
        Ok(None)
    } else {
        v.as_i64()
            .map(Some)
            .ok_or_else(|| "Invalid Windows native error code".into())
    }
}
fn native_path(v: &Value) -> Result<Vec<u16>> {
    let a = v
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 32767)
        .ok_or("Invalid Windows sync native path")?;
    let units: Vec<u16> = a
        .iter()
        .map(|x| u16::try_from(uint(x)?).map_err(Into::into))
        .collect::<Result<_>>()?;
    require(
        !units.contains(&0)
            && units.starts_with(&[92, 92, 63, 92])
            && units.len() > 7
            && matches!(units[4], 65..=90 | 97..=122)
            && units[5] == 58
            && units[6] == 92,
        "Windows sync path is not an admitted absolute drive expression",
    )?;
    require(
        !units[7..]
            .split(|x| *x == 92)
            .any(|part| part.is_empty() || part == [46] || part == [46, 46]),
        "Windows sync path has ambiguous components",
    )?;
    Ok(units)
}
fn identity(v: &Value) -> Result<()> {
    fields(v, &["volume", "file"])?;
    uint(&v["volume"])?;
    let a = v["file"]
        .as_array()
        .filter(|a| a.len() == 16)
        .ok_or("Windows identity requires all sixteen native file ID bytes")?;
    for x in a {
        require(uint(x)? <= 255, "Windows file ID unit exceeds a byte")?;
    }
    require(
        a.iter().any(|x| x.as_u64() != Some(0)),
        "Empty Windows file identity",
    )
}
fn signature(v: &Value) -> Result<()> {
    fields(
        v,
        &["identity", "size", "modified_native", "changed_native"],
    )?;
    identity(&v["identity"])?;
    uint(&v["size"])?;
    for k in ["modified_native", "changed_native"] {
        let a = v[k]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("Incomplete Windows native signature timestamp")?;
        for x in a {
            require(
                x.as_i64().is_some(),
                "Invalid native signature timestamp unit",
            )?;
        }
    }
    Ok(())
}
fn file(v: &Value) -> Result<()> {
    fields(v, &["signature", "parent", "bytes", "readonly"])?;
    signature(&v["signature"])?;
    identity(&v["parent"])?;
    require(
        v["parent"]["volume"] == v["signature"]["identity"]["volume"],
        "Held file and parent belong to different native volumes",
    )?;
    let a = v["bytes"]
        .as_array()
        .filter(|a| a.len() <= 1024 * 1024)
        .ok_or("Windows sync owned fixture bytes exceed bound")?;
    for x in a {
        require(uint(x)? <= 255, "Invalid fixture byte")?;
    }
    require(
        v["readonly"].is_boolean() && v["signature"]["size"].as_u64() == Some(a.len() as u64),
        "Windows signature size/readonly observation differs from actual bytes",
    )
}
fn nullable_signature(v: &Value) -> Result<()> {
    if v.is_null() { Ok(()) } else { signature(v) }
}
fn source(v: &Value) -> Result<()> {
    fields(
        v,
        &["file", "write_open_succeeded", "write_open_native_code"],
    )?;
    file(&v["file"])?;
    require(
        v["write_open_succeeded"] == false
            && nullable_code(&v["write_open_native_code"])? == Some(32),
        "Held source did not retain actual write-sharing denial",
    )
}
fn snapshot(v: &Value) -> Result<()> {
    fields(v, &["source", "copy"])?;
    if !v["source"].is_null() {
        file(&v["source"])?;
    }
    file(&v["copy"])
}
fn sync(v: &Value) -> Result<()> {
    fields(v, &["method", "state", "native_code"])?;
    require(
        v["method"] == "FlushFileBuffers",
        "Windows synchronization did not record its actual method",
    )?;
    match v["state"].as_str() {
        Some("Succeeded") => require(
            nullable_code(&v["native_code"])?.is_none(),
            "Successful native flush retains an error",
        ),
        Some("Failed" | "Unsupported") => require(
            nullable_code(&v["native_code"])?.is_some_and(|c| c > 0),
            "Failed native flush lacks its actual cause",
        ),
        _ => Err("Unknown native flush observation".into()),
    }
}
fn namespace(v: &Value) -> Result<()> {
    fields(
        v,
        &[
            "receipt",
            "required_edges",
            "completed_edges",
            "failed_edge_index",
        ],
    )?;
    let required = uint(&v["required_edges"])?;
    let completed = uint(&v["completed_edges"])?;
    require(
        required <= u32::MAX as u64 && completed <= required,
        "Invalid endpoint namespace edge counts",
    )?;
    if !v["failed_edge_index"].is_null() {
        require(
            uint(&v["failed_edge_index"])? < required,
            "Invalid failed endpoint edge index",
        )?;
    }
    if !v["receipt"].is_null() {
        sync(&v["receipt"])?;
    }
    Ok(())
}

fn step(v: &Value) -> Result<()> {
    fields(
        v,
        &[
            "operation",
            "result",
            "signature",
            "parent",
            "source",
            "bytes_equal",
            "native_code",
            "sync",
            "namespace",
        ],
    )?;
    require(
        matches!(
            v["result"].as_str(),
            Some("succeeded" | "failed" | "equal" | "unequal" | "changed" | "refused")
        ),
        "Unknown Windows synchronization step result",
    )?;
    nullable_signature(&v["signature"])?;
    if !v["parent"].is_null() {
        identity(&v["parent"])?;
    }
    if !v["source"].is_null() {
        source(&v["source"])?;
    }
    require(
        v["bytes_equal"].is_null() || v["bytes_equal"].is_boolean(),
        "Invalid byte comparison observation",
    )?;
    nullable_code(&v["native_code"])?;
    if !v["sync"].is_null() {
        sync(&v["sync"])?;
    }
    if !v["namespace"].is_null() {
        namespace(&v["namespace"])?;
    }
    Ok(())
}
fn validate_record(v: &Value, architecture: &str) -> Result<(&'static str, &'static str)> {
    fields(
        v,
        &[
            "schema",
            "schema_version",
            "case",
            "architecture",
            "process_id",
            "current_exe_utf16",
            "fixture",
            "requested_parent_utf16",
            "before",
            "steps",
            "outcome",
            "after",
        ],
    )?;
    require(
        v["schema"] == "eml-windows-existing-sync-evidence"
            && v["schema_version"] == 1
            && v["architecture"] == architecture,
        "Windows sync schema/native CPU differs",
    )?;
    require(
        uint(&v["process_id"])? > 0 && uint(&v["process_id"])? <= u32::MAX as u64,
        "Invalid actual test process ID",
    )?;
    native_path(&v["current_exe_utf16"])?;
    fields(&v["fixture"], &["root_utf16", "identity", "filesystem"])?;
    native_path(&v["fixture"]["root_utf16"])?;
    identity(&v["fixture"]["identity"])?;
    require(
        v["fixture"]["filesystem"] == "NTFS",
        "Existing-copy native fixture did not observe held local NTFS",
    )?;
    native_path(&v["requested_parent_utf16"])?;
    snapshot(&v["before"])?;
    snapshot(&v["after"])?;
    for snap in [&v["before"], &v["after"]] {
        require(
            snap["copy"]["signature"]["identity"]["volume"] == v["fixture"]["identity"]["volume"],
            "Copy/fixture volume observations differ",
        )?;
    }
    fields(&v["outcome"], &["classification", "native_code"])?;
    nullable_code(&v["outcome"]["native_code"])?;
    let &(name, operations, outcome, group) = CASES
        .iter()
        .find(|(name, _, _, _)| v["case"] == *name)
        .ok_or("Unexpected Windows synchronization case")?;
    let steps = v["steps"]
        .as_array()
        .filter(|s| s.len() == operations.len())
        .ok_or("Windows synchronization trace is incomplete")?;
    for (s, operation) in steps.iter().zip(operations) {
        step(s)?;
        require(
            s["operation"] == *operation,
            "Windows synchronization operations are out of order",
        )?;
    }
    require(
        v["outcome"]["classification"] == outcome,
        "Windows sync final outcome differs from the controlled case",
    )?;
    require(
        v["before"]["source"] == v["after"]["source"],
        "Existing-only execution changed original source bytes/signature/parent/attributes",
    )?;
    if name.starts_with("core-") || name == "adapter-handoff-success" {
        require(
            !v["before"]["source"].is_null(),
            "Core sync control lacks its actual original source",
        )?;
    } else {
        require(
            v["before"]["source"].is_null(),
            "Adapter control cannot invent an original source",
        )?;
    }
    if !matches!(
        name,
        "core-gap-same-id" | "core-gap-replacement" | "adapter-parent-retarget"
    ) {
        require(
            v["before"]["copy"] == v["after"]["copy"],
            "Existing-only control changed copy bytes/signature/parent/attributes",
        )?;
    }
    semantics(v, name)?;
    Ok((name, group))
}
fn nulls(v: &Value, keys: &[&str]) -> Result<()> {
    require(
        keys.iter().all(|k| v.get(k) == Some(&Value::Null)),
        "Unattempted synchronization stage contains invented observations",
    )
}
fn semantics(v: &Value, name: &str) -> Result<()> {
    let steps = v["steps"].as_array().unwrap();
    let before = &v["before"];
    let after = &v["after"];
    for s in steps {
        let op = s["operation"].as_str().unwrap();
        let result = match op {
            "comparison_read" | "sync_read" | "comparison_matched" | "recompared" => "equal",
            "readonly_flush" | "sync_open_denied" => "failed",
            "parent_retargeted" => "changed",
            "binding_refused" | "comparison_refused" | "core_returned" => "refused",
            _ => "succeeded",
        };
        require(
            s["result"] == result,
            "Native sync step result contradicts its operation",
        )?;
        let held = name == "adapter-handoff-success"
            || matches!(
                name,
                "core-sync-success"
                    | "core-gap-same-id"
                    | "core-gap-replacement"
                    | "core-source-alias"
            );
        if held {
            require(
                !s["source"].is_null() && s["source"]["file"] == before["source"],
                "Native synchronization lost its unchanged held original source",
            )?;
        } else {
            nulls(s, &["source"])?;
        }
        let code = if op == "readonly_flush" || op == "sync_open_denied" {
            Some(if name == "adapter-close-required" {
                32
            } else {
                5
            })
        } else if op == "core_returned" && name == "core-readonly-equal" {
            Some(5)
        } else {
            None
        };
        require(
            nullable_code(&s["native_code"])? == code,
            "Native operation error contradicts its controlled cause",
        )?;
        if matches!(op, "readonly_flush" | "file_flush") {
            require(
                !s["sync"].is_null()
                    && s["sync"]["state"]
                        == if op == "readonly_flush" {
                            "Failed"
                        } else {
                            "Succeeded"
                        }
                    && nullable_code(&s["sync"]["native_code"])? == code,
                "Actual file flush receipt differs",
            )?;
        } else {
            nulls(s, &["sync"])?;
        }
        if op == "endpoint_flush" {
            require(
                s["namespace"]
                    == json!({"receipt":{"method":"FlushFileBuffers","state":"Succeeded","native_code":null},"required_edges":1,"completed_edges":1,"failed_edge_index":null}),
                "Actual endpoint flush is not complete",
            )?;
        } else if op == "core_returned" {
            require(
                s["namespace"]
                    == json!({"receipt":null,"required_edges":if name=="core-readonly-equal"{1}else{0},"completed_edges":0,"failed_edge_index":null}),
                "Refused existing-only path gained synchronization",
            )?;
        } else {
            nulls(s, &["namespace"])?;
        }
        if matches!(
            op,
            "comparison_closed" | "sync_closed" | "readonly_flush" | "file_flush"
        ) {
            nulls(s, &["signature", "parent", "bytes_equal"])?;
        }
        if op == "endpoint_flush" {
            nulls(s, &["signature", "bytes_equal"])?;
            require(
                s["parent"] == after["copy"]["parent"],
                "Endpoint receipt identifies another directory",
            )?;
        }
        if matches!(
            op,
            "comparison_matched" | "comparison_read" | "sync_read" | "recompared" | "requalified"
        ) {
            require(
                s["bytes_equal"] == true,
                "A matching comparison did not retain equality",
            )?;
        }
        if matches!(
            op,
            "comparison_opened" | "comparison_matched" | "comparison_read"
        ) {
            require(
                s["signature"] == before["copy"]["signature"]
                    && s["parent"] == before["copy"]["parent"],
                "Initial comparison signature/parent differs",
            )?;
        }
        if matches!(
            op,
            "sync_opened"
                | "sync_read"
                | "binding_matched"
                | "recompared"
                | "requalified"
                | "parent_retargeted"
        ) {
            require(
                s["signature"] == after["copy"]["signature"]
                    && s["parent"] == after["copy"]["parent"],
                "Fresh opened signature/parent differs",
            )?;
        }
        if op == "binding_refused" {
            if name == "adapter-parent-retarget" {
                nulls(s, &["signature", "parent", "bytes_equal"])?;
            } else {
                require(
                    s["signature"] == after["copy"]["signature"]
                        && s["parent"] == after["copy"]["parent"],
                    "Rejected fresh binding observation differs",
                )?;
            }
        }
        if matches!(
            op,
            "sync_open_denied"
                | "comparison_refused"
                | "binding_matched"
                | "sync_opened"
                | "comparison_opened"
                | "parent_retargeted"
                | "binding_refused"
        ) {
            nulls(s, &["bytes_equal"])?;
        }
        if op == "sync_open_denied" {
            nulls(s, &["signature", "parent"])?;
        }
        if op == "comparison_refused" {
            require(
                s["signature"] == before["copy"]["signature"]
                    && s["parent"] == before["copy"]["parent"],
                "Alias refusal does not identify the held alias",
            )?;
        }
        if op == "core_returned" {
            require(
                s["signature"] == after["copy"]["signature"]
                    && s["parent"] == after["copy"]["parent"]
                    && s["bytes_equal"] == (name == "core-readonly-equal"),
                "Readonly outcome lost actual comparison/signature",
            )?;
        }
    }
    let code = if matches!(name, "adapter-readonly-sync-denied" | "core-readonly-equal") {
        Some(5)
    } else {
        None
    };
    require(
        nullable_code(&v["outcome"]["native_code"])? == code,
        "Final native cause differs from actual operation",
    )?;
    match name {
        "core-readonly-equal" | "core-readonly-unequal" | "adapter-readonly-sync-denied" => {
            require(
                before["copy"]["readonly"] == true && after["copy"]["readonly"] == true,
                "Readonly fixture cleared its actual attribute",
            )?
        }
        "core-gap-same-id" => require(
            before["copy"]["signature"]["identity"] == after["copy"]["signature"]["identity"]
                && before["copy"]["signature"] != after["copy"]["signature"]
                && before["copy"]["bytes"] != after["copy"]["bytes"]
                && before["copy"]["parent"] == after["copy"]["parent"],
            "Same-ID gap did not retain changed full signature/bytes",
        )?,
        "core-gap-replacement" => require(
            before["copy"]["signature"]["identity"] != after["copy"]["signature"]["identity"]
                && before["copy"]["parent"] == after["copy"]["parent"],
            "Replacement gap lacks its distinct actual identity",
        )?,
        "adapter-parent-retarget" => require(
            before["copy"]["signature"] == after["copy"]["signature"]
                && before["copy"]["bytes"] == after["copy"]["bytes"]
                && before["copy"]["parent"] != after["copy"]["parent"],
            "Parent retarget did not preserve file signature while changing parent identity",
        )?,
        "core-source-alias" => require(
            before["copy"]["signature"]["identity"] == before["source"]["signature"]["identity"]
                && before["copy"]["bytes"] == before["source"]["bytes"],
            "Alias refusal lacks its actual shared source identity/bytes",
        )?,
        _ => {}
    }
    Ok(())
}
const REQUIRED_TESTS: [&str; 8] = [
    "held_readonly_file_must_be_checked_closed_before_fresh_sync_access",
    "native_existing_sync_access_keeps_identity_bytes_and_self_validation",
    "readonly_existing_file_is_readable_but_sync_access_is_refused_without_changes",
    "retargeted_parent_with_same_file_hardlink_is_a_changed_binding",
    "identical_readonly_copy_retains_equality_and_never_clears_its_attribute",
    "readable_different_readonly_copy_is_a_conflict_before_any_sync_access",
    "controlled_resync_gap_distinguishes_file_replacement_and_same_id_change",
    "hardlink_to_current_source_is_refused_before_byte_comparison",
];
fn byte_identity(v: &Value) -> Result<ByteIdentity> {
    fields(v, &["byte_length", "sha256"])?;
    let value: ByteIdentity = serde_json::from_value(v.clone())?;
    require(
        value.byte_length > 0
            && value.sha256.len() == 64
            && value
                .sha256
                .bytes()
                .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x)),
        "Invalid retained Windows evidence byte identity",
    )?;
    Ok(value)
}
fn retained(v: &Value, limit: usize) -> Result<Value> {
    fields(v, &["identity", "document"])?;
    let text = v["document"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= limit)
        .ok_or("Retained Windows record exceeds its bound")?;
    require(
        byte_identity(&v["identity"])? == ByteIdentity::of(text.as_bytes()),
        "Retained Windows record bytes changed",
    )?;
    observation_document(text.as_bytes())
}
fn process(v: &Value) -> Result<()> {
    require(
        v["native_exit_code"] == 0
            && v["timed_out"] == false
            && v["cancelled"] == false
            && v["cleanup_complete"] == true
            && v["unexpected_survivors"] == 0
            && ["cleanup_error", "observation_error", "termination_error"]
                .iter()
                .all(|k| v.get(k) == Some(&Value::Null)),
        "Windows native test process was not successfully settled",
    )
}
/// Recheck retained actual record bytes during release-input admission on any OS.
pub(crate) fn validate(
    value: &Value,
    source: &str,
    epoch: u64,
    target: &str,
    profile: &str,
) -> Result<()> {
    validate_observations(value, source, epoch, Some(target), profile)
}
fn validate_observations(
    value: &Value,
    source: &str,
    epoch: u64,
    target: Option<&str>,
    profile: &str,
) -> Result<()> {
    fields(
        value,
        &[
            "schema",
            "schema_version",
            "source",
            "source_epoch",
            "source_provenance",
            "target",
            "profile",
            "host",
            "test_execution",
            "images",
            "records",
        ],
    )?;
    let architecture = match target {
        Some("x86_64-pc-windows-gnullvm") => "x86_64",
        Some("aarch64-pc-windows-gnullvm") => "aarch64",
        Some(_) => {
            return Err("Windows synchronization evidence bound to another native target".into());
        }
        None => value["host"]["process_architecture"]
            .as_str()
            .filter(|a| matches!(*a, "x86_64" | "aarch64"))
            .ok_or("Default compiler-host evidence lacks its actual CPU")?,
    };
    require(
        value["schema"] == "eml-windows-existing-sync-campaign"
            && value["schema_version"] == 1
            && value["source"] == source
            && value["source_epoch"] == epoch
            && (value["source_provenance"] == "git_commit"
                || (target.is_none() && value["source_provenance"] == "declared_archive"))
            && value["target"] == json!(target)
            && value["profile"] == profile,
        "Windows synchronization evidence source/target/profile differs",
    )?;
    let host = &value["host"];
    let actual_target = host["target"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or("Missing actual Windows compiler host")?;
    require(
        target.is_none_or(|expected| actual_target == expected)
            && actual_target.starts_with(&format!("{architecture}-"))
            && host["compiler"].as_str().is_some_and(|text| {
                text.lines()
                    .any(|line| line == format!("host: {actual_target}"))
            })
            && host["process_os"] == "windows"
            && host["process_architecture"] == architecture
            && host["native_machine"]
                == if architecture == "x86_64" {
                    0x8664
                } else {
                    0xaa64
                }
            && host["emulated_process_machine"] == 0
            && host["actual_kernel"]["major"]
                .as_u64()
                .is_some_and(|n| n >= 10)
            && host["actual_kernel"]["minor"].as_u64().is_some()
            && host["actual_kernel"]["build"].as_u64().is_some(),
        "Windows synchronization evidence lacks matching actual native PE/CPU/kernel/compiler facts",
    )?;
    let execution = &value["test_execution"];
    fields(
        execution,
        &[
            "discovery_identity",
            "execution_identity",
            "control",
            "required_tests",
        ],
    )?;
    byte_identity(&execution["discovery_identity"])?;
    byte_identity(&execution["execution_identity"])?;
    process(&retained(&execution["control"], MAX_RECORD)?)?;
    let tests = execution["required_tests"]
        .as_array()
        .filter(|t| t.len() == REQUIRED_TESTS.len())
        .ok_or("Incomplete Windows synchronization test discovery/execution")?;
    let mut names = BTreeSet::new();
    for test in tests {
        fields(test, &["name", "discovered", "executed"])?;
        let name = test["name"].as_str().ok_or("Invalid required test name")?;
        require(
            REQUIRED_TESTS.contains(&name)
                && names.insert(name)
                && test["discovered"] == 1
                && test["executed"] == 1,
            "Windows synchronization case was not discovered and executed exactly once",
        )?;
    }
    let images = value["images"]
        .as_array()
        .filter(|s| !s.is_empty() && s.len() <= CASES.len())
        .ok_or("Missing actual launched Windows test image identities")?;
    let mut image_paths = BTreeSet::new();
    for image in images {
        fields(image, &["path_utf16", "identity"])?;
        require(
            image_paths.insert(native_path(&image["path_utf16"])?),
            "Duplicate retained Windows test image",
        )?;
        byte_identity(&image["identity"])?;
    }
    let records = value["records"]
        .as_array()
        .filter(|v| v.len() == CASES.len())
        .ok_or("Missing or extra native Windows synchronization case records")?;
    let mut cases = BTreeSet::new();
    let mut groups = std::collections::BTreeMap::<&str, (u64, Vec<u16>)>::new();
    for record in records {
        let raw = retained(record, MAX_RECORD)?;
        let (case, group) = validate_record(&raw, architecture)?;
        require(
            cases.insert(case),
            "Duplicate native Windows synchronization case",
        )?;
        let path = native_path(&raw["current_exe_utf16"])?;
        require(
            image_paths.contains(&path),
            "Windows record is not bound to an actually launched retained test image",
        )?;
        let file = path.rsplit(|x| *x == 92).next().unwrap();
        let prefix: Vec<u16> = format!("{group}-").encode_utf16().collect();
        let suffix: Vec<u16> = ".exe".encode_utf16().collect();
        require(
            file.starts_with(&prefix) && file.ends_with(&suffix),
            "Native record came from another test image group",
        )?;
        let observed = (uint(&raw["process_id"])?, path);
        if let Some(previous) = groups.get(group) {
            require(
                previous == &observed,
                "One test image group has inconsistent actual test process identity",
            )?;
        } else {
            groups.insert(group, observed);
        }
    }
    require(
        image_paths == groups.values().map(|(_, path)| path.clone()).collect(),
        "Windows sync summary retains missing or unrelated test images",
    )?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn collect(
    plan: &crate::distribution::BuildPlan,
    target: Option<&str>,
    release: bool,
    out: &std::path::Path,
    cleanup: &Value,
    host: &Value,
) -> Result<Value> {
    use std::{fs, os::windows::ffi::OsStrExt};
    let architecture = host["process_architecture"]
        .as_str()
        .ok_or("Missing actual Windows test CPU")?;
    let directory = out.join("windows-creation-evidence");
    let mut records = Vec::new();
    let mut record_images = BTreeSet::new();
    let expected: BTreeSet<_> = CASES
        .iter()
        .map(|(case, _, _, _)| format!("existing-sync-{case}.json"))
        .collect();
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or("Evidence filename is not the fixed ASCII case name")?;
        if !name.starts_with("existing-sync-") {
            continue;
        }
        require(
            expected.contains(name) && found.insert(name.to_owned()),
            "Unexpected Windows synchronization evidence filename",
        )?;
        let metadata = fs::symlink_metadata(entry.path())?;
        require(
            metadata.is_file() && metadata.len() <= MAX_RECORD as u64,
            "Windows synchronization record is not a bounded ordinary file",
        )?;
        use std::io::Read;
        let file = fs::File::open(entry.path())?;
        require(
            file.metadata()?.is_file(),
            "Windows synchronization record is not an ordinary opened file",
        )?;
        let mut bytes = Vec::new();
        file.take(MAX_RECORD as u64 + 1).read_to_end(&mut bytes)?;
        require(
            bytes.len() <= MAX_RECORD,
            "Windows synchronization record grew beyond its bound",
        )?;
        let raw = observation_document(&bytes)?;
        validate_record(&raw, architecture)?;
        record_images.insert(native_path(&raw["current_exe_utf16"])?);
        require(
            name == format!("existing-sync-{}.json", raw["case"].as_str().unwrap()),
            "Windows case filename and contents differ",
        )?;
        records.push(
            json!({"identity":ByteIdentity::of(&bytes),"document":String::from_utf8(bytes)?}),
        );
    }
    require(
        found == expected,
        "Native Windows synchronization evidence is incomplete",
    )?;
    let discovery = fs::read(out.join("native-test-discovery.log"))?;
    let executed = fs::read(out.join("native-tests.log"))?;
    let discovered = crate::qualify::test_names(std::str::from_utf8(&discovery)?, true);
    let passed = crate::qualify::test_names(std::str::from_utf8(&executed)?, false);
    let required: Vec<_> = REQUIRED_TESTS
        .iter()
        .map(|name| {
            let count = |map: &std::collections::BTreeMap<String, usize>| {
                map.iter()
                    .filter(|(n, _)| n.rsplit("::").next() == Some(*name))
                    .map(|(_, n)| *n)
                    .sum::<usize>()
            };
            json!({"name":name,"discovered":count(&discovered),"executed":count(&passed)})
        })
        .collect();
    let control = fs::read(out.join("native-tests.control.json"))?;
    let mut images = Vec::new();
    for image in cleanup["executable_identities"]
        .as_array()
        .ok_or("No retained executed test images")?
    {
        let path = std::path::Path::new(
            image["path"]
                .as_str()
                .ok_or("Invalid retained test image path")?,
        );
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if record_images.contains(&units) {
            images.push(json!({"path_utf16":units,"identity":image["identity"]}));
        }
    }
    let summary = json!({"schema":"eml-windows-existing-sync-campaign","schema_version":1,"source":plan.source_commit(),"source_epoch":plan.source_epoch(),"source_provenance":plan.source_provenance(),"target":target,"profile":if release{"release"}else{"test"},"host":host,"test_execution":{"discovery_identity":ByteIdentity::of(&discovery),"execution_identity":ByteIdentity::of(&executed),"control":{"identity":ByteIdentity::of(&control),"document":String::from_utf8(control)?},"required_tests":required},"images":images,"records":records});
    validate_observations(
        &summary,
        plan.source_commit(),
        u64::from(plan.source_epoch().seconds()),
        target,
        if release { "release" } else { "test" },
    )?;
    fs::write(
        out.join("windows-existing-sync-evidence.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(summary)
}
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::fixture as test_fixture;
