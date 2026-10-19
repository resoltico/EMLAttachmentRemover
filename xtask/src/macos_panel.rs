//! Non-product AppKit boundary observation on the fresh hosted runner only.
use crate::{Result, controller, macos::MacBuild};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
use xtask::{archive::ByteIdentity, process::ProcessResult};

pub(crate) fn observe(root: &Path, build: &MacBuild, out: &Path) -> Result<Value> {
    observe_for(root, build, out, InterventionPurpose::Diagnostic)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InterventionPurpose {
    Diagnostic,
    PickerQualification,
}

fn authorized_job(purpose: InterventionPurpose, workflow: &str, job: &str) -> bool {
    match purpose {
        InterventionPurpose::Diagnostic => {
            workflow == "Native AppKit panel observation" && job == "panel"
        }
        InterventionPurpose::PickerQualification => {
            matches!(workflow, "CI" | "Owner release") && job == "app_picker_modified"
        }
    }
}

fn authorized_qualification_file(workflow: &str, repository: &str, reference: &str) -> bool {
    let file = match workflow {
        "CI" => ".github/workflows/ci.yml",
        "Owner release" => ".github/workflows/release.yml",
        _ => return false,
    };
    !repository.is_empty() && reference.starts_with(&format!("{repository}/{file}@"))
}

pub(crate) fn require_picker_preparation_context() -> Result<()> {
    let workflow = std::env::var("GITHUB_WORKFLOW").unwrap_or_default();
    let job = std::env::var("GITHUB_JOB").unwrap_or_default();
    let workflow_ref = std::env::var("GITHUB_WORKFLOW_REF").unwrap_or_default();
    let workflow_sha = std::env::var("GITHUB_WORKFLOW_SHA").unwrap_or_default();
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || !authorized_job(InterventionPurpose::PickerQualification, &workflow, &job)
        || !authorized_qualification_file(
            &workflow,
            &std::env::var("GITHUB_REPOSITORY").unwrap_or_default(),
            &workflow_ref,
        )
        || workflow_sha.len() != 40
        || !workflow_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!("Controlled picker preparation requires its exact fresh hosted qualification job; observed workflow={workflow:?}, job={job:?}; owner desktop is protected").into());
    }
    Ok(())
}

fn bounded_bytes(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err("Picker input is not an ordinary bounded file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("Picker input grew beyond its bound".into());
    }
    Ok(bytes)
}

pub(crate) fn prepare_picker_host(
    root: &Path,
    build: &MacBuild,
    archive: &Path,
    reference: &Path,
    normal_failure: &Path,
    out: &Path,
) -> Result<()> {
    require_picker_preparation_context()?;
    let mut version = build.command(root, "/usr/bin/sw_vers");
    version.arg("-productVersion");
    let version = String::from_utf8(crate::capture(&mut version)?)?;
    if build.host != "x86_64-apple-darwin" || version.trim().split('.').next() != Some("26") {
        return Err("Controlled picker preparation is confined to native Intel macOS26".into());
    }
    let source = crate::git(root, &["rev-parse", "HEAD"])?;
    let reference_bytes = bounded_bytes(reference, 2 * 1024 * 1024)?;
    let declared: Value = serde_json::from_slice(&reference_bytes)?;
    if declared["source"] != source
        || declared["target"] != "x86_64-apple-darwin"
        || declared["architecture"] != "x86_64"
    {
        return Err("Controlled picker reference does not bind the current native source".into());
    }
    let expected = json!({"source":source,"target":"x86_64-apple-darwin","run":std::env::var("GITHUB_RUN_ID")?,"attempt":std::env::var("GITHUB_RUN_ATTEMPT")?,"workflow_ref":std::env::var("GITHUB_WORKFLOW_REF")?,"workflow_sha":std::env::var("GITHUB_WORKFLOW_SHA")?,"archive":ByteIdentity::of(&bounded_bytes(archive,512*1024*1024)?),"reference":ByteIdentity::of(&reference_bytes)});
    let _ = bounded_bytes(normal_failure, 2 * 1024 * 1024)?;
    let mut admission = build.command(root, "jq");
    admission
        .args(["-e", "-L", ".github/scripts", "--argjson", "e"])
        .arg(expected.to_string())
        .arg("include \"picker-evidence\"; known_failure($e)")
        .arg(normal_failure);
    crate::capture(&mut admission)?;
    // Keep the original probe failure in its receipt. Success here means only
    // that the existing, unchanged preparation sequence was fully admitted.
    let _baseline = observe_for(root, build, out, InterventionPurpose::PickerQualification)?;
    let receipt = out.join("icon-service-experiment/result.json");
    let _ = bounded_bytes(&receipt, 2 * 1024 * 1024)?;
    let mut validation = build.command(root, "jq");
    validation.args(["-e", "-L", ".github/scripts", "--argjson", "e"]).arg(expected.to_string())
        .arg("include \"picker-evidence\"; preparation($e.source) and .purpose==\"picker_qualification_preparation\" and .baseline.run==$e.run and .baseline.attempt==$e.attempt").arg(receipt);
    crate::capture(&mut validation)?;
    Ok(())
}

fn observe_for(
    root: &Path,
    build: &MacBuild,
    out: &Path,
    purpose: InterventionPurpose,
) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Native panel observation requires a fresh hosted Mac; owner desktop/preferences are protected".into());
    }
    let host = crate::native_host::inspect_target(root, &build.host)?;
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
    let source = crate::git(root, &["rev-parse", "HEAD"])?;
    let identity = probe_identity(root, build, out, &app, "baseline")?;
    let log = out.join("panel.log");
    let process = controller()?.run(
        build.command(root, &executable),
        &log,
        Duration::from_secs(90),
    )?;
    let result = json!({"scope":"non-product native AppKit boundary observation; does not qualify product picker", "host":host,"build":build,"entitlements":null,"process":process.evidence(),"identity":identity,"source":source,"run":std::env::var("GITHUB_RUN_ID").ok(),"attempt":std::env::var("GITHUB_RUN_ATTEMPT").ok(),"runner_image":std::env::var("ImageOS").ok(),"runner_image_version":std::env::var("ImageVersion").ok()});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    if !settled(&process) {
        process.require_success(&log)?;
    }
    let diagnostic = (|| -> Result<()> {
        if expected_panel_failure(&process, &fs::read_to_string(&log)?)
            && build.architecture == "x86_64"
        {
            let versions: Vec<_> = host["actual_os"]
                .as_str()
                .ok_or("Missing actual host OS observation")?
                .lines()
                .filter_map(|line| line.strip_prefix("ProductVersion:").map(str::trim))
                .collect();
            let [version] = versions.as_slice() else {
                return Err("Ambiguous actual host OS version; no intervention admitted".into());
            };
            if version.split('.').next() == Some("26") {
                let experiment = out.join("icon-service-experiment");
                fs::create_dir(&experiment)?;
                let mut receipt = json!({"scope":"authorized disposable-host diagnostic OS-service intervention; not owned-agent cleanup, product qualification or an OS exception", "baseline":result,"stage":"admission","original_failure_remains_authoritative":true,"restoration":"not attempted; disposable VM disposal ends environment","causal_limit":"same-VM retry, time and state confound the result; bootout alone differs from the upstream bootout-plus-disable report"});
                receipt["purpose"] = json!(match purpose {
                    InterventionPurpose::Diagnostic => "standalone_diagnostic",
                    InterventionPurpose::PickerQualification => "picker_qualification_preparation",
                });
                if purpose == InterventionPurpose::PickerQualification {
                    receipt["scope"] = json!(
                        "owner-authorized controlled fresh-host preparation only; actual same-frozen-product acceptance is separately required; original probe failure retained"
                    );
                }
                receipt["service_observation_scope"] = json!(
                    "targeted service-state fields only; no Metal assertion or blocking request-chain observation in this experiment"
                );
                let observed = icon_service_experiment(
                    root,
                    build,
                    &app,
                    &identity,
                    &experiment,
                    &mut receipt,
                    purpose,
                );
                if let Err(error) = &observed {
                    receipt["failure"] = json!(error.to_string());
                }
                fs::write(
                    experiment.join("result.json"),
                    serde_json::to_vec_pretty(&receipt)?,
                )?;
                observed?;
            }
        }
        Ok(())
    })();
    if let Err(error) = diagnostic {
        eprintln!(
            "Icon-service diagnostic stopped: {error}. Original panel result remains authoritative at {}",
            out.join("result.json").display()
        );
    }
    // A settled diagnostic failure is evidence, not a reason to skip the product.
    Ok(result)
}

fn expected_panel_failure(process: &ProcessResult, log: &str) -> bool {
    let phases: Vec<_> = log
        .lines()
        .filter_map(|line| line.strip_prefix("Native AppKit panel diagnostic: "))
        .collect();
    process.status.code() == Some(2)
        && settled(process)
        && phases
            == [
                "constructor entry",
                "constructor returned",
                "presentation returned",
                "cancel retained panel; visible=false",
                "callback deadline exceeded",
            ]
}

fn settled(process: &ProcessResult) -> bool {
    !process.timed_out
        && !process.cancelled
        && process.cleanup_complete
        && process.unexpected_survivors == 0
}

fn probe_identity(
    root: &Path,
    build: &MacBuild,
    out: &Path,
    app: &Path,
    phase: &str,
) -> Result<Value> {
    let mut verify = build.command(root, "/usr/bin/codesign");
    verify.args(["--verify", "--strict"]).arg(app);
    crate::macos::checked_run(
        verify,
        &out.join(format!("{phase}-signature-verify.log")),
        30,
    )?;
    let mut display = build.command(root, "/usr/bin/codesign");
    display.args(["--display", "--verbose=4"]).arg(app);
    let log = out.join(format!("{phase}-signature.log"));
    crate::macos::checked_run(display, &log, 30)?;
    let signature = fs::read_to_string(log)?;
    if !signature.lines().any(|line| line == "Signature=adhoc") {
        return Err("Standalone probe signature is not ad hoc".into());
    }
    let mut files = serde_json::Map::new();
    for relative in [
        "Contents/Info.plist",
        "Contents/MacOS/NativePanelDiagnostic",
        "Contents/_CodeSignature/CodeResources",
    ] {
        files.insert(
            relative.into(),
            serde_json::to_value(ByteIdentity::of(&fs::read(app.join(relative))?))?,
        );
    }
    Ok(
        json!({"path":app,"files":files,"signature_display":signature,"scope":"exact signed probe identity; no behavioral qualification"}),
    )
}

fn service_observation(root: &Path, build: &MacBuild, target: &str) -> Result<Value> {
    // Keep raw launchctl output outside uploaded evidence: it can include an
    // inherited environment. Retain only targeted state fields and exit facts.
    let temporary = tempfile::tempdir()?;
    let mut command = build.command(root, "/bin/launchctl");
    command.args(["print", target]);
    let process = match controller()?.run(
        command,
        &temporary.path().join("service.log"),
        Duration::from_secs(15),
    ) {
        Ok(process) => process,
        Err(error) => {
            let _ = temporary.keep();
            return Err(error);
        }
    };
    if !settled(&process) {
        let log = temporary.keep().join("service.log");
        process.require_success(&log)?;
        return Err("Service observation did not settle; raw private diagnostic retained".into());
    }
    let text = fs::read_to_string(temporary.path().join("service.log"))?;
    let facts: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| {
            [
                "state = ",
                "pid = ",
                "runs = ",
                "successive crashes = ",
                "last terminating signal = ",
                "last exit code = ",
            ]
            .iter()
            .any(|key| line.starts_with(key))
        })
        .map(str::to_owned)
        .collect();
    Ok(
        json!({"target":target,"process":process.evidence(),"entry_observed":process.status.success(),"selected_state_fields":facts,"scope":"single targeted launchctl observation; no continuous availability, service ownership or restoration claim"}),
    )
}

fn icon_service_experiment(
    root: &Path,
    build: &MacBuild,
    app: &Path,
    identity: &Value,
    out: &Path,
    receipt: &mut Value,
    purpose: InterventionPurpose,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if purpose == InterventionPurpose::PickerQualification {
        require_picker_preparation_context()?;
    }
    if !authorized_job(
        purpose,
        &std::env::var("GITHUB_WORKFLOW").unwrap_or_default(),
        &std::env::var("GITHUB_JOB").unwrap_or_default(),
    ) || [
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "ImageOS",
        "ImageVersion",
    ]
    .iter()
    .any(|key| !matches!(std::env::var(key), Ok(value) if !value.is_empty()))
        || !controller()?.admits_work()
    {
        return Err(
            "OS-service intervention requires its exact authorized job and healthy controller"
                .into(),
        );
    }
    let uid = rustix::process::getuid();
    if uid.is_root()
        || rustix::process::geteuid() != uid
        || fs::metadata("/dev/console")?.uid() != uid.as_raw()
    {
        return Err("Current process is not the observed non-root GUI user; no alternate domain is authorized".into());
    }
    let target = format!(
        "gui/{}/com.apple.iconservices.iconservicesagent",
        uid.as_raw()
    );
    receipt["target"] = json!(target);
    receipt["gui_uid"] = json!(uid.as_raw());
    receipt["stage"] = json!("unchanged-repeat");
    let before_repeat = probe_identity(root, build, out, app, "repeat-before")?;
    if before_repeat != *identity {
        return Err("Probe identity changed before unchanged repeat".into());
    }
    let executable = app.join("Contents/MacOS/NativePanelDiagnostic");
    let repeat = controller()?.run(
        build.command(root, &executable),
        &out.join("unchanged-repeat.log"),
        Duration::from_secs(90),
    )?;
    receipt["unchanged_repeat"] = json!({"identity":before_repeat,"process":repeat.evidence()});
    if !expected_panel_failure(
        &repeat,
        &fs::read_to_string(out.join("unchanged-repeat.log"))?,
    ) || !controller()?.admits_work()
    {
        return Err(
            "Unchanged repeat did not retain the settled known panel failure; no further command admitted"
                .into(),
        );
    }
    let after_repeat = probe_identity(root, build, out, app, "repeat-after")?;
    receipt["unchanged_repeat"]["identity_after"] = after_repeat.clone();
    if after_repeat != *identity || !controller()?.admits_work() {
        return Err("Unchanged repeat did not retain the same settled panel failure; no intervention admitted".into());
    }
    receipt["stage"] = json!("service-before");
    receipt["service_before"] = service_observation(root, build, &target)?;
    if receipt["service_before"]["entry_observed"] != true || !controller()?.admits_work() {
        return Err(
            "Targeted GUI-user service entry was not observed; no intervention admitted".into(),
        );
    }
    receipt["stage"] = json!("bootout");
    if fs::metadata("/dev/console")?.uid() != uid.as_raw() || !controller()?.admits_work() {
        return Err(
            "GUI-user ownership or controller admission changed; no intervention admitted".into(),
        );
    }
    let mut command = build.command(root, "/bin/launchctl");
    command.args(["bootout", &target]);
    let log = out.join("bootout.log");
    let process = controller()?.run(command, &log, Duration::from_secs(20))?;
    receipt["bootout"] = json!({"program":"/bin/launchctl","arguments":["bootout",target],"process":process.evidence(),"scope":"controller owns launchctl invocation, not the OS service"});
    process.require_success(&log)?;
    receipt["stage"] = json!("service-after");
    receipt["service_after"] = service_observation(root, build, &target)?;
    let before = probe_identity(root, build, out, app, "intervention-before")?;
    if before != *identity || !controller()?.admits_work() {
        return Err("Unchanged probe or healthy controller unavailable after intervention".into());
    }
    receipt["stage"] = json!("intervention-probe");
    let process = controller()?.run(
        build.command(root, &executable),
        &out.join("intervention-probe.log"),
        Duration::from_secs(90),
    )?;
    receipt["intervention_probe"] = json!({"identity":before,"process":process.evidence()});
    if !settled(&process) || !controller()?.admits_work() {
        return Err("Intervention probe settlement or admission is unavailable; no further command admitted".into());
    }
    receipt["intervention_probe"]["identity_after"] =
        probe_identity(root, build, out, app, "intervention-after")?;
    if receipt["intervention_probe"]["identity_after"] != *identity {
        return Err("Probe identity changed during intervention run".into());
    }
    receipt["stage"] = json!("observed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    const PANEL_FAILURE: &str = "Native AppKit panel diagnostic: constructor entry\nNative AppKit panel diagnostic: constructor returned\nNative AppKit panel diagnostic: presentation returned\nNative AppKit panel diagnostic: cancel retained panel; visible=false\nNative AppKit panel diagnostic: callback deadline exceeded\n";

    fn settled_exit(code: i32) -> ProcessResult {
        ProcessResult {
            status: std::process::ExitStatus::from_raw(code << 8),
            timed_out: false,
            cancelled: false,
            unexpected_survivors: 0,
            cleanup_complete: true,
            elapsed: Duration::ZERO,
            cleanup_elapsed: Duration::ZERO,
        }
    }

    #[test]
    fn intervention_purposes_admit_only_their_exact_jobs() {
        assert!(authorized_qualification_file(
            "CI",
            "owner/repo",
            "owner/repo/.github/workflows/ci.yml@refs/pull/39/merge"
        ));
        assert!(authorized_qualification_file(
            "Owner release",
            "owner/repo",
            "owner/repo/.github/workflows/release.yml@refs/heads/main"
        ));
        assert!(!authorized_qualification_file(
            "CI",
            "owner/repo",
            "owner/repo/.github/workflows/unrelated.yml@main"
        ));
        assert!(!authorized_qualification_file(
            "CI",
            "owner/repo",
            "other/repo/.github/workflows/ci.yml@main"
        ));
        assert!(authorized_job(
            InterventionPurpose::Diagnostic,
            "Native AppKit panel observation",
            "panel"
        ));
        for workflow in ["CI", "Owner release"] {
            assert!(authorized_job(
                InterventionPurpose::PickerQualification,
                workflow,
                "app_picker_modified"
            ));
            assert!(!authorized_job(
                InterventionPurpose::Diagnostic,
                workflow,
                "app_picker_modified"
            ));
            for job in ["app_picker_normal", "app_picker", "panel", "app", ""] {
                assert!(!authorized_job(
                    InterventionPurpose::PickerQualification,
                    workflow,
                    job
                ));
            }
        }
        assert!(!authorized_job(
            InterventionPurpose::PickerQualification,
            "Native AppKit panel observation",
            "panel"
        ));
        assert!(!authorized_job(
            InterventionPurpose::PickerQualification,
            "unrelated",
            "app_picker_modified"
        ));
    }

    #[test]
    fn service_experiment_requires_the_observed_panel_failure_not_arbitrary_exit_two() {
        assert!(expected_panel_failure(&settled_exit(2), PANEL_FAILURE));
        assert!(!expected_panel_failure(
            &settled_exit(2),
            "constructor failed\n"
        ));
        assert!(!expected_panel_failure(&settled_exit(0), PANEL_FAILURE));
        assert!(!expected_panel_failure(&settled_exit(3), PANEL_FAILURE));
        assert!(!expected_panel_failure(
            &settled_exit(2),
            &PANEL_FAILURE.replace("visible=false", "visible=true")
        ));
    }

    #[test]
    fn service_experiment_rejects_unsettled_or_cancelled_baselines() {
        for field in 0..4 {
            let mut process = settled_exit(2);
            match field {
                0 => process.timed_out = true,
                1 => process.cancelled = true,
                2 => process.unexpected_survivors = 1,
                _ => process.cleanup_complete = false,
            }
            assert!(!expected_panel_failure(&process, PANEL_FAILURE));
        }
    }
}
