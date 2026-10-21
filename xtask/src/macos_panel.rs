//! Non-product AppKit boundary observation on the fresh hosted runner only.
use crate::{Result, controller, macos::MacBuild};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
use xtask::{archive::ByteIdentity, process::ProcessResult};

fn bounded_bytes(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err("Panel diagnostic input is not an ordinary bounded file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("Panel diagnostic input grew beyond its bound".into());
    }
    Ok(bytes)
}

fn authorized_job(workflow: &str, job: &str) -> bool {
    workflow == "Native AppKit panel observation" && job == "panel"
}

pub(crate) fn observe(root: &Path, build: &MacBuild, out: &Path) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Native panel observation requires a fresh hosted Mac; owner desktop/preferences are protected".into());
    }
    let host = crate::native_host::inspect_target(root, &build.host)?;
    fs::create_dir(out)?;
    let app = build_panel_diagnostic(root, build, out)?;
    let executable = app.join("Contents/MacOS/NativePanelDiagnostic");
    let source = crate::git(root, &["rev-parse", "HEAD"])?;
    let identity = probe_identity(root, build, out, &app, "baseline")?;
    let log = out.join("panel.log");
    let process = controller()?.run(
        build.command(root, &executable),
        &log,
        Duration::from_secs(90),
    )?;
    let after = probe_identity(root, build, out, &app, "baseline-after");
    let mut result = json!({"scope":"non-product native AppKit boundary observation; does not qualify product picker", "host":host,"build":build,"entitlements":null,"identity":identity,"source":source,"run":std::env::var("GITHUB_RUN_ID").ok(),"attempt":std::env::var("GITHUB_RUN_ATTEMPT").ok(),"runner_image":std::env::var("ImageOS").ok(),"runner_image_version":std::env::var("ImageVersion").ok()});
    let observation = record_probe_result(&mut result, &process, &log, after, &identity);
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    let observation = observation?;
    let versions: Vec<_> = host["actual_os"]
        .as_str()
        .ok_or("Missing actual host OS observation")?
        .lines()
        .filter_map(|line| line.strip_prefix("ProductVersion:").map(str::trim))
        .collect();
    let [version] = versions.as_slice() else {
        return Err("Ambiguous actual host OS version; no preparation admitted".into());
    };
    if build.architecture == "x86_64" && version.split('.').next() == Some("26") {
        let prepared = observe_preparation(root, build, out, &app, &result, observation);
        if let Err(error) = &prepared {
            eprintln!(
                "Panel preparation stopped: {error}. Original panel result remains authoritative at {}",
                out.join("result.json").display()
            );
        }
    }
    // Standalone diagnostics retain the original baseline's authority even if a repeat recovers.
    Ok(result)
}

fn observe_preparation(
    root: &Path,
    build: &MacBuild,
    out: &Path,
    app: &Path,
    baseline: &Value,
    observation: PanelObservation,
) -> Result<()> {
    let experiment = out.join("icon-service-experiment");
    fs::create_dir(&experiment)?;
    let mut receipt = json!({"scope":"authorized disposable-host panel preparation; never product qualification or a healthy unmodified-host compatibility claim", "baseline":baseline,"stage":"admission"});
    if observation == PanelObservation::KnownPanelFailure {
        receipt["original_failure_remains_authoritative"] = json!(true);
    }
    receipt["purpose"] = json!("standalone_diagnostic");
    let prepared = prepare_observed_host(root, build, app, &experiment, &mut receipt, observation);
    if let Err(error) = &prepared {
        receipt["failure"] = json!(error.to_string());
    }
    fs::write(
        experiment.join("result.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    prepared
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum PanelObservation {
    Healthy,
    KnownPanelFailure,
}

fn classify_panel(process: &ProcessResult, phases: &[&str]) -> Result<PanelObservation> {
    if !settled(process) {
        return Err("Native panel process did not settle cleanly".into());
    }
    match (process.status.code(), phases) {
        (
            Some(0),
            [
                "constructor entry",
                "constructor returned",
                "presentation returned",
                "cancel retained panel; visible=true",
                "callback response 0",
            ],
        ) => Ok(PanelObservation::Healthy),
        (
            Some(2),
            [
                "constructor entry",
                "constructor returned",
                "presentation returned",
                "cancel retained panel; visible=false",
                "callback deadline exceeded",
            ],
        ) => Ok(PanelObservation::KnownPanelFailure),
        _ => Err(
            "Native panel result does not match an exact healthy or known-failure observation"
                .into(),
        ),
    }
}

fn record_probe_result(
    record: &mut Value,
    process: &ProcessResult,
    log: &Path,
    after: Result<Value>,
    identity: &Value,
) -> Result<PanelObservation> {
    record["process"] = process.evidence();
    record["identity_after"] = after.as_ref().ok().cloned().unwrap_or(Value::Null);
    if let Err(error) = &after {
        record["identity_after_error"] = json!(error.to_string());
    }
    let bytes = bounded_bytes(log, 2 * 1024 * 1024)?;
    let text = std::str::from_utf8(&bytes)?;
    let phases: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix("Native AppKit panel diagnostic: "))
        .collect();
    record["phases"] = json!(phases);
    let observation = classify_panel(process, &phases);
    record["observation"] = observation
        .as_ref()
        .ok()
        .map_or(Value::Null, |value| json!(value));
    if let Err(error) = &observation {
        record["observation_error"] = json!(error.to_string());
    }
    if after? != *identity {
        return Err("Native panel probe identity changed during execution".into());
    }
    observation
}

fn build_panel_diagnostic(root: &Path, build: &MacBuild, out: &Path) -> Result<std::path::PathBuf> {
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
    Ok(app)
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

fn prepare_observed_host(
    root: &Path,
    build: &MacBuild,
    app: &Path,
    out: &Path,
    receipt: &mut Value,
    observation: PanelObservation,
) -> Result<()> {
    let uid = require_preparation_authority()?;
    receipt["gui_uid"] = json!(uid);
    let identity = receipt["baseline"]["identity"].clone();
    if observation == PanelObservation::Healthy {
        return finish_unmodified_preparation(receipt, uid);
    }
    if unchanged_probe_repeat(root, build, app, &identity, out, receipt)?
        == PanelObservation::Healthy
    {
        return finish_unmodified_preparation(receipt, uid);
    }
    service_intervention(root, build, app, out, receipt, &identity, uid)
}

fn require_preparation_authority() -> Result<u32> {
    if !authorized_job(
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
            "Panel preparation requires its exact authorized job and healthy controller".into(),
        );
    }
    let uid = rustix::process::getuid().as_raw();
    require_gui_owner(uid)?;
    Ok(uid)
}

fn require_gui_owner(uid: u32) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let actual = rustix::process::getuid();
    if actual.is_root()
        || actual.as_raw() != uid
        || rustix::process::geteuid() != actual
        || fs::metadata("/dev/console")?.uid() != uid
    {
        return Err("Current process is not the observed non-root GUI user; no alternate domain is authorized".into());
    }
    Ok(())
}

fn service_intervention(
    root: &Path,
    build: &MacBuild,
    app: &Path,
    out: &Path,
    receipt: &mut Value,
    identity: &Value,
    uid: u32,
) -> Result<()> {
    receipt["restoration"] = json!("not attempted; disposable VM disposal ends environment");
    receipt["causal_limit"] = json!(
        "same-VM retry, time and state confound the result; bootout alone differs from the upstream bootout-plus-disable report"
    );
    receipt["service_observation_scope"] = json!(
        "targeted service-state fields only; no Metal assertion or blocking request-chain observation in this experiment"
    );
    let target = format!("gui/{uid}/com.apple.iconservices.iconservicesagent");
    receipt["target"] = json!(target);
    let executable = app.join("Contents/MacOS/NativePanelDiagnostic");
    receipt["stage"] = json!("service-before");
    receipt["service_before"] = service_observation(root, build, &target)?;
    if receipt["service_before"]["entry_observed"] != true || !controller()?.admits_work() {
        return Err(
            "Targeted GUI-user service entry was not observed; no intervention admitted".into(),
        );
    }
    receipt["stage"] = json!("bootout");
    require_gui_owner(uid)?;
    if !controller()?.admits_work() {
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
    receipt["intervention_probe"] = json!({"identity":before});
    let after = probe_identity(root, build, out, app, "intervention-after");
    let observed = record_probe_result(
        &mut receipt["intervention_probe"],
        &process,
        &out.join("intervention-probe.log"),
        after,
        identity,
    )?;
    if observed != PanelObservation::Healthy || !controller()?.admits_work() {
        return Err("Intervention probe did not demonstrate clean visible cancellation; no product admitted".into());
    }
    receipt["outcome"] = json!("service_intervention");
    receipt["stage"] = json!("observed");
    Ok(())
}

fn finish_unmodified_preparation(receipt: &mut Value, uid: u32) -> Result<()> {
    require_gui_owner(uid)?;
    if !controller()?.admits_work() {
        return Err("Controller admission closed before unmodified preparation completed".into());
    }
    receipt["outcome"] = json!("unmodified");
    receipt["restoration"] =
        json!("not applicable; no service observation or intervention performed");
    receipt["stage"] = json!("observed");
    Ok(())
}

fn unchanged_probe_repeat(
    root: &Path,
    build: &MacBuild,
    app: &Path,
    identity: &Value,
    out: &Path,
    receipt: &mut Value,
) -> Result<PanelObservation> {
    receipt["stage"] = json!("unchanged-repeat");
    let before = probe_identity(root, build, out, app, "repeat-before")?;
    receipt["unchanged_repeat"] = json!({"identity":before});
    if before != *identity {
        return Err("Probe identity changed before unchanged repeat".into());
    }
    let process = controller()?.run(
        build.command(root, app.join("Contents/MacOS/NativePanelDiagnostic")),
        &out.join("unchanged-repeat.log"),
        Duration::from_secs(90),
    )?;
    let after = probe_identity(root, build, out, app, "repeat-after");
    let observation = record_probe_result(
        &mut receipt["unchanged_repeat"],
        &process,
        &out.join("unchanged-repeat.log"),
        after,
        identity,
    )?;
    if !controller()?.admits_work() {
        return Err("Unchanged repeat did not retain healthy controller admission".into());
    }
    Ok(observation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    const PANEL_FAILURE: &str = "Native AppKit panel diagnostic: constructor entry\nNative AppKit panel diagnostic: constructor returned\nNative AppKit panel diagnostic: presentation returned\nNative AppKit panel diagnostic: cancel retained panel; visible=false\nNative AppKit panel diagnostic: callback deadline exceeded\n";

    const PANEL_HEALTHY: &str = "Native AppKit panel diagnostic: constructor entry\nNative AppKit panel diagnostic: constructor returned\nNative AppKit panel diagnostic: presentation returned\nNative AppKit panel diagnostic: cancel retained panel; visible=true\nNative AppKit panel diagnostic: callback response 0\n";

    fn classify_log(process: &ProcessResult, log: &str) -> Result<PanelObservation> {
        let phases: Vec<_> = log
            .lines()
            .filter_map(|line| line.strip_prefix("Native AppKit panel diagnostic: "))
            .collect();
        classify_panel(process, &phases)
    }

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
    fn standalone_diagnostic_admits_only_its_own_job() {
        assert!(authorized_job("Native AppKit panel observation", "panel"));
        for (workflow, job) in [
            ("CI", "panel"),
            ("Owner release", "panel"),
            ("CI", "app_picker"),
            ("Native AppKit panel observation", "app_picker"),
            ("unrelated", "panel"),
            ("Native AppKit panel observation", ""),
        ] {
            assert!(!authorized_job(workflow, job));
        }
    }

    #[test]
    fn panel_classification_requires_observed_phases_and_matching_exit() {
        assert_eq!(
            classify_log(&settled_exit(2), PANEL_FAILURE).unwrap(),
            PanelObservation::KnownPanelFailure
        );
        assert!(classify_log(&settled_exit(2), "constructor failed\n").is_err());
        assert!(classify_log(&settled_exit(0), PANEL_FAILURE).is_err());
        assert!(classify_log(&settled_exit(3), PANEL_FAILURE).is_err());
        assert!(
            classify_log(
                &settled_exit(2),
                &PANEL_FAILURE.replace("visible=false", "visible=true")
            )
            .is_err()
        );
    }

    #[test]
    fn healthy_panel_requires_exact_visible_cancellation_phases_and_exit_zero() {
        assert_eq!(
            classify_log(&settled_exit(0), PANEL_HEALTHY).unwrap(),
            PanelObservation::Healthy
        );
        for text in [
            String::new(),
            PANEL_HEALTHY.replace("visible=true", "visible=false"),
            PANEL_HEALTHY.replace("callback response 0", "callback response 1"),
            PANEL_HEALTHY.replace("constructor returned\n", ""),
            format!("{PANEL_HEALTHY}Native AppKit panel diagnostic: callback response 0\n"),
        ] {
            assert!(
                classify_log(&settled_exit(0), &text).is_err(),
                "Accepted {text:?}"
            );
        }
        assert!(classify_log(&settled_exit(2), PANEL_HEALTHY).is_err());
        let mut killed = settled_exit(0);
        killed.status = std::process::ExitStatus::from_raw(9);
        assert!(classify_log(&killed, PANEL_HEALTHY).is_err());
    }

    #[test]
    fn identity_after_is_retained_even_when_the_observation_or_identity_is_rejected() {
        let out = tempfile::tempdir().unwrap();
        let log = out.path().join("probe.log");
        fs::write(&log, PANEL_HEALTHY).unwrap();
        let identity = json!({"files":{"probe":"original"}});
        let changed = json!({"files":{"probe":"changed"}});
        let mut record = json!({});
        assert_eq!(
            record_probe_result(
                &mut record,
                &settled_exit(0),
                &log,
                Ok(identity.clone()),
                &identity
            )
            .unwrap(),
            PanelObservation::Healthy
        );
        assert!(
            record_probe_result(
                &mut record,
                &settled_exit(0),
                &log,
                Ok(changed.clone()),
                &identity
            )
            .is_err()
        );
        assert_eq!(record["identity_after"], changed);
        assert_eq!(record["phases"].as_array().unwrap().len(), 5);
        fs::write(&log, "Native AppKit panel diagnostic: unknown\n").unwrap();
        assert!(
            record_probe_result(
                &mut record,
                &settled_exit(0),
                &log,
                Ok(identity.clone()),
                &identity
            )
            .is_err()
        );
        assert_eq!(record["identity_after"], identity);
        assert!(record["observation"].is_null());
        assert!(
            record_probe_result(
                &mut record,
                &settled_exit(0),
                &log,
                Err("Identity capture unavailable".into()),
                &identity
            )
            .is_err()
        );
        assert!(record["identity_after"].is_null());
        assert!(
            record["identity_after_error"]
                .as_str()
                .unwrap()
                .contains("unavailable")
        );
    }

    #[test]
    fn panel_classification_rejects_unclean_processes() {
        for field in 0..4 {
            let mut process = settled_exit(2);
            match field {
                0 => process.timed_out = true,
                1 => process.cancelled = true,
                2 => process.unexpected_survivors = 1,
                _ => process.cleanup_complete = false,
            }
            assert!(classify_log(&process, PANEL_FAILURE).is_err());
            process.status = settled_exit(0).status;
            assert!(classify_log(&process, PANEL_HEALTHY).is_err());
        }
    }
}
