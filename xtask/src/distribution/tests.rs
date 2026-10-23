use super::*;
const TABLE: &str = include_str!("../../distribution.toml");
const TOOLS: &str = include_str!("../../tools.toml");
fn parse(input: &str) -> Result<BuildPlan> {
    BuildPlan::parse(input, TOOLS, "7.0.0".into(), PackageSource::fixture())
}
fn approved(plan: &BuildPlan) -> Result<()> {
    // Independent support oracle, deliberately not used to generate production policy.
    let required = [
        ("macos-aarch64", "aarch64-apple-darwin", "macos-26"),
        ("macos-x86_64", "x86_64-apple-darwin", "macos-26-intel"),
        ("linux-x86_64", "x86_64-unknown-linux-gnu", "ubuntu-22.04"),
        (
            "linux-aarch64",
            "aarch64-unknown-linux-gnu",
            "ubuntu-22.04-arm",
        ),
        (
            "windows-x86_64",
            "x86_64-pc-windows-gnullvm",
            "windows-2022",
        ),
        (
            "windows-aarch64",
            "aarch64-pc-windows-gnullvm",
            "windows-11-arm",
        ),
    ];
    let actual: BTreeSet<_> = plan
        .targets()
        .map(|row| {
            (
                row.platform().to_string(),
                row.rust_target().to_owned(),
                row.build_runner().to_owned(),
            )
        })
        .collect();
    if actual
        != required
            .iter()
            .map(|(platform, target, runner)| {
                (platform.to_string(), target.to_string(), runner.to_string())
            })
            .collect()
    {
        return Err("Approved native scope changed".into());
    }
    let mut artifacts: BTreeSet<_> = required
        .iter()
        .map(|(platform, _, _)| (platform.to_string(), ProductRole::Cli))
        .collect();
    for (platform, floor, picker) in [
        ("macos-aarch64", "macos-15", None),
        ("macos-x86_64", "macos-15-intel", Some("macos-26-intel")),
    ] {
        artifacts.insert((platform.to_owned(), ProductRole::DesktopApp));
        let desktop = plan
            .lookup_platform(platform)?
            .desktop()
            .ok_or("Missing complete desktop floor role")?;
        if desktop.floor_runner() != floor
            || desktop.floor_major() != 15
            || desktop.picker().map(PickerCheck::runner) != picker
            || desktop
                .picker()
                .is_some_and(|picker| picker.os_major() != 26)
        {
            return Err("Approved desktop consumer role changed".into());
        }
    }
    if plan
        .expected_artifacts()
        .map(|row| (row.key().platform().to_string(), row.key().role()))
        .collect::<BTreeSet<_>>()
        != artifacts
    {
        return Err("Approved shipping product roles changed".into());
    }
    Ok(())
}
#[test]
fn approved_shipping_scope_and_required_consumers_have_an_independent_deletion_control() {
    approved(&parse(TABLE).unwrap()).unwrap();
    let table: toml::Value = toml::from_str(TABLE).unwrap();
    for case in ["target", "desktop", "floor", "picker"] {
        let mut value = table.clone();
        let targets = value["targets"].as_array_mut().unwrap();
        match case {
            "target" => {
                targets.remove(2);
            }
            "desktop" => {
                targets[0]["products"] =
                    toml::Value::Array(vec![toml::Value::String("cli".into())]);
                targets[0].as_table_mut().unwrap().remove("desktop");
            }
            "floor" => targets[0]["desktop"]["floor_major"] = toml::Value::Integer(16),
            "picker" => {
                targets[1]["desktop"]
                    .as_table_mut()
                    .unwrap()
                    .remove("picker");
            }
            _ => unreachable!(),
        }
        let plan = parse(&toml::to_string(&value).unwrap()).unwrap();
        assert!(approved(&plan).is_err(), "{case}");
    }
}
#[test]
fn typed_lookup_rejects_external_unknown_platform_role_target_and_artifact() {
    let plan = parse(TABLE).unwrap();
    assert!(plan.lookup_target("x86_64-unknown-linux-musl").is_err());
    assert!(plan.lookup_platform("macos-universal").is_err());
    assert!(plan.named_artifact("../archive.zip").is_err());
    assert!(
        plan.lookup_artifact(
            PlatformId::parse("linux-aarch64").unwrap(),
            ProductRole::DesktopApp
        )
        .is_err()
    );
    for input in [
        TABLE.replace(
            "platform = \"macos-aarch64\"",
            "platform = \"unknown-aarch64\"",
        ),
        TABLE.replace("products = [\"cli\"]", "products = [\"cli\", \"cli\"]"),
        TABLE.replace("aarch64-apple-darwin", "x86_64-apple-darwin"),
        TABLE.replace("runner = \"macos-26\"", "runner = \"../unsafe\""),
    ] {
        assert!(parse(&input).is_err());
    }
}
#[test]
fn one_renderer_exposes_matrix_roles_tools_and_exact_versioned_names() {
    let plan = BuildPlan::parse(
        TABLE,
        TOOLS,
        "7.2.1-rc.2+build.5".into(),
        PackageSource::fixture(),
    )
    .unwrap();
    let rendered = plan.render();
    assert_eq!(rendered["schema"], "eml-build-plan");
    assert_eq!(rendered["schema_version"], 1);
    assert!(
        plan.expected_artifacts()
            .all(|row| row.basename().contains("7.2.1-rc.2+build.5"))
    );
    assert_eq!(
        rendered["native_matrix"]["include"]
            .as_array()
            .unwrap()
            .len(),
        plan.targets().count()
    );
    assert!(
        rendered["picker_matrix"]["include"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["os_major"] == 26)
    );
    let context = BuildContext {
        root: PathBuf::from("unused"),
        plan,
    };
    let file = tempfile::NamedTempFile::new().unwrap();
    run(&context, Some(file.path())).unwrap();
    let output = std::fs::read_to_string(file.path()).unwrap();
    let mut keys = BTreeSet::new();
    for line in output.lines() {
        let (key, value) = line.split_once('=').unwrap();
        assert!(keys.insert(key));
        assert!(!value.contains(['\n', '\r']));
    }
    assert!(
        keys.contains("native")
            && keys.contains("floors")
            && keys.contains("picker")
            && keys.contains("cargo_about")
    );
}
#[test]
fn malformed_tool_floor_version_and_source_values_cannot_form_a_plan() {
    for version in ["", "../other", "1/2", "1\\2", "1\n2", "1\0", "1 2"] {
        assert!(BuildPlan::parse(TABLE, TOOLS, version.into(), PackageSource::fixture()).is_err());
    }
    assert!(
        BuildPlan::parse(
            TABLE,
            &TOOLS.replace("version = \"26.3\"", "version = \"26\""),
            "7.0.0".into(),
            PackageSource::fixture()
        )
        .is_err()
    );
    for source in ["", "short", &"g".repeat(40), &"A".repeat(40)] {
        assert!(SourceCommit::parse(source.into()).is_err());
    }
    assert!(parse(&TABLE.replace("15.0", "15..0")).is_err());
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
}
fn metadata(version: &str) -> serde_json::Value {
    let root = source_root();
    serde_json::json!({
        "workspace_root": root,
        "workspace_members": ["owned-workspace-cli"],
        "packages": [{"id":"owned-workspace-cli", "name":"emlattachmentremover",
            "version":version, "source":null,
            "manifest_path":root.join("crates/eml-cli/Cargo.toml")}]
    })
}
fn version(value: &serde_json::Value) -> Result<String> {
    workspace_version(&source_root(), &serde_json::to_vec(value).unwrap())
}

#[test]
fn metadata_must_identify_the_actual_local_workspace_cli() {
    let valid = metadata("6.0.0");
    assert_eq!(version(&valid).unwrap(), "6.0.0");
    for mutation in [
        "workspace",
        "membership",
        "member_id",
        "source",
        "path",
        "name",
        "empty",
        "duplicate",
        "missing_version",
        "missing_source",
    ] {
        let mut value = valid.clone();
        match mutation {
            "workspace" => value["workspace_root"] = serde_json::json!(source_root().join("xtask")),
            "membership" => value["workspace_members"] = serde_json::json!([]),
            "member_id" => value["packages"][0]["id"] = serde_json::json!("different-id"),
            "source" => {
                value["packages"][0]["source"] =
                    serde_json::json!("registry+https://example.invalid")
            }
            "path" => {
                value["packages"][0]["manifest_path"] =
                    serde_json::json!(source_root().join("xtask/Cargo.toml"))
            }
            "name" => value["packages"][0]["name"] = serde_json::json!("other-package"),
            "empty" => value["packages"] = serde_json::json!([]),
            "duplicate" => {
                value["packages"]
                    .as_array_mut()
                    .unwrap()
                    .push(valid["packages"][0].clone());
            }
            "missing_version" => {
                value["packages"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("version");
            }
            "missing_source" => {
                value["packages"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("source");
            }
            _ => unreachable!(),
        }
        assert!(version(&value).is_err(), "{mutation}");
    }
    for bytes in [&b"{}"[..], b"null", b"[]", b"invalid"] {
        assert!(workspace_version(&source_root(), bytes).is_err());
    }
    let positional = serde_json::json!([
        valid["packages"],
        valid["workspace_members"],
        valid["workspace_root"]
    ]);
    assert!(version(&positional).is_err());
}

#[test]
fn colliding_tool_output_cannot_replace_a_matrix_or_partially_write_workflow_fields() {
    let tools = TOOLS.replace("[versions]", "[versions]\nnative='1.0.0'");
    let plan = BuildPlan::parse(TABLE, &tools, "7.0.0".into(), PackageSource::fixture()).unwrap();
    let context = BuildContext {
        root: PathBuf::from("unused"),
        plan,
    };
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"prior=unchanged\n").unwrap();
    assert!(run(&context, Some(file.path())).is_err());
    assert_eq!(std::fs::read(file.path()).unwrap(), b"prior=unchanged\n");
}

#[test]
fn canonical_downloads_have_exact_names_roots_executables_and_platform_formats() {
    let plan = parse(TABLE).unwrap();
    let expected = [
        (
            "emlattachmentremover-7.0.0-macos-aarch64.tar.gz",
            "emlattachmentremover-7.0.0-macos-aarch64",
            "emlattachmentremover",
            ArchiveFormat::TarGz,
        ),
        (
            "emlattachmentremover-7.0.0-macos-x86_64.tar.gz",
            "emlattachmentremover-7.0.0-macos-x86_64",
            "emlattachmentremover",
            ArchiveFormat::TarGz,
        ),
        (
            "emlattachmentremover-7.0.0-linux-aarch64.tar.gz",
            "emlattachmentremover-7.0.0-linux-aarch64",
            "emlattachmentremover",
            ArchiveFormat::TarGz,
        ),
        (
            "emlattachmentremover-7.0.0-linux-x86_64.tar.gz",
            "emlattachmentremover-7.0.0-linux-x86_64",
            "emlattachmentremover",
            ArchiveFormat::TarGz,
        ),
        (
            "emlattachmentremover-7.0.0-windows-aarch64.zip",
            "emlattachmentremover-7.0.0-windows-aarch64",
            "emlattachmentremover.exe",
            ArchiveFormat::Zip,
        ),
        (
            "emlattachmentremover-7.0.0-windows-x86_64.zip",
            "emlattachmentremover-7.0.0-windows-x86_64",
            "emlattachmentremover.exe",
            ArchiveFormat::Zip,
        ),
        (
            "emlattachmentremover-gui-7.0.0-macos-aarch64.zip",
            "EML Attachment Remover.app",
            "Contents/MacOS/EMLAttachmentRemover",
            ArchiveFormat::Zip,
        ),
        (
            "emlattachmentremover-gui-7.0.0-macos-x86_64.zip",
            "EML Attachment Remover.app",
            "Contents/MacOS/EMLAttachmentRemover",
            ArchiveFormat::Zip,
        ),
    ];
    assert_eq!(plan.expected_artifacts().count(), expected.len());
    for (name, root, executable, format) in expected {
        let actual = plan.named_artifact(name).unwrap();
        assert_eq!(
            (actual.root(), actual.executable(), actual.format()),
            (root, executable, format)
        );
    }
    for invalid in [
        "remove-eml-attachments-7.0.0-macos-aarch64.zip",
        "emlattachmentremover-7.0.0-macos-aarch64.zip",
        "emlattachmentremover-7.0.0-windows-x86_64.tar.gz",
        "emlattachmentremover-gui-7.0.0-linux-x86_64.tar.gz",
        "EMLAttachmentRemover-7.0.0-macos-x86_64.zip",
    ] {
        assert!(plan.named_artifact(invalid).is_err(), "{invalid}");
    }
    assert_eq!(
        plan.render()["source"],
        json!({"commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "epoch":1609459201, "provenance":"git_commit"})
    );
}
