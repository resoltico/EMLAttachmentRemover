//! Controlled finite preparation measurements against the frozen shipping CLI.
use crate::{Result, cli_build::FrozenCli, controller};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path, process::Command, time::Duration};
use xtask::archive::ByteIdentity;
pub(crate) fn run(root: &Path, cli: &FrozenCli, out: &Path) -> Result<Value> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let owner = xtask::archive::create_artifact_directory(&out)?;
    let binary = owner.path().join("remove-eml-attachments");
    let mut image = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&binary)?;
    image.write_all(cli.bytes())?;
    image.set_permissions(fs::Permissions::from_mode(0o755))?;
    image.sync_all()?;
    drop(image);
    if ByteIdentity::of(&fs::read(&binary)?) != *cli.identity() {
        return Err("Resource executable differs from frozen shipping bytes".into());
    }
    let source = crate::git(root, &["rev-parse", "HEAD"])?;
    let mut command = Command::new("python3");
    command
        .current_dir(root)
        .arg(root.join("tools/measure_preparation.py"))
        .arg(&binary)
        .arg(out.join("measurement"))
        .arg(&source)
        .env("LC_ALL", "C");
    let log = out.join("campaign.log");
    let observed = controller()?.run(command, &log, Duration::from_secs(600));
    let measurements = (|| -> Result<Value> {
        Ok(serde_json::from_slice(&fs::read(
            out.join("measurement/measurements.json"),
        )?)?)
    })();
    let result = json!({"source":source,"binary":cli.identity(),"process":observed.as_ref().ok().map(|p|p.evidence()),"process_failure":observed.as_ref().err().map(ToString::to_string),"measurements":measurements.as_ref().ok(),"measurement_read_failure":measurements.as_ref().err().map(ToString::to_string)});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    let process = observed?;
    process.require_success(&log)?;
    measurements?;
    if result["measurements"]["passed"] != true
        || result["measurements"]["source"] != source
        || result["measurements"]["binary_sha256"] != cli.identity().sha256
        || ByteIdentity::of(&fs::read(&binary)?) != *cli.identity()
    {
        return Err("Finite resource qualification failed or changed identity".into());
    }
    Ok(result)
}
