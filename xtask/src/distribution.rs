//! Expected native distribution identities, never fabricated qualification.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

const CLI_PACKAGE: &str = "remove-eml-attachments";
const MAX_PLAN_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Distribution {
    policy: Policy,
    targets: Vec<Target>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    macos_minimum: String,
    linux_minimum_ubuntu: String,
    linux_glibc_maximum: String,
    windows_rust_minimum: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    platform: String,
    target: String,
    runner: String,
    app_smoke_runner: Option<String>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Macos,
    Linux,
    Windows,
}
impl Family {
    fn name(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
        }
    }
    fn target_suffix(self) -> &'static str {
        match self {
            Self::Macos => "apple-darwin",
            Self::Linux => "unknown-linux-gnu",
            Self::Windows => "pc-windows-gnullvm",
        }
    }
}
#[derive(Serialize)]
struct NativeTarget {
    platform: String,
    target: String,
    runner: String,
    app_smoke_runner: Option<String>,
    os: &'static str,
    architecture: String,
    app: bool,
    platform_text: String,
}
#[derive(Serialize)]
struct ExpectedArtifact {
    name: String,
    platform: String,
    role: &'static str,
    root: String,
    executable: &'static str,
}
#[derive(Serialize)]
struct Plan {
    version: String,
    policy: Policy,
    native_targets: Vec<NativeTarget>,
    expected_artifacts: Vec<ExpectedArtifact>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Xcode {
    version: String,
    build: String,
}
impl Xcode {
    fn parse(input: &str) -> Result<Self> {
        let document: toml::Value = toml::from_str(input)?;
        let selected: Self = document
            .get("xcode")
            .ok_or("Missing Xcode build input")?
            .clone()
            .try_into()?;
        let pieces: Vec<_> = selected.version.split('.').collect();
        if !(2..=3).contains(&pieces.len())
            || selected.version.len() > 16
            || pieces
                .iter()
                .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
            || selected.build.is_empty()
            || selected.build.len() > 32
            || !selected
                .build
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err("Invalid Xcode version/build input".into());
        }
        Ok(selected)
    }
}
#[derive(Serialize)]
struct BuildPlan {
    #[serde(flatten)]
    distribution: Plan,
    xcode: Xcode,
    tools: BTreeMap<String, String>,
}

fn tool_versions(input: &str) -> Result<BTreeMap<String, String>> {
    let document: toml::Value = toml::from_str(input)?;
    let tools: BTreeMap<String, String> = document
        .get("versions")
        .ok_or("Missing developer tool versions")?
        .clone()
        .try_into()?;
    if tools.is_empty()
        || tools.iter().any(|(name, version)| {
            name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
                || version.is_empty()
                || version.len() > 64
                || !version
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        })
    {
        return Err("Invalid developer tool identity".into());
    }
    Ok(tools)
}

impl Target {
    fn coordinates(&self) -> Result<(Family, &str)> {
        let (family, architecture) = self
            .platform
            .split_once('-')
            .ok_or("Invalid platform identifier")?;
        let family = match family {
            "macos" => Family::Macos,
            "linux" => Family::Linux,
            "windows" => Family::Windows,
            _ => return Err("Unsupported platform family".into()),
        };
        if !matches!(architecture, "x86_64" | "aarch64") {
            return Err("Unsupported native architecture".into());
        }
        if self.target != format!("{architecture}-{}", family.target_suffix()) {
            return Err("Rust target does not match its native platform".into());
        }
        let valid_runner = |label: &str| {
            !label.is_empty()
                && label.len() <= 64
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
                && label.as_bytes()[0].is_ascii_alphanumeric()
        };
        if !valid_runner(&self.runner)
            || self
                .app_smoke_runner
                .as_deref()
                .is_some_and(|label| !valid_runner(label))
            || (family == Family::Macos) != self.app_smoke_runner.is_some()
        {
            return Err("Invalid native build or macOS app smoke runner".into());
        }
        Ok((family, architecture))
    }
}
impl Distribution {
    fn parse(input: &str) -> Result<Self> {
        let table: Self = toml::from_str(input)?;
        if table.targets.len() != 6 {
            return Err("Distribution requires exactly six native targets".into());
        }
        let mut platforms = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for row in &table.targets {
            row.coordinates()?;
            if !platforms.insert(&row.platform) || !targets.insert(&row.target) {
                return Err("Duplicate native platform or Rust target".into());
            }
        }
        // Exactly six unique members of the supported three-family/two-CPU
        // product also establishes that no required native platform is missing.
        for value in [
            &table.policy.macos_minimum,
            &table.policy.linux_minimum_ubuntu,
            &table.policy.linux_glibc_maximum,
            &table.policy.windows_rust_minimum,
        ] {
            let parts: Vec<_> = value.split('.').collect();
            if value.len() > 16
                || parts.is_empty()
                || parts.len() > 3
                || parts
                    .iter()
                    .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
                || parts[0].bytes().all(|byte| byte == b'0')
            {
                return Err("Invalid distribution platform floor".into());
            }
        }
        Ok(table)
    }

    fn plan(self, version: String) -> Result<Plan> {
        if version.is_empty()
            || !version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        {
            return Err("Cargo version cannot form a safe archive component".into());
        }
        let mut native_targets = Vec::new();
        let mut expected_artifacts = Vec::new();
        for row in &self.targets {
            let (family, architecture) = row.coordinates()?;
            let platform_text = match family {
                Family::Macos => format!(
                    "macOS {}+, {architecture}. Native build host {}; app archive smoke host {}. A deployment minimum is not execution evidence for the original OS release.",
                    self.policy.macos_minimum,
                    row.runner,
                    row.app_smoke_runner
                        .as_deref()
                        .ok_or("Missing macOS app smoke host")?
                ),
                Family::Linux => format!(
                    "Ubuntu {}+, GNU/glibc {architecture}; required GLIBC symbols no newer than {}. Native qualification required on {}; other distributions are unqualified.",
                    self.policy.linux_minimum_ubuntu, self.policy.linux_glibc_maximum, row.runner
                ),
                Family::Windows => format!(
                    "Rust target minimum Windows {}+, {architecture}. Native qualification required on {}; client patch coverage depends on actual qualification records.",
                    self.policy.windows_rust_minimum, row.runner
                ),
            };
            native_targets.push(NativeTarget {
                platform: row.platform.clone(),
                target: row.target.clone(),
                runner: row.runner.clone(),
                app_smoke_runner: row.app_smoke_runner.clone(),
                os: family.name(),
                architecture: architecture.into(),
                app: family == Family::Macos,
                platform_text,
            });
            let root = format!("{CLI_PACKAGE}-{version}-{}", row.platform);
            let extension = if family == Family::Linux {
                "tar.gz"
            } else {
                "zip"
            };
            expected_artifacts.push(ExpectedArtifact {
                name: format!("{root}.{extension}"),
                root,
                platform: row.platform.clone(),
                role: "cli",
                executable: if family == Family::Windows {
                    "remove-eml-attachments.exe"
                } else {
                    CLI_PACKAGE
                },
            });
            if family == Family::Macos {
                expected_artifacts.push(ExpectedArtifact {
                    name: format!("EML-Attachment-Remover-{version}-{}.zip", row.platform),
                    root: "EML Attachment Remover.app".into(),
                    platform: row.platform.clone(),
                    role: "app",
                    executable: "Contents/MacOS/EMLAttachmentRemover",
                });
            }
        }
        expected_artifacts.sort_by(|left, right| left.name.cmp(&right.name));
        let mut names = BTreeSet::new();
        for artifact in &expected_artifacts {
            if artifact.name.len() > 255 || !names.insert(&artifact.name) {
                return Err(
                    "Expected archive basenames must be unique and fit one component".into(),
                );
            }
        }
        Ok(Plan {
            version,
            policy: self.policy,
            native_targets,
            expected_artifacts,
        })
    }
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
}
#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    #[serde(deserialize_with = "required_source")]
    source: Option<String>,
}
fn required_source<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::deserialize(input)
}
fn workspace_version(root: &Path, bytes: &[u8]) -> Result<String> {
    let document: serde_json::Value = serde_json::from_slice(bytes)?;
    if !document.is_object()
        || !document
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|packages| packages.iter().all(serde_json::Value::is_object))
    {
        return Err("Cargo metadata requires workspace and package objects".into());
    }
    let metadata: Metadata = serde_json::from_value(document)?;
    let root = root.canonicalize()?;
    if metadata.workspace_root.canonicalize()? != root {
        return Err("Cargo metadata refers to another workspace".into());
    }
    let mut candidates = metadata
        .packages
        .into_iter()
        .filter(|package| package.name == CLI_PACKAGE);
    let package = candidates
        .next()
        .ok_or("Workspace CLI package missing from Cargo metadata")?;
    if candidates.next().is_some()
        || package.id.is_empty()
        || metadata
            .workspace_members
            .iter()
            .filter(|id| **id == package.id)
            .count()
            != 1
        || package.source.is_some()
        || package.manifest_path.canonicalize()?
            != root.join("crates/eml-cli/Cargo.toml").canonicalize()?
    {
        return Err("Cargo CLI package identity does not match this workspace source".into());
    }
    Ok(package.version)
}

pub(crate) fn run(root: &Path) -> Result<()> {
    let table = Distribution::parse(&std::fs::read_to_string(
        root.join("xtask/distribution.toml"),
    )?)?;
    // Fixed read-only Cargo metadata, not a campaign worker. No Unix controller,
    // copied helper, custom signal handler or bounded process-lifetime claim.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let metadata = Command::new(cargo)
        .current_dir(root)
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()?;
    if !metadata.status.success() {
        std::io::stderr().write_all(&metadata.stderr)?;
        return Err(format!("Cargo metadata failed: {}", metadata.status).into());
    }
    let version = workspace_version(root, &metadata.stdout)?;
    let plan = table.plan(version)?;
    let tool_input = std::fs::read_to_string(root.join("xtask/tools.toml"))?;
    let plan = BuildPlan {
        distribution: plan,
        xcode: Xcode::parse(&tool_input)?,
        tools: tool_versions(&tool_input)?,
    };
    let mut bytes = serde_json::to_vec(&plan)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_PLAN_BYTES {
        return Err("Distribution metadata exceeds its bounded output size".into());
    }
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const TABLE: &str = include_str!("../distribution.toml");

    #[test]
    fn developer_tool_output_uses_the_same_validated_pin_authority() {
        let tools = tool_versions(include_str!("../tools.toml")).unwrap();
        assert_eq!(tools["cargo_about"], "0.9.2");
        assert_eq!(tools["fuzz_compiler"], "nightly-2026-09-30");
        for invalid in [
            "",
            "[versions]",
            "[versions]\ncargo_about=1",
            "[versions]\n'bad key'='1'",
            "[versions]\ncargo_about='../1'",
            "[versions]\ncargo_about='1 2'",
        ] {
            assert!(tool_versions(invalid).is_err());
        }
    }

    #[test]
    fn xcode_input_rejects_missing_unknown_and_unsafe_path_components() {
        assert!(Xcode::parse(include_str!("../tools.toml")).is_ok());
        for input in [
            "[versions]\ntool='1'",
            "[xcode]\nversion='26.3'",
            "[xcode]\nversion='../26.3'\nbuild='17C529'",
            "[xcode]\nversion='26.3'\nbuild=''",
            "[xcode]\nversion='26.3'\nbuild='17C529'\nextra=true",
            "[xcode]\nversion=26.3\nbuild='17C529'",
        ] {
            assert!(
                Xcode::parse(input).is_err(),
                "Accepted invalid input: {input}"
            );
        }
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
            "packages": [{"id":"owned-workspace-cli", "name":"remove-eml-attachments",
                "version":version, "source":null,
                "manifest_path":root.join("crates/eml-cli/Cargo.toml")}]
        })
    }
    fn version(value: &serde_json::Value) -> Result<String> {
        workspace_version(&source_root(), &serde_json::to_vec(value).unwrap())
    }

    #[test]
    fn distribution_records_match_all_six_approved_native_tuples() {
        let table = Distribution::parse(TABLE).unwrap();
        let actual: Vec<_> = table
            .targets
            .iter()
            .map(|row| {
                (
                    row.platform.as_str(),
                    row.target.as_str(),
                    row.runner.as_str(),
                )
            })
            .collect();
        assert_eq!(
            actual,
            [
                ("macos-aarch64", "aarch64-apple-darwin", "macos-26"),
                ("macos-x86_64", "x86_64-apple-darwin", "macos-26-intel"),
                ("linux-x86_64", "x86_64-unknown-linux-gnu", "ubuntu-22.04"),
                (
                    "linux-aarch64",
                    "aarch64-unknown-linux-gnu",
                    "ubuntu-22.04-arm"
                ),
                (
                    "windows-x86_64",
                    "x86_64-pc-windows-gnullvm",
                    "windows-2022"
                ),
                (
                    "windows-aarch64",
                    "aarch64-pc-windows-gnullvm",
                    "windows-11-arm"
                ),
            ]
        );
        assert_eq!(table.policy.macos_minimum, "15.0");
        assert_eq!(table.policy.linux_minimum_ubuntu, "22.04");
        assert_eq!(table.policy.linux_glibc_maximum, "2.35");
        assert_eq!(table.policy.windows_rust_minimum, "10");
        assert_eq!(
            table.targets[0].app_smoke_runner.as_deref(),
            Some("macos-15")
        );
        assert_eq!(
            table.targets[1].app_smoke_runner.as_deref(),
            Some("macos-15-intel")
        );
    }

    #[test]
    fn expected_archive_names_are_exactly_six_cli_and_two_separate_mac_apps() {
        let plan = Distribution::parse(TABLE)
            .unwrap()
            .plan("6.0.0".into())
            .unwrap();
        let names: Vec<_> = plan
            .expected_artifacts
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "EML-Attachment-Remover-6.0.0-macos-aarch64.zip",
                "EML-Attachment-Remover-6.0.0-macos-x86_64.zip",
                "remove-eml-attachments-6.0.0-linux-aarch64.tar.gz",
                "remove-eml-attachments-6.0.0-linux-x86_64.tar.gz",
                "remove-eml-attachments-6.0.0-macos-aarch64.zip",
                "remove-eml-attachments-6.0.0-macos-x86_64.zip",
                "remove-eml-attachments-6.0.0-windows-aarch64.zip",
                "remove-eml-attachments-6.0.0-windows-x86_64.zip",
            ]
        );
        assert_eq!(
            plan.expected_artifacts
                .iter()
                .filter(|row| row.role == "app")
                .count(),
            2
        );
        assert_eq!(plan.native_targets.iter().filter(|row| row.app).count(), 2);
        for artifact in &plan.expected_artifacts {
            if artifact.role == "app" {
                assert_eq!(artifact.root, "EML Attachment Remover.app");
                assert_eq!(artifact.executable, "Contents/MacOS/EMLAttachmentRemover");
            } else {
                assert!(artifact.name.starts_with(&artifact.root));
                assert_eq!(
                    artifact.executable,
                    if artifact.platform.starts_with("windows-") {
                        "remove-eml-attachments.exe"
                    } else {
                        "remove-eml-attachments"
                    }
                );
            }
        }
    }

    #[test]
    fn alternate_cargo_metadata_version_changes_every_expected_archive() {
        let value = metadata("7.2.1-rc.2+build.5");
        let plan = Distribution::parse(TABLE)
            .unwrap()
            .plan(version(&value).unwrap())
            .unwrap();
        assert_eq!(plan.version, "7.2.1-rc.2+build.5");
        assert!(
            plan.expected_artifacts
                .iter()
                .all(|row| row.name.contains("-7.2.1-rc.2+build.5-"))
        );
        assert!(
            plan.native_targets
                .iter()
                .all(|row| row.platform_text.contains(&row.runner))
        );
        let bytes = serde_json::to_vec(&plan).unwrap();
        assert!(bytes.len() + 1 < MAX_PLAN_BYTES);
        assert_eq!(bytes, serde_json::to_vec(&plan).unwrap());
    }

    #[test]
    fn unknown_missing_duplicate_and_unsafe_distribution_inputs_are_rejected() {
        let missing_last = TABLE.rsplit_once("\n[[targets]]").unwrap().0.to_owned();
        for input in [
            format!("unknown = true\n{TABLE}"),
            format!("{TABLE}\nunknown = true\n"),
            TABLE.replace("[policy]", "[policy]\nunknown = true"),
            TABLE.replace("[policy]", "[policy]\nmacos_minimum = \"15.0\""),
            TABLE.replace("target = \"aarch64-apple-darwin\"\n", ""),
            TABLE.replace("macos_minimum = \"15.0\"\n", ""),
            TABLE.replace("runner = \"macos-26\"", "runner = 1"),
            TABLE.replace("app_smoke_runner = \"macos-15\"\n", ""),
            TABLE.replace(
                "app_smoke_runner = \"macos-15\"",
                "app_smoke_runner = \"../unsafe\"",
            ),
            TABLE.replace("macos-aarch64", "macos-universal"),
            TABLE.replace("macos-aarch64", "other-aarch64"),
            TABLE.replace("aarch64-apple-darwin", "x86_64-apple-darwin"),
            TABLE.replace("unknown-linux-gnu", "unknown-linux-musl"),
            TABLE.replace("runner = \"macos-26\"", "runner = \"../unsafe\""),
            TABLE.replace("runner = \"macos-26\"", "runner = \"line\\nunsafe\""),
            TABLE.replace("runner = \"macos-26\"", "runner = \"\""),
            TABLE.replace("macos_minimum = \"15.0\"", "macos_minimum = \"15..0\""),
            TABLE.replace(
                "linux_glibc_maximum = \"2.35\"",
                "linux_glibc_maximum = \"0.35\"",
            ),
            missing_last,
        ] {
            assert!(Distribution::parse(&input).is_err());
        }
        let duplicate = TABLE
            .replace("windows-aarch64", "windows-x86_64")
            .replace("aarch64-pc-windows-gnullvm", "x86_64-pc-windows-gnullvm");
        assert!(
            Distribution::parse(&duplicate)
                .err()
                .unwrap()
                .to_string()
                .contains("Duplicate")
        );
    }

    #[test]
    fn archive_identity_rejects_unsafe_or_overlong_version_components() {
        for value in ["", "../other", "1/2", "1\\2", "1\n2", "1\0", "1 2"] {
            assert!(
                Distribution::parse(TABLE)
                    .unwrap()
                    .plan(value.into())
                    .is_err()
            );
        }
        let overlong = format!("1.0.0-{}", "a".repeat(255));
        assert!(Distribution::parse(TABLE).unwrap().plan(overlong).is_err());
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
                "workspace" => {
                    value["workspace_root"] = serde_json::json!(source_root().join("xtask"))
                }
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
}
