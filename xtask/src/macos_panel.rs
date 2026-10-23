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

pub(crate) struct ProbeOwner {
    app: std::path::PathBuf,
    identity: Value,
}
fn fresh_host() -> Result<()> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Native panel probe requires a fresh hosted Mac; owner desktop/preferences are protected".into());
    }
    Ok(())
}
pub(crate) fn build_probe(root: &Path, build: &MacBuild, out: &Path) -> Result<ProbeOwner> {
    fresh_host()?;
    fs::create_dir(out)?;
    let app = build_panel_diagnostic(root, build, out)?;
    let identity = probe_identity(root, build, out, &app, "frozen")?;
    Ok(ProbeOwner { app, identity })
}
pub(crate) fn run_probe(
    root: &Path,
    build: &MacBuild,
    probe: &ProbeOwner,
    out: &Path,
) -> Result<Value> {
    fresh_host()?;
    fs::create_dir(out)?;
    let before = probe_identity(root, build, out, &probe.app, "before")?;
    if before != probe.identity {
        return Err("Frozen native probe changed before execution".into());
    }
    let log = out.join("panel.log");
    let process = controller()?.run(
        build.command(root, probe.app.join("Contents/MacOS/NativePanelDiagnostic")),
        &log,
        Duration::from_secs(90),
    )?;
    let after = probe_identity(root, build, out, &probe.app, "after");
    let mut result = json!({"identity":before,"identity_after":null,"process":null,"phases":[],"observation":null});
    let observed = record_probe_result(&mut result, &process, &log, after, &probe.identity);
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    observed?;
    Ok(result)
}
pub(crate) fn observe(root: &Path, build: &MacBuild, out: &Path) -> Result<Value> {
    fresh_host()?;
    if !authorized_job(
        &std::env::var("GITHUB_WORKFLOW").unwrap_or_default(),
        &std::env::var("GITHUB_JOB").unwrap_or_default(),
    ) {
        return Err("Standalone panel observation requires its own diagnostic job".into());
    }
    fs::create_dir(out)?;
    let host = crate::native_host::inspect(root, &build.host, "macos", &build.architecture)?;
    let probe = build_probe(root, build, &out.join("probe"))?;
    let mut result = run_probe(root, build, &probe, &out.join("observation"))?;
    result["host"] = host;
    result["scope"] = json!(
        "Nonmutating standalone AppKit observation; original probe result is authoritative and no service action or product test was run"
    );
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
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
