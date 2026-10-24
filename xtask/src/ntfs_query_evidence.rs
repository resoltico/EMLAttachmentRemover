//! Actual NTFS query facts and separately identified controlled lifetime checks.
use crate::{Result, picker_evidence::observation_document};
use serde::Deserialize;
use serde_json::Value;
#[cfg(any(windows, test))]
use serde_json::json;
use std::collections::BTreeSet;
#[cfg(any(windows, test))]
use std::{fs, io::Read, path::Path};
use xtask::archive::ByteIdentity;

const LIMIT: u64 = 256 * 1024;
const FILES: [&str; 3] = [
    "ntfs-query-native.json",
    "ntfs-query-adapters.json",
    "ntfs-query-pending-owners.json",
];
pub(crate) const REQUIRED_TESTS: [&str; 3] = [
    "windows::name_domain::tests::native_locality_query_retains_actual_completion_fields",
    "windows::name_domain::tests::native_failed_wait_retains_concrete_query_owners_until_event_and_thread_settle",
    "windows::name_domain::tests::only_exact_complete_local_disk_success_qualifies",
];

fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    volume: u64,
    file: [u8; 16],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Native {
    schema: u8,
    case: String,
    origin: String,
    process_os: String,
    process_architecture: String,
    directory_identity_before: Identity,
    directory_identity_after: Identity,
    query_identity: Identity,
    filesystem_units: Vec<u16>,
    return_status: i32,
    completion_status: i32,
    information: u64,
    device_type: u32,
    characteristics: u32,
    handle_information_return: i32,
    // deserialize_with makes an absent nullable field an error.
    #[serde(deserialize_with = "Option::<u32>::deserialize")]
    handle_information_error: Option<u32>,
    handle_information_flags: u32,
    validator_accepted: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Adapter {
    case: String,
    return_status: i32,
    completion_status: i32,
    information: u64,
    device_type: u32,
    characteristics: u32,
    validator_accepted: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Adapters {
    schema: u8,
    case: String,
    origin: String,
    process_os: String,
    process_architecture: String,
    cases: Vec<Adapter>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Initial {
    completion_status: i32,
    information: u64,
    device_type: u32,
    characteristics: u32,
    owner_drop_count: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wait {
    ordinal: u8,
    native_wait_return: u32,
    #[serde(deserialize_with = "Option::<u32>::deserialize")]
    native_wait_error: Option<u32>,
    completion_status: i32,
    information: u64,
    device_type: u32,
    characteristics: u32,
    handle_information_return: i32,
    #[serde(deserialize_with = "Option::<u32>::deserialize")]
    handle_information_error: Option<u32>,
    handle_information_flags: u32,
    owner_drop_count: u64,
    buffers_stable: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Final {
    completion_status: i32,
    information: u64,
    device_type: u32,
    characteristics: u32,
    pending_return_validator_accepted: bool,
    remote_validator_accepted: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    schema: u8,
    case: String,
    origin: String,
    process_os: String,
    process_architecture: String,
    initial: Initial,
    waits: [Wait; 2],
    completion_thread_joined: bool,
    r#final: Final,
    owner_drop_count_after: u64,
}

fn header(schema: u8, os: &str, architecture: &str, expected: &str) -> Result<()> {
    require(
        schema == 1
            && os == "windows"
            && architecture == expected
            && matches!(expected, "x86_64" | "aarch64"),
        "NTFS query evidence schema/actual CPU/OS differs",
    )
}

fn validate(name: &str, document: &Value, architecture: &str) -> Result<()> {
    require(document.is_object(), "NTFS query record is not an object")?;
    match name {
        "ntfs-query-native.json" => {
            for field in [
                "directory_identity_before",
                "directory_identity_after",
                "query_identity",
            ] {
                require(
                    document[field].is_object(),
                    "NTFS query identity is not an object",
                )?;
            }
            let r: Native = serde_json::from_value(document.clone())?;
            header(
                r.schema,
                &r.process_os,
                &r.process_architecture,
                architecture,
            )?;
            require(
                r.case == "native-local-ntfs-query"
                    && r.origin == "native_ntqueryvolumeinformationfile",
                "NTFS native query origin differs",
            )?;
            require(
                r.directory_identity_before.file.iter().any(|b| *b != 0)
                    && r.directory_identity_before == r.directory_identity_after
                    && r.directory_identity_before == r.query_identity,
                "NTFS query did not retain the full held directory identity",
            )?;
            require(
                r.filesystem_units == [78, 84, 70, 83]
                    && r.return_status == 0
                    && r.completion_status == 0
                    && r.information == 8
                    && r.device_type == 7
                    && r.characteristics & 16 == 0
                    && r.validator_accepted,
                "NTFS query did not observe an exact completed local disk result",
            )?;
            require(
                r.handle_information_return != 0 && r.handle_information_error.is_none(),
                "NTFS query handle was not observed live",
            )?;
            let _actual_handle_flags = r.handle_information_flags;
        }
        "ntfs-query-adapters.json" => {
            require(
                document["cases"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().all(Value::is_object)),
                "NTFS adapter cases are not objects",
            )?;
            let r: Adapters = serde_json::from_value(document.clone())?;
            header(
                r.schema,
                &r.process_os,
                &r.process_architecture,
                architecture,
            )?;
            require(
                r.case == "completed-status-adapters"
                    && r.origin == "controlled_completed_status_inputs",
                "Controlled NTFS status inputs are mislabeled",
            )?;
            // These literal obligations are independent of the native completed() implementation.
            let expected = [
                ("exact-local-success", 0, 0, 8, 7, 0, true),
                ("return-pending", 259, 0, 8, 7, 0, false),
                ("completion-pending", 0, 259, 8, 7, 0, false),
                ("return-positive", 1, 0, 8, 7, 0, false),
                ("completion-positive", 0, 1, 8, 7, 0, false),
                ("information-zero", 0, 0, 0, 7, 0, false),
                ("information-long", 0, 0, 9, 7, 0, false),
                ("information-overflow", 0, 0, u64::MAX, 7, 0, false),
                ("return-failed", -1, 0, 8, 7, 0, false),
                ("completion-failed", 0, -1, 8, 7, 0, false),
                ("information-short", 0, 0, 7, 7, 0, false),
                ("remote-characteristic", 0, 0, 8, 7, 16, false),
                ("unknown-device", 0, 0, 8, u32::MAX, 0, false),
            ];
            require(
                r.cases.len() == expected.len(),
                "Incomplete NTFS status adapter selection",
            )?;
            for (a, e) in r.cases.iter().zip(expected) {
                require(
                    (
                        a.case.as_str(),
                        a.return_status,
                        a.completion_status,
                        a.information,
                        a.device_type,
                        a.characteristics,
                        a.validator_accepted,
                    ) == e,
                    "NTFS status adapter input or actual refusal differs",
                )?;
            }
        }
        "ntfs-query-pending-owners.json" => {
            require(
                document["initial"].is_object()
                    && document["final"].is_object()
                    && document["waits"]
                        .as_array()
                        .is_some_and(|waits| waits.iter().all(Value::is_object)),
                "NTFS lifetime observations are not objects",
            )?;
            let r: Pending = serde_json::from_value(document.clone())?;
            header(
                r.schema,
                &r.process_os,
                &r.process_architecture,
                architecture,
            )?;
            require(
                r.case == "pending-owner-lifetime"
                    && r.origin == "controlled_completion_real_event",
                "Pending fixture does not distinguish controlled completion from disk I/O",
            )?;
            require(
                (
                    r.initial.completion_status,
                    r.initial.information,
                    r.initial.device_type,
                    r.initial.characteristics,
                    r.initial.owner_drop_count,
                ) == (259, 0, 0, 0, 0),
                "Pending owner initial state differs",
            )?;
            for (w, e) in r
                .waits
                .iter()
                .zip([(1, u32::MAX, Some(6), 259, 0, 0), (2, 0, None, 0, 8, 7)])
            {
                require(
                    (
                        w.ordinal,
                        w.native_wait_return,
                        w.native_wait_error,
                        w.completion_status,
                        w.information,
                        w.device_type,
                    ) == e
                        && w.characteristics == 0
                        && w.handle_information_return != 0
                        && w.handle_information_error.is_none()
                        && w.owner_drop_count == 0
                        && w.buffers_stable,
                    "Native wait, held buffers or owner lifetime differs",
                )?;
                let _actual_handle_flags = w.handle_information_flags;
            }
            let f = r.r#final;
            require(
                r.completion_thread_joined
                    && r.owner_drop_count_after == 1
                    && (
                        f.completion_status,
                        f.information,
                        f.device_type,
                        f.characteristics,
                    ) == (0, 8, 7, 0)
                    && !f.pending_return_validator_accepted
                    && !f.remote_validator_accepted,
                "Pending owners were released early or completion/refusals differ",
            )?;
        }
        _ => return Err("Unexpected NTFS query evidence case".into()),
    }
    Ok(())
}

#[cfg(any(windows, test))]
pub(crate) fn read(directory: &Path, architecture: &str) -> Result<Value> {
    let names = fs::read_dir(directory)?
        .map(|entry| -> Result<String> {
            let entry = entry?;
            require(
                entry.file_type()?.is_file(),
                "NTFS query record is not an ordinary file",
            )?;
            Ok(entry
                .file_name()
                .into_string()
                .map_err(|_| "NTFS query record name is not UTF8")?)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    require(
        names == FILES.into_iter().map(str::to_owned).collect(),
        "NTFS query evidence case set differs",
    )?;
    let mut records = Vec::new();
    for name in FILES {
        let path = directory.join(name);
        require(
            fs::symlink_metadata(&path)?.is_file() && fs::metadata(&path)?.len() <= LIMIT,
            "NTFS query record exceeds its bound or is not ordinary",
        )?;
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)?;
        require(
            bytes.len() as u64 <= LIMIT,
            "NTFS query record grew beyond its bound",
        )?;
        let document = observation_document(&bytes)?;
        validate(name, &document, architecture)?;
        records.push(json!({"name":name,"document":String::from_utf8(bytes.clone())?,"identity":ByteIdentity::of(&bytes)}));
    }
    Ok(json!({"schema":1,"architecture":architecture,"records":records}))
}

pub(crate) fn validate_summary(summary: &Value, architecture: &str) -> Result<()> {
    let object = summary
        .as_object()
        .ok_or("NTFS query summary is not an object")?;
    require(
        object.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == ["schema", "architecture", "records"].into_iter().collect()
            && summary["schema"] == 1
            && summary["architecture"] == architecture,
        "NTFS query summary fields/schema/CPU differ",
    )?;
    let records = summary["records"]
        .as_array()
        .ok_or("Missing NTFS query raw records")?;
    require(
        records.len() == FILES.len(),
        "Incomplete NTFS query raw records",
    )?;
    for (r, name) in records.iter().zip(FILES) {
        let object = r
            .as_object()
            .ok_or("NTFS query raw record is not an object")?;
        require(
            object.keys().map(String::as_str).collect::<BTreeSet<_>>()
                == ["name", "document", "identity"].into_iter().collect()
                && r["name"] == name,
            "NTFS query raw record name/fields/order differ",
        )?;
        let raw = r["document"]
            .as_str()
            .ok_or("Missing retained NTFS query document")?;
        require(
            raw.len() as u64 <= LIMIT,
            "Retained NTFS query document exceeds bound",
        )?;
        require(
            r["identity"].is_object(),
            "NTFS query byte identity is not an object",
        )?;
        let identity: ByteIdentity = serde_json::from_value(r["identity"].clone())?;
        require(
            identity == ByteIdentity::of(raw.as_bytes()),
            "Retained NTFS query bytes changed",
        )?;
        validate(name, &observation_document(raw.as_bytes())?, architecture)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_summary(architecture: &str) -> Value {
    let records: Vec<_> = FILES
        .into_iter()
        .zip(tests::literals())
        .map(|(name, mut document)| {
            document["process_architecture"] = json!(architecture);
            let raw = serde_json::to_string(&document).unwrap();
            json!({"name":name,"identity":ByteIdentity::of(raw.as_bytes()),"document":raw})
        })
        .collect();
    json!({"schema":1,"architecture":architecture,"records":records})
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn literals() -> Vec<Value> {
        let id = json!({"volume":97,"file":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]});
        let native = json!({"schema":1,"case":"native-local-ntfs-query","origin":"native_ntqueryvolumeinformationfile",
            "process_os":"windows","process_architecture":"aarch64",
            "directory_identity_before":id,"directory_identity_after":id,"query_identity":id,
            "filesystem_units":[78,84,70,83],"return_status":0,"completion_status":0,"information":8,
            "device_type":7,"characteristics":256,"handle_information_return":1,"handle_information_error":null,
            "handle_information_flags":0,"validator_accepted":true});
        // An independent literal transcript, not generated by the production validator.
        let rows: Value = serde_json::from_str(
            r#"[
            ["exact-local-success",0,0,8,7,0,true],
            ["return-pending",259,0,8,7,0,false],
            ["completion-pending",0,259,8,7,0,false],
            ["return-positive",1,0,8,7,0,false],
            ["completion-positive",0,1,8,7,0,false],
            ["information-zero",0,0,0,7,0,false],
            ["information-long",0,0,9,7,0,false],
            ["information-overflow",0,0,18446744073709551615,7,0,false],
            ["return-failed",-1,0,8,7,0,false],
            ["completion-failed",0,-1,8,7,0,false],
            ["information-short",0,0,7,7,0,false],
            ["remote-characteristic",0,0,8,7,16,false],
            ["unknown-device",0,0,8,4294967295,0,false]
        ]"#,
        )
        .unwrap();
        let cases: Vec<_> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                json!({
                    "case":r[0],"return_status":r[1],"completion_status":r[2],"information":r[3],
                    "device_type":r[4],"characteristics":r[5],"validator_accepted":r[6]
                })
            })
            .collect();
        let adapters = json!({"schema":1,"case":"completed-status-adapters","origin":"controlled_completed_status_inputs",
            "process_os":"windows","process_architecture":"aarch64","cases":cases});
        let pending = json!({"schema":1,"case":"pending-owner-lifetime","origin":"controlled_completion_real_event",
            "process_os":"windows","process_architecture":"aarch64",
            "initial":{"completion_status":259,"information":0,"device_type":0,"characteristics":0,"owner_drop_count":0},
            "waits":[
                {"ordinal":1,"native_wait_return":4294967295u64,"native_wait_error":6,"completion_status":259,"information":0,
                 "device_type":0,"characteristics":0,"handle_information_return":1,"handle_information_error":null,
                 "handle_information_flags":0,"owner_drop_count":0,"buffers_stable":true},
                {"ordinal":2,"native_wait_return":0,"native_wait_error":null,"completion_status":0,"information":8,
                 "device_type":7,"characteristics":0,"handle_information_return":1,"handle_information_error":null,
                 "handle_information_flags":0,"owner_drop_count":0,"buffers_stable":true}],
            "completion_thread_joined":true,"final":{"completion_status":0,"information":8,"device_type":7,
                "characteristics":0,"pending_return_validator_accepted":false,"remote_validator_accepted":false},
            "owner_drop_count_after":1});
        vec![native, adapters, pending]
    }

    #[test]
    fn literal_records_require_actual_query_and_distinct_controlled_origins() {
        let good = literals();
        for (name, document) in FILES.into_iter().zip(&good) {
            validate(name, document, "aarch64").unwrap();
            assert!(validate(name, document, "x86_64").is_err());
        }
        for (index, pointer, replacement) in [
            (0, "/query_identity/file/15", json!(99)),
            (0, "/return_status", json!(259)),
            (0, "/completion_status", json!(259)),
            (0, "/information", json!(9)),
            (0, "/characteristics", json!(16)),
            (0, "/handle_information_error", json!(6)),
            (0, "/validator_accepted", json!(false)),
            (1, "/cases/1/validator_accepted", json!(true)),
            (1, "/origin", json!("native_ntqueryvolumeinformationfile")),
            (2, "/origin", json!("native_ntqueryvolumeinformationfile")),
            (2, "/waits/0/owner_drop_count", json!(1)),
            (2, "/waits/0/native_wait_error", Value::Null),
            (2, "/waits/1/buffers_stable", json!(false)),
            (2, "/completion_thread_joined", json!(false)),
            (2, "/final/pending_return_validator_accepted", json!(true)),
            (2, "/owner_drop_count_after", json!(0)),
        ] {
            let mut bad = good[index].clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate(FILES[index], &bad, "aarch64").is_err(),
                "Accepted substitution {pointer}"
            );
        }
        let mut absent = good[0].clone();
        absent
            .as_object_mut()
            .unwrap()
            .remove("handle_information_error");
        assert!(validate(FILES[0], &absent, "aarch64").is_err());
        let mut extra = good[2].clone();
        extra["waits"][0]["extra"] = json!(true);
        assert!(validate(FILES[2], &extra, "aarch64").is_err());
        let mut short = good[1].clone();
        short["cases"].as_array_mut().unwrap().pop();
        assert!(validate(FILES[1], &short, "aarch64").is_err());
        let mut positional = good[0].clone();
        for name in [
            "directory_identity_before",
            "directory_identity_after",
            "query_identity",
        ] {
            positional[name] = json!([97, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]]);
        }
        assert!(validate(FILES[0], &positional, "aarch64").is_err());
        for (index, pointer, replacement) in [
            (
                1,
                "/cases/0",
                json!(["exact-local-success", 0, 0, 8, 7, 0, true]),
            ),
            (2, "/initial", json!([259, 0, 0, 0, 0])),
            (
                2,
                "/waits/0",
                json!([1, 4294967295u64, 6, 259, 0, 0, 0, 1, null, 0, 0, true]),
            ),
            (2, "/final", json!([0, 8, 7, 0, false, false])),
        ] {
            let mut bad = good[index].clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(validate(FILES[index], &bad, "aarch64").is_err());
        }
        let duplicate = serde_json::to_string(&good[0])
            .unwrap()
            .replacen('{', "{\"schema\":1,", 1);
        assert!(observation_document(duplicate.as_bytes()).is_err());
    }

    #[test]
    fn retained_query_records_refuse_missing_extra_and_changed_bytes() {
        let root = tempfile::tempdir().unwrap();
        assert!(read(root.path(), "aarch64").is_err());
        for (name, value) in FILES.into_iter().zip(literals()) {
            fs::write(root.path().join(name), serde_json::to_vec(&value).unwrap()).unwrap();
        }
        let good = read(root.path(), "aarch64").unwrap();
        validate_summary(&good, "aarch64").unwrap();
        let mut changed = good.clone();
        changed["records"][0]["document"] = json!("{}");
        assert!(validate_summary(&changed, "aarch64").is_err());
        let mut positional = good.clone();
        positional["records"][0]["identity"] = json!([
            good["records"][0]["identity"]["byte_length"],
            good["records"][0]["identity"]["sha256"]
        ]);
        assert!(validate_summary(&positional, "aarch64").is_err());
        let mut repeated = good.clone();
        repeated["records"][1] = repeated["records"][0].clone();
        assert!(validate_summary(&repeated, "aarch64").is_err());
        let mut foreign = good;
        foreign["records"][0]["document"] = json!("{\"schema\":1}");
        foreign["records"][0]["identity"] =
            serde_json::to_value(ByteIdentity::of(b"{\"schema\":1}")).unwrap();
        assert!(validate_summary(&foreign, "aarch64").is_err());
        fs::write(root.path().join("extra.json"), b"{}").unwrap();
        assert!(read(root.path(), "aarch64").is_err());
        fs::remove_file(root.path().join("extra.json")).unwrap();
        fs::write(root.path().join(FILES[0]), vec![b' '; LIMIT as usize + 1]).unwrap();
        assert!(read(root.path(), "aarch64").is_err());
    }
}
