//! Direct owned signed-app startup diagnostics, separate from Finder qualification.
use crate::{Result, controller, macos, macos_app};
use serde_json::json;
use std::{fs, path::Path, process::Command, time::Duration};

pub(crate) fn run(root: &Path, out: &Path) -> Result<()> {
    for (key, value) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
    ] {
        if std::env::var(key).as_deref() != Ok(value) {
            return Err(
                "Direct app startup observation requires its fresh owned hosted-runner job".into(),
            );
        }
    }
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let build = out.join("app-build");
    macos_app::run(root, &build)?;
    let context = macos::load(root)?;
    let app = build.join("EML Attachment Remover.app");
    let executable = app.join("Contents/MacOS/EMLAttachmentRemover");
    let bytes = fs::read(&executable)?;
    let image = macos::fixture_identity(&bytes, &context.architecture, context.minimum)?;
    let mut signature = context.command(root, "/usr/bin/codesign");
    signature
        .args(["--verify", "--strict", "--verbose=2"])
        .arg(&app);
    macos::checked_run(signature, &out.join("signature.log"), 30)?;
    fs::write(
        out.join("input.json"),
        serde_json::to_vec_pretty(
            &json!({"app_build":"app-build/build.json","image":image,"binary":xtask::archive::ByteIdentity::of(&bytes),"invocation":["-copyDestinationBytes",""],"scope":"direct owned executable startup only; no file ingress, Finder equivalence or GUI acceptance"}),
        )?,
    )?;
    let mut command = Command::new(&executable);
    command
        .current_dir(&out)
        .args(["-copyDestinationBytes", ""]);
    let result = controller()?.run_separate_output(
        command,
        &out.join("startup.log"),
        Duration::from_secs(10),
    )?;
    fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(
            &json!({"process":result.evidence(),"scope":"finite direct signed executable observation; timeout cleanup is not a normal quit or GUI pass; no later process admission by this Controller/command"}),
        )?,
    )?;
    // A surviving idle app reaches the finite allowance and seals admission;
    // only file evidence is written afterward. No later GUI case is attempted.
    if result.cancelled || result.unexpected_survivors != 0 || !result.cleanup_complete {
        return Err(
            "Direct app startup cleanup is unverified; retain owned resources and abort".into(),
        );
    }
    if !result.timed_out && !result.status.success() {
        return Err("Direct app exited abnormally; retained actual status and output".into());
    }
    Ok(())
}
