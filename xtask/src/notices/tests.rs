use super::*;

#[test]
fn generated_source_identity_rejects_stale_and_dirty_receipts() {
    let source = json!({"head":"owned-head","working_tree":""});
    source_identity(&source, "owned-head", "").unwrap();
    assert!(source_identity(&source, "different-head", "").is_err());
    assert!(source_identity(&source, "owned-head", " M source").is_err());
    for invalid in [
        json!({"head":"owned-head","working_tree":" M source"}),
        json!({"head":"owned-head"}),
        json!({"working_tree":""}),
    ] {
        assert!(source_identity(&invalid, "owned-head", "").is_err());
    }
}

#[test]
fn current_policy_requires_every_notice_pair_and_rejects_extra_or_mismatched_components() {
    let plan = json!({"version":"6.0.0","native_targets":[{"platform":"macos-aarch64","target":"aarch64-apple-darwin"},{"platform":"linux-x86_64","target":"x86_64-unknown-linux-gnu"}]});
    let mut all = Files::new();
    for (prefix, platform, target, component) in [
        (
            "macos-aarch64/",
            "macos-aarch64",
            "aarch64-apple-darwin",
            "cli",
        ),
        (
            "gui/macos-aarch64/",
            "macos-aarch64",
            "aarch64-apple-darwin",
            "gui",
        ),
        (
            "worker/macos-aarch64/",
            "macos-aarch64",
            "aarch64-apple-darwin",
            "worker",
        ),
        (
            "linux-x86_64/",
            "linux-x86_64",
            "x86_64-unknown-linux-gnu",
            "cli",
        ),
    ] {
        all.insert(format!("{prefix}licenses/materials.json"), serde_json::to_vec(&json!({"platform":platform,"target":target,"component":component,"version":"6.0.0"})).unwrap());
    }
    add_directories(&mut all);
    assert_eq!(checked_pairs(&all, &plan).unwrap(), 4);
    let path = "gui/macos-aarch64/licenses/materials.json";
    let mut missing = all.clone();
    missing.remove(path);
    assert!(checked_pairs(&missing, &plan).is_err());
    let mut extra = all.clone();
    extra.insert(
        "gui/linux-x86_64/licenses/materials.json".into(),
        b"{}".to_vec(),
    );
    assert!(checked_pairs(&extra, &plan).is_err());
    for key in ["platform", "target", "component", "version"] {
        let mut invalid = all.clone();
        let mut identity: Value = serde_json::from_slice(&invalid[path]).unwrap();
        identity[key] = json!("wrong");
        invalid.insert(path.into(), serde_json::to_vec(&identity).unwrap());
        assert!(checked_pairs(&invalid, &plan).is_err(), "{key}");
    }
}

#[test]
fn existing_generated_tree_check_rejects_changed_missing_and_extra_files() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("materials");
    let expected = Files::from([("license".into(), b"owned text".to_vec())]);
    write_new_tree(&output, &expected).unwrap();
    matches_tree(&output, &expected).unwrap();
    fs::write(output.join("license"), b"alter text").unwrap();
    assert!(matches_tree(&output, &expected).is_err());
    fs::remove_file(output.join("license")).unwrap();
    assert!(matches_tree(&output, &expected).is_err());
    fs::write(output.join("license"), b"owned text").unwrap();
    fs::write(output.join("extra"), b"extra").unwrap();
    assert!(matches_tree(&output, &expected).is_err());
}

#[test]
fn standard_template_retains_declarations_and_refuses_changed_source_material() {
    let root = tempfile::tempdir().unwrap();
    let declaration = "Owned synthetic license declaration and attribution\n";
    fs::write(root.path().join("Cargo.toml"), declaration).unwrap();
    let standard = "Identified synthetic standard text, not an upstream attribution\n";
    let package = json!({"name":"owned-declaration","version":"1.0.0","source":null,"manifest_path":root.path().join("Cargo.toml")});
    let report: About =
        serde_json::from_value(json!({"crates":[{"package":package,"license":"MIT"}],
            "licenses":[{"id":"MIT","text":standard,"used_by":[{"crate":package}]}]}))
        .unwrap();
    let review: Review = serde_json::from_value(json!({"generator_pin":{},"source_inputs":[],"targets":[{"target":"owned-target"}],
            "packages":[{"name":"owned-declaration","version":"1.0.0","source":null,"checksum":null,"license_expression":"MIT","scopes":["owned-target"],
                "provenance":{"kind":"workspace","manifest_path":"Cargo.toml"},"bindings":[{"license_id":"MIT","generator_text_sha256":digest(standard.as_bytes()),"generator_text_bytes":standard.len(),
                "mapping":"identified_standard_text_with_source_declarations","scopes":["owned-target"],
                "standard_text":{"license_id":"MIT","sha256":digest(standard.as_bytes()),"bytes":standard.len()},
                "materials":[{"kind":"package_file","path":"Cargo.toml","bytes":declaration.len(),"sha256":digest(declaration.as_bytes())}]}]}]})).unwrap();
    let lock: Lock = serde_json::from_value(json!({"package":[{"name":"owned-declaration","version":"1.0.0","source":null,"checksum":null}]})).unwrap();
    let (files, inventory) = render(root.path(), "owned-target", &report, &review, &lock).unwrap();
    assert_eq!(
        files[inventory[0]["file"].as_str().unwrap()],
        standard.as_bytes()
    );
    let additional = inventory[0]["additional_material_files"]
        .as_array()
        .unwrap();
    assert_eq!(additional.len(), 1);
    assert_eq!(
        files[additional[0].as_str().unwrap()],
        declaration.as_bytes()
    );
    fs::write(root.path().join("Cargo.toml"), b"changed declaration").unwrap();
    assert!(render(root.path(), "owned-target", &report, &review, &lock).is_err());
}

#[test]
fn omitted_reference_requires_explicit_material_and_preserves_generator_omission() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Cargo.toml"), b"owned manifest").unwrap();
    let text = "Synthetic owned custom license material\n";
    fs::write(root.path().join("LICENSE"), text).unwrap();
    let package = json!({"name":"owned-reference","version":"1.0.0","source":null,"manifest_path":root.path().join("Cargo.toml")});
    let mut report: About = serde_json::from_value(
        json!({"crates":[{"package":package,"license":"LicenseRef-Owned-Fixture"}],"licenses":[]}),
    )
    .unwrap();
    let mut review: Review = serde_json::from_value(json!({"generator_pin":{},"source_inputs":[],"targets":[{"target":"owned-target"}],
            "packages":[{"name":"owned-reference","version":"1.0.0","source":null,"checksum":null,"license_expression":"LicenseRef-Owned-Fixture",
                "scopes":["owned-target"],"provenance":{"kind":"workspace","manifest_path":"Cargo.toml"},"bindings":[]}]})).unwrap();
    let lock: Lock = serde_json::from_value(json!({"package":[{"name":"owned-reference","version":"1.0.0","source":null,"checksum":null}]})).unwrap();
    assert!(
        render(root.path(), "owned-target", &report, &review, &lock)
            .unwrap_err()
            .to_string()
            .contains("no legal material binding")
    );
    review.packages[0].bindings.push(serde_json::from_value(json!({"license_id":"LicenseRef-Owned-Fixture","generator_text_sha256":"","generator_text_bytes":0,
            "mapping":"explicit_package_license_reference_supplement","scopes":["owned-target"],
            "materials":[{"kind":"package_file","path":"LICENSE","bytes":text.len(),"sha256":digest(text.as_bytes())}]})).unwrap());
    let (files, inventory) = render(root.path(), "owned-target", &report, &review, &lock).unwrap();
    assert_eq!(
        files[inventory[0]["file"].as_str().unwrap()],
        text.as_bytes()
    );
    assert!(inventory[0]["generator_text_sha256"].is_null());
    assert_eq!(
        inventory[0]["generator_observation"],
        "selected package present; license text group omitted"
    );
    fs::write(root.path().join("LICENSE"), b"changed material").unwrap();
    assert!(render(root.path(), "owned-target", &report, &review, &lock).is_err());
    fs::write(root.path().join("LICENSE"), text).unwrap();
    report.licenses.push(
        serde_json::from_value(
            json!({"id":"LicenseRef-Owned-Fixture","text":text,"used_by":[{"crate":package}]}),
        )
        .unwrap(),
    );
    assert!(
        render(root.path(), "owned-target", &report, &review, &lock).is_err(),
        "Changed generator behavior requires review, not a duplicate supplement"
    );
}

#[test]
fn activated_features_preserve_occurrences_without_host_paths() {
    let packages = BTreeSet::from([
        ("tool".into(), "1.0.0".into()),
        ("derive".into(), "2.0.0".into()),
    ]);
    let text =
        "0tool v1.0.0 (/owned/source with spaces)|std\n1derive v2.0.0 (proc-macro)|default (*)\n";
    let record = feature_tree(text, &packages).unwrap();
    assert!(!record.to_string().contains("/owned"));
    assert_eq!(record["entries"][1]["proc_macro"], true);
    assert_eq!(record["entries"][1]["repeated"], true);
    for invalid in [
        "",
        "0tool v1.0.0|std\n",
        "0tool v1.0.0|std\n2derive v2.0.0|\n",
        "0tool v1.0.0|std\n1foreign v2.0.0|\n",
    ] {
        assert!(feature_tree(invalid, &packages).is_err());
    }
}

#[test]
fn grouped_representative_cannot_cover_another_packages_missing_file() {
    let root = tempfile::tempdir().unwrap();
    let text = "Synthetic owned license fixture\n";
    let mut packages = Vec::new();
    let mut reviewed = Vec::new();
    let mut locked = Vec::new();
    for name in ["first", "second"] {
        fs::create_dir(root.path().join(name)).unwrap();
        fs::write(
            root.path().join(name).join("Cargo.toml"),
            "owned manifest fixture",
        )
        .unwrap();
        fs::write(root.path().join(name).join("LICENSE"), text).unwrap();
        let package = json!({"name":name,"version":"1.0.0","source":null,"manifest_path":root.path().join(name).join("Cargo.toml")});
        packages.push(package);
        locked.push(json!({"name":name,"version":"1.0.0","source":null,"checksum":null}));
        reviewed.push(json!({"name":name,"version":"1.0.0","source":null,"checksum":null,
                "license_expression":"MIT","scopes":["owned-target"],
                "provenance":{"kind":"workspace","manifest_path":format!("{name}/Cargo.toml")},
                "bindings":[{"license_id":"MIT","generator_text_sha256":digest(text.as_bytes()),"generator_text_bytes":text.len(),
                    "mapping":"exact_per_package_legal_file","scopes":["owned-target"],
                    "materials":[{"kind":"package_file","path":"LICENSE","bytes":text.len(),"sha256":digest(text.as_bytes())}]}]}));
    }
    let mut report: About = serde_json::from_value(json!({"crates":packages.iter().map(|package| json!({"package":package,"license":"MIT"})).collect::<Vec<_>>(),
            "licenses":[{"id":"MIT","text":text,"source_path":root.path().join("first/LICENSE"),
                "used_by":packages.iter().map(|package| json!({"crate":package})).collect::<Vec<_>>() }]})).unwrap();
    let review: Review = serde_json::from_value(json!({"generator_pin":{},"packages":reviewed,"source_inputs":[],"targets":[{"target":"owned-target"}]})).unwrap();
    let lock: Lock = serde_json::from_value(json!({"package":locked})).unwrap();
    assert_eq!(
        render(root.path(), "owned-target", &report, &review, &lock)
            .unwrap()
            .1
            .len(),
        2
    );
    fs::remove_file(root.path().join("second/LICENSE")).unwrap();
    assert!(render(root.path(), "owned-target", &report, &review, &lock).is_err());
    assert_eq!(
        fs::read_to_string(root.path().join("first/LICENSE")).unwrap(),
        text
    );
    fs::write(root.path().join("second/LICENSE"), text).unwrap();
    assert!(render(root.path(), "owned-target", &report, &review, &lock).is_ok());
    assert!(render(root.path(), "another-target", &report, &review, &lock).is_err());
    report.licenses[0].text.push_str("changed selected text");
    assert!(render(root.path(), "owned-target", &report, &review, &lock).is_err());
}

#[test]
fn actual_copyright_bearing_microsoft_text_replaces_generic_fallback() {
    let owned = tempfile::tempdir().unwrap();
    let directory = owned
        .path()
        .join("registry/src/owned-index/owned-binding-1.0.0");
    let cache = owned.path().join("registry/cache/owned-index");
    fs::create_dir_all(&directory).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let legal = include_bytes!("../../../tests/contract/notices/windows-bindings-license-mit.txt");
    fs::write(directory.join("license-mit"), legal).unwrap();
    fs::write(directory.join("Cargo.toml"), b"owned manifest fixture").unwrap();
    let archive = b"owned synthetic archive identity; not a real registry package";
    fs::write(cache.join("owned-binding-1.0.0.crate"), archive).unwrap();
    let fallback = "MIT template with <year> and <copyright holders>";
    let package = json!({"name":"owned-binding","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index","manifest_path":directory.join("Cargo.toml")});
    let report: About = serde_json::from_value(json!({"crates":[{"package":package,"license":"MIT OR Apache-2.0"}],
            "licenses":[{"id":"MIT","text":fallback,"source_path":null,"used_by":[{"crate":package}]}]})).unwrap();
    let review: Review = serde_json::from_value(json!({"generator_pin":{},"source_inputs":[],"targets":[{"target":"owned-target"}],
            "packages":[{"name":"owned-binding","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index","checksum":digest(archive),
                "license_expression":"MIT OR Apache-2.0","scopes":["owned-target"],"provenance":{"kind":"registry"},
                "bindings":[{"license_id":"MIT","generator_text_sha256":digest(fallback.as_bytes()),"generator_text_bytes":fallback.len(),
                    "mapping":"complete_source_license_materials","scopes":["owned-target"],
                    "materials":[{"kind":"package_file","path":"license-mit","bytes":legal.len(),"sha256":"c2cfccb812fe482101a8f04597dfc5a9991a6b2748266c47ac91b6a5aae15383"}]}]}]})).unwrap();
    let lock: Lock = serde_json::from_value(json!({"package":[{"name":"owned-binding","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index","checksum":digest(archive)}]})).unwrap();
    let (files, inventory) = render(owned.path(), "owned-target", &report, &review, &lock).unwrap();
    let selected = inventory[0]["file"].as_str().unwrap();
    assert_eq!(files[selected], legal);
    assert!(
        String::from_utf8(files[selected].clone())
            .unwrap()
            .contains("Copyright (c) Microsoft Corporation.")
    );
    assert!(!files.values().any(|bytes| bytes == fallback.as_bytes()));
}

#[test]
fn empty_extra_directories_and_deep_or_oversized_trees_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("license"), b"owned").unwrap();
    let expected = enumerate(root.path()).unwrap();
    fs::create_dir(root.path().join("unrelated-empty")).unwrap();
    assert!(matches_tree(root.path(), &expected).is_err());
    fs::remove_dir(root.path().join("unrelated-empty")).unwrap();
    let mut deep = root.path().to_owned();
    for _ in 0..17 {
        deep.push("a");
        fs::create_dir(&deep).unwrap();
    }
    assert!(enumerate(root.path()).is_err());

    let large = tempfile::tempdir().unwrap();
    for index in 0..4 {
        fs::File::create(large.path().join(index.to_string()))
            .unwrap()
            .set_len(16 * 1024 * 1024)
            .unwrap();
    }
    assert_eq!(
        enumerate(large.path())
            .unwrap()
            .values()
            .map(Vec::len)
            .sum::<usize>(),
        64 * 1024 * 1024
    );
    fs::write(large.path().join("one-more-byte"), [1]).unwrap();
    assert!(enumerate(large.path()).is_err());
}

#[test]
fn all_inventory_nodes_count_toward_the_4096_entry_bound() {
    let owned = tempfile::tempdir().unwrap();
    for index in 0..4096 {
        fs::File::create(owned.path().join(format!("n-{index:04}"))).unwrap();
    }
    assert_eq!(enumerate(owned.path()).unwrap().len(), 4096);
    fs::create_dir(owned.path().join("one-more-directory")).unwrap();
    assert!(enumerate(owned.path()).is_err());
}

#[test]
#[cfg(unix)]
fn native_backslash_names_are_refused_without_collapsing_aliases() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("a")).unwrap();
    fs::write(root.path().join("a/b"), b"nested").unwrap();
    fs::write(root.path().join("a\\b"), b"literal backslash").unwrap();
    assert!(enumerate(root.path()).is_err());
    assert_eq!(fs::read(root.path().join("a/b")).unwrap(), b"nested");
    assert_eq!(
        fs::read(root.path().join("a\\b")).unwrap(),
        b"literal backslash"
    );
}

#[test]
#[cfg(unix)]
fn generated_tree_symlinks_are_rejected_without_following_them() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("owned-target"), b"unchanged").unwrap();
    std::os::unix::fs::symlink(root.path().join("owned-target"), root.path().join("link")).unwrap();
    assert!(enumerate(root.path()).is_err());
    assert_eq!(
        fs::read(root.path().join("owned-target")).unwrap(),
        b"unchanged"
    );
}

#[test]
fn generation_refuses_an_existing_output_without_changing_it() {
    let owned = tempfile::tempdir().unwrap();
    let output = owned.path().join("generated");
    let files = BTreeMap::from([("license".into(), b"retained legal text".to_vec())]);
    write_new_tree(&output, &files).unwrap();
    fs::write(output.join("unrelated"), b"keep this").unwrap();
    assert!(write_new_tree(&output, &files).is_err());
    assert_eq!(fs::read(output.join("unrelated")).unwrap(), b"keep this");
    assert_eq!(
        fs::read(output.join("license")).unwrap(),
        b"retained legal text"
    );
}
