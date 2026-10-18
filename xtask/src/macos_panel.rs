//! Non-product AppKit boundary observation on the fresh hosted runner only.
use crate::{Result, controller, macos::MacBuild};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

pub(crate) fn observe(root: &Path, build: &MacBuild, out: &Path) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Native panel observation requires a fresh hosted Mac; owner desktop/preferences are protected".into());
    }
    fs::create_dir(out)?;
    let app = out.join("NativePanelDiagnostic.app");
    let contents = app.join("Contents");
    let executable = contents.join("MacOS/NativePanelDiagnostic");
    fs::create_dir_all(
        executable
            .parent()
            .ok_or("Diagnostic executable has no parent")?,
    )?;
    let info = contents.join("Info.plist");
    fs::write(&info, br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>org.emlattachmentremover.tests.NativePanelDiagnostic</string>
<key>CFBundleExecutable</key><string>NativePanelDiagnostic</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
</dict></plist>
"#)?;
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
        .arg(root.join("tests/macos/NativePanelDiagnostic.swift"))
        .arg("-o")
        .arg(&executable);
    crate::macos::checked_run(compile, &out.join("build.log"), 120)?;
    let mut sign = build.command(root, "/usr/bin/codesign");
    sign.args(["--sign", "-", "--timestamp=none"]).arg(&app);
    crate::macos::checked_run(sign, &out.join("sign.log"), 30)?;
    let mut inspect = build.command(root, "/usr/bin/codesign");
    inspect
        .args(["--display", "--entitlements", "-", "--xml"])
        .arg(&executable);
    let entitlements = out.join("entitlements.plist");
    controller()?
        .run_separate_output(inspect, &entitlements, Duration::from_secs(30))?
        .require_success(&entitlements)?;
    if !fs::read(&entitlements)?.is_empty() {
        return Err(
            "Native diagnostic unexpectedly contains entitlements; retained for inspection".into(),
        );
    }
    let log = out.join("panel.log");
    let process = controller()?.run(
        build.command(root, &executable),
        &log,
        Duration::from_secs(90),
    )?;
    let result = json!({"scope":"non-product native AppKit boundary observation; does not qualify product picker", "entitlements":null,"process":process.evidence()});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    if process.timed_out
        || process.cancelled
        || process.unexpected_survivors != 0
        || !process.cleanup_complete
    {
        process.require_success(&log)?;
    }
    // A settled diagnostic failure is evidence, not a reason to skip the product.
    Ok(result)
}
