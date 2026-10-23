//! Nonshipping synchronization of real Core operations through the actual GUI.
use crate::{Result, controller, macos::MacBuild, macos_interaction::DesktopCase};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

fn marker(directory: &Path, name: &str) -> Result<Option<Vec<u8>>> {
    let path = directory.join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() > 128 {
        return Err("Nonordinary or oversized publication observation".into());
    }
    let bytes = fs::read(path)?;
    if bytes.len() > 128 {
        return Err("Publication observation grew beyond its bound".into());
    }
    Ok(Some(bytes))
}

fn release(directory: &Path) -> Result<()> {
    let temporary = directory.join("owned-release");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(b"release\n")?;
    file.sync_all()?;
    drop(file);
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        temporary,
        rustix::fs::CWD,
        directory.join("release"),
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    Ok(())
}

pub(crate) fn execute(
    case: DesktopCase,
    fixture: &Path,
    launch: impl FnOnce() -> Result<xtask::process::ProcessResult>,
) -> Result<xtask::process::ProcessResult> {
    if !case.is_publication() {
        return launch();
    }
    let finished = AtomicBool::new(false);
    let (process, actor) = std::thread::scope(|scope| {
        let actor = scope.spawn(|| {
            let result = (|| -> Result<Value> {
                let directory = fixture.join("publication-control");
                let deadline =
                    std::time::Instant::now() + crate::macos_interaction::EXECUTION_LIMIT;
                let mut ready = false;
                let mut released = false;
                while !finished.load(Ordering::Acquire) {
                    ready |= marker(&directory, "ready")?.as_deref()
                        == Some(b"real publication boundary\n");
                    if ready && !released && case == DesktopCase::PublishedError {
                        let output = fixture.join("publication-output");
                        if fs::read(
                            output.join("one.7wxqoa2jifs7cdaqt2anqogulu.no-attachments.eml"),
                        )? != include_bytes!(
                            "../../tests/contract/mime/golden/01-gmail-attachment.eml"
                        ) {
                            return Err(
                                "Real published candidate differs from independent golden".into()
                            );
                        }
                        fs::rename(&output, fixture.join("published-output"))?;
                        fs::create_dir(output)?;
                        release(&directory)?;
                        released = true;
                    }
                    if ready
                        && !released
                        && case == DesktopCase::Stop
                        && marker(&directory, "first-interrupt")?.is_some()
                    {
                        release(&directory)?;
                        released = true;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(
                            "Publication actor deadline exceeded; no synthetic release supplied"
                                .into(),
                        );
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                if !ready
                    || (matches!(case, DesktopCase::Stop | DesktopCase::PublishedError)
                        && !released)
                {
                    return Err("Required real publication actor stimulus was not completed".into());
                }
                Ok(json!({"ready_observed":ready,"released":released,"case":case.name()}))
            })();
            result.map_err(|error| error.to_string())
        });
        let process = launch();
        finished.store(true, Ordering::Release);
        let actor = actor
            .join()
            .unwrap_or_else(|_| Err("Publication actor panicked".into()));
        (process, actor)
    });
    fs::write(
        fixture.join("actor-watch.json"),
        serde_json::to_vec_pretty(
            &json!({"actor":actor.as_ref().map_err(|error|error.to_string()),"process":process.as_ref().map(|p|p.evidence()).map_err(|error|error.to_string())}),
        )?,
    )?;
    // Preserve a process failure before the independent actor result.
    let process = process?;
    if !process.success() {
        return Ok(process);
    }
    actor?;
    Ok(process)
}

fn interrupt(bytes: Option<Vec<u8>>) -> Result<Option<u64>> {
    bytes
        .map(|bytes| -> Result<u64> {
            let text = std::str::from_utf8(&bytes)?;
            let value = text
                .strip_prefix("observed ")
                .and_then(|s| s.strip_suffix('\n'))
                .ok_or("Invalid native interrupt observation")?;
            Ok(value.parse()?)
        })
        .transpose()
}

pub(crate) fn finish(
    root: &Path,
    build: &MacBuild,
    fixture: &Path,
    out: &Path,
    case: DesktopCase,
) -> Result<Value> {
    let directory = fixture.join("publication-control");
    let first = interrupt(marker(&directory, "first-interrupt")?)?;
    let second = interrupt(marker(&directory, "second-interrupt")?)?;
    match case {
        DesktopCase::Stop if first.is_some() && second.is_none() => (),
        DesktopCase::ForceStop if first.zip(second).is_some_and(|(a, b)| b >= a) => (),
        DesktopCase::ActiveQuit
            if first
                .zip(second)
                .is_some_and(|(a, b)| b >= a && (9_800_000..=12_500_000).contains(&(b - a))) => {}
        DesktopCase::PublishedError if first.is_none() && second.is_none() => (),
        _ => {
            return Err(
                "Actual native interrupt observations do not match the GUI action/deadline".into(),
            );
        }
    }
    let pid = marker(&directory, "worker-pid")?.ok_or("Missing observed worker PID")?;
    let pid: i32 = std::str::from_utf8(&pid)?.trim_end_matches('\n').parse()?;
    if pid <= 0 {
        return Err("Invalid observed worker PID".into());
    }
    // Read-only absence check. This numeric PID never grants signal authority.
    let mut inspect = build.command(root, "/bin/ps");
    inspect.args(["-p", &pid.to_string(), "-o", "pid=,ppid=,state=,comm="]);
    let log = out.join("observed-worker-absence.log");
    let process = controller()?.run_separate_output(inspect, &log, Duration::from_secs(10))?;
    if process.status.code() != Some(1)
        || process.timed_out
        || process.cancelled
        || !process.cleanup_complete
        || process.unexpected_survivors != 0
        || fs::metadata(&log)?.len() != 0
        || fs::metadata(log.with_extension("stderr"))?.len() != 0
    {
        return Err("Observed real worker absence could not be established; actual supervisor settlement remains separately required".into());
    }
    let golden = include_bytes!("../../tests/contract/mime/golden/01-gmail-attachment.eml");
    let output = fixture.join(if case == DesktopCase::PublishedError {
        "published-output"
    } else {
        "publication-output"
    });
    if fs::read(output.join("one.7wxqoa2jifs7cdaqt2anqogulu.no-attachments.eml"))? != golden {
        return Err("Completed controlled output differs from independent golden".into());
    }
    if output
        .join("two.ctpjzw7urreuwuroqgjcbuwt3m.no-attachments.eml")
        .exists()
    {
        return Err("Held second item unexpectedly published".into());
    }
    if case == DesktopCase::PublishedError
        && fs::read_dir(fixture.join("publication-output"))?
            .next()
            .is_some()
    {
        return Err("Replacement output address is not empty".into());
    }
    Ok(
        json!({"worker_pid":pid,"worker_absent":true,"first_interrupt_micros":first,"second_interrupt_micros":second,"independent_copy_golden":true,"case":case.name(),"scope":"actual fixture-enabled Core worker, native signals and filesystem observations; no replayed outcome or shipping archive claim"}),
    )
}

pub(crate) fn run(
    root: &Path,
    app: &Path,
    build: &MacBuild,
    out: &Path,
    source_archive: &xtask::archive::ByteIdentity,
) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Observed GUI requires a fresh hosted Mac".into());
    }
    fs::create_dir(out)?;
    let before = crate::macos_archive::bundle_identity(app)?;
    let observed_app = out.join("Observed Core.app");
    let mut copy = build.command(root, "/usr/bin/ditto");
    copy.args(["--norsrc", "--noextattr", "--noqtn", "--noacl"])
        .arg(app)
        .arg(&observed_app);
    crate::macos::checked_run(copy, &out.join("bundle-copy.log"), 120)?;
    let worker_image = install_observed_worker(root, &observed_app, build, out)?;
    let identity = crate::macos_archive::bundle_identity(&observed_app)?;
    let cases = (|| -> Result<Vec<Value>> {
        let mut cases = Vec::new();
        for case in [
            DesktopCase::Stop,
            DesktopCase::ForceStop,
            DesktopCase::ActiveQuit,
            DesktopCase::PublishedError,
        ] {
            cases.push(crate::macos_interaction::run_case(
                root,
                &observed_app,
                build,
                &out.join(case.name()),
                case,
                source_archive,
            )?);
        }
        Ok(cases)
    })();
    let after = crate::macos_archive::bundle_identity(app)?;
    let observed_after = crate::macos_archive::bundle_identity(&observed_app)?;
    fs::write(
        out.join("bundle-identities.json"),
        serde_json::to_vec_pretty(
            &json!({"shipping_before":before,"shipping_after":after,"observed_bundle":identity,"observed_bundle_after":observed_after,"worker_image":worker_image,"cases":cases.as_ref().map_err(|e|e.to_string()),"fixture_features":["eml-worker/native-fixtures","eml-core/native-fixtures"]}),
        )?,
    )?;
    if before != after {
        return Err("Observed fixture changed the original shipping bundle".into());
    }
    if identity != observed_after {
        return Err("GUI execution changed the observed fixture bundle".into());
    }
    let cases = cases?;
    let result = json!({"cases":cases,"observed_bundle":identity,"shipping_bundle_unchanged":true,"scope":"separately signed nonshipping worker synchronization and actual GUI integration; unmodified shipping archive checks remain required"});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}

fn install_observed_worker(
    root: &Path,
    observed_app: &Path,
    build: &MacBuild,
    out: &Path,
) -> Result<crate::macos::MachIdentity> {
    use std::os::unix::fs::PermissionsExt;
    let mut compile = build.command(root, "cargo");
    compile
        .args([
            "build",
            "--locked",
            "--release",
            "--package",
            "eml-worker",
            "--features",
            "eml-worker/native-fixtures",
            "--target",
            &build.host,
            "--message-format=json",
        ])
        .env("CARGO_TARGET_DIR", out.join("cargo"))
        .env("MACOSX_DEPLOYMENT_TARGET", &build.minimum_text);
    let log = out.join("worker-build.jsonl");
    controller()?
        .run_separate_output(compile, &log, Duration::from_secs(1200))?
        .require_success(&log)?;
    if fs::metadata(&log)?.len() > 16 * 1024 * 1024 {
        return Err("Observed worker compiler log exceeds its document bound".into());
    }
    let worker = crate::cli_build::named_artifact(
        root,
        &fs::read(&log)?,
        "crates/eml-worker/Cargo.toml",
        "crates/eml-worker/src/main.rs",
        "eml-worker",
    )?;
    let metadata = fs::symlink_metadata(&worker)?;
    if !metadata.is_file()
        || metadata.len() > 256 * 1024 * 1024
        || !worker
            .canonicalize()?
            .starts_with(out.join("cargo").canonicalize()?)
    {
        return Err("Observed worker escaped its bounded owned native build".into());
    }
    let bytes = fs::read(worker)?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("Observed worker grew beyond its image bound".into());
    }
    let worker_image = crate::macos::fixture_identity(&bytes, &build.architecture, build.minimum)?;
    let worker_path = observed_app.join("Contents/Helpers/eml-worker");
    if !fs::symlink_metadata(&worker_path)?.is_file() {
        return Err("Copied worker is not an ordinary file".into());
    }
    fs::write(&worker_path, bytes)?;
    fs::set_permissions(&worker_path, fs::Permissions::from_mode(0o755))?;
    for (label, path) in [("worker", worker_path.as_path()), ("app", observed_app)] {
        let mut sign = build.command(root, "/usr/bin/codesign");
        sign.args(["--force", "--sign", "-", "--timestamp=none"])
            .arg(path);
        crate::macos::checked_run(sign, &out.join(format!("{label}-sign.log")), 30)?;
        let mut verify = build.command(root, "/usr/bin/codesign");
        verify
            .args(["--verify", "--strict", "--verbose=2"])
            .arg(path);
        crate::macos::checked_run(verify, &out.join(format!("{label}-verify.log")), 30)?;
    }
    Ok(worker_image)
}
