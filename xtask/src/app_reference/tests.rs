//! Independent literal reference and native-observation controls; no native execution.
use super::*;
use serde_json::json;

pub(crate) fn test_fixture(shipping: &Value, archive: &ByteIdentity) -> (Value, Value) {
    use serde_json::json;
    let mut shipping = shipping.clone();
    shipping["minimum"] = json!(983040);
    let version = shipping["version"].as_str().unwrap();
    let target = shipping["target"].as_str().unwrap();
    let names = [
        "eml-worker",
        "eml-app",
        "eml-core",
        "eml-fs",
        "eml-macos",
        "eml-protocol",
    ];
    let entries:Vec<_>=names.iter().enumerate().map(|(i,name)|json!({"depth":i,"name":name,"version":version,"features":[],"proc_macro":false,"repeated":false})).collect();
    let before = json!({"scope":"actual selected component normal/build feature tree; host paths omitted without merging distinct occurrences","entries":entries});
    let mut after = before.clone();
    for row in after["entries"].as_array_mut().unwrap() {
        row["features"] = json!(["native-fixtures"]);
    }
    let args = json!([
        "tree",
        "--locked",
        "--package",
        "eml-worker",
        "--target",
        target,
        "--edges",
        "normal,build",
        "--prefix",
        "depth",
        "--format",
        "{p}|{f}"
    ]);
    let packages: Vec<_> = names
        .iter()
        .map(|name| json!({"name":name,"version":version,"source":null,"checksum":null}))
        .collect();
    let material =
        json!({"version":version,"generation":{"feature_arguments":args},"packages":packages});
    let mut fixture_material = material.clone();
    fixture_material["generation"]["feature_arguments"]
        .as_array_mut()
        .unwrap()
        .extend([json!("--features"), json!("eml-worker/native-fixtures")]);
    let document = |v: &Value| {
        let text = serde_json::to_string(v).unwrap();
        json!({"identity":ByteIdentity::of(text.as_bytes()),"contents":text})
    };
    let legal = json!({"shipping_features":document(&before),"fixture_features":document(&after),"shipping_materials":document(&material),"fixture_materials":document(&fixture_material)});
    let id = |s: &str| ByteIdentity::of(s.as_bytes());
    for (name, value, mode) in [
        (MAIN, json!(id("shipping GUI")), 493),
        (WORKER, json!(id("shipping worker")), 493),
        (
            "Contents/_CodeSignature/CodeResources",
            json!(id("shipping seal")),
            420,
        ),
        (
            FEATURES,
            legal["shipping_features"]["identity"].clone(),
            420,
        ),
        (
            MATERIALS,
            legal["shipping_materials"]["identity"].clone(),
            420,
        ),
    ] {
        shipping["files"][name] = json!([value, mode]);
    }
    let mut app = shipping.clone();
    app["root"] = json!(ROOT);
    for (name, value) in [
        (MAIN, json!(id("controlled GUI signature"))),
        (WORKER, json!(id("controlled worker"))),
        (
            "Contents/_CodeSignature/CodeResources",
            json!(id("controlled seal")),
        ),
        (FEATURES, legal["fixture_features"]["identity"].clone()),
        (MATERIALS, legal["fixture_materials"]["identity"].clone()),
    ] {
        app["files"][name][0] = value;
    }
    let process = json!({"exit_code":0,"signal":null,"timed_out":false,"cancelled":false,"unexpected_survivors":0,"cleanup_complete":true,"elapsed_seconds":0.1,"cleanup_seconds":0.0});
    let signature = json!({"identifier":"io.github.resoltico.emlattachmentremover","flags":2,"entitlements":null,"details_process":process,"entitlements_process":process});
    let raw_shipping = serde_json::to_vec(&shipping).unwrap();
    let reference = json!({"schema":1,"shipping_archive":archive,"shipping_reference":ByteIdentity::of(&raw_shipping),"app":app,
 "worker":{"compiled_image":id("compiled fixture worker"),"features":["native-fixtures"],"profile":{"opt_level":"3","debuginfo":0,"debug_assertions":false,"overflow_checks":true,"test":false},"toolchain":{"rust_identity":"rustc 1.99.1","xcode_identity":"Xcode 26.3","sdk_version":"26.2","target":shipping["target"],"architecture":shipping["architecture"],"minimum":983040},"process":process,"shipping_signature":signature,"fixture_signature":signature},
 "gui":{"shipping_signed":shipping["files"][MAIN][0],"fixture_signed":app["files"][MAIN][0],"shipping_stripped":id("same complete GUI bytes"),"fixture_stripped":id("same complete GUI bytes"),"shipping_signature":signature,"fixture_signature":signature,"shipping_strip_process":process,"fixture_strip_process":process},"legal":legal,"changed_paths":[WORKER,MAIN,FEATURES,MATERIALS,"Contents/_CodeSignature/CodeResources"]});
    (
        shipping,
        json!({"archive":id("frozen controlled companion archive"),"reference":document(&reference)}),
    )
}

fn literal_pair() -> (Value, Value, ByteIdentity) {
    let archive = ByteIdentity::of(b"independent shipping archive");
    let normal = json!({
        "schema":2,"workflow":null,"source":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "source_provenance":"git_commit","source_epoch":1792824989u64,"version":"7.0.0",
        "target":"x86_64-apple-darwin","architecture":"x86_64","minimum":983040,
        "root":"EML Attachment Remover.app",
        "files":{"Contents/Resources/LICENSE":[ByteIdentity::of(b"independent unchanged license"),420]}
    });
    let (normal, envelope) = test_fixture(&normal, &archive);
    (normal, envelope, archive)
}
fn reference_value(envelope: &Value) -> Value {
    serde_json::from_str(envelope["reference"]["contents"].as_str().unwrap()).unwrap()
}
fn change_reference(envelope: &mut Value, change: impl FnOnce(&mut Value)) {
    let mut reference = reference_value(envelope);
    change(&mut reference);
    let bytes = serde_json::to_vec(&reference).unwrap();
    envelope["reference"]["contents"] = String::from_utf8(bytes.clone()).unwrap().into();
    envelope["reference"]["identity"] = json!(ByteIdentity::of(&bytes));
}
fn admit(
    normal: &Value,
    envelope: &Value,
    archive: &ByteIdentity,
) -> Result<OperationFixtureReference> {
    let envelope: OperationFixtureEnvelope = serde_json::from_value(envelope.clone())?;
    envelope.validate(archive, &serde_json::to_vec(normal)?)
}
fn document(value: &Value) -> Value {
    let bytes = serde_json::to_vec(value).unwrap();
    json!({"identity":ByteIdentity::of(&bytes),"contents":String::from_utf8(bytes).unwrap()})
}
fn replace_legal(normal: &mut Value, envelope: &mut Value, change: impl FnOnce(&mut Value)) {
    change_reference(envelope, |reference| {
        change(&mut reference["legal"]);
        normal["files"][FEATURES][0] = reference["legal"]["shipping_features"]["identity"].clone();
        normal["files"][MATERIALS][0] =
            reference["legal"]["shipping_materials"]["identity"].clone();
        reference["app"]["files"][FEATURES][0] =
            reference["legal"]["fixture_features"]["identity"].clone();
        reference["app"]["files"][MATERIALS][0] =
            reference["legal"]["fixture_materials"]["identity"].clone();
        reference["shipping_reference"] =
            json!(ByteIdentity::of(&serde_json::to_vec(normal).unwrap()));
    });
}

#[test]
fn literal_reference_shapes_require_objects_and_every_nullable_observation() {
    let (normal, envelope, archive) = literal_pair();
    assert!(admit(&normal, &envelope, &archive).is_ok());
    for path in [
        "",
        "/app",
        "/app/workflow",
        "/app/files",
        "/worker",
        "/worker/profile",
        "/worker/toolchain",
        "/worker/compiled_image",
        "/worker/process",
        "/gui",
        "/gui/shipping_signature",
        "/gui/fixture_signature/entitlements",
        "/gui/shipping_strip_process",
        "/legal",
        "/legal/fixture_features",
        "/legal/fixture_features/identity",
        "/shipping_archive",
        "/shipping_reference",
    ] {
        let mut bad = envelope.clone();
        change_reference(&mut bad, |reference| {
            *reference.pointer_mut(path).unwrap() = json!([]);
        });
        assert!(admit(&normal, &bad, &archive).is_err(), "array {path}");
    }
    let mut bad = envelope.clone();
    change_reference(&mut bad, |reference| {
        let id = &mut reference["app"]["files"][MAIN][0];
        *id = json!([id["byte_length"], id["sha256"]]);
    });
    assert!(admit(&normal, &bad, &archive).is_err());
    for (path, key) in [
        ("/app", "workflow"),
        ("/gui/shipping_signature", "entitlements"),
        ("/gui/shipping_strip_process", "signal"),
        ("/worker", "compiled_image"),
        ("/legal", "fixture_materials"),
    ] {
        let mut bad = envelope.clone();
        change_reference(&mut bad, |reference| {
            reference
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
        });
        assert!(
            admit(&normal, &bad, &archive).is_err(),
            "missing {path}/{key}"
        );
    }
    let mut duplicate = envelope;
    let text = duplicate["reference"]["contents"]
        .as_str()
        .unwrap()
        .replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1);
    duplicate["reference"]["identity"] = json!(ByteIdentity::of(text.as_bytes()));
    duplicate["reference"]["contents"] = text.into();
    assert!(admit(&normal, &duplicate, &archive).is_err());
}

#[test]
fn required_inventory_paths_refuse_matching_omissions_without_panicking() {
    let (normal, envelope, archive) = literal_pair();
    for path in [
        MAIN,
        WORKER,
        FEATURES,
        MATERIALS,
        "Contents/_CodeSignature/CodeResources",
    ] {
        let mut normal = normal.clone();
        let mut bad = envelope.clone();
        normal["files"].as_object_mut().unwrap().remove(path);
        change_reference(&mut bad, |reference| {
            reference["app"]["files"]
                .as_object_mut()
                .unwrap()
                .remove(path);
            reference["shipping_reference"] =
                json!(ByteIdentity::of(&serde_json::to_vec(&normal).unwrap()));
        });
        assert!(admit(&normal, &bad, &archive).is_err(), "missing {path}");
    }
    let mut malformed = normal.clone();
    let id = &mut malformed["files"][MAIN][0];
    *id = json!([id["byte_length"], id["sha256"]]);
    assert!(parse_reference(&serde_json::to_vec(&malformed).unwrap()).is_err());
}

#[test]
fn source_images_security_processes_and_closed_changes_have_literal_refusals() {
    let (normal, envelope, archive) = literal_pair();
    for (path, replacement) in [
        ("/schema", json!(2)),
        ("/app/source", json!("b".repeat(40))),
        ("/app/source_epoch", json!(1792824990u64)),
        ("/app/target", json!("aarch64-apple-darwin")),
        ("/app/architecture", json!("aarch64")),
        ("/app/minimum", json!(983041)),
        ("/app/root", json!("Other.app")),
        ("/app/version", json!("7.0.1")),
        ("/worker/features", json!([])),
        ("/worker/profile/opt_level", json!("2")),
        ("/worker/profile/debug_assertions", json!(true)),
        ("/worker/profile/overflow_checks", json!(false)),
        ("/worker/profile/test", json!(true)),
        ("/gui/fixture_stripped/sha256", json!("b".repeat(64))),
        (
            "/gui/fixture_signature/identifier",
            json!("different.identifier"),
        ),
        ("/gui/fixture_signature/flags", json!(65538)),
        (
            "/gui/fixture_signature/entitlements",
            json!({"com.apple.security.get-task-allow":true}),
        ),
        (
            "/gui/shipping_signature/details_process/exit_code",
            json!(1),
        ),
        (
            "/gui/fixture_signature/entitlements_process/cleanup_complete",
            json!(false),
        ),
        ("/gui/shipping_strip_process/timed_out", json!(true)),
        ("/worker/process/unexpected_survivors", json!(1)),
        ("/worker/fixture_signature/flags", json!(0)),
    ] {
        let mut bad = envelope.clone();
        change_reference(&mut bad, |reference| {
            *reference.pointer_mut(path).unwrap() = replacement
        });
        assert!(admit(&normal, &bad, &archive).is_err(), "{path}");
    }
    for path in [MAIN, "Contents/Resources/LICENSE"] {
        let mut bad = envelope.clone();
        change_reference(&mut bad, |reference| {
            reference["app"]["files"][path][1] = json!(384);
        });
        assert!(admit(&normal, &bad, &archive).is_err());
    }
    let mut bad = envelope.clone();
    change_reference(&mut bad, |reference| {
        reference["app"]["files"]["Contents/Resources/LICENSE"][0] =
            json!(ByteIdentity::of(b"changed license"));
    });
    assert!(admit(&normal, &bad, &archive).is_err());
    let mut bad = envelope.clone();
    change_reference(&mut bad, |reference| {
        reference["changed_paths"] = json!([MAIN, WORKER])
    });
    assert!(admit(&normal, &bad, &archive).is_err());
    let mut wrong_archive = archive.clone();
    wrong_archive.sha256 = "c".repeat(64);
    assert!(admit(&normal, &envelope, &wrong_archive).is_err());
}

#[test]
fn legal_features_and_generation_provenance_cannot_drift_after_rehashing() {
    let (normal, envelope, archive) = literal_pair();
    for target in [
        "fixture_features",
        "fixture_materials",
        "shipping_materials",
    ] {
        let mut n = normal.clone();
        let mut bad = envelope.clone();
        replace_legal(&mut n, &mut bad, |legal| {
            let mut value: Value =
                serde_json::from_str(legal[target]["contents"].as_str().unwrap()).unwrap();
            match target {
                "fixture_features" => {
                    value["entries"][0]["features"] = json!(["native-fixtures", "extra"])
                }
                "fixture_materials" => {
                    value["generation"]["feature_arguments"] = json!(["wrong command"])
                }
                _ => value["packages"][0]["source"] = json!("registry+counterfeit-workspace"),
            }
            legal[target] = document(&value);
        });
        assert!(admit(&n, &bad, &archive).is_err(), "{target}");
    }
    let mut n = normal.clone();
    let mut good = envelope.clone();
    replace_legal(&mut n, &mut good, |legal| {
        for key in ["shipping_features", "fixture_features"] {
            let mut value: Value =
                serde_json::from_str(legal[key]["contents"].as_str().unwrap()).unwrap();
            value["entries"].as_array_mut().unwrap().push(json!({
                "depth":1,"name":"serde","version":"1.0.228","features":["std"],
                "proc_macro":false,"repeated":false
            }));
            legal[key] = document(&value);
        }
        for key in ["shipping_materials", "fixture_materials"] {
            let mut value: Value =
                serde_json::from_str(legal[key]["contents"].as_str().unwrap()).unwrap();
            value["packages"].as_array_mut().unwrap().push(json!({
                "name":"serde","version":"1.0.228",
                "source":"registry+https://github.com/rust-lang/crates.io-index",
                "checksum":"c".repeat(64)
            }));
            legal[key] = document(&value);
        }
    });
    assert!(admit(&n, &good, &archive).is_ok());
    let mut unknown_package = good.clone();
    let mut n_unknown = n.clone();
    replace_legal(&mut n_unknown, &mut unknown_package, |legal| {
        for key in ["shipping_features", "fixture_features"] {
            let mut value: Value =
                serde_json::from_str(legal[key]["contents"].as_str().unwrap()).unwrap();
            value["entries"].as_array_mut().unwrap().push(json!({
                "depth":1,"name":"unknown-package","version":"1.0.0","features":[],
                "proc_macro":false,"repeated":false
            }));
            legal[key] = document(&value);
        }
    });
    assert!(admit(&n_unknown, &unknown_package, &archive).is_err());
    let mut bad = good;
    replace_legal(&mut n, &mut bad, |legal| {
        let mut value: Value =
            serde_json::from_str(legal["fixture_features"]["contents"].as_str().unwrap()).unwrap();
        value["entries"].as_array_mut().unwrap().last_mut().unwrap()["features"] =
            json!(["std", "alloc"]);
        legal["fixture_features"] = document(&value);
    });
    assert!(admit(&n, &bad, &archive).is_err());
}

#[test]
fn archive_image_profile_and_observed_toolchain_boundaries_are_distinct() {
    let (normal, envelope, archive) = literal_pair();
    let mut at_limit = envelope.clone();
    at_limit["archive"]["byte_length"] = json!(512 * 1024 * 1024usize);
    assert!(admit(&normal, &at_limit, &archive).is_ok());
    at_limit["archive"]["byte_length"] = json!(512 * 1024 * 1024usize + 1);
    assert!(admit(&normal, &at_limit, &archive).is_err());
    let mut image = envelope.clone();
    change_reference(&mut image, |reference| {
        reference["worker"]["compiled_image"]["byte_length"] = json!(256 * 1024 * 1024usize)
    });
    assert!(admit(&normal, &image, &archive).is_ok());
    change_reference(&mut image, |reference| {
        reference["worker"]["compiled_image"]["byte_length"] = json!(256 * 1024 * 1024usize + 1)
    });
    assert!(admit(&normal, &image, &archive).is_err());
    for wrong in [
        Value::Null,
        json!({}),
        json!([]),
        json!(0.0),
        json!(1),
        json!(2),
    ] {
        let mut bad = envelope.clone();
        change_reference(&mut bad, |reference| {
            reference["worker"]["profile"]["debuginfo"] = wrong
        });
        assert!(admit(&normal, &bad, &archive).is_err());
    }
    let reference = admit(&normal, &envelope, &archive).unwrap();
    let toolchain = reference.worker.toolchain;
    assert!(
        toolchain
            .validate_observed("rustc 1.99.1", "Xcode 26.3", 0x001a0200)
            .is_ok()
    );
    assert!(
        toolchain
            .validate_observed("rustc wrong", "Xcode 26.3", 0x001a0200)
            .is_err()
    );
    assert!(
        toolchain
            .validate_observed("rustc 1.99.1", "Xcode wrong", 0x001a0200)
            .is_err()
    );
    assert!(
        toolchain
            .validate_observed("rustc 1.99.1", "Xcode 26.3", 0x001a0300)
            .is_err()
    );
    assert!(packed_version("26.2.0").is_ok_and(|version| version == 0x001a0200));
    for bad in [
        "26", "26.2.0.0", "65536.0", "26.256", "26.2.256", "-1.0", "wrong",
    ] {
        assert!(packed_version(bad).is_err(), "{bad}");
    }
}
