//! Actual extracted-app interaction on a fresh native hosted Mac through XCTest.
use crate::{Result, controller, macos::MacBuild};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

fn preference_snapshot(
    root: &Path,
    app: &Path,
    build: &MacBuild,
    tool: &Path,
    out: &Path,
    label: &str,
) -> Result<Value> {
    let log = out.join(format!("preferences-{label}.plist"));
    let mut command = build.command(root, tool);
    command.arg(app);
    let process = controller()?.run_separate_output(command, &log, Duration::from_secs(15))?;
    process.require_success(&log)?;
    let mut convert = build.command(root, "/usr/bin/plutil");
    convert.args(["-convert", "json", "-o", "-"]).arg(&log);
    let bytes = crate::capture(&mut convert)?;
    if bytes.len() > 1024 * 1024 {
        return Err("Owned app preference observation exceeds its bound".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub(crate) fn run(root: &Path, app: &Path, build: &MacBuild, out: &Path) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Desktop interaction requires a fresh hosted runner; owner desktop/preferences are protected".into());
    }
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let fixture = xtask::archive::create_artifact_directory(&out)?;
    let original = include_bytes!("../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    for name in ["one.eml", "two.eml"] {
        fs::write(fixture.path().join(name), original)?;
    }
    // One native job owns this new input directory. The test derives its location
    // from its actual source path; there is no shipping automation mode or saved
    // preference transport. Refuse accidental reuse instead of replacing inputs.
    let input = root.join("target/desktop-interaction");
    fs::create_dir(&input)?;
    fs::write(
        input.join("input.json"),
        serde_json::to_vec_pretty(&json!({
            "app":app,"fixture":fixture.path(),"host_scope":"fresh-github-hosted-runner"
        }))?,
    )?;
    let observer = out.join("DesktopPreferenceSnapshot");
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
        .arg(root.join("tests/macos/DesktopPreferenceSnapshot.swift"))
        .arg("-o")
        .arg(&observer);
    crate::macos::checked_run(compile, &out.join("preference-observer-build.log"), 120)?;
    let before = preference_snapshot(root, app, build, &observer, &out, "before")?;
    let results = out.join("Results.xcresult");
    let mut command = build.command(root, build.developer.join("usr/bin/xcodebuild"));
    // Swift Build's documented default connection mode honors this per-process
    // override. Keep the build service within xcodebuild's owned lifetime rather
    // than accepting a persistent child or weakening generic survivor rejection.
    command.env("SWIFTBUILD_LAUNCH_IN_PROCESS", "YES");
    command
        .args([
            "-project",
            "tests/macos/DesktopInteraction.xcodeproj",
            "-scheme",
            "DesktopInteraction",
            "-destination",
            "platform=macOS",
            "-parallel-testing-enabled",
            "NO",
            "-derivedDataPath",
        ])
        .arg(out.join("build"))
        .arg("-resultBundlePath")
        .arg(&results)
        .arg(format!("ARCHS={}", build.swift_arch))
        .args([
            "ONLY_ACTIVE_ARCH=YES",
            "ALWAYS_SEARCH_USER_PATHS=NO",
            "test",
        ]);
    let process = controller()?.run(command, &out.join("xctest.log"), Duration::from_secs(300))?;
    fs::write(
        out.join("process.json"),
        serde_json::to_vec_pretty(&process.evidence())?,
    )?;
    // Preserve the first failure. Querying a result bundle must not hide a failed
    // test command or attempt work after process admission has closed.
    process.require_success(&out.join("xctest.log"))?;
    let after = preference_snapshot(root, app, build, &observer, &out, "after")?;
    if before != after {
        return Err(
            "Actual app persistent preferences changed across the argument-domain-only GUI run"
                .into(),
        );
    }
    let mut summary = build.command(root, "/usr/bin/xcrun");
    summary
        .args(["xcresulttool", "get", "test-results", "summary", "--path"])
        .arg(&results);
    let bytes = crate::capture(&mut summary)?;
    if bytes.len() > 1024 * 1024 {
        return Err("XCTest summary exceeds its document bound".into());
    }
    let summary: Value = serde_json::from_slice(&bytes)?;
    fs::write(
        out.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    if summary["totalTestCount"] != 1
        || summary["passedTests"] != 1
        || summary["failedTests"] != 0
        || summary["skippedTests"] != 0
    {
        return Err("Actual desktop test discovery/execution is incomplete or failed".into());
    }
    let result = json!({"source":crate::git(root,&["rev-parse","HEAD"])?,"host":build.host,"app":app,"process":process.evidence(),"summary":summary,"persistent_preferences":{"fresh_process_before":before,"fresh_process_after":after,"unchanged":true},
        "scope":"actual extracted app: owned NSWorkspace launch, About/version, complete license value and keyboard Edit command, main close/reopen, two-file native Open With batch, two independent golden/original checks, terminal worker settlement rendered, native screenshots, Quit cancel/confirm and destination override non-persistence; other GUI paths remain separate required checks"});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}
