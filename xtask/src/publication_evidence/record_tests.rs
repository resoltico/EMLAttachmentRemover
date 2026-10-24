use super::*;
fn id(os: &str, n: u8) -> Value {
    if os == "windows" {
        json!({"volume":9,"file":[n,n,n,n,n,n,n,n,n,n,n,n,n,n,n,n]})
    } else {
        json!({"device":9,"inode":n})
    }
}
fn sig(os: &str, n: u8, len: usize) -> Value {
    json!({"identity":id(os,n),"size":len,"modified_native":[10,0],"changed_native":[11,0]})
}
fn entry(os: &str, path: &[&str], n: u8, len: Option<usize>) -> Value {
    json!({"path":path.iter().map(|s|s.as_bytes()).collect::<Vec<_>>(),"kind":if len.is_some(){"File"}else{"Directory"},"identity":id(os,n),"signature":len.map(|len|sig(os,n,len))})
}
fn inv(mut v: Vec<Value>) -> Value {
    v.sort_by_key(|v| serde_json::from_value::<Vec<Vec<u16>>>(v["path"].clone()).unwrap());
    json!(v)
}
fn sync_value(os: &str, file: bool) -> Value {
    json!({"method":if os=="windows"{"FlushFileBuffers"}else if os=="macos"&&file{"FsyncFullfsync"}else{"Fsync"},"state":"Succeeded","native_code":null})
}
fn cleanup_value(state: &str) -> Value {
    json!({"state":state,"residual_name":null,"native_code":null})
}
fn edge(os: &str, success: bool) -> Value {
    json!({"primitive":match os{"linux"=>"linkat","macos"=>"renameat_with_noreplace",_=>"SetFileInformationByHandle_FileRenameInfo"},"succeeded":success,"raw_return":if os=="windows"{Some(i32::from(success))}else{None},"native_code":if success{None}else{Some(if os=="windows"{183}else{17})}})
}
/// Independently authored literal native data, never evidence of native execution.
pub(super) fn fixture(name: &str, os: &str, arch: &str) -> Value {
    let mut v = json!({"schema":1,"case":name.strip_suffix(".json").unwrap(),"origin":"","process_os":os,"process_architecture":arch});
    let data = match name {
        "core-stop-before.json" | "core-stop-after.json" => {
            let after = name == "core-stop-after.json";
            let mut inventory = vec![entry(os, &["a.eml"], 1, Some(SOURCE.len()))];
            let before = inv(inventory.clone());
            if after {
                inventory.push(entry(
                    os,
                    &["a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml"],
                    2,
                    Some(GOLDEN.len()),
                ));
            }
            json!({"origin":"actual_core_with_controlled_stop","engine_phases":if after{vec!["Preflight","StageWrite","FileSync","Publication","AfterPublication","NamespaceSync"]}else{vec!["Preflight","StageWrite","FileSync","Publication"]},"source":{"expected":SOURCE,"before":SOURCE,"after":SOURCE,"signature_before":sig(os,1,SOURCE.len()),"signature_after":sig(os,1,SOURCE.len())},"candidate_expected":GOLDEN,"final":if after{Some(json!({"signature":sig(os,2,GOLDEN.len()),"bytes":GOLDEN}))}else{None},"receipt":{"classification":if after{"Created"}else{"Failed"},"error_code":if after{None}else{Some("Interrupted")},"error_phase":if after{None}else{Some("Publish")},"native_code":null,"effect":if after{"Published"}else{"NotPublished"},"candidate_identity":id(os,2),"address":if after{"Verified"}else{"NotChecked"},"file_sync":sync_value(os,true),"namespace":{"receipt":if after{Some(sync_value(os,false))}else{None},"required_edges":1,"completed_edges":u8::from(after),"failed_edge_index":null},"cleanup":cleanup_value(if after&&os=="windows"{"NotNeeded"}else{"Complete"})},"inventory_before":before,"inventory_after":inv(inventory)})
        }
        "native-racing-winner.json" => {
            let mut before = vec![entry(os, &["winner.eml"], 3, Some(16))];
            if os == "windows" {
                before.push(entry(os, &["stage"], 2, Some(15)));
            } else {
                before.push(entry(os, &["stage"], 6, None));
                before.push(entry(os, &["stage", "candidate"], 2, Some(15)));
            }
            let winner = json!({"signature":sig(os,3,16),"bytes":b"synthetic winner"});
            json!({"origin":"actual_native_publication","events":["stage_returned","winner_created","native_publication_returned","cleanup_returned"],"candidate_identity":id(os,2),"candidate_expected":b"synthetic loser","candidate_actual":b"synthetic loser","file_sync":sync_value(os,true),"native_edge":edge(os,false),"publication":"NotPublished","cleanup":cleanup_value("Complete"),"winner_before":winner,"winner_after":winner,"inventory_before":inv(before),"inventory_after":[entry(os,&["winner.eml"],3,Some(16))]})
        }
        "native-parent-binding.json" => {
            let win = os == "windows";
            let payload: &[u8] = if win {
                b"Subject: synthetic\r\n\r\ncomplete verified body\r\n"
            } else {
                b"held verified bytes"
            };
            let before = if win {
                inv(vec![entry(os, &["stage"], 2, Some(payload.len()))])
            } else {
                inv(vec![
                    entry(os, &["destination"], 4, None),
                    entry(os, &["destination", "stage"], 6, None),
                    entry(os, &["destination", "stage", "candidate"], 2, Some(19)),
                ])
            };
            let after = if win {
                inv(vec![entry(os, &["result.eml"], 2, Some(payload.len()))])
            } else {
                inv(vec![
                    entry(os, &["destination"], 7, None),
                    entry(os, &["moved"], 4, None),
                    entry(os, &["moved", "copy"], 2, Some(19)),
                    entry(os, &["moved", "held-stage"], 6, None),
                    entry(os, &["moved", "stage"], 8, None),
                    entry(os, &["moved", "stage", "candidate"], 5, Some(10)),
                ])
            };
            json!({"origin":"actual_native_parent_interference","events":if win{vec!["stage_returned","parent_rename_denied","native_publication_returned","cleanup_returned"]}else{vec!["stage_returned","parent_renamed","replacement_parent_created","stage_renamed","replacement_stage_created","substitute_written","native_publication_returned","cleanup_returned"]},"parent_before":id(os,4),"held_parent_after":id(os,4),"requested_parent_after":id(os,if win{4}else{7}),"actor_native_code":if win{Some(32)}else{None},"actor_changed_address":!win,"requested_address_verified":win,"candidate_identity":id(os,2),"candidate_expected":payload,"candidate_actual":payload,"native_edge":edge(os,true),"publication":"Published","cleanup":cleanup_value(if win{"NotNeeded"}else{"Unknown"}),"inventory_before":before,"inventory_after":after,"substitute_expected":if win{None}else{Some(b"fake bytes")},"substitute_after":if win{None}else{Some(b"fake bytes")},"file_sync":sync_value(os,true),"namespace_sync":null})
        }
        _ => {
            json!({"origin":"controlled_stage_failure_translation","cases":[{"checkpoint_stopped":false,"original_error_kind":"Interrupted","translated_code":"WriteFailed","translated_phase":"Write","file_sync":{"method":"Fsync","state":"Succeeded","native_code":null},"cleanup":{"state":"Residual","residual_name":b".owned-stage","native_code":5}},{"checkpoint_stopped":true,"original_error_kind":"Interrupted","translated_code":"Interrupted","translated_phase":"Write","file_sync":{"method":"Fsync","state":"Succeeded","native_code":null},"cleanup":{"state":"Residual","residual_name":b".owned-stage","native_code":5}}]})
        }
    };
    v.as_object_mut()
        .unwrap()
        .extend(data.as_object().unwrap().clone());
    v
}
#[test]
fn literal_actual_and_controlled_publication_records_cover_six_native_targets() {
    for os in ["linux", "macos", "windows"] {
        for arch in ["x86_64", "aarch64"] {
            for name in FILES {
                validate_record(name, &fixture(name, os, arch), os, arch)
                    .unwrap_or_else(|e| panic!("{os}/{arch}/{name}: {e}"));
            }
        }
    }
}
#[test]
fn publication_records_reject_missing_duplicate_or_contradictory_actual_facts() {
    for os in ["linux", "macos", "windows"] {
        let cases: [(&str, &str, Value); 18] = [
            (
                "core-stop-before.json",
                "/receipt/candidate_identity",
                Value::Null,
            ),
            (
                "core-stop-before.json",
                "/receipt/effect",
                json!("Published"),
            ),
            (
                "core-stop-before.json",
                "/receipt/namespace/completed_edges",
                json!(1),
            ),
            (
                "core-stop-before.json",
                "/receipt/cleanup/native_code",
                json!(5),
            ),
            ("core-stop-after.json", "/source/after/0", json!(0)),
            (
                "core-stop-after.json",
                "/source/signature_after/changed_native/0",
                json!(99),
            ),
            (
                "core-stop-after.json",
                "/receipt/candidate_identity",
                id(os, 1),
            ),
            (
                "core-stop-after.json",
                "/receipt/file_sync/state",
                json!("Failed"),
            ),
            (
                "core-stop-after.json",
                "/engine_phases/3",
                json!("NamespaceSync"),
            ),
            (
                "native-racing-winner.json",
                "/winner_after/bytes/0",
                json!(0),
            ),
            (
                "native-racing-winner.json",
                "/native_edge/succeeded",
                json!(true),
            ),
            ("native-racing-winner.json", "/candidate_actual/0", json!(0)),
            (
                "native-racing-winner.json",
                "/native_edge/native_code",
                json!(5),
            ),
            (
                "native-parent-binding.json",
                "/requested_address_verified",
                json!(os != "windows"),
            ),
            (
                "native-parent-binding.json",
                "/namespace_sync",
                sync_value(os, false),
            ),
            (
                "controlled-cleanup-cause.json",
                "/origin",
                json!("actual_native_publication"),
            ),
            (
                "controlled-cleanup-cause.json",
                "/cases/0/cleanup/native_code",
                Value::Null,
            ),
            (
                "controlled-cleanup-cause.json",
                "/cases/1/translated_code",
                json!("WriteFailed"),
            ),
        ];
        for (name, path, value) in cases {
            let mut v = fixture(name, os, "x86_64");
            *v.pointer_mut(path).unwrap() = value;
            assert!(
                validate_record(name, &v, os, "x86_64").is_err(),
                "accepted {os}/{name}{path}"
            );
        }
        for name in FILES {
            let mut v = fixture(name, os, "x86_64");
            v.as_object_mut().unwrap().remove("origin");
            assert!(validate_record(name, &v, os, "x86_64").is_err());
            let mut v = fixture(name, os, "x86_64");
            v["unknown"] = json!(true);
            assert!(validate_record(name, &v, os, "x86_64").is_err());
            assert!(validate_record(name, &fixture(name, os, "x86_64"), os, "aarch64").is_err());
        }
        let name = "native-racing-winner.json";
        let mut v = fixture(name, os, "x86_64");
        let duplicated = v["inventory_before"][0].clone();
        v["inventory_after"]
            .as_array_mut()
            .unwrap()
            .push(duplicated);
        assert!(validate_record(name, &v, os, "x86_64").is_err());
    }
}

#[test]
fn native_return_full_signature_and_ordinary_inventory_substitutions_refuse() {
    for os in ["linux", "macos", "windows"] {
        let name = "native-racing-winner.json";
        let mut value = fixture(name, os, "x86_64");
        value["native_edge"]["raw_return"] = if os == "windows" { json!(1) } else { json!(0) };
        assert!(validate_record(name, &value, os, "x86_64").is_err());
        let mut value = fixture(name, os, "x86_64");
        value["native_edge"]
            .as_object_mut()
            .unwrap()
            .remove("native_code");
        assert!(validate_record(name, &value, os, "x86_64").is_err());
        let name = "core-stop-after.json";
        let mut value = fixture(name, os, "x86_64");
        value["source"]["signature_before"]["modified_native"][1] = json!(-1);
        value["source"]["signature_after"]["modified_native"][1] = json!(-1);
        assert!(validate_record(name, &value, os, "x86_64").is_err());
        if os != "windows" {
            let name = "native-parent-binding.json";
            let mut value = fixture(name, os, "x86_64");
            let entries = value["inventory_after"].as_array_mut().unwrap();
            let held = entries
                .iter()
                .find(|e| path_eq(&e["path"], &["moved", "held-stage"]))
                .unwrap()["identity"]
                .clone();
            let replacement = entries
                .iter_mut()
                .find(|e| path_eq(&e["path"], &["moved", "stage"]))
                .unwrap();
            replacement["identity"] = held;
            assert!(validate_record(name, &value, os, "x86_64").is_err());
        }
    }
}

#[test]
fn coherent_claimed_and_actual_bytes_still_require_independent_oracles() {
    for os in ["linux", "macos", "windows"] {
        let mut forgeries = vec![
            (
                "core-stop-after.json",
                vec!["/source/expected/0", "/source/before/0", "/source/after/0"],
            ),
            (
                "core-stop-after.json",
                vec!["/candidate_expected/0", "/final/bytes/0"],
            ),
            (
                "native-racing-winner.json",
                vec!["/candidate_expected/0", "/candidate_actual/0"],
            ),
            (
                "native-racing-winner.json",
                vec!["/winner_before/bytes/0", "/winner_after/bytes/0"],
            ),
            (
                "native-parent-binding.json",
                vec!["/candidate_expected/0", "/candidate_actual/0"],
            ),
        ];
        if os != "windows" {
            forgeries.push((
                "native-parent-binding.json",
                vec!["/substitute_expected/0", "/substitute_after/0"],
            ));
        }
        for (name, paths) in forgeries {
            let mut value = fixture(name, os, "aarch64");
            for path in &paths {
                *value.pointer_mut(path).unwrap() = json!(0);
            }
            assert!(
                validate_record(name, &value, os, "aarch64").is_err(),
                "Accepted coherent wrong bytes: {os}/{name}/{paths:?}"
            );
        }
    }
}
