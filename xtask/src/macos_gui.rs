//! Owned hosted-runner GUI integration. Controller cleanup does not prove that
//! LaunchServices applications or their helpers have settled.
use crate::{Result, macos};
use serde_json::json;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

pub(crate) fn run(root: &Path, app: &Path, out: &Path) -> Result<()> {
    for (key, expected) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
    ] {
        if std::env::var(key).as_deref() != Ok(expected) {
            return Err("GUI execution requires the owned GitHub-hosted workflow; environment flags alone are not provenance".into());
        }
    }
    let temporary =
        fs::canonicalize(std::env::var_os("RUNNER_TEMP").ok_or("Missing RUNNER_TEMP")?)?;
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    use std::os::unix::fs::PermissionsExt;
    let owned = tempfile::Builder::new()
        .prefix("eml-gui-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&temporary)?
        .keep();
    let owned = owned.canonicalize()?;
    if owned == temporary
        || !owned.starts_with(&temporary)
        || fs::symlink_metadata(&owned)?.file_type().is_symlink()
    {
        return Err("GUI fixture directory is not an exclusive non-symlink descendant".into());
    }
    fs::write(
        out.join("context.json"),
        serde_json::to_vec_pretty(&json!({
            "owned_fixture":owned,
            "scope":"ordinary actual GUI Open With only; other GUI conditions remain pending",
            "ownership":"returned exact NSRunningApplication; Controller cleanup is not LaunchServices absence evidence"
        }))?,
    )?;
    let result = run_owned(root, &app.canonicalize()?, &out, &owned);
    if let Err(error) = &result {
        eprintln!(
            "GUI qualification failed; retain owned fixture {} and abort later GUI cases",
            owned.display()
        );
        if let Err(record) = fs::write(out.join("failure.log"), format!("{error}\n")) {
            eprintln!("Could not retain GUI failure: {record}");
        }
    }
    result
}

fn run_owned(root: &Path, app: &Path, out: &Path, owned: &Path) -> Result<()> {
    let build = macos::load(root)?;
    for name in ["EMLAttachmentRemover", "remove-eml-attachments"] {
        let executable = app.join("Contents/MacOS").join(name);
        macos::fixture_identity(&fs::read(executable)?, &build.architecture, build.minimum)?;
    }
    for (name, path) in [
        ("app", app.to_path_buf()),
        ("helper", app.join("Contents/MacOS/remove-eml-attachments")),
    ] {
        let mut verify = build.command(root, "/usr/bin/codesign");
        verify
            .args(["--verify", "--strict", "--verbose=2"])
            .arg(path);
        macos::checked_run(verify, &out.join(format!("{name}-signature.log")), 30)?;
    }
    let launcher = out.join("GUIIntegration");
    let reader = out.join("PreferenceSnapshot");
    for (binary, sources, log) in [
        (
            &launcher,
            vec!["GUIIntegration.swift", "GUIHelperObservation.swift"],
            "launcher-build.log",
        ),
        (
            &reader,
            vec!["PreferenceSnapshot.swift"],
            "snapshot-build.log",
        ),
    ] {
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
            .arg("-module-cache-path")
            .arg(out.join("module-cache"));
        for source in sources {
            compile.arg(root.join("macos-app/Tests").join(source));
        }
        compile.arg("-o").arg(binary);
        macos::checked_run(compile, &out.join(log), 120)?;
    }
    // The shared allowance begins before preference preflight. Compiling the
    // tools cannot consume that allowance and then launch an app anyway.
    let began = Instant::now();
    let total = Duration::from_secs(120);
    let before = out.join("preferences-before.plist");
    let after = out.join("preferences-after.plist");
    let mut snapshot = build.command(root, &reader);
    snapshot.arg("before").arg(&before);
    macos::checked_run(snapshot, &out.join("preferences-before.log"), 5)?;
    let remaining = total
        .checked_sub(began.elapsed())
        .ok_or("GUI preflight exhausted the case budget")?;
    let seconds = remaining
        .checked_sub(Duration::from_secs(5))
        .ok_or("No reserved after-observation allowance remains")?
        .as_secs();
    if seconds < 30 {
        return Err("GUI preflight exhausted the cleanup reserve".into());
    }
    let mut launch = build.command(root, &launcher);
    launch
        .arg(app)
        .arg(root)
        .arg(owned)
        .arg(seconds.to_string());
    // Failure/timeout is Unknown and retained. This controller's process group
    // cannot prove cleanup of an application launched by LaunchServices.
    macos::checked_run(launch, &out.join("ordinary-gui.log"), seconds + 5)?;
    let remaining = total
        .checked_sub(began.elapsed())
        .ok_or("No budget remains for fresh preference observation")?;
    if remaining < Duration::from_secs(5) {
        return Err("Preference after-observation budget unavailable".into());
    }
    let mut snapshot = build.command(root, &reader);
    snapshot.arg("after").arg(&after).arg(&before);
    macos::checked_run(snapshot, &out.join("preferences-after.log"), 5)?;
    if began.elapsed() > total {
        return Err(
            "GUI case exceeded the shared allowance during final observation or cleanup".into(),
        );
    }
    fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(&json!({
            "case":"ordinary-open-with", "elapsed_ms":began.elapsed().as_millis(),
            "selected_processing_run":"Created1/upToDate0/failed0; actual helper0/summary0; independent source/golden bytes",
            "settlement":"exact returned application terminated after selected processing settlement",
            "preferences":"fresh current-user any-host/current-host product-domain observations both empty; no physical absence or transient-write claim",
            "other_gui_conditions":"pending"
        }))?,
    )?;
    Ok(())
}
