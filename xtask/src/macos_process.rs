//! Actual same-CPU helper/process fixtures, separate from GUI/archive qualification.
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

fn selected_command(root: &Path, developer: &Path, program: impl AsRef<OsStr>) -> Command {
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
struct MachIdentity {
    architecture: String,
    minimum: u32,
    sdk: u32,
}

fn macho_architecture(file: &object::File<'_>) -> Result<&'static str> {
    if file.kind() != object::ObjectKind::Executable || !matches!(file, object::File::MachO64(_)) {
        return Err("Expected one thin 64-bit Mach-O executable".into());
    }
    Ok(match file.architecture() {
        object::Architecture::Aarch64 => "aarch64",
        object::Architecture::X86_64 => "x86_64",
        _ => return Err("Unexpected macOS CPU".into()),
    })
}

fn macho_identity(bytes: &[u8]) -> Result<MachIdentity> {
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

fn packed_version(value: &str) -> Result<u32> {
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

fn fixture_identity(bytes: &[u8], architecture: &str, minimum: u32) -> Result<MachIdentity> {
    let identity = macho_identity(bytes)?;
    if identity.architecture != architecture || identity.minimum != minimum {
        return Err("Actual fixture executable CPU/deployment minimum mismatch".into());
    }
    Ok(identity)
}

fn fixture_plist(version: &str, minimum: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>io.github.resoltico.emlattachmentremover.process-fixture</string><key>CFBundleExecutable</key><string>ProcessIntegration</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>{version}</string><key>CFBundleVersion</key><string>{version}</string><key>LSMinimumSystemVersion</key><string>{minimum}</string></dict></plist>"
    )
}

fn cli_artifact(root: &Path, output: &[u8]) -> Result<PathBuf> {
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

fn checked_run(command: Command, log: &Path, seconds: u64) -> Result<()> {
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

pub(crate) fn run(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("target/macos-process-tests"))?;
    let work = tempfile::Builder::new()
        .prefix("native-")
        .tempdir_in(root.join("target/macos-process-tests"))?
        .keep();
    eprintln!("Native process fixture evidence: {}", work.display());
    let result = run_owned(root, &work);
    if let Err(error) = &result
        && let Err(record) = fs::write(work.join("failure.log"), format!("{error}\n"))
    {
        eprintln!("Could not retain fixture failure record: {record}");
    }
    result
}

fn run_owned(root: &Path, work: &Path) -> Result<()> {
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
    let cargo_output = capture(
        command("cargo")
            .args([
                "build",
                "--locked",
                "--package",
                "remove-eml-attachments",
                "--target",
                host,
                "--message-format=json",
            ])
            .env("MACOSX_DEPLOYMENT_TARGET", &plan.policy.macos_minimum)
            .env("SDKROOT", &sdk)
            .env("CARGO_TARGET_DIR", work.join("cargo")),
    )?;
    fs::write(work.join("cargo.jsonl"), &cargo_output)?;
    let helper = cli_artifact(root, &cargo_output)?;
    let helper_identity = fixture_identity(&fs::read(&helper)?, &target.architecture, minimum)?;
    let swift_target = format!("{swift_arch}-apple-macos{}", plan.policy.macos_minimum);
    let mut model_sources = fs::read_dir(root.join("macos-app/Sources/RunModel"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    model_sources.retain(|path| path.extension().is_some_and(|value| value == "swift"));
    model_sources.sort();
    if model_sources.is_empty() {
        return Err("No production RunModel sources found".into());
    }
    let mut model = selected_command(root, &developer, &swiftc);
    model
        .args([
            "-emit-library",
            "-static",
            "-emit-module",
            "-module-name",
            "RunModel",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-target",
            &swift_target,
        ])
        .arg("-sdk")
        .arg(&sdk)
        .arg("-module-cache-path")
        .arg(work.join("swift-module-cache"))
        .args(model_sources)
        .arg("-o")
        .arg(work.join("libRunModel.a"))
        .arg("-emit-module-path")
        .arg(work.join("RunModel.swiftmodule"));
    checked_run(model, &work.join("model-build.log"), 180)?;
    let mut app_sources = fs::read_dir(root.join("macos-app/Sources/EMLAttachmentRemover"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    app_sources.retain(|path| path.extension().is_some_and(|value| value == "swift"));
    app_sources.sort();
    if app_sources.is_empty() {
        return Err("No native app component sources found".into());
    }
    let mut components = selected_command(root, &developer, &swiftc);
    components
        .args([
            "-typecheck",
            "-parse-as-library",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-target",
            &swift_target,
            "-sdk",
        ])
        .arg(&sdk)
        .arg("-I")
        .arg(work)
        .arg("-module-cache-path")
        .arg(work.join("swift-module-cache"))
        .args(app_sources);
    checked_run(components, &work.join("app-components.log"), 180)?;
    let app = work.join("Fixture.app");
    let macos = app.join("Contents/MacOS");
    fs::create_dir_all(&macos)?;
    fs::copy(&helper, macos.join("remove-eml-attachments"))?;
    let mut driver = selected_command(root, &developer, &swiftc);
    driver
        .args([
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-target",
            &swift_target,
            "-I",
        ])
        .arg(work)
        .arg("-sdk")
        .arg(&sdk)
        .arg("-module-cache-path")
        .arg(work.join("swift-module-cache"))
        .arg("-L")
        .arg(work)
        .arg("-lRunModel");
    for name in ["HelperChild", "HelperRunControl", "HelperProcess"] {
        driver.arg(root.join(format!(
            "macos-app/Sources/EMLAttachmentRemover/{name}.swift"
        )));
    }
    driver
        .arg(root.join("macos-app/Tests/ProcessIntegration.swift"))
        .arg("-o")
        .arg(macos.join("ProcessIntegration"));
    checked_run(driver, &work.join("driver-build.log"), 180)?;
    let driver_identity = fixture_identity(
        &fs::read(macos.join("ProcessIntegration"))?,
        &target.architecture,
        minimum,
    )?;
    fs::write(
        app.join("Contents/Info.plist"),
        fixture_plist(&plan.version, &plan.policy.macos_minimum),
    )?;
    let sources = work.join("sources");
    fs::create_dir(&sources)?;
    let mut execute = selected_command(root, &developer, macos.join("ProcessIntegration"));
    execute
        .current_dir(root)
        .env("DEVELOPER_DIR", &developer)
        .arg(root)
        .arg(&sources);
    checked_run(execute, &work.join("ordinary.log"), 60)?;
    let output = fs::read_to_string(work.join("ordinary.log"))?;
    for receipt in [
        "actual native fixture machine:",
        "actual Create reader agreement:",
        "actual owned SIGKILL:",
        "actual helper created:",
        "actual helper up_to_date:",
        "actual helper pre-stream refusal:",
        "pre-admission stop and force intent:",
        "actual native Unicode separators/LF/ESC path:",
    ] {
        if output.matches(receipt).count() != 1 {
            return Err(format!("Missing/duplicate real fixture case receipt: {receipt}").into());
        }
    }
    for (mode, version, include_helper) in [
        (
            "version-mismatch",
            if plan.version == "0.0.0" {
                "0.0.1"
            } else {
                "0.0.0"
            },
            true,
        ),
        ("helper-missing", plan.version.as_str(), false),
    ] {
        let variant = work.join(format!("{mode}.app/Contents"));
        let executable = variant.join("MacOS/ProcessIntegration");
        fs::create_dir_all(variant.join("MacOS"))?;
        fs::copy(macos.join("ProcessIntegration"), &executable)?;
        if include_helper {
            fs::copy(&helper, variant.join("MacOS/remove-eml-attachments"))?;
        }
        fs::write(
            variant.join("Info.plist"),
            fixture_plist(version, &plan.policy.macos_minimum),
        )?;
        let sources = work.join(format!("{mode}-sources"));
        fs::create_dir(&sources)?;
        let mut execute = selected_command(root, &developer, &executable);
        execute
            .current_dir(root)
            .env("DEVELOPER_DIR", &developer)
            .arg(root)
            .arg(&sources)
            .arg(mode);
        let log = work.join(format!("{mode}.log"));
        checked_run(execute, &log, 60)?;
        let output = fs::read_to_string(log)?;
        if output.matches("actual native fixture machine:").count() != 1
            || output.matches(&format!("actual {mode}:")).count() != 1
        {
            return Err("Missing/duplicate actual negative fixture receipts".into());
        }
    }
    fs::write(
        work.join("identity.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"version":plan.version,"rustc":compiler,"xcode":xcode,"swift":swift,"sdk":{"path":sdk,"version":sdk_version.trim_end()},"helper":helper_identity,"driver":driver_identity,"scope":"owned process boundary; no GUI/archive qualification"}),
        )?,
    )?;
    eprintln!(
        "Native process fixture passed; {}",
        work.join("ordinary.log").display()
    );
    Ok(())
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
