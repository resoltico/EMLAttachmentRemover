//! Nonshipping observed processing and export operations through the actual GUI.
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
        return Err("Nonordinary or oversized operation observation".into());
    }
    let bytes = fs::read(path)?;
    if bytes.len() > 128 {
        return Err("Operation observation grew beyond its bound".into());
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
                let directory = fixture.join("operation-control");
                let deadline =
                    std::time::Instant::now() + crate::macos_interaction::EXECUTION_LIMIT;
                let mut ready = false;
                let mut released = false;
                while !finished.load(Ordering::Acquire) {
                    ready |= marker(&directory, "ready")?.is_some_and(|line| {
                        line.starts_with(if case == DesktopCase::PublishedError {
                            b"operation=processing phase=after-publication index=0 operation_id="
                        } else {
                            b"operation=processing phase=publication index=1 operation_id="
                        })
                    });
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
    if !case.is_publication() {
        return finish_child_control(root, build, fixture, out, case);
    }
    let directory = fixture.join("operation-control");
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
    let observed_app = out.join("Observed Operations.app");
    let mut copy = build.command(root, "/usr/bin/ditto");
    copy.args(["--norsrc", "--noextattr", "--noqtn", "--noacl"])
        .arg(app)
        .arg(&observed_app);
    crate::macos::checked_run(copy, &out.join("bundle-copy.log"), 120)?;
    let worker_image = install_observed_worker(root, &observed_app, build, out)?;
    let identity = crate::macos_archive::bundle_identity(&observed_app)?;
    let mut cases = Vec::new();
    let mut failures = Vec::new();
    let mut stopped = None;
    let mut not_executed = Vec::new();
    for &case in DesktopCase::controlled_cases() {
        if let Some(reason) = &stopped {
            not_executed.push(json!({"case":case.name(),"reason":reason}));
            continue;
        }
        let case_out = out.join(case.name());
        match crate::macos_interaction::run_case(
            root,
            &observed_app,
            build,
            &case_out,
            case,
            source_archive,
        ) {
            Ok(value) => cases.push(value),
            Err(error) => {
                let safe = known_case_settlement(root, build, &case_out, case).unwrap_or(false);
                failures.push(json!({"case":case.name(),"failure":error.to_string(),"known_safe_settlement":safe}));
                if !safe {
                    stopped=Some("Earlier case ownership/worker settlement is unverified; no further native application admitted".to_owned());
                }
            }
        }
    }
    fs::write(
        out.join("case-attempts.json"),
        serde_json::to_vec_pretty(
            &json!({"cases":cases,"failures":failures,"not_executed":not_executed}),
        )?,
    )?;
    let after = crate::macos_archive::bundle_identity(app)?;
    let observed_after = crate::macos_archive::bundle_identity(&observed_app)?;
    fs::write(
        out.join("bundle-identities.json"),
        serde_json::to_vec_pretty(
            &json!({"shipping_before":before,"shipping_after":after,"observed_bundle":identity,"observed_bundle_after":observed_after,"worker_image":worker_image,"cases":cases,"failures":failures,"not_executed":not_executed,"requested_fixture_feature":"eml-worker/native-fixtures"}),
        )?,
    )?;
    if before != after {
        return Err("Observed fixture changed the original shipping bundle".into());
    }
    if identity != observed_after {
        return Err("GUI execution changed the observed fixture bundle".into());
    }
    if !failures.is_empty() {
        return Err(format!("{} native operation-control cases failed; {} not executed; retained case-attempts.json",failures.len(),not_executed.len()).into());
    }
    let result = json!({"cases":cases,"observed_bundle":identity,"shipping_bundle_unchanged":true,"scope":"separately signed nonshipping worker synchronization and actual GUI integration; unmodified shipping archive checks remain required"});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}

// A failed test is never retried. Only an independently settled case can be
// followed by another owned app; cancellation/admission alone is not proof.
fn known_case_settlement(
    root: &Path,
    build: &MacBuild,
    out: &Path,
    case: DesktopCase,
) -> Result<bool> {
    let path = out.join("owner-result.json");
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Ok(false);
    }
    let bytes = fs::read(path)?;
    if bytes.len() > 64 * 1024 {
        return Ok(false);
    }
    let owner = crate::picker_evidence::observation_document(&bytes)?;
    crate::picker_evidence::check_fixture_filesystem(&owner["fixture_filesystem"])?;
    crate::picker_evidence::check_panel_path_clipboard(
        &owner["panel_path_clipboard"],
        case.uses_panel_path_clipboard(),
        "requested",
    )?;

    if owner["cancelled"] != false
        || owner["timed_out"] != false
        || owner["runner_reaped"] != true
        || owner["owned_app_terminated"] != true
        || owner["forced_app_cleanup"] != false
        || owner["originals_unchanged"] != true
        || owner["originals_captured"] != case.originals()
        || owner.get("source_check_failure") != Some(&Value::Null)
        || owner["owned_pasteboard_released"] != true
        || owner["owned_settings_terminated"] != true
        || owner["native_picker_diagnostic_settled"] != true
        || owner["unexpected_application_pids"] != json!([])
        || owner["unexpected_settings_application_pids"] != json!([])
    {
        return Ok(false);
    }
    let mut pids = std::collections::BTreeSet::new();
    for entry in fs::read_dir(out)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || !entry.file_name().to_string_lossy().starts_with("artifact-")
        {
            continue;
        }
        for relative in [
            "operation-control",
            "operation-control/processing",
            "operation-control/export",
            "control-history/racing-winner",
            "control-history/pre-edge-stop",
            "control-history/post-edge-stop",
        ] {
            if let Some(bytes) = marker(&entry.path().join(relative), "worker-pid")? {
                let pid: i32 = std::str::from_utf8(&bytes)?
                    .trim_end_matches('\n')
                    .parse()?;
                if pid <= 0 {
                    return Ok(false);
                }
                pids.insert(pid);
            }
        }
    }
    for pid in pids {
        let mut inspect = build.command(root, "/bin/ps");
        inspect.args(["-p", &pid.to_string(), "-o", "pid=,ppid=,state=,comm="]);
        let log = out.join(format!("failed-case-worker-{pid}-absence.log"));
        let observed = controller()?.run_separate_output(inspect, &log, Duration::from_secs(10))?;
        if observed.status.code() != Some(1)
            || observed.cancelled
            || observed.timed_out
            || !observed.cleanup_complete
            || observed.unexpected_survivors != 0
            || fs::metadata(&log)?.len() != 0
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn finish_child_control(
    root: &Path,
    build: &MacBuild,
    fixture: &Path,
    out: &Path,
    case: DesktopCase,
) -> Result<Value> {
    let document = fs::read(fixture.join("child-ui-observation.json"))?;
    if document.len() > 1024 * 1024 {
        return Err("Owned GUI control receipt exceeds its bound".into());
    }
    let ui = crate::picker_evidence::observation_document(&document)?;
    if ui["case"] != case.name() {
        return Err("Owned GUI control receipt belongs to another case".into());
    }
    let base = fixture.join("operation-control");
    let directory = if case == DesktopCase::BlockedExportPreflight {
        base.join("export")
    } else {
        base.clone()
    };
    let ready = marker(&directory, "ready")?.ok_or("Missing actual selected operation boundary")?;
    let operation = case.blocked().map_or("export", |(operation, _)| operation);
    let phase = case.blocked().map_or(
        if case == DesktopCase::ExportWriterBlock {
            "request-body"
        } else {
            "after-publication"
        },
        |(_, phase)| phase,
    );
    let prefix = format!(
        "operation={operation} phase={phase} index={} operation_id=",
        if operation == "processing" {
            "0"
        } else {
            "none"
        }
    );
    let operation_id: u64 = std::str::from_utf8(&ready)?
        .strip_prefix(&prefix)
        .and_then(|line| line.strip_suffix('\n'))
        .ok_or("Actual child boundary does not match the selected lane/phase")?
        .parse()?;
    if operation_id == 0 || ui["operation_id"] != operation_id {
        return Err("GUI control targeted another child operation".into());
    }
    let pid: i32 =
        std::str::from_utf8(&marker(&directory, "worker-pid")?.ok_or("Missing actual child PID")?)?
            .trim_end_matches('\n')
            .parse()?;
    if pid <= 0 {
        return Err("Invalid actual child PID".into());
    }
    let mut inspect = build.command(root, "/bin/ps");
    inspect.args(["-p", &pid.to_string(), "-o", "pid=,ppid=,state=,comm="]);
    let log = out.join("observed-child-absence.log");
    let observed = controller()?.run_separate_output(inspect, &log, Duration::from_secs(10))?;
    if observed.status.code() != Some(1)
        || observed.timed_out
        || observed.cancelled
        || !observed.cleanup_complete
        || observed.unexpected_survivors != 0
        || fs::metadata(&log)?.len() != 0
    {
        return Err("Controlled child absence was not observed".into());
    }
    let first = interrupt(marker(&directory, "first-interrupt")?)?;
    let second = interrupt(marker(&directory, "second-interrupt")?)?;
    if case.blocked().is_some()
        && first
            .zip(second)
            .is_none_or(|(first, second)| second < first)
    {
        return Err("Both actual child interrupt deliveries were not retained".into());
    }
    let additional_worker = if case == DesktopCase::BlockedExportPreflight {
        let processing = base.join("processing");
        let ready = marker(&processing, "ready")?.ok_or("Missing second blocked child boundary")?;
        let text = std::str::from_utf8(&ready)?;
        let id: u64 = text
            .strip_prefix("operation=processing phase=preflight index=0 operation_id=")
            .and_then(|line| line.strip_suffix('\n'))
            .ok_or("Wrong second blocked child identity")?
            .parse()?;
        let pid: i32 = std::str::from_utf8(
            &marker(&processing, "worker-pid")?.ok_or("Missing second child PID")?,
        )?
        .trim_end_matches('\n')
        .parse()?;
        if id == 0 || pid <= 0 || ui["processing_operation_id"] != id || id == operation_id {
            return Err("Dual child identities were not independent".into());
        }
        let first = interrupt(marker(&processing, "first-interrupt")?)?
            .ok_or("Missing second child Stop")?;
        let second = interrupt(marker(&processing, "second-interrupt")?)?
            .ok_or("Missing second child Force Stop")?;
        let mut command = build.command(root, "/bin/ps");
        command.args(["-p", &pid.to_string(), "-o", "pid=,ppid=,state=,comm="]);
        let log = out.join("observed-processing-child-absence.log");
        let observed = controller()?.run_separate_output(command, &log, Duration::from_secs(10))?;
        if observed.status.code() != Some(1)
            || !observed.cleanup_complete
            || observed.timed_out
            || observed.cancelled
            || observed.unexpected_survivors != 0
            || fs::metadata(&log)?.len() != 0
            || second
                .checked_sub(first)
                .is_none_or(|elapsed| !(9_800_000..=12_500_000).contains(&elapsed))
        {
            return Err("Dual child shutdown deadline or actual process settlement failed".into());
        }
        let target =
            fixture.join("publication-output/folder.lhv5cptzmajo2v546wflkqm5xa.no-attachments.eml");
        match fs::symlink_metadata(&target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => {
                return Err(
                    "Second preflight-blocked child unexpectedly published a target".into(),
                );
            }
        }
        Some(
            json!({"operation_id":id,"run_id":3,"target":{"path":target,"identity":null},"worker_pid":pid,"worker_absent":true,"first_interrupt_micros":first,"second_interrupt_micros":second}),
        )
    } else {
        None
    };
    let originals = [
        "one.eml",
        "two.eml",
        "folder-input/folder.eml",
        "service-input/日本語-مرحبا-\u{202e}report.eml",
    ]
    .iter()
    .map(|name| {
        fs::read(fixture.join(name))
            .map(|bytes| json!({"name":name,"identity":xtask::archive::ByteIdentity::of(&bytes)}))
    })
    .collect::<std::io::Result<Vec<_>>>()?;
    let target = if operation == "export" {
        fixture.join("blocked-report.json")
    } else {
        fixture.join("publication-output/two.ctpjzw7urreuwuroqgjcbuwt3m.no-attachments.eml")
    };
    let target_bytes = match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.is_file() => {
            Some(xtask::archive::ByteIdentity::of(&fs::read(&target)?))
        }
        Ok(_) => return Err("Controlled publication target is not an ordinary file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some((operation, phase)) = case.blocked() {
        if matches!(phase, "readback" | "namespace-sync") {
            let bytes = fs::read(&target)?;
            if (operation == "processing"
                && bytes
                    != include_bytes!("../../tests/contract/mime/golden/01-gmail-attachment.eml"))
                || (operation == "export" && eml_app::report_document::validate(&bytes).is_err())
            {
                return Err("Actual post-edge or preseeded target bytes differ".into());
            }
        } else if target_bytes.is_some() {
            return Err("A blocked pre-edge operation published an unexpected target".into());
        }
    }
    if case == DesktopCase::ExportWriterBlock && target_bytes.is_some() {
        return Err("A body-blocked export performed target I/O".into());
    }
    let matrix = if case == DesktopCase::ExportPublication {
        let expected = fs::read(fixture.join("independent-report.json"))?;
        let expected_identity = xtask::archive::ByteIdentity::of(&expected);
        if expected.len() > 65536
            || ui["expected_bytes"] != expected_identity.byte_length
            || ui["expected_sha256"] != expected_identity.sha256
            || eml_app::report_document::validate(&expected).is_err()
        {
            return Err("Independent metadata-only export oracle differs".into());
        }
        for name in [
            "ordinary-report.json",
            "post-edge-report.json",
            "blocked-report.json",
        ] {
            if fs::read(fixture.join(name))? != expected {
                return Err("Native exported bytes differ from independent exact document".into());
            }
        }
        let winner = b"Independent report occupancy sentinel\n";
        for name in ["occupied-report.json", "racing-report.json"] {
            if fs::read(fixture.join(name))? != winner {
                return Err("Native report publication replaced an independent winner".into());
            }
        }
        if !fs::symlink_metadata(fixture.join("symlink-report.json"))?.is_symlink()
            || fs::read_link(fixture.join("symlink-report.json"))?
                != fixture.join("occupied-report.json")
            || fs::read_dir(fixture.join("directory-report.json"))?
                .next()
                .is_some()
            || fixture.join("pre-edge-report.json").try_exists()?
        {
            return Err("Native report occupancy or pre-edge stop effects differ".into());
        }
        let mut history = Vec::new();
        for (name, phase, stopped) in [
            ("racing-winner", "publication", false),
            ("pre-edge-stop", "publication", true),
            ("post-edge-stop", "namespace-sync", true),
        ] {
            let directory = fixture.join("control-history").join(name);
            let ready = marker(&directory, "ready")?.ok_or("Missing matrix boundary")?;
            let prefix = format!("operation=export phase={phase} index=none operation_id=");
            let id: u64 = std::str::from_utf8(&ready)?
                .strip_prefix(&prefix)
                .and_then(|line| line.strip_suffix('\n'))
                .ok_or("Unexpected matrix child boundary")?
                .parse()?;
            let pid: i32 = std::str::from_utf8(
                &marker(&directory, "worker-pid")?.ok_or("Missing matrix child PID")?,
            )?
            .trim_end_matches('\n')
            .parse()?;
            if id == 0 || pid <= 0 {
                return Err("Invalid matrix operation identity".into());
            }
            let first = interrupt(marker(&directory, "first-interrupt")?)?;
            let second = interrupt(marker(&directory, "second-interrupt")?)?;
            if first.is_some() != stopped || second.is_some() {
                return Err("Matrix control observations differ".into());
            }
            let mut inspect = build.command(root, "/bin/ps");
            inspect.args(["-p", &pid.to_string(), "-o", "pid=,ppid=,state=,comm="]);
            let log = out.join(format!("matrix-{name}-absence.log"));
            let process =
                controller()?.run_separate_output(inspect, &log, Duration::from_secs(10))?;
            if process.status.code() != Some(1)
                || process.timed_out
                || process.cancelled
                || !process.cleanup_complete
                || process.unexpected_survivors != 0
                || fs::metadata(&log)?.len() != 0
            {
                return Err("Matrix child did not settle".into());
            }
            history.push(json!({"case":name,"phase":phase,"operation_id":id,"worker_pid":pid,"worker_absent":true,"first_interrupt_micros":first,"second_interrupt_micros":second}));
        }
        Some(
            json!({"expected_document":expected_identity,"expected_document_contents":String::from_utf8(expected.clone())?,"winner":xtask::archive::ByteIdentity::of(winner),"controls":history,"directory_unchanged":true,"symlink_unchanged":true,"pre_edge_absent":true}),
        )
    } else {
        None
    };
    let writer = if case == DesktopCase::ExportWriterBlock {
        let bytes = fs::read(fixture.join("measured-report.json"))?;
        let identity = xtask::archive::ByteIdentity::of(&bytes);
        if bytes.len() <= 1024 * 1024
            || bytes.len() > 256 * 1024 * 1024
            || ui["document_bytes"] != identity.byte_length
            || ui["document_sha256"] != identity.sha256
            || eml_app::report_document::validate(&bytes).is_err()
        {
            return Err("Actual blocked-writer report is not the bounded measured document".into());
        }
        Some(identity)
    } else {
        None
    };
    Ok(
        json!({"case":case.name(),"operation":operation,"phase":phase,"operation_id":operation_id,"worker_pid":pid,"worker_absent":true,"additional_worker":additional_worker,"matrix":matrix,"writer_document":writer,"first_interrupt_micros":first,"second_interrupt_micros":second,"ui_receipt":{"identity":xtask::archive::ByteIdentity::of(&document),"contents":String::from_utf8(document)?},"originals":originals,"target":{"path":target,"identity":target_bytes},"scope":"actual native fixture child, GUI actions and retained effects; lost receipts never imply publication rollback"}),
    )
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
