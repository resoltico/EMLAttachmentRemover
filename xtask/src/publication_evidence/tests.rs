//! Synthetic source/process envelopes retain independent literal record fixtures.
use super::*;
const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(crate) fn fixture(
    source: &str,
    epoch: u64,
    target: &str,
    os: &str,
    arch: &str,
    profile: &str,
) -> Value {
    let records: Vec<_> = record::test_records(os, arch)
        .into_iter()
        .map(|(name, value)| {
            let text = serde_json::to_string(&value).unwrap();
            json!({"name":name,"identity":ByteIdentity::of(text.as_bytes()),"document":text})
        })
        .collect();
    let names = if os == "windows" {
        [
            "run::tests::controlled_stop_before_and_after_the_real_edge_preserves_actual_effect",
            "run::tests::stage_checkpoint_origin_is_distinct_from_a_native_interrupted_error",
            "windows::publication::tests::native_rename_conflict_and_preedge_name_refusals_only_clean_owned_staging",
            "windows::publication::tests::native_no_replace_edge_keeps_complete_same_object_and_release_retains_final_file",
        ]
    } else {
        [
            "run::tests::controlled_stop_before_and_after_the_real_edge_preserves_actual_effect",
            "run::tests::stage_checkpoint_origin_is_distinct_from_a_native_interrupted_error",
            "candidate::tests::actual_storage_private_modes_sync_and_no_replace_preserve_winner",
            "candidate::tests::descriptor_publication_survives_destination_and_stage_name_substitution",
        ]
    };
    let kernel = if os == "macos" && arch == "aarch64" {
        "arm64"
    } else {
        arch
    };
    json!({"schema":"eml-native-publication-evidence","schema_version":1,"source":source,"source_epoch":epoch,"source_provenance":"git_commit","target":target,"profile":profile,"host":{"target":target,"process_os":os,"process_architecture":arch,"compiler":format!("rustc fixture\nhost: {target}\n"),"actual_os":"Independent fixture OS observation","kernel_machine":kernel,"native_machine":if arch=="x86_64"{0x8664}else{0xaa64},"emulated_process_machine":0,"actual_kernel":{"major":10,"minor":0,"build":20348}},"test_execution":{"process":{"exit_code":0,"signal":null,"timed_out":false,"cancelled":false,"unexpected_survivors":0,"cleanup_complete":true,"elapsed_seconds":0.1,"cleanup_seconds":0.0},"discovery_identity":ByteIdentity::of(b"actual discovery"),"execution_identity":ByteIdentity::of(b"actual execution"),"required_tests":names.iter().map(|name|json!({"name":name,"discovered":1,"executed":1})).collect::<Vec<_>>()},"images":[{"path":"observed-owned-test-image","identity":ByteIdentity::of(b"actual observed test image")}],"records":records})
}
fn valid(v: &Value) -> bool {
    validate_summary(
        v,
        SOURCE,
        123,
        "x86_64-unknown-linux-gnu",
        "linux",
        "x86_64",
        "release",
    )
    .is_ok()
}
#[test]
fn native_publication_envelope_requires_current_source_process_images_and_exact_selection() {
    let good = fixture(
        SOURCE,
        123,
        "x86_64-unknown-linux-gnu",
        "linux",
        "x86_64",
        "release",
    );
    assert!(valid(&good));
    for substitution in 0..9 {
        let mut bad = good.clone();
        match substitution {
            0 => {
                bad["source"] = json!("b".repeat(40));
            }
            1 => {
                bad["source_epoch"] = json!(124);
            }
            2 => {
                bad["test_execution"]["process"]["cleanup_complete"] = json!(false);
            }
            3 => {
                bad["test_execution"]["required_tests"][0]["executed"] = json!(0);
            }
            4 => {
                bad["images"] = json!([]);
            }
            5 => {
                bad["host"]["process_architecture"] = json!("aarch64");
            }
            6 => {
                bad["records"].as_array_mut().unwrap().pop();
            }
            7 => {
                bad["records"].as_array_mut().unwrap().swap(0, 1);
            }
            _ => {
                bad["images"][0]["identity"] = json!([25, "a".repeat(64)]);
            }
        }
        assert!(!valid(&bad), "substitution {substitution}");
    }
    let mut local = good.clone();
    local["target"] = Value::Null;
    local["source_provenance"] = json!("declared_archive");
    assert!(validate_context(&local, SOURCE, 123, None, "linux", "x86_64", "release").is_ok());
    assert!(!valid(&local));
}
#[test]
fn changed_raw_edge_or_controlled_origin_cannot_be_hidden_by_rehashing_a_summary() {
    let good = fixture(
        SOURCE,
        123,
        "x86_64-unknown-linux-gnu",
        "linux",
        "x86_64",
        "release",
    );
    let mut bad = good.clone();
    bad["records"][0]["document"] = json!("{}");
    assert!(!valid(&bad));
    let mut bad = good.clone();
    let mut raw: Value =
        serde_json::from_str(bad["records"][2]["document"].as_str().unwrap()).unwrap();
    raw["native_edge"]["succeeded"] = json!(true);
    let text = serde_json::to_string(&raw).unwrap();
    bad["records"][2] = json!({"name":"native-racing-winner.json","identity":ByteIdentity::of(text.as_bytes()),"document":text});
    assert!(!valid(&bad));
    let mut bad = good.clone();
    let text = bad["records"][4]["document"].as_str().unwrap().replacen(
        "controlled_stage_failure_translation",
        "actual_native_publication",
        1,
    );
    bad["records"][4] = json!({"name":"controlled-cleanup-cause.json","identity":ByteIdentity::of(text.as_bytes()),"document":text});
    assert!(!valid(&bad));
}
