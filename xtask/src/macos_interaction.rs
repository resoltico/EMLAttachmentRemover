//! Actual extracted-app interaction on a fresh native hosted Mac through XCTest.
use crate::{Result, controller, macos::MacBuild};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

fn owner_controls(
    root: &Path,
    app: &Path,
    build: &MacBuild,
    owner: &Path,
    out: &Path,
) -> Result<Value> {
    use std::{
        io::Write,
        sync::atomic::{AtomicBool, Ordering},
        time::Instant,
    };
    let parent = out.join("owner-controls");
    fs::create_dir(&parent)?;
    let mut compile = build.command(root, "cargo");
    compile.args([
        "build",
        "--locked",
        "--package",
        "xtask",
        "--bin",
        "process-probe",
        "--message-format=json",
    ]);
    let cargo = crate::capture(&mut compile)?;
    fs::write(parent.join("probe-build.jsonl"), &cargo)?;
    let probe = crate::cli_build::named_artifact(
        root,
        &cargo,
        "xtask/Cargo.toml",
        "xtask/tests/support/process_probe.rs",
        "process-probe",
    )?;
    let mut results = Vec::new();
    for case in [
        "deadline",
        "cancel",
        "runner-crash",
        "source-change",
        "force-app",
    ] {
        let case_parent = parent.join(case);
        fs::create_dir(&case_parent)?;
        let enclosure = xtask::archive::create_artifact_directory(&case_parent)?;
        let directory = enclosure.path();
        for name in [
            "one.eml",
            "two.eml",
            "folder-input/folder.eml",
            "service-input/日本語.eml",
        ] {
            let file = directory.join(name);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(
                file,
                include_bytes!("../../tests/contract/mime/inputs/01-gmail-attachment.eml"),
            )?;
        }
        let input = directory.join("input.json");
        let receipt = directory.join("ownership.json");
        let ready = directory.join("ownership.ready.json");
        let result_file = directory.join("owner-result.json");
        fs::write(
            &input,
            serde_json::to_vec_pretty(
                &json!({"app":app,"fixture":directory,"host_scope":"fresh-github-hosted-runner", "owner_control":match case { "deadline"=>"deadline", "force-app"=>"force-app", _=>"normal" }, "runner_ready":directory.join("worker-ready")}),
            )?,
        )?;
        let mut command = build.command(root, owner);
        command
            .current_dir(directory)
            .arg(&input)
            .arg(&receipt)
            .arg(&result_file)
            .arg(&probe)
            .arg(if case == "runner-crash" {
                "abort"
            } else {
                "cooperative-resource"
            })
            .arg(directory);
        let local = xtask::process::Controller::new(root, &std::env::current_exe()?)?;
        let finished = AtomicBool::new(false);
        let (observed, action) = std::thread::scope(|scope| {
            let watching = scope.spawn(|| -> std::result::Result<(), String> {
                let began = Instant::now();
                let mut acted = false;
                while !finished.load(Ordering::Relaxed) {
                    if !controller()
                        .map_err(|error| error.to_string())?
                        .admits_work()
                    {
                        local.request_stop();
                        return Err("Owner control interrupted by caller cancellation".into());
                    }
                    if !acted && ready.is_file() {
                        acted = true;
                        match case {
                            "cancel" => local.request_stop(),
                            "source-change" => {
                                let mut file = fs::OpenOptions::new()
                                    .append(true)
                                    .open(directory.join("one.eml"))
                                    .map_err(|error| error.to_string())?;
                                file.write_all(b"owned change\n")
                                    .map_err(|error| error.to_string())?;
                                drop(file);
                                fs::write(
                                    directory.join("control-release"),
                                    b"release owned probe\n",
                                )
                                .map_err(|error| error.to_string())?;
                            }
                            _ => (),
                        }
                    }
                    if !acted && began.elapsed() > Duration::from_secs(30) {
                        local.request_stop();
                        return Err("Native owner control never established readiness".into());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                if matches!(case, "cancel" | "source-change") && !acted {
                    return Err("Native control stimulus was not delivered".into());
                }
                Ok(())
            });
            let observed = local.run_with_cooperative_stop(
                command,
                &directory.join("owner.log"),
                Duration::from_secs(60),
                xtask::process::COOPERATIVE_STOP_LIMIT,
            );
            finished.store(true, Ordering::Relaxed);
            (
                observed,
                watching
                    .join()
                    .unwrap_or_else(|_| Err("Owner control watcher panicked".into())),
            )
        });
        let read_facts = fs::symlink_metadata(&result_file)
            .map_err(|error| error.to_string())
            .and_then(|metadata| {
                if !metadata.is_file() || metadata.len() > 64 * 1024 {
                    Err("Owner control result is not an ordinary bounded document".into())
                } else {
                    Ok(())
                }
            })
            .and_then(|()| fs::read(&result_file).map_err(|error| error.to_string()))
            .and_then(|bytes| {
                if bytes.len() > 64 * 1024 {
                    Err("Owner control result grew beyond its bound".into())
                } else {
                    Ok(bytes)
                }
            })
            .and_then(|bytes| {
                serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
            });
        fs::write(
            case_parent.join("observed.json"),
            serde_json::to_vec_pretty(
                &json!({"case":case,"owner":read_facts,"process":observed.as_ref().map(|process|process.evidence()).map_err(|error|error.to_string()),"stimulus":action}),
            )?,
        )?;
        action.map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
        let observed = observed?;
        let facts = read_facts.map_err(|error| {
            format!(
                "Owner control {case} result unavailable: {error}; actual process {}; retained {}",
                observed.evidence(),
                directory.display()
            )
        })?;
        if observed.success()
            || !observed.cleanup_complete
            || observed.unexpected_survivors != 0
            || facts.get("failure").is_none_or(Value::is_null)
            || facts["originals_captured"] != 4
            || facts["runner_reaped"] != true
            || facts["owned_app_terminated"] != true
            || facts["owned_pasteboard_released"] != true
            || facts["unexpected_application_pids"] != json!([])
            || facts["originals_unchanged"] != (case != "source-change")
            || facts["timed_out"] != (case == "deadline")
            || facts["cancelled"] != (case == "cancel")
            || observed.cancelled != (case == "cancel")
            || observed.timed_out
        {
            return Err(format!("Native desktop owner control {case} did not prove its required rejection/cleanup; retained {}", directory.display()).into());
        }
        if case == "runner-crash" && facts["runner_status"] != 6 {
            return Err("Native runner crash control did not observe actual SIGABRT".into());
        }
        let expected_failure = match case {
            "deadline" => Some("deadline"),
            "cancel" => Some("cancelled"),
            "force-app" => Some("forcedApplication"),
            _ => None,
        };
        if expected_failure.is_some_and(|reason| facts["failure"] != reason) {
            return Err(
                format!("Native owner control {case} failed for another primary reason").into(),
            );
        }
        if case == "source-change" {
            if facts["source_check_failure"]
                .as_str()
                .is_none_or(str::is_empty)
            {
                return Err(
                    "Native source comparison did not independently record its failure".into(),
                );
            }
        } else if facts.get("source_check_failure") != Some(&Value::Null) {
            return Err("Unchanged-source control has a missing or failed source check".into());
        }
        if case == "source-change" && facts["runner_status"] != 0 {
            return Err("Source-byte control failed before the runner's normal exit".into());
        }
        if case == "force-app" && facts["forced_app_cleanup"] != true {
            return Err("Native retained-app force control was not exercised".into());
        }
        if case == "cancel" && local.admits_work() {
            return Err("Cancelled native owner controller left admission open".into());
        }
        results.push(json!({"case":case,"owner":facts,"process":observed.evidence()}));
    }
    let result = json!({"cases":results,"scope":"real Cocoa app lifetime authority under controlled runner failure/deadline/cancellation/forced app termination/source-byte change; no GUI processing, natural app-crash, or Cocoa EOF claim"});
    fs::write(
        parent.join("result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

fn preference_snapshot(
    root: &Path,
    app: &Path,
    build: &MacBuild,
    tool: &Path,
    out: &Path,
    label: &str,
) -> Result<Value> {
    let log = out.join(format!("preferences-{label}.json"));
    let mut command = build.command(root, tool);
    command
        .arg(app)
        .arg(out.join(format!("preferences-{label}.plist")));
    let process = controller()?.run_separate_output(command, &log, Duration::from_secs(15))?;
    process.require_success(&log)?;
    if fs::metadata(&log)?.len() > 1024 * 1024 {
        return Err("Owned app preference observation exceeds its bound".into());
    }
    let bytes = fs::read(log)?;
    if bytes.len() > 1024 * 1024 {
        return Err("Owned app preference observation grew beyond its bound".into());
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
    for (directory, name) in [
        ("folder-input", "folder.eml"),
        ("service-input", "日本語.eml"),
    ] {
        let directory = fixture.path().join(directory);
        fs::create_dir(&directory)?;
        fs::write(directory.join(name), original)?;
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
    let observation_tests = out.join("DesktopPreferenceSnapshotTests");
    let mut compile = build.command(root, &build.swiftc);
    compile
        .args([
            "-parse-as-library",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-D",
            "PREFERENCE_OBSERVATION_TESTS",
            "-target",
            &build.swift_target(),
            "-sdk",
        ])
        .arg(&build.sdk)
        .arg(root.join("tests/macos/DesktopPreferenceSnapshot.swift"))
        .arg(root.join("tests/macos/DesktopPreferenceSnapshotTests.swift"))
        .arg("-o")
        .arg(&observation_tests);
    crate::macos::checked_run(
        compile,
        &out.join("preference-observation-tests-build.log"),
        120,
    )?;
    crate::macos::checked_run(
        build.command(root, &observation_tests),
        &out.join("preference-observation-tests.log"),
        30,
    )?;
    let owner = out.join("DesktopInteractionOwner");
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
        .arg(root.join("tests/macos/DesktopInteractionOwner.swift"))
        .arg("-o")
        .arg(&owner);
    crate::macos::checked_run(compile, &out.join("interaction-owner-build.log"), 120)?;
    let before = preference_snapshot(root, app, build, &observer, &out, "before")?;
    let lifetime_controls = owner_controls(root, app, build, &owner, &out)?;
    let after_controls =
        preference_snapshot(root, app, build, &observer, &out, "after-owner-controls")?;
    let control_preferences = crate::desktop_preferences::compare(&before, &after_controls, false)?;
    fs::write(
        out.join("owner-controls/preferences-result.json"),
        serde_json::to_vec_pretty(
            &json!({"before":before,"after":after_controls,"comparison":control_preferences}),
        )?,
    )?;
    let results = out.join("Results.xcresult");
    let mut command = build.command(root, &owner);
    let owner_result = out.join("owner-result.json");
    command
        .arg(input.join("input.json"))
        .arg(input.join("ownership.json"))
        .arg(&owner_result)
        .arg(build.developer.join("usr/bin/xcodebuild"));
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
    // The expanded multi-window scenario includes XCTest's own interruption
    // synchronization. On the native Intel runner individual checks took 60s
    // while the app continued responding. Keep behavior/settlement deadlines in
    // the test unchanged; allow their full sequence to finish before outer cleanup.
    let process = controller()?.run_with_cooperative_stop(
        command,
        &out.join("xctest.log"),
        Duration::from_secs(1260),
        xtask::process::COOPERATIVE_STOP_LIMIT,
    )?;
    fs::write(
        out.join("process.json"),
        serde_json::to_vec_pretty(&process.evidence())?,
    )?;
    // Preserve the first failure. Querying a result bundle must not hide a failed
    // test command or attempt work after process admission has closed.
    process.require_success(&out.join("xctest.log"))?;
    if fs::symlink_metadata(&owner_result)?.len() > 64 * 1024 {
        return Err("Desktop lifetime result exceeds its bound".into());
    }
    let ownership: Value = serde_json::from_slice(&fs::read(owner_result)?)?;
    if ownership.get("failure") != Some(&Value::Null)
        || ownership["timed_out"] != false
        || ownership["cancelled"] != false
        || ownership["runner_reaped"] != true
        || ownership["runner_status"] != 0
        || ownership["owned_app_terminated"] != true
        || ownership["forced_app_cleanup"] != false
        || ownership["originals_unchanged"] != true
        || ownership["originals_captured"] != 4
        || ownership.get("source_check_failure") != Some(&Value::Null)
        || ownership["owned_pasteboard_released"] != true
        || ownership["unexpected_application_pids"] != json!([])
    {
        return Err(
            "Desktop lifetime owner did not verify normal app/runner/resource settlement".into(),
        );
    }
    let after = preference_snapshot(root, app, build, &observer, &out, "after")?;
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
    // XCTest carries observations through its native result bundle. Its runner
    // need not gain write authority over the source checkout to emit evidence.
    let attachments = out.join("attachments");
    let mut export = build.command(root, "/usr/bin/xcrun");
    export
        .args(["xcresulttool", "export", "attachments", "--path"])
        .arg(&results)
        .arg("--output-path")
        .arg(&attachments);
    crate::macos::checked_run(export, &out.join("attachment-export.log"), 120)?;
    let manifest_path = attachments.join("manifest.json");
    let metadata = fs::symlink_metadata(&manifest_path)?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("XCTest attachment manifest is not an ordinary bounded document".into());
    }
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let picker_path = attachments.join(picker_attachment_name(&manifest)?);
    let receipt_path = input.join("ownership.json");
    for path in [&picker_path, &receipt_path] {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() > 4096 {
            return Err(
                "Native picker/ownership observation is not an ordinary bounded document".into(),
            );
        }
    }
    let picker: Value = serde_json::from_slice(&fs::read(picker_path)?)?;
    let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
    if picker["schema"] != 1
        || !picker["owned_pid"].as_i64().is_some_and(|pid| pid > 0)
        || picker["owned_pid"] != receipt["pid"]
        || picker["message"] != fixture.path().join("one.eml").to_string_lossy().as_ref()
        || picker["folder"]
            != fixture
                .path()
                .join("folder-input")
                .to_string_lossy()
                .as_ref()
    {
        return Err("Native picker evidence is missing or belongs to another app/fixture".into());
    }
    let preferences = crate::desktop_preferences::compare(&before, &after, true)?;
    let result = json!({"source":crate::git(root,&["rev-parse","HEAD"])?,"host":build.host,"app":app,"process":process.evidence(),"lifetime_controls":lifetime_controls,"lifetime_owner":ownership,"summary":summary,"ownership_receipt":receipt,"picker_observation":picker,"persistent_preferences":{"fresh_process_before":before,"fresh_process_after":after,"comparison":preferences,"argument_only_controls":control_preferences},
        "scope":"actual extracted app: owned NSWorkspace launch, About/version, complete license value and keyboard Edit command, main close/reopen, two-file Open With, native message/folder import, report-space pause/dismiss/resume, real Services with CJK native filename, eight waiting requests/ninth refusal/UI removal, independent golden/original checks, settled worker presentation, screenshots, hidden-window Quit cancel/confirm and destination override non-persistence; remaining GUI paths require separate checks"});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}

fn picker_attachment_name(manifest: &Value) -> Result<&str> {
    let tests = manifest
        .as_array()
        .ok_or("Invalid XCTest attachment manifest")?;
    let mut matches = Vec::new();
    for test in tests {
        if test["testIdentifier"]
            != "DesktopInteractionTests/testOwnedDesktopWorkflowAndNativeIngress()"
        {
            continue;
        }
        for attachment in test["attachments"]
            .as_array()
            .ok_or("Missing XCTest attachment list")?
        {
            if attachment["suggestedHumanReadableName"]
                .as_str()
                .is_some_and(|name| {
                    name.starts_with("native-picker-observation_") && name.ends_with(".json")
                })
            {
                let name = attachment["exportedFileName"]
                    .as_str()
                    .ok_or("Missing exported picker attachment filename")?;
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                    || name == "."
                    || name == ".."
                {
                    return Err("Picker attachment is not a plain exported basename".into());
                }
                matches.push(name);
            }
        }
    }
    if matches.len() != 1 {
        return Err("Exactly one actual picker attachment is required".into());
    }
    Ok(matches[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_attachment_requires_one_matching_test_and_a_plain_filename() {
        let attachment = json!({"suggestedHumanReadableName":"native-picker-observation_0_OWNED.json","exportedFileName":"OWNED.json"});
        let manifest = |attachments: Value| json!([{"testIdentifier":"DesktopInteractionTests/testOwnedDesktopWorkflowAndNativeIngress()","attachments":attachments}]);
        assert_eq!(
            picker_attachment_name(&manifest(json!([attachment]))).unwrap(),
            "OWNED.json"
        );
        assert!(picker_attachment_name(&manifest(json!([]))).is_err());
        assert!(picker_attachment_name(&manifest(json!([attachment, attachment]))).is_err());
        let mut unrelated = manifest(json!([attachment]));
        unrelated[0]["testIdentifier"] = json!("OtherTest/unrelated()");
        assert!(picker_attachment_name(&unrelated).is_err());
        for filename in [
            "../outside.json",
            "/absolute.json",
            "nested/owned.json",
            "..",
            "",
        ] {
            let mut invalid = attachment.clone();
            invalid["exportedFileName"] = json!(filename);
            assert!(picker_attachment_name(&manifest(json!([invalid]))).is_err());
        }
    }
}
