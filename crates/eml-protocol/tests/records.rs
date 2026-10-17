//! Independent consumer fixtures, map representation and required-null controls.
use eml_protocol::records::{EventRecord, decode_record};
fn created() -> Vec<serde_json::Value> {
    include_str!("../../../tests/contract/protocol/created.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
#[test]
fn hand_authored_records_decode_without_the_producer() {
    for record in created() {
        decode_record(&serde_json::to_vec(&record).unwrap()).unwrap();
    }
}
#[test]
fn every_known_field_is_required_across_all_scenarios_including_present_nullable_values() {
    fn visit(value: &serde_json::Value, path: &str, root: &serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, child) in object {
                    let mut missing = root.clone();
                    missing
                        .pointer_mut(path)
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .remove(key);
                    assert!(
                        decode_record(&serde_json::to_vec(&missing).unwrap()).is_err(),
                        "Missing required key accepted: {path}/{key}"
                    );
                    let escaped = key.replace('~', "~0").replace('/', "~1");
                    visit(child, &format!("{path}/{escaped}"), root);
                }
            }
            serde_json::Value::Array(array) => {
                for (index, child) in array.iter().enumerate() {
                    visit(child, &format!("{path}/{index}"), root);
                }
            }
            _ => {}
        }
    }
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/contract/protocol");
    let mut scenarios = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            scenarios += 1;
            for line in std::fs::read_to_string(path).unwrap().lines() {
                decode_record(line.as_bytes()).unwrap();
                let record = serde_json::from_str(line).unwrap();
                visit(&record, "", &record);
            }
        }
    }
    assert_eq!(
        scenarios, 22,
        "Every shared hand-authored scenario must be discovered"
    );
}
#[test]
fn arrays_aliases_tagged_strings_and_duplicate_known_fields_reject() {
    let item = created().remove(1);
    let mut changed = item.clone();
    changed["source"] = serde_json::json!(["a.eml",{"encoding":"unix-bytes","base64":"YS5lbWw="}]);
    assert!(decode_record(&serde_json::to_vec(&changed).unwrap()).is_err());
    changed = item.clone();
    changed["status"] = serde_json::json!({"created":null});
    assert!(decode_record(&serde_json::to_vec(&changed).unwrap()).is_err());
    changed = item.clone();
    let source = changed
        .as_object_mut()
        .unwrap()
        .remove("source_bytes")
        .unwrap();
    changed["sourceBytes"] = source;
    assert!(decode_record(&serde_json::to_vec(&changed).unwrap()).is_err());
    let text = serde_json::to_string(&item).unwrap();
    let duplicate = text.replacen("\"index\":0", "\"index\":1,\"index\":0", 1);
    assert_ne!(text, duplicate);
    assert!(decode_record(duplicate.as_bytes()).is_err());
    assert!(decode_record(b"[]").is_err());
    assert!(decode_record(b"{\"type\":\"start\",\"type\":\"item\"}").is_err());
}
#[test]
fn additive_fields_are_ignored_and_do_not_enter_technical_record_exports() {
    let mut item = created().remove(1);
    item["future_extra"] = serde_json::json!({"arbitrary":"not retained"});
    let decoded = decode_record(&serde_json::to_vec(&item).unwrap()).unwrap();
    assert!(matches!(decoded, EventRecord::Item(_)));
    assert!(
        !serde_json::to_string(&decoded)
            .unwrap()
            .contains("not retained")
    );
}
