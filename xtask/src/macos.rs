//! Shared pinned native Mac build inputs and executable inspection.
use crate::{Result, capture, controller};
use object::Object;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(crate) fn selected_command(
    root: &Path,
    developer: &Path,
    program: impl AsRef<OsStr>,
) -> Command {
    let mut command = Command::new(program);
    command.current_dir(root).env("DEVELOPER_DIR", developer);
    for name in [
        "SDKROOT",
        "TOOLCHAINS",
        "SWIFT_EXEC",
        "SWIFT_DRIVER_SWIFT_FRONTEND_EXEC",
        "SWIFT_DRIVER_CLANG_EXEC",
    ] {
        command.env_remove(name);
    }
    command
}

#[derive(Deserialize)]
struct Plan {
    version: String,
    policy: Policy,
    xcode: Xcode,
    native_targets: Vec<Target>,
}
#[derive(Deserialize)]
struct Policy {
    macos_minimum: String,
}
#[derive(Deserialize)]
struct Xcode {
    version: String,
    build: String,
}
#[derive(Deserialize)]
struct Target {
    target: String,
    architecture: String,
    os: String,
}
#[derive(Debug, Serialize)]
pub(crate) struct MachIdentity {
    pub architecture: String,
    pub minimum: u32,
    pub sdk: u32,
}

pub(crate) fn macho_architecture(file: &object::File<'_>) -> Result<&'static str> {
    if file.kind() != object::ObjectKind::Executable || !matches!(file, object::File::MachO64(_)) {
        return Err("Expected one thin 64-bit Mach-O executable".into());
    }
    Ok(match file.architecture() {
        object::Architecture::Aarch64 => "aarch64",
        object::Architecture::X86_64 => "x86_64",
        _ => return Err("Unexpected macOS CPU".into()),
    })
}

pub(crate) fn macho_identity(bytes: &[u8]) -> Result<MachIdentity> {
    let file = object::File::parse(bytes)?;
    let architecture = macho_architecture(&file)?;
    let object::File::MachO64(mach) = file else {
        return Err("Expected one thin 64-bit Mach-O executable".into());
    };
    let mut commands = mach.macho_load_commands()?;
    let mut build = None;
    while let Some(command) = commands.next()? {
        if let Some(value) = command.build_version()? {
            if build.is_some() || value.platform.get(mach.endian()) != object::macho::PLATFORM_MACOS
            {
                return Err("Expected exactly one macOS build-version command".into());
            }
            build = Some((value.minos.get(mach.endian()), value.sdk.get(mach.endian())));
        }
    }
    let (minimum, sdk) = build.ok_or("Missing macOS build version")?;
    Ok(MachIdentity {
        architecture: architecture.into(),
        minimum,
        sdk,
    })
}

pub(crate) fn packed_version(value: &str) -> Result<u32> {
    let parts: Vec<u32> = value
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    if !(2..=3).contains(&parts.len())
        || parts[0] > 65_535
        || parts[1] > 255
        || parts.get(2).is_some_and(|value| *value > 255)
    {
        return Err("Invalid macOS deployment version".into());
    }
    Ok((parts[0] << 16) | (parts[1] << 8) | parts.get(2).copied().unwrap_or(0))
}

pub(crate) fn fixture_identity(
    bytes: &[u8],
    architecture: &str,
    minimum: u32,
) -> Result<MachIdentity> {
    let identity = macho_identity(bytes)?;
    if identity.architecture != architecture || identity.minimum != minimum {
        return Err("Actual fixture executable CPU/deployment minimum mismatch".into());
    }
    Ok(identity)
}

pub(crate) fn cli_artifact(root: &Path, output: &[u8]) -> Result<PathBuf> {
    let manifest = root.join("crates/eml-cli/Cargo.toml").canonicalize()?;
    let source = root.join("crates/eml-cli/src/main.rs").canonicalize()?;
    let mut candidates = Vec::new();
    let mut finished = 0;
    for document in serde_json::Deserializer::from_slice(output).into_iter::<serde_json::Value>() {
        let value = document?;
        if value["reason"] == "build-finished" {
            if value["success"] != true {
                return Err("Cargo reported an unsuccessful build".into());
            }
            finished += 1;
        }
        if value["reason"] != "compiler-artifact"
            || value["target"]["name"] != "remove-eml-attachments"
            || value["target"]["kind"] != serde_json::json!(["bin"])
        {
            continue;
        }
        let path = value["manifest_path"]
            .as_str()
            .ok_or("Artifact manifest missing")?;
        let main = value["target"]["src_path"]
            .as_str()
            .ok_or("Artifact source missing")?;
        if Path::new(path).canonicalize()? != manifest
            || Path::new(main).canonicalize()? != source
            || value["profile"]["test"] != false
        {
            continue;
        }
        candidates.push(PathBuf::from(
            value["executable"]
                .as_str()
                .ok_or("CLI executable artifact missing")?,
        ));
    }
    if finished != 1 || candidates.len() != 1 {
        return Err(
            "Expected one successful Cargo build and one actual workspace CLI executable".into(),
        );
    }
    Ok(candidates.remove(0))
}

pub(crate) fn checked_run(command: Command, log: &Path, seconds: u64) -> Result<()> {
    let result = controller()?.run(command, log, Duration::from_secs(seconds))?;
    if !result.success() {
        return Err(format!(
            "Native process fixture command failed; evidence {}",
            log.display()
        )
        .into());
    }
    Ok(())
}

#[derive(Serialize)]
pub(crate) struct MacBuild {
    pub version: String,
    pub minimum_text: String,
    pub minimum: u32,
    pub host: String,
    pub architecture: String,
    pub swift_arch: String,
    pub developer: PathBuf,
    pub sdk: PathBuf,
    pub swiftc: PathBuf,
    pub rust_identity: String,
    pub xcode_identity: String,
    pub swift_identity: String,
    pub sdk_version: String,
}
impl MacBuild {
    pub fn command(&self, root: &Path, program: impl AsRef<OsStr>) -> Command {
        let mut command = selected_command(root, &self.developer, program);
        command.env("SDKROOT", &self.sdk);
        command
    }
    pub fn swift_target(&self) -> String {
        format!("{}-apple-macos{}", self.swift_arch, self.minimum_text)
    }
}
pub(crate) fn load(root: &Path) -> Result<MacBuild> {
    crate::campaign::require_stock_topology(root)?;
    let plan: Plan = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .arg("distribution")
            .current_dir(root),
    )?)?;
    let developer = PathBuf::from(format!(
        "/Applications/Xcode_{}.app/Contents/Developer",
        plan.xcode.version
    ));
    if !developer.is_dir() {
        return Err(format!(
            "Required Xcode {} is unavailable at {}",
            plan.xcode.version,
            developer.display()
        )
        .into());
    }
    let command = |program: &str| selected_command(root, &developer, program);
    let xcode = String::from_utf8(capture(command("/usr/bin/xcodebuild").arg("-version"))?)?;
    if xcode.trim_end()
        != format!(
            "Xcode {}\nBuild version {}",
            plan.xcode.version, plan.xcode.build
        )
    {
        return Err("Actual selected Xcode version/build differs from tools authority".into());
    }
    let sdk = PathBuf::from(
        String::from_utf8(capture(command("/usr/bin/xcrun").args([
            "--sdk",
            "macosx",
            "--toolchain",
            "XcodeDefault",
            "--show-sdk-path",
        ]))?)?
        .trim_end(),
    );
    if !sdk.is_dir() || !sdk.canonicalize()?.starts_with(developer.canonicalize()?) {
        return Err("Selected macOS SDK is not inside the pinned Xcode".into());
    }
    let swiftc = developer.join("Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc");
    let swift = String::from_utf8(capture(
        selected_command(root, &developer, &swiftc)
            .arg("--version")
            .env("SDKROOT", &sdk),
    )?)?;
    let sdk_version = String::from_utf8(capture(command("/usr/bin/xcrun").args([
        "--sdk",
        "macosx",
        "--toolchain",
        "XcodeDefault",
        "--show-sdk-version",
    ]))?)?;
    let compiler = String::from_utf8(capture(command("rustc").arg("-vV"))?)?;
    let pin: toml::Value = toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml"))?)?;
    let required = pin
        .get("toolchain")
        .and_then(|value| value.get("channel"))
        .and_then(toml::Value::as_str)
        .ok_or("Compiler pin missing")?;
    let host = compiler
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("Compiler host missing")?;
    if !compiler
        .lines()
        .any(|line| line == format!("release: {required}"))
    {
        return Err("Actual Rust release differs from pinned compiler".into());
    }
    let target = plan
        .native_targets
        .iter()
        .find(|row| row.target == host && row.os == "macos")
        .ok_or("Native macOS compiler host required")?;
    let swift_arch = match target.architecture.as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => return Err("Unsupported native Mac CPU".into()),
    };
    let own_bytes = fs::read(std::env::current_exe()?)?;
    let own = object::File::parse(own_bytes.as_slice())?;
    if macho_architecture(&own)? != target.architecture {
        return Err("Running xtask CPU differs from compiler host".into());
    }
    let minimum = packed_version(&plan.policy.macos_minimum)?;
    Ok(MacBuild {
        version: plan.version.clone(),
        minimum_text: plan.policy.macos_minimum.clone(),
        minimum,
        host: host.to_owned(),
        architecture: target.architecture.clone(),
        swift_arch: swift_arch.to_owned(),
        developer,
        sdk,
        swiftc,
        rust_identity: compiler,
        xcode_identity: xcode,
        swift_identity: swift,
        sdk_version: sdk_version.trim_end().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cargo_selection_requires_one_real_workspace_bin_and_successful_completion() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let artifact = serde_json::json!({"reason":"compiler-artifact","manifest_path":root.join("crates/eml-cli/Cargo.toml"),"target":{"name":"remove-eml-attachments","kind":["bin"],"src_path":root.join("crates/eml-cli/src/main.rs")},"profile":{"test":false},"executable":root.join("target/owned-test-executable")});
        let finished = serde_json::json!({"reason":"build-finished","success":true});
        let encode = |values: &[serde_json::Value]| {
            values
                .iter()
                .map(|value| serde_json::to_string(value).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(
            cli_artifact(
                &root,
                encode(&[artifact.clone(), finished.clone()]).as_bytes()
            )
            .unwrap(),
            root.join("target/owned-test-executable")
        );
        for values in [
            vec![],
            vec![artifact.clone()],
            vec![finished.clone()],
            vec![artifact.clone(), artifact.clone(), finished.clone()],
            vec![
                artifact.clone(),
                serde_json::json!({"reason":"build-finished","success":false}),
            ],
        ] {
            assert!(cli_artifact(&root, encode(&values).as_bytes()).is_err());
        }
        let mut harness = artifact.clone();
        harness["profile"]["test"] = true.into();
        assert!(cli_artifact(&root, encode(&[harness, finished.clone()]).as_bytes()).is_err());
        let mut foreign = artifact;
        foreign["manifest_path"] = root.join("xtask/Cargo.toml").to_str().unwrap().into();
        assert!(cli_artifact(&root, encode(&[foreign, finished]).as_bytes()).is_err());
    }
    fn macho(cpu: u32, minimum: u32, platform: u32, duplicate: bool) -> Vec<u8> {
        // Independent SDK layout: mach_header_64, then LC_BUILD_VERSION.
        let commands = if duplicate { 2 } else { 1 };
        let mut words = vec![0xfeedfacf, cpu, 0, 2, commands, commands * 24, 0, 0];
        for _ in 0..commands {
            words.extend([0x32, 24, platform, minimum, 0x001a0300, 0]);
        }
        words.into_iter().flat_map(u32::to_le_bytes).collect()
    }
    #[test]
    fn macho_requires_thin_executable_supported_cpu_and_one_macos_build_command() {
        let valid = macho(0x0100000c, 0x000f0000, 1, false);
        let identity = macho_identity(&valid).unwrap();
        assert_eq!(identity.architecture, "aarch64");
        assert_eq!(identity.minimum, 0x000f0000);
        assert!(fixture_identity(&valid, "x86_64", 0x000f0000).is_err());
        assert!(fixture_identity(&valid, "aarch64", 0x000e0000).is_err());
        assert!(fixture_identity(&valid, "aarch64", 0x000f0000).is_ok());
        assert!(macho_identity(&macho(12, 0x000f0000, 1, false)).is_err());
        assert!(macho_identity(&macho(0x0100000c, 0x000f0000, 2, false)).is_err());
        assert!(macho_identity(&macho(0x0100000c, 0x000f0000, 1, true)).is_err());
        assert!(macho_identity(b"not an executable").is_err());
        let older_tool: Vec<u8> = [
            0xfeedfacf_u32,
            0x01000007,
            3,
            2,
            1,
            16,
            0,
            0,
            0x24,
            16,
            0x000a0c00,
            0x000f0400,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        let file = object::File::parse(older_tool.as_slice()).unwrap();
        assert_eq!(macho_architecture(&file).unwrap(), "x86_64");
        assert!(fixture_identity(&older_tool, "x86_64", 0x000f0000).is_err());
    }
    #[test]
    fn deployment_version_uses_checked_native_fields() {
        assert_eq!(packed_version("15.0").unwrap(), 0x000f0000);
        assert_eq!(packed_version("15.2.1").unwrap(), 0x000f0201);
        for value in ["15", "15.256", "15.0.256", "65536.0", "15.-1", "15.0.0.1"] {
            assert!(packed_version(value).is_err());
        }
    }
}
