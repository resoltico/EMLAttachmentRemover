//! Actual extracted-app interaction on a fresh native hosted Mac through XCTest.
use crate::{Result, controller, macos::MacBuild};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
pub(crate) const EXECUTION_LIMIT: Duration = Duration::from_secs(1860);
const XCODE_RESULTS: &str = "Results.xcresult";
const OWNER_RESULT: &str = "owner-result.json";

mod fixture;
use fixture::*;
mod owner;
use owner::owner_controls;

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

pub(crate) use crate::desktop_evidence::DesktopCase;

pub(crate) fn run_case(
    root: &Path,
    app: &Path,
    build: &MacBuild,
    out: &Path,
    case: DesktopCase,
    source_archive: &xtask::archive::ByteIdentity,
) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Desktop interaction requires a fresh hosted runner; owner desktop/preferences are protected".into());
    }
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let fixture = xtask::archive::create_artifact_directory(&out)?;
    prepare_case_fixture(fixture.path(), case)?;
    let input = prepare_case_input(root, app, fixture.path(), case)?;
    let CaseEvidence {
        process,
        ownership,
        before,
        after,
        summary,
        lifetime_controls,
        control_preferences,
    } = observe_case(root, app, build, &out, fixture.path(), &input, case)?;
    if case.is_publication() {
        let actor = crate::macos_publication_ui::finish(root, build, fixture.path(), &out, case)?;
        let preferences = crate::desktop_preferences::compare(&before, &after, false)?;
        let result = json!({"source":crate::git(root,&["rev-parse","HEAD"])?,"case":case.name(),"test_method":case.method(),"source_archive":source_archive,"app":app,"process":process.evidence(),"lifetime_owner":ownership,"summary":summary,"actor":actor,"persistent_preferences":{"before":before,"after":after,"comparison":preferences},"scope":"nonshipping real-Core publication synchronization with actual native GUI lifecycle/presentation; unchanged shipping archive qualification remains separate"});
        fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
        return Ok(result);
    }
    let (attachments, manifest) = export_attachments(root, build, &out)?;
    if case == DesktopCase::Pagination {
        let observation = pagination_observation(&attachments, &manifest, &ownership)?;
        let preferences = crate::desktop_preferences::compare(&before, &after, false)?;
        let result = json!({"source":crate::git(root,&["rev-parse","HEAD"])?,"case":case.name(),"test_method":case.method(),"source_archive":source_archive,"app":app,"process":process.evidence(),"summary":summary,"lifetime_owner":ownership,"observation":observation,"persistent_preferences":{"before":before,"after":after,"comparison":preferences}});
        fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
        return Ok(result);
    }
    if case == DesktopCase::Appearance {
        let observation = appearance_observation(&attachments, &manifest, &ownership)?;
        let preferences = crate::desktop_preferences::compare(&before, &after, false)?;
        let result = json!({"source":crate::git(root,&["rev-parse","HEAD"])?,"case":case.name(),"test_method":case.method(),"source_archive":source_archive,"app":app,"process":process.evidence(),"lifetime_owner":ownership,"summary":summary,"appearance":observation,"persistent_preferences":{"before":before,"after":after,"comparison":preferences},"scope":"actual extracted shipping app under native light/dark/Increase Contrast changes in a final disposable-host phase; original selectors restored through native Settings UI"});
        fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
        return Ok(result);
    }
    let (picker, receipt) =
        picker_observation(&attachments, &manifest, &input, case, fixture.path())?;
    let preferences = crate::desktop_preferences::compare(&before, &after, true)?;
    if case == DesktopCase::Workflow
        && (ownership["missing_copy_observation"]["held"] != true
            || ownership["missing_copy_observation"]["restored_by_request"] != true
            || ownership["missing_copy_observation"]["copy_left_held"] != false
            || ownership["missing_copy_observation"]["restoration_failure"] != Value::Null)
    {
        return Err(
            "The owned missing-copy navigation control did not complete and restore".into(),
        );
    }
    let scope = if case == DesktopCase::Picker {
        "actual extracted app focused picker scenario: original About/license/close/reopen/Open With prelude, native message/folder and keyboard pickers, independent golden/original bytes, settled worker presentation, exact preference guard and hidden-window normal Quit; full Finder and broader integration are required separately on both macOS15 consumers"
    } else {
        "actual extracted app full workflow: owned launch, About/license/close/reopen/Open With prelude, native pickers, Services, queue8/refusal9/removal, Save/readback/collision, missing-copy restoration, Finder reveal/selection/move/drag, queued destination, independent golden/original checks, settled worker presentation, screenshots, exact preference guard and hidden-window normal Quit"
    };
    let result = json!({"case":case.name(),"test_method":case.method(),"source_archive":source_archive,"source":crate::git(root,&["rev-parse","HEAD"])?,"host":build.host,"host_intervention":"not_requested_by_consumer","app":app,"process":process.evidence(),"lifetime_controls":lifetime_controls,"lifetime_owner":ownership,"summary":summary,"ownership_receipt":receipt,"picker_observation":picker,"persistent_preferences":{"fresh_process_before":before,"fresh_process_after":after,"comparison":preferences,"argument_only_controls":control_preferences},"scope":scope});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}

// Facts are returned only after owned runner settlement and exact XCTest discovery.
// The fixture enclosure stays owned by run_case throughout final evidence checks.
struct CaseEvidence {
    process: xtask::process::ProcessResult,
    ownership: Value,
    before: Value,
    after: Value,
    summary: Value,
    lifetime_controls: Value,
    control_preferences: Value,
}
fn observe_case(
    root: &Path,
    app: &Path,
    build: &MacBuild,
    out: &Path,
    fixture: &Path,
    input: &Path,
    case: DesktopCase,
) -> Result<CaseEvidence> {
    let observer = compile_preference_observers(root, build, out)?;
    let owner = compile_interaction_owner(root, build, out, case)?;
    let before = preference_snapshot(root, app, build, &observer, out, "before")?;
    let (lifetime_controls, control_preferences) = if case.uses_native_pickers() {
        let controls = owner_controls(root, app, build, &owner, out)?;
        let after_controls =
            preference_snapshot(root, app, build, &observer, out, "after-owner-controls")?;
        let comparison = crate::desktop_preferences::compare(&before, &after_controls, false)?;
        fs::write(
            out.join("owner-controls/preferences-result.json"),
            serde_json::to_vec_pretty(
                &json!({"before":before,"after":after_controls,"comparison":comparison}),
            )?,
        )?;
        (controls, comparison)
    } else {
        (Value::Null, Value::Null)
    };
    let (process, ownership) = execute_case(root, build, out, fixture, input, case, &owner)?;
    let after = preference_snapshot(root, app, build, &observer, out, "after")?;
    let summary = observed_xctest_summary(root, build, out)?;
    Ok(CaseEvidence {
        process,
        ownership,
        before,
        after,
        summary,
        lifetime_controls,
        control_preferences,
    })
}
fn case_command(
    root: &Path,
    build: &MacBuild,
    out: &Path,
    input: &Path,
    case: DesktopCase,
    owner: &Path,
) -> std::process::Command {
    let results = out.join(XCODE_RESULTS);
    let mut command = build.command(root, owner);
    let owner_result = out.join(OWNER_RESULT);
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
        .arg(format!(
            "-only-testing:DesktopInteraction/DesktopInteractionTests/{}",
            case.method()
        ))
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
    command
}
fn execute_case(
    root: &Path,
    build: &MacBuild,
    out: &Path,
    fixture: &Path,
    input: &Path,
    case: DesktopCase,
    owner: &Path,
) -> Result<(xtask::process::ProcessResult, Value)> {
    let command = case_command(root, build, out, input, case, owner);
    let owner_result = out.join(OWNER_RESULT);
    // The expanded multi-window scenario includes XCTest's own interruption
    // synchronization. On the native Intel runner individual checks took 60s
    // while the app continued responding. Keep behavior/settlement deadlines in
    // the test unchanged; allow their full sequence to finish before outer cleanup.
    let process = crate::macos_publication_ui::execute(case, fixture, || {
        controller()?.run_with_cooperative_stop(
            command,
            &out.join("xctest.log"),
            EXECUTION_LIMIT,
            xtask::process::COOPERATIVE_STOP_LIMIT,
        )
    })?;
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
    qualify_owner_receipt(&ownership, case)?;

    Ok((process, ownership))
}
fn qualify_owner_receipt(ownership: &Value, case: DesktopCase) -> Result<()> {
    if ownership.get("failure") != Some(&Value::Null)
        || ownership["timed_out"] != false
        || ownership["cancelled"] != false
        || ownership["runner_reaped"] != true
        || ownership["runner_status"] != 0
        || ownership["owned_app_terminated"] != true
        || ownership["forced_app_cleanup"] != false
        || ownership["originals_unchanged"] != true
        || ownership["originals_captured"] != case.originals()
        || ownership.get("source_check_failure") != Some(&Value::Null)
        || ownership["owned_pasteboard_released"] != true
        || ownership["unexpected_application_pids"] != json!([])
    {
        return Err(
            "Desktop lifetime owner did not verify normal app/runner/resource settlement".into(),
        );
    }
    Ok(())
}
fn observed_xctest_summary(root: &Path, build: &MacBuild, out: &Path) -> Result<Value> {
    let results = out.join(XCODE_RESULTS);
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
    Ok(summary)
}
fn export_attachments(
    root: &Path,
    build: &MacBuild,
    out: &Path,
) -> Result<(std::path::PathBuf, Value)> {
    let results = out.join(XCODE_RESULTS);
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
    Ok((attachments, manifest))
}
fn pagination_observation(
    attachments: &Path,
    manifest: &Value,
    ownership: &Value,
) -> Result<Value> {
    let name = attachment_name(
        manifest,
        "DesktopInteractionTests/testOwnedPagination()",
        "native-pagination-observation_",
    )?;
    let path = attachments.join(name);
    if !fs::symlink_metadata(&path)?.is_file() || fs::metadata(&path)?.len() > 4096 {
        return Err("Pagination observation is not an ordinary bounded document".into());
    }
    let observation: Value = serde_json::from_slice(&fs::read(path)?)?;
    if observation["owned_pid"] != ownership["ownership_receipt"]["pid"]
        || observation["items"] != 65
        || observation["removed_parts_in_first_item"] != 65
        || observation["page_transitions"]
            != json!([
                "parts-next",
                "parts-previous",
                "items-next",
                "items-previous"
            ])
        || observation["outputs_match_independent_goldens"] != true
    {
        return Err("Actual enabled pagination observations are incomplete".into());
    }
    Ok(observation)
}
fn appearance_observation(
    attachments: &Path,
    manifest: &Value,
    ownership: &Value,
) -> Result<Value> {
    let name = attachment_name(
        manifest,
        "DesktopInteractionTests/testOwnedNativeAppearance()",
        "native-appearance-observation_",
    )?;
    let path = attachments.join(name);
    if !fs::symlink_metadata(&path)?.is_file() || fs::metadata(&path)?.len() > 16 * 1024 {
        return Err("Native appearance observation is not an ordinary bounded document".into());
    }
    let observation: Value = serde_json::from_slice(&fs::read(path)?)?;
    if observation["schema"] != 1
        || observation["restored"] != true
        || !observation["owned_pid"].as_i64().is_some_and(|pid| pid > 0)
        || !observation["settings_pid"]
            .as_i64()
            .is_some_and(|pid| pid > 0)
        || observation["owned_pid"] != ownership["ownership_receipt"]["pid"]
        || observation["settings_pid"] != ownership["ownership_receipt"]["settings"]["pid"]
        || observation["conditions"] != json!(["light", "dark", "increased-contrast"])
        || observation["native_product_observations"]
            != json!([
                "Interface native appearance: light; increased contrast: false",
                "Interface native appearance: dark; increased contrast: false",
                "Interface native appearance: dark; increased contrast: true"
            ])
        || !matches!(
            observation["original_selector"].as_str(),
            Some("Light" | "Dark" | "Auto")
        )
        || !native_display_settings_match(&observation)
        || ownership["owned_settings_terminated"] != true
        || ownership["unexpected_settings_application_pids"] != json!([])
    {
        return Err("Actual appearance evidence or owned Settings settlement is incomplete".into());
    }
    Ok(observation)
}
fn picker_observation(
    attachments: &Path,
    manifest: &Value,
    input: &Path,
    case: DesktopCase,
    fixture: &Path,
) -> Result<(Value, Value)> {
    let picker_path = attachments.join(attachment_name(
        manifest,
        &format!("DesktopInteractionTests/{}()", case.method()),
        "native-picker-observation_",
    )?);
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
        || picker["scenario"] != case.name()
        || !picker["owned_pid"].as_i64().is_some_and(|pid| pid > 0)
        || picker["owned_pid"] != receipt["pid"]
        || picker["message"] != fixture.join("one.eml").to_string_lossy().as_ref()
        || picker["folder"] != fixture.join("folder-input").to_string_lossy().as_ref()
    {
        return Err("Native picker evidence is missing or belongs to another app/fixture".into());
    }
    Ok((picker, receipt))
}

fn attachment_name<'a>(
    manifest: &'a Value,
    test_identifier: &str,
    prefix: &str,
) -> Result<&'a str> {
    let tests = manifest
        .as_array()
        .ok_or("Invalid XCTest attachment manifest")?;
    let mut matches = Vec::new();
    for test in tests {
        if test["testIdentifier"] != test_identifier {
            continue;
        }
        for attachment in test["attachments"]
            .as_array()
            .ok_or("Missing XCTest attachment list")?
        {
            if attachment["suggestedHumanReadableName"]
                .as_str()
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(".json"))
            {
                let name = attachment["exportedFileName"]
                    .as_str()
                    .ok_or("Missing exported native observation attachment filename")?;
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                    || name == "."
                    || name == ".."
                {
                    return Err(
                        "Native observation attachment is not a plain exported basename".into(),
                    );
                }
                matches.push(name);
            }
        }
    }
    if matches.len() != 1 {
        return Err("Exactly one matching native observation attachment is required".into());
    }
    Ok(matches[0])
}

use crate::desktop_evidence::native_display_settings_match;

#[cfg(test)]
mod tests {
    fn picker_attachment_name(manifest: &serde_json::Value) -> crate::Result<&str> {
        super::attachment_name(
            manifest,
            "DesktopInteractionTests/testOwnedDesktopWorkflowAndNativeIngress()",
            "native-picker-observation_",
        )
    }
    use super::*;

    #[test]
    fn native_display_receipt_requires_actual_states_and_exact_restoration() {
        let valid = json!({
            "original_contrast":false,
            "original_reduce_transparency":true,
            "native_settings_observations": {
                "before":{"increase_contrast":false,"reduce_transparency":true},
                "increased_contrast":{"increase_contrast":true,"reduce_transparency":true},
                "restored":{"increase_contrast":false,"reduce_transparency":true}
            }
        });
        assert!(native_display_settings_match(&valid));
        let mut missing = valid.clone();
        missing
            .as_object_mut()
            .unwrap()
            .remove("native_settings_observations");
        assert!(!native_display_settings_match(&missing));
        let mut unchanged = valid.clone();
        unchanged["native_settings_observations"]["increased_contrast"]["increase_contrast"] =
            json!(false);
        assert!(!native_display_settings_match(&unchanged));
        let mut unrestored = valid.clone();
        unrestored["native_settings_observations"]["restored"]["reduce_transparency"] =
            json!(false);
        assert!(!native_display_settings_match(&unrestored));
        let mut coerced = valid;
        coerced["original_contrast"] = json!("false");
        assert!(!native_display_settings_match(&coerced));
    }

    #[test]
    fn native_observation_requires_the_requested_case_and_attachment_kind() {
        let mut manifest = json!([
            {"testIdentifier":"DesktopInteractionTests/testOwnedNativeAppearance()", "attachments":[
                {"suggestedHumanReadableName":"native-appearance-observation_0.json","exportedFileName":"appearance.json"}
            ]},
            {"testIdentifier":"DesktopInteractionTests/testOwnedDesktopWorkflowAndNativeIngress()", "attachments":[
                {"suggestedHumanReadableName":"native-picker-observation_0.json","exportedFileName":"picker.json"}
            ]}
        ]);
        let focused_identifier =
            format!("DesktopInteractionTests/{}()", DesktopCase::Picker.method());
        assert!(
            attachment_name(&manifest, &focused_identifier, "native-picker-observation_").is_err()
        );
        manifest.as_array_mut().unwrap().push(json!({
            "testIdentifier": focused_identifier,
            "attachments": [{"suggestedHumanReadableName":"native-picker-observation_1.json","exportedFileName":"focused-picker.json"}]
        }));
        assert_eq!(
            attachment_name(&manifest, &focused_identifier, "native-picker-observation_").unwrap(),
            "focused-picker.json"
        );
        assert_eq!(picker_attachment_name(&manifest).unwrap(), "picker.json");
        assert_eq!(
            attachment_name(
                &manifest,
                "DesktopInteractionTests/testOwnedNativeAppearance()",
                "native-appearance-observation_"
            )
            .unwrap(),
            "appearance.json"
        );
        assert!(
            attachment_name(
                &manifest,
                "DesktopInteractionTests/testOwnedNativeAppearance()",
                "native-picker-observation_"
            )
            .is_err()
        );
        assert!(
            attachment_name(
                &manifest,
                "OtherCase/testOwnedNativeAppearance()",
                "native-appearance-observation_"
            )
            .is_err()
        );
    }

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
    #[test]
    fn lifetime_receipt_requires_actual_settlement_and_complete_unchanged_sources() {
        let clean = json!({"failure":null,"timed_out":false,"cancelled":false,
            "runner_reaped":true,"runner_status":0,"owned_app_terminated":true,
            "forced_app_cleanup":false,"originals_unchanged":true,"originals_captured":4,
            "source_check_failure":null,"owned_pasteboard_released":true,
            "unexpected_application_pids":[]});
        qualify_owner_receipt(&clean, DesktopCase::Workflow).unwrap();
        for field in [
            "runner_reaped",
            "owned_app_terminated",
            "originals_unchanged",
            "owned_pasteboard_released",
        ] {
            let mut changed = clean.clone();
            changed[field] = json!(false);
            assert!(
                qualify_owner_receipt(&changed, DesktopCase::Workflow).is_err(),
                "{field}"
            );
        }
        let mut missing = clean.clone();
        missing.as_object_mut().unwrap().remove("failure");
        assert!(qualify_owner_receipt(&missing, DesktopCase::Workflow).is_err());
        let mut failed = clean.clone();
        failed["runner_status"] = json!(1);
        assert!(qualify_owner_receipt(&failed, DesktopCase::Workflow).is_err());
        assert!(qualify_owner_receipt(&clean, DesktopCase::Pagination).is_err());
        let mut pagination = clean;
        pagination["originals_captured"] = json!(69);
        qualify_owner_receipt(&pagination, DesktopCase::Pagination).unwrap();
    }
}
