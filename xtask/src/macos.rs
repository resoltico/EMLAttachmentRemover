//! Shared pinned native Mac build inputs and executable inspection.
use crate::distribution::BuildPlan;
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
        .arg(root.join("tests/macos/MetalDeviceObservation.swift"))
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

pub(crate) fn check_swift_sources(
    root: &Path,
    plan: &BuildPlan,
    out: &Path,
) -> Result<serde_json::Value> {
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let build = load(root, plan)?;
    let paths = crate::source_files::swift(root)?;
    if paths.is_empty() {
        return Err("Swift bridge/test discovery is empty; investigate retained sources".into());
    }
    let version = String::from_utf8(capture(
        build
            .command(root, "/usr/bin/xcrun")
            .args(["swift-format", "--version"]),
    )?)?;
    let defaults = capture(
        build
            .command(root, "/usr/bin/xcrun")
            .args(["swift-format", "dump-configuration"]),
    )?;
    if defaults.len() > 1024 * 1024 || version.len() > 4096 {
        return Err("Selected Swift formatter observation exceeds its bound".into());
    }
    let _: serde_json::Value = serde_json::from_slice(&defaults)?;
    let configuration = out.join("formatter-defaults.json");
    fs::write(&configuration, defaults)?;
    let log = out.join("swift-format.log");
    let mut command = build.command(root, "/usr/bin/xcrun");
    command
        .args(["swift-format", "lint", "--strict", "--configuration"])
        .arg(&configuration)
        .arg("--")
        .args(&paths);
    let process = controller()?.run(command, &log, Duration::from_secs(120))?;
    let result = serde_json::json!({"build":build,"formatter_version":version.trim(),"source_count":paths.len(),"sources":paths,
        "process":process.evidence(),"scope":"strict formatting of discovered retained Swift sources using selected Xcode; native compilation and GUI qualification remain separate"});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    process.require_success(&log)?;
    Ok(result)
}

pub(crate) fn check_desktop(root: &Path, plan: &BuildPlan, out: &Path) -> Result<()> {
    fs::create_dir(out)?;
    let build = load(root, plan)?;
    let mut lint = build.command(root, "cargo");
    lint.args([
        "clippy",
        "--locked",
        "--package",
        "eml-gui",
        "--features",
        "macos-gui,native-fixtures",
        "--target",
        &build.host,
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ])
    .env("MACOSX_DEPLOYMENT_TARGET", &build.minimum_text)
    .env("SLINT_COMPILER_DENY_WARNINGS", "1");
    let lint_result = checked_run(lint, &out.join("slint-profile-clippy.log"), 1200);
    let borrowed_result = check_borrowed_url_docs(root, &build, &out.join("borrowed-url-docs"));
    let fixture = out.join("child-fixtures");
    fs::create_dir(&fixture)?;
    let fixture_build = fixture.join("build");
    fs::create_dir(&fixture_build)?;
    let fixture_owner = eml_fs::BoundDirectory::open(&fixture_build)?;
    let tests = crate::qualify::checked_commands(&fixture, |listing| {
        let mut command = build.command(root, "cargo");
        command
            .args([
                "test",
                "--locked",
                "--package",
                "eml-macos",
                "--package",
                "eml-app",
                "--package",
                "eml-worker",
                "--package",
                "eml-gui",
                "--features",
                "eml-macos/native-fixtures,eml-worker/native-fixtures,eml-gui/macos-gui,eml-gui/native-fixtures",
                "--target",
                &build.host,
                "--",
                "--color",
                "never",
            ])
            .env("CARGO_TARGET_DIR", &fixture_build)
            .env("CARGO_BUILD_BUILD_DIR", &fixture_build)
            .env("CARGO_INCREMENTAL", "0")
            .env("MACOSX_DEPLOYMENT_TARGET", &build.minimum_text);
        if listing {
            command.arg("--list");
        }
        command
    });
    let cleanup = crate::qualify::cleanup_private_test_build(
        root,
        &fixture_build,
        &fixture,
        controller()?.ownership_settled(),
        &fixture_owner,
    );
    let recorded: Result<()> = (|| {
        fs::write(
            out.join("result.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"build":build,"tests":tests.as_ref().ok(),"test_failure":tests.as_ref().err().map(ToString::to_string),"test_build_cleanup":cleanup.as_ref().ok(),"cleanup_failure":cleanup.as_ref().err().map(ToString::to_string),"borrowed_url_docs":{"passed":borrowed_result.is_ok(),"failure":borrowed_result.as_ref().err().map(ToString::to_string)},"slint_profile_lint":{"passed":lint_result.is_ok(),"failure":lint_result.as_ref().err().map(ToString::to_string)},"scope":"native Slint profile and owned Darwin child fixtures; no actual GUI/bundle/archive acceptance"}),
            )?,
        )?;
        Ok(())
    })();
    // Keep the original execution failure primary; cleanup has its own receipt.
    if let Err(error) = &cleanup {
        eprintln!("Native desktop private build cleanup also failed: {error}");
    }
    if let Err(error) = &recorded {
        eprintln!("Native desktop result recording also failed: {error}");
    }
    tests?;
    lint_result?;
    borrowed_result?;
    cleanup?;
    recorded?;
    Ok(())
}

/// Three authored borrowed-batch guards; upstream vendor doctests are deliberately
/// outside this finite selection. The same desktop gate compiles the application's
/// actual callback/as_ptr use as its positive import and use control.
fn check_borrowed_url_docs(root: &Path, build: &MacBuild, out: &Path) -> Result<()> {
    fs::create_dir(out)?;
    let private_build = out.join("build");
    fs::create_dir(&private_build)?;
    let owner = eml_fs::BoundDirectory::open(&private_build)?;
    let mut command = build.command(root, "cargo");
    command
        .args([
            "test",
            "--manifest-path",
            "vendor/winit/Cargo.toml",
            "--locked",
            "--doc",
            "--no-default-features",
            "--features",
            "rwh_06",
            "--target",
            &build.host,
            "--",
            "BorrowedFileUrls",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("CARGO_TARGET_DIR", &private_build)
        .env("CARGO_BUILD_BUILD_DIR", &private_build)
        .env("CARGO_INCREMENTAL", "0")
        .env("MACOSX_DEPLOYMENT_TARGET", &build.minimum_text)
        .env("CARGO_TERM_COLOR", "never");
    let log = out.join("rustdoc.log");
    let process = controller()?.run_separate_output(command, &log, Duration::from_secs(1200));
    let validation = (|| -> Result<()> {
        process
            .as_ref()
            .map_err(ToString::to_string)?
            .require_success(&log)?;
        let stderr = log.with_extension("stderr");
        if fs::metadata(&log)?.len() > 1024 * 1024
            || fs::metadata(&stderr)?.len() > 16 * 1024 * 1024
        {
            return Err("Borrowed URL rustdoc output exceeds evidence bounds".into());
        }
        verify_borrowed_url_docs(&fs::read_to_string(&log)?, &fs::read_to_string(stderr)?)
    })();
    let cleanup = crate::qualify::cleanup_private_test_build(
        root,
        &private_build,
        out,
        controller()?.ownership_settled(),
        &owner,
    );
    let recorded: Result<()> = (|| {
        fs::write(
            out.join("result.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "process": process.as_ref().ok().map(|process| process.evidence()), "passed": validation.is_ok() && cleanup.is_ok(),
                "test_build_cleanup":cleanup.as_ref().ok(),"cleanup_failure":cleanup.as_ref().err().map(ToString::to_string),
                "failure": validation.as_ref().err().map(ToString::to_string),
                "required_selection": {"selected": 3, "intentionally_filtered_upstream": 27},
                "qualified_counts": (validation.is_ok() && cleanup.is_ok()).then(|| serde_json::json!({"passed":3,"failed":0,"ignored":0,"intentionally_filtered_upstream":27})),
                "positive_control": "same desktop gate Slint Clippy compiles actual BorrowedFileUrls callback/as_ptr use",
                "scope": "three authored lifetime/Send/Sync compile-fail doctests; no GUI execution or complete upstream vendor suite"
            }))?,
        )?;
        Ok(())
    })();

    if let Err(error) = &cleanup {
        eprintln!("Borrowed URL private build cleanup also failed: {error}");
    }
    if let Err(error) = &recorded {
        eprintln!("Borrowed URL result recording also failed: {error}");
    }
    process?;
    validation?;
    cleanup?;
    recorded?;
    Ok(())
}

fn verify_borrowed_url_docs(stdout: &str, stderr: &str) -> Result<()> {
    let tests: Vec<_> = stdout
        .lines()
        .filter(|line| line.starts_with("test ") && !line.starts_with("test result:"))
        .collect();
    let expected = [
        "test src/platform/macos.rs - platform::macos::BorrowedFileUrls (line 42) - compile fail ... ok",
        "test src/platform/macos.rs - platform::macos::BorrowedFileUrls (line 46) - compile fail ... ok",
        "test src/platform/macos.rs - platform::macos::BorrowedFileUrls (line 51) - compile fail ... ok",
    ];
    if tests != expected || stdout.lines().filter(|line| *line == "running 3 tests").count() != 1
        || stdout.lines().filter(|line| line.starts_with("running ")).count() != 1
        || stdout.lines().filter(|line| line.starts_with("test result:")).count() != 1
        || stdout.lines().filter(|line| line.starts_with("test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 27 filtered out; finished in ")).count() != 1 {
        return Err("Borrowed URL rustdoc selection/count/result mismatch".into());
    }
    let errors: Vec<_> = stderr
        .lines()
        .filter(|line| line.starts_with("error"))
        .collect();
    if errors
        != [
            "error: lifetime may not live long enough",
            "error: aborting due to 1 previous error",
            "error[E0277]: `Rc<()>` cannot be sent between threads safely",
            "error: aborting due to 1 previous error",
            "error[E0277]: `Rc<()>` cannot be shared between threads safely",
            "error: aborting due to 1 previous error",
        ]
    {
        return Err(
            "Borrowed URL doctests did not fail solely for the three required compiler causes"
                .into(),
        );
    }
    // Match source-bound causal facts, not merely a compile-fail exit. These also
    // prove that the imported concrete batch type and its lifetime were resolved.
    for fact in [
        "--> src/platform/macos.rs:45:75",
        "fn retain<'a>(batch: BorrowedFileUrls<'a>) -> BorrowedFileUrls<'static> { batch }",
        "returning this value requires that `'a` must outlive `'static`",
        "--> src/platform/macos.rs:50:16",
        "require_send::<BorrowedFileUrls<'static>>();",
        "the trait `Send` is not implemented for `Rc<()>`",
        "--> src/platform/macos.rs:49:20",
        "fn require_send<T: Send>() {}",
        "--> src/platform/macos.rs:55:16",
        "require_sync::<BorrowedFileUrls<'static>>();",
        "the trait `Sync` is not implemented for `Rc<()>`",
        "--> src/platform/macos.rs:54:20",
        "fn require_sync<T: Sync>() {}",
    ] {
        if stderr.matches(fact).count() != 1 {
            return Err(format!("Missing or duplicate borrowed URL compiler proof: {fact}").into());
        }
    }
    Ok(())
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
        if let Some((value, _tools)) = command.build_version(mach.endian())? {
            if build.is_some() || value.platform.get(mach.endian()) != object::macho::PLATFORM_MACOS
            {
                return Err("Expected exactly one macOS build-version command".into());
            }
            build = Some((
                value.minos.get(mach.endian()).0,
                value.sdk.get(mach.endian()).0,
            ));
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
    pub(crate) fn cargo_directory(&self, root: &Path) -> PathBuf {
        root.join("target/native-cargo").join(&self.host)
    }
    pub(crate) fn command(&self, root: &Path, program: impl AsRef<OsStr>) -> Command {
        let mut command = selected_command(root, &self.developer, program);
        command.env("SDKROOT", &self.sdk);
        command
    }
    pub(crate) fn swift_target(&self) -> String {
        format!("{}-apple-macos{}", self.swift_arch, self.minimum_text)
    }
}
pub(crate) fn load(root: &Path, plan: &BuildPlan) -> Result<MacBuild> {
    crate::campaign::require_stock_topology(root)?;
    let developer = PathBuf::from(format!(
        "/Applications/Xcode_{}.app/Contents/Developer",
        plan.xcode().version()
    ));
    if !developer.is_dir() {
        return Err(format!(
            "Required Xcode {} is unavailable at {}",
            plan.xcode().version(),
            developer.display()
        )
        .into());
    }
    let (xcode, sdk) = selected_xcode(root, &developer, plan.xcode())?;
    let command = |program: &str| selected_command(root, &developer, program);
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
    let target = plan.lookup_target(host)?;
    if target.platform().family() != crate::distribution::OsFamily::Macos {
        return Err("Native macOS compiler host required".into());
    }
    let swift_arch = match target.platform().architecture() {
        crate::distribution::Architecture::Aarch64 => "arm64",
        crate::distribution::Architecture::X86_64 => "x86_64",
    };
    let own_bytes = fs::read(std::env::current_exe()?)?;
    let own = object::File::parse(own_bytes.as_slice())?;
    if macho_architecture(&own)? != target.platform().architecture().as_str() {
        return Err("Running xtask CPU differs from compiler host".into());
    }
    let minimum = packed_version(plan.floors().macos_minimum())?;
    Ok(MacBuild {
        version: plan.version().to_owned(),
        minimum_text: plan.floors().macos_minimum().to_owned(),
        minimum,
        host: host.to_owned(),
        architecture: target.platform().architecture().as_str().to_owned(),
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

fn selected_xcode(
    root: &Path,
    developer: &Path,
    required: &crate::distribution::XcodePin,
) -> Result<(String, PathBuf)> {
    let command = |program: &str| selected_command(root, developer, program);
    let xcode = String::from_utf8(capture(command("/usr/bin/xcodebuild").arg("-version"))?)?;
    if xcode.trim_end()
        != format!(
            "Xcode {}\nBuild version {}",
            required.version(),
            required.build()
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
    Ok((xcode, sdk))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borrowed_url_docs_require_exact_selection_and_causal_compiler_failures() {
        let stdout = "running 3 tests\ntest src/platform/macos.rs - platform::macos::BorrowedFileUrls (line 42) - compile fail ... ok\ntest src/platform/macos.rs - platform::macos::BorrowedFileUrls (line 46) - compile fail ... ok\ntest src/platform/macos.rs - platform::macos::BorrowedFileUrls (line 51) - compile fail ... ok\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 27 filtered out; finished in 0.21s\n";
        let diagnostics = include_str!("../tests/fixtures/borrowed-file-urls-diagnostics.txt");
        verify_borrowed_url_docs(stdout, diagnostics).unwrap();
        for invalid in [
            diagnostics.replace(
                "the trait `Send` is not implemented for `Rc<()>`",
                "unrelated trait failure",
            ),
            diagnostics.replace("src/platform/macos.rs:45:75", "unrelated.rs:45:75"),
            diagnostics.replace("must outlive `'static`", "must outlive `'a`"),
            diagnostics.replace(
                "error[E0277]: `Rc<()>` cannot be shared between threads safely",
                "error[E0433]: unresolved module",
            ),
            format!("{diagnostics}\nerror[E0425]: unrelated missing name\n"),
            diagnostics.repeat(2),
            String::new(),
        ] {
            assert!(verify_borrowed_url_docs(stdout, &invalid).is_err());
        }
        for invalid in [
            stdout.replace("27 filtered", "26 filtered"),
            format!("{stdout}test result: FAILED. 1 failed\n"),
            stdout.replace("line 46", "line 42"),
            stdout.replace("... ok", "... FAILED"),
            String::new(),
        ] {
            assert!(verify_borrowed_url_docs(&invalid, diagnostics).is_err());
        }
    }

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
                bytes.extend(object::macho::LC_UUID.0.to_le_bytes());
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
