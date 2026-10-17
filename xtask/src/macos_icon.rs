//! Two finite observations of the unchanged pinned icon invocation. Diagnostic
//! output never substitutes for the actual app/signature/archive gates.
use crate::{Result, controller, macos, macos_app};
use serde_json::{Value, json};
use std::{fs, os::unix::process::ExitStatusExt, path::Path, time::Duration};

fn fallback_classes(root: &Path, developer: &Path) -> Vec<&'static str> {
    std::env::var_os("DYLD_FALLBACK_LIBRARY_PATH")
        .map(|value| {
            std::env::split_paths(&value)
                .map(|path| {
                    if path.as_os_str().is_empty() {
                        "empty-unknown"
                    } else if let Ok(path) = root.join(path).canonicalize() {
                        if path.starts_with(root) {
                            "workspace"
                        } else if path.starts_with(developer) {
                            "selected-xcode"
                        } else if [
                            "/usr/lib",
                            "/System/Library",
                            "/Library/Apple/System/Library",
                        ]
                        .iter()
                        .any(|base| path.starts_with(base))
                        {
                            "system"
                        } else {
                            "unknown"
                        }
                    } else {
                        "unknown-unresolved"
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn run(root: &Path, out: &Path) -> Result<()> {
    let build = macos::load(root)?;
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    macos::observe_graphics(root, &out.join("graphics"), &build)?;
    let classes = fallback_classes(&root.canonicalize()?, &build.developer.canonicalize()?);
    fs::write(
        out.join("context.json"),
        serde_json::to_vec_pretty(&json!({
            "build":build, "fallback_selector_classes":classes,
            "scope":"two observed icon attempts only; fresh output is not proof of cold/warm XPC service or cache state; no packaging qualification"
        }))?,
    )?;
    let mut observations: Vec<Value> = Vec::new();
    let mut failed = false;
    for index in 1..=2 {
        let stage = out.join(format!("attempt-{index}"));
        fs::create_dir(&stage)?;
        let (command, icons) = macos_app::icon_command(root, &stage, &build)?;
        let result = controller()?.run(
            command,
            &stage.join("icon-build.log"),
            Duration::from_secs(180),
        )?;
        let validation = result
            .success()
            .then(|| macos_app::icon_outputs(root, &icons, &build).map(|_| ()));
        let succeeded = result.success() && validation.as_ref().is_some_and(|value| value.is_ok());
        let output_check_failed = validation.as_ref().is_some_and(|value| value.is_err());
        let validation_state = match &validation {
            None => "unattempted",
            Some(Ok(())) => "succeeded",
            Some(Err(_)) => "failed",
        };
        let observation = json!({
            "attempt":index, "raw_status":result.status.into_raw(), "timed_out":result.timed_out,
            "cancelled":result.cancelled, "cleanup_complete":result.cleanup_complete,
            "unexpected_survivors":result.unexpected_survivors, "succeeded":succeeded,
            "output_validation_state":validation_state,
            "output_validation_error":validation.and_then(|value| value.err()).map(|error| error.to_string()),
            "scope":"icon-only observation, not app/signature/archive qualification"
        });
        fs::write(
            stage.join("result.json"),
            serde_json::to_vec_pretty(&observation)?,
        )?;
        observations.push(observation);
        fs::write(
            out.join("observations.json"),
            serde_json::to_vec_pretty(&observations)?,
        )?;
        failed |= !succeeded;
        if output_check_failed {
            // Validation can itself invoke plutil through Controller. Its
            // failure must not admit another attempt with uncertain cleanup.
            return Err(
                "Icon output validation failed; retain context and abort later attempts".into(),
            );
        }
        if result.timed_out
            || result.cancelled
            || !result.cleanup_complete
            || result.unexpected_survivors != 0
        {
            return Err("Icon diagnostic lifetime/cleanup is unconfirmed; retain context and abort later attempts".into());
        }
    }
    if failed {
        return Err("Icon diagnostic retained failed attempts; product qualification remains separate and incomplete".into());
    }
    Ok(())
}
