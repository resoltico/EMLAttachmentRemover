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

/// Effective loader values for these inheriting diagnostic commands; env_clear
/// is not used by their reviewed constructors. No other CI environment is read.
pub(crate) fn loader_environment(command: &Command) -> serde_json::Value {
    use base64::Engine;
    use std::os::unix::ffi::OsStrExt;
    let values = [
        "DYLD_LIBRARY_PATH",
        "DYLD_FRAMEWORK_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "DYLD_FALLBACK_FRAMEWORK_PATH",
    ]
    .into_iter()
    .map(|name| {
        let explicit = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new(name))
            .map(|(_, value)| value.map(OsStr::to_owned));
        let value = explicit.unwrap_or_else(|| std::env::var_os(name));
        let value = match value {
            Some(value) if value.as_os_str().as_bytes().len() > 16_384 => {
                serde_json::json!({"state":"unobserved","reason":"over_limit","limit_bytes":16384})
            }
            Some(value) => serde_json::json!({
                "state":"observed", "display_utf8_lossy":value.to_string_lossy(),
                "native_encoding":"unix-bytes",
                "native_base64":base64::engine::general_purpose::STANDARD.encode(value.as_os_str().as_bytes())
            }),
            None => serde_json::Value::Null,
        };
        (name.to_owned(), value)
    })
    .collect();
    serde_json::Value::Object(values)
}

pub(crate) fn observe_graphics(root: &Path, out: &Path, build: &MacBuild) -> Result<()> {
    for (key, expected) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
    ] {
        if std::env::var(key).as_deref() != Ok(expected) {
            return Err("Graphics observation requires a fresh owned hosted-runner job".into());
        }
    }
    fs::create_dir(out)?;
    let tool = out.join("MetalDeviceObservation");
    let mut compile = build.command(root, &build.swiftc);
    compile
        .args([
            "-parse-as-library",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-target",
            &build.swift_target(),
            "-sdk",
        ])
        .arg(&build.sdk)
        .args(["-framework", "CoreGraphics"])
        .arg(root.join("macos-app/Tests/MetalDeviceObservation.swift"))
        .arg("-o")
        .arg(&tool);
    checked_run(compile, &out.join("compile.log"), 120)?;
    let bytes = fs::read(&tool)?;
    let identity = fixture_identity(&bytes, &build.architecture, build.minimum)?;
    let mut command = Command::new(&tool);
    command.current_dir(root);
    fs::write(
        out.join("input.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"build":build,"tool_image":identity,"tool_bytes":xtask::archive::ByteIdentity::of(&bytes),"loader_environment":loader_environment(&command),"coregraphics_explicitly_linked":true,"scope":"read-only public Metal metadata; no private target architecture, rendering or GPU selection"}),
        )?,
    )?;
    let log = out.join("device.log");
    let result = controller()?.run_separate_output(command, &log, Duration::from_secs(15))?;
    fs::write(
        out.join("process.json"),
        serde_json::to_vec_pretty(&result.evidence())?,
    )?;
    if !controller()?.admits_work() {
        return Err("Graphics observer lifetime/control closed later process admission".into());
    }
    if result.success() {
        if fs::metadata(&log)?.len() > 65_536 {
            return Err("Public Metal observation exceeds its document bound".into());
        }
        let value: GraphicsObservation = serde_json::from_slice(&fs::read(&log)?)?;
        value.validate()?;
        fs::write(out.join("device.json"), serde_json::to_vec_pretty(&value)?)?;
    }
    // A settled ordinary observer failure stays distinct from the unchanged
    // product probe. Its raw status/stdout/stderr remain available for diagnosis.
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphicsObservation {
    scope: String,
    device_count: usize,
    devices: Vec<GraphicsDevice>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphicsDevice {
    name: String,
    registry_id: u64,
    has_unified_memory: bool,
    families: std::collections::BTreeMap<String, bool>,
    architecture: GraphicsArchitecture,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum GraphicsArchitecture {
    Observed { public_architecture_name: String },
    Unobserved {},
}
impl GraphicsObservation {
    fn validate(&self) -> Result<()> {
        if self.scope.len() > 512
            || self.device_count != self.devices.len()
            || self.devices.len() > 16
        {
            return Err("Inconsistent public graphics observation".into());
        }
        for device in &self.devices {
            let expected = ["apple1", "apple3", "apple5", "apple7", "metal3"];
            if device.name.len() > 512
                || device.families.len() != expected.len()
                || expected
                    .iter()
                    .any(|key| !device.families.contains_key(*key))
                || matches!(&device.architecture, GraphicsArchitecture::Observed { public_architecture_name } if public_architecture_name.len() > 512)
            {
                return Err("Invalid public graphics device record".into());
            }
        }
        Ok(())
    }
}

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

pub(crate) fn main_image_uuid(bytes: &[u8]) -> Result<[u8; 16]> {
    let file = object::File::parse(bytes)?;
    macho_architecture(&file)?;
    let object::File::MachO64(mach) = file else {
        return Err("Expected a thin main image".into());
    };
    let mut commands = mach.macho_load_commands()?;
    let mut uuid = None;
    while let Some(command) = commands.next()? {
        if command.cmd() == object::macho::LC_UUID {
            if uuid.is_some() || command.cmdsize() != 24 {
                return Err("Expected exactly one 24-byte LC_UUID".into());
            }
            uuid = Some(command.raw_data()[8..24].try_into()?);
        }
    }
    uuid.ok_or_else(|| "Owned app main image has no UUID".into())
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

pub(crate) use crate::cli_build::artifact as cli_artifact;

pub(crate) fn parse_system_dependencies(output: &str) -> Result<Vec<String>> {
    let mut lines = output.lines();
    if !lines.next().is_some_and(|line| line.ends_with(':')) {
        return Err("Missing native Mach-O dependency header".into());
    }
    let mut dependencies = Vec::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let (path, versions) = line
            .trim()
            .split_once(" (compatibility version ")
            .ok_or("Malformed Mach-O dependency record")?;
        if path.split('/').skip(1).any(|component| {
            component.is_empty() || matches!(component, "." | "..") || component.contains('\\')
        }) {
            return Err("Non-normal Mach-O install-name component".into());
        }
        if !versions.ends_with(')')
            || (!path.starts_with("/usr/lib/") && !path.starts_with("/System/Library/"))
        {
            return Err("Unreviewed non-system or malformed Mach-O dependency".into());
        }
        dependencies.push(path.to_owned());
    }
    if dependencies.is_empty() {
        return Err("Mach-O dependency discovery unexpectedly empty".into());
    }
    Ok(dependencies)
}

pub(crate) fn inspect_system_dependencies(
    mut command: Command,
    executable: &Path,
    log: &Path,
) -> Result<Vec<String>> {
    command.args(["-L"]).arg(executable).env("LC_ALL", "C");
    let result = controller()?.run_separate_output(command, log, Duration::from_secs(30))?;
    if !result.success() {
        return Err(format!(
            "Native dependency inspection failed; evidence {}",
            log.display()
        )
        .into());
    }
    if fs::metadata(log)?.len() > 4 * 1024 * 1024
        || fs::metadata(log.with_extension("stderr"))?.len() != 0
    {
        return Err("Native dependency output is oversized or has diagnostics".into());
    }
    parse_system_dependencies(&fs::read_to_string(log)?)
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
    fn loader_observation_respects_explicit_removals_and_bounds_before_encoding() {
        use std::os::unix::ffi::OsStringExt;
        let mut command = Command::new("owned-unlaunched-fixture");
        command.env_remove("DYLD_LIBRARY_PATH");
        command.env(
            "DYLD_FRAMEWORK_PATH",
            std::ffi::OsString::from_vec(vec![0xff, b'/']),
        );
        command.env("DYLD_FALLBACK_LIBRARY_PATH", "x".repeat(16_385));
        command.env("DYLD_FALLBACK_FRAMEWORK_PATH", "");
        let observed = loader_environment(&command);
        assert!(observed["DYLD_LIBRARY_PATH"].is_null());
        assert_eq!(observed["DYLD_FRAMEWORK_PATH"]["native_base64"], "/y8=");
        assert_eq!(
            observed["DYLD_FALLBACK_LIBRARY_PATH"],
            serde_json::json!({"state":"unobserved","reason":"over_limit","limit_bytes":16384})
        );
        assert_eq!(
            observed["DYLD_FALLBACK_FRAMEWORK_PATH"]["native_base64"],
            ""
        );
    }
    #[test]
    fn graphics_records_reject_inconsistent_or_untyped_device_observations() {
        let empty =
            serde_json::json!({"scope":"public enumeration", "device_count":0,"devices":[]});
        serde_json::from_value::<GraphicsObservation>(empty.clone())
            .unwrap()
            .validate()
            .unwrap();
        let mut mismatch = empty.clone();
        mismatch["device_count"] = serde_json::json!(1);
        assert!(
            serde_json::from_value::<GraphicsObservation>(mismatch)
                .unwrap()
                .validate()
                .is_err()
        );
        assert!(serde_json::from_value::<GraphicsObservation>(serde_json::Value::Null).is_err());
        let device = serde_json::json!({"name":"owned fixture", "registry_id":1,"has_unified_memory":false,"families":{"apple1":false,"apple3":false,"apple5":false,"apple7":false,"metal3":false},"architecture":{"state":"unobserved"}});
        let valid =
            serde_json::json!({"scope":"public enumeration","device_count":1,"devices":[device]});
        serde_json::from_value::<GraphicsObservation>(valid.clone())
            .unwrap()
            .validate()
            .unwrap();
        for invalid in [
            {
                let mut value = valid.clone();
                value["devices"][0]["families"]["unknown"] = serde_json::json!(false);
                value
            },
            {
                let mut value = valid.clone();
                value["devices"][0]["architecture"] = serde_json::json!({"state":"observed"});
                value
            },
            {
                let mut value = valid.clone();
                value["devices"][0]["architecture"] = serde_json::json!({"state":"unobserved","public_architecture_name":"unsupported inference"});
                value
            },
        ] {
            assert!(
                serde_json::from_value::<GraphicsObservation>(invalid)
                    .and_then(|value| value.validate().map_err(serde::de::Error::custom))
                    .is_err()
            );
        }
    }
    #[test]
    fn main_uuid_requires_one_exact_complete_command() {
        let image = |sizes: &[u32]| {
            let mut bytes: Vec<u8> = [
                0xfeedfacf_u32,
                0x0100000c,
                0,
                2,
                sizes.len() as u32,
                sizes.iter().sum(),
                0,
                0,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            for size in sizes {
                bytes.extend(object::macho::LC_UUID.to_le_bytes());
                bytes.extend(size.to_le_bytes());
                bytes.extend(std::iter::repeat_n(0x42, size.saturating_sub(8) as usize));
            }
            bytes
        };
        assert_eq!(main_image_uuid(&image(&[24])).unwrap(), [0x42; 16]);
        for invalid in [image(&[]), image(&[16]), image(&[32]), image(&[24, 24])] {
            assert!(main_image_uuid(&invalid).is_err());
        }
        let mut truncated = image(&[24]);
        truncated.pop();
        assert!(main_image_uuid(&truncated).is_err());
    }
    #[test]
    fn native_dependency_parser_rejects_empty_malformed_and_non_system_records() {
        let valid = "owned:\n /usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n";
        assert_eq!(
            parse_system_dependencies(valid).unwrap(),
            ["/usr/lib/libSystem.B.dylib"]
        );
        for bad in [
            "",
            "owned:\n",
            "owned:\n /usr/lib/no-version\n",
            "owned:\n @rpath/libCustom.dylib (compatibility version 1.0.0)\n",
            "owned:\n /usr/lib/../../opt/custom.dylib (compatibility version 1.0.0)\n",
        ] {
            assert!(parse_system_dependencies(bad).is_err());
        }
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
