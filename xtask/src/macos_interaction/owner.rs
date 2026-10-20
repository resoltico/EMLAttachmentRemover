//! Native owner rejection controls retain actual process and cleanup observations.
use super::*;
use std::{
    io::Write,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub(super) fn owner_controls(
    root: &Path,
    app: &Path,
    build: &MacBuild,
    owner: &Path,
    out: &Path,
) -> Result<Value> {
    let parent = out.join("owner-controls");
    fs::create_dir(&parent)?;
    let probe = build_control_probe(root, build, &parent)?;
    let mut results = Vec::new();
    for case in [
        "deadline",
        "cancel",
        "runner-crash",
        "source-change",
        "force-app",
    ] {
        results.push(run_owner_control(
            root, app, build, owner, &probe, &parent, case,
        )?);
    }
    let result = json!({"cases":results,"scope":"real Cocoa app lifetime authority under controlled runner failure/deadline/cancellation/forced app termination/source-byte change; no GUI processing, natural app-crash, or Cocoa EOF claim"});
    fs::write(
        parent.join("result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}
fn build_control_probe(root: &Path, build: &MacBuild, parent: &Path) -> Result<std::path::PathBuf> {
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
    Ok(probe)
}
fn run_owner_control(
    root: &Path,
    app: &Path,
    build: &MacBuild,
    owner: &Path,
    probe: &Path,
    parent: &Path,
    case: &str,
) -> Result<Value> {
    let case_parent = parent.join(case);
    fs::create_dir(&case_parent)?;
    let enclosure = xtask::archive::create_artifact_directory(&case_parent)?;
    let directory = enclosure.path();
    let result_file = directory.join(OWNER_RESULT);
    prepare_control_fixture(app, case, directory)?;
    let command = control_command(root, build, owner, probe, case, directory);
    let local = xtask::process::Controller::new(root, &std::env::current_exe()?)?;
    let (observed, action) = execute_control(&local, command, case, directory);
    let read_facts = read_control_facts(&result_file);
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
    qualify_control(case, &observed, &facts, &local, directory)?;
    Ok(json!({"case":case,"owner":facts,"process":observed.evidence()}))
}
fn prepare_control_fixture(app: &Path, case: &str, directory: &Path) -> Result<()> {
    for name in [
        "one.eml",
        "two.eml",
        "folder-input/folder.eml",
        "service-input/日本語-مرحبا-\u{202e}report.eml",
    ] {
        let file = directory.join(name);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            file,
            include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml"),
        )?;
    }
    let input = directory.join("input.json");
    fs::write(
        &input,
        serde_json::to_vec_pretty(
            &json!({"app":app,"fixture":directory,"host_scope":"fresh-github-hosted-runner", "owner_control":match case { "deadline"=>"deadline", "force-app"=>"force-app", _=>"normal" }, "runner_ready":directory.join("worker-ready")}),
        )?,
    )?;
    Ok(())
}
fn control_command(
    root: &Path,
    build: &MacBuild,
    owner: &Path,
    probe: &Path,
    case: &str,
    directory: &Path,
) -> Command {
    let input = directory.join("input.json");
    let receipt = directory.join("ownership.json");
    let result_file = directory.join(OWNER_RESULT);
    let mut command = build.command(root, owner);
    command
        .current_dir(directory)
        .arg(&input)
        .arg(&receipt)
        .arg(&result_file)
        .arg(probe)
        .arg(if case == "runner-crash" {
            "abort"
        } else {
            "cooperative-resource"
        })
        .arg(directory);
    command
}
fn execute_control(
    local: &xtask::process::Controller,
    command: Command,
    case: &str,
    directory: &Path,
) -> (
    Result<xtask::process::ProcessResult>,
    std::result::Result<(), String>,
) {
    let ready = directory.join("ownership.ready.json");
    let finished = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let watching = scope.spawn(|| -> std::result::Result<(), String> {
            stimulate_owner_control(&finished, local, &ready, case, directory)
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
    })
}
fn stimulate_owner_control(
    finished: &AtomicBool,
    local: &xtask::process::Controller,
    ready: &Path,
    case: &str,
    directory: &Path,
) -> std::result::Result<(), String> {
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
                    fs::write(directory.join("control-release"), b"release owned probe\n")
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
}
fn read_control_facts(result_file: &Path) -> std::result::Result<Value, String> {
    fs::symlink_metadata(result_file)
        .map_err(|error| error.to_string())
        .and_then(|metadata| {
            if !metadata.is_file() || metadata.len() > 64 * 1024 {
                Err("Owner control result is not an ordinary bounded document".into())
            } else {
                Ok(())
            }
        })
        .and_then(|()| fs::read(result_file).map_err(|error| error.to_string()))
        .and_then(|bytes| {
            if bytes.len() > 64 * 1024 {
                Err("Owner control result grew beyond its bound".into())
            } else {
                Ok(bytes)
            }
        })
        .and_then(|bytes| {
            serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
        })
}
fn qualify_control(
    case: &str,
    observed: &xtask::process::ProcessResult,
    facts: &Value,
    local: &xtask::process::Controller,
    directory: &Path,
) -> Result<()> {
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
            return Err("Native source comparison did not independently record its failure".into());
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
    Ok(())
}
