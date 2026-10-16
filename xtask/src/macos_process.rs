//! Real native helper/pipe fixtures; GUI and archive qualification are separate.
use crate::macos::{checked_run, cli_artifact, fixture_identity, selected_command};
use crate::{Result, capture};
use std::{fs, path::Path};
fn fixture_plist(version: &str, minimum: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>io.github.resoltico.emlattachmentremover.process-fixture</string><key>CFBundleExecutable</key><string>ProcessIntegration</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>{version}</string><key>CFBundleVersion</key><string>{version}</string><key>LSMinimumSystemVersion</key><string>{minimum}</string></dict></plist>"
    )
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
    let build = crate::macos::load(root)?;
    let developer = &build.developer;
    let sdk = &build.sdk;
    let swiftc = &build.swiftc;
    let host = build.host.as_str();
    let minimum = build.minimum;
    let command = |program: &str| build.command(root, program);
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
            .env("MACOSX_DEPLOYMENT_TARGET", &build.minimum_text)
            .env("SDKROOT", sdk)
            .env("CARGO_TARGET_DIR", work.join("cargo")),
    )?;
    fs::write(work.join("cargo.jsonl"), &cargo_output)?;
    let helper = cli_artifact(root, &cargo_output)?;
    let helper_identity = fixture_identity(&fs::read(&helper)?, &build.architecture, minimum)?;
    let swift_target = build.swift_target();
    let mut model_sources = fs::read_dir(root.join("macos-app/Sources/RunModel"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    model_sources.retain(|path| path.extension().is_some_and(|value| value == "swift"));
    model_sources.sort();
    if model_sources.is_empty() {
        return Err("No production RunModel sources found".into());
    }
    let mut model = selected_command(root, developer, swiftc);
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
        .arg(sdk)
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
    let mut components = selected_command(root, developer, swiftc);
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
        .arg(sdk)
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
    let mut driver = selected_command(root, developer, swiftc);
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
        .arg(sdk)
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
        &build.architecture,
        minimum,
    )?;
    fs::write(
        app.join("Contents/Info.plist"),
        fixture_plist(&build.version, &build.minimum_text),
    )?;
    let sources = work.join("sources");
    fs::create_dir(&sources)?;
    let mut execute = selected_command(root, developer, macos.join("ProcessIntegration"));
    execute
        .current_dir(root)
        .env("DEVELOPER_DIR", developer)
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
            if build.version == "0.0.0" {
                "0.0.1"
            } else {
                "0.0.0"
            },
            true,
        ),
        ("helper-missing", build.version.as_str(), false),
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
            fixture_plist(version, &build.minimum_text),
        )?;
        let sources = work.join(format!("{mode}-sources"));
        fs::create_dir(&sources)?;
        let mut execute = selected_command(root, developer, &executable);
        execute
            .current_dir(root)
            .env("DEVELOPER_DIR", developer)
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
            &serde_json::json!({"version":build.version,"rustc":build.rust_identity,"xcode":build.xcode_identity,"swift":build.swift_identity,"sdk":{"path":sdk,"version":build.sdk_version},"helper":helper_identity,"driver":driver_identity,"scope":"owned process boundary; no GUI/archive qualification"}),
        )?,
    )?;
    eprintln!(
        "Native process fixture passed; {}",
        work.join("ordinary.log").display()
    );
    Ok(())
}
