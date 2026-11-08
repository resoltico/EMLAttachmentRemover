use crate::{Result, host, run as command};
use std::process::Command;

pub(crate) fn run(rust_only: bool) -> Result<()> {
    if !rust_only {
        crate::lint::run()?;
    }
    if cfg!(target_os = "linux") {
        crate::tools::qt()?;
    }
    let target = host()?;
    let exclusions = if cfg!(windows) {
        &["--exclude", "eml-app", "--exclude", "eml-gui"][..]
    } else {
        &[]
    };
    // Force warnings keep structural review thresholds visible even with warnings=deny.
    let mut clippy = Command::new("cargo");
    clippy
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--target",
            &target,
        ])
        .args(exclusions);
    if cfg!(target_os = "macos") {
        clippy.args(["--features", "eml-gui/desktop-winit"]);
    } else if cfg!(target_os = "linux") {
        clippy.args(["--features", "eml-gui/desktop-qt"]);
    }
    clippy.args([
        "--",
        "--force-warn",
        "clippy::too_many_lines",
        "--force-warn",
        "clippy::cognitive_complexity",
    ]);
    command(&mut clippy)?;
    command(
        Command::new("cargo")
            .args(["test", "--workspace", "--locked"])
            .args(["--target", &target])
            .args(exclusions),
    )?;
    if cfg!(target_os = "linux") {
        // Native adapter tests use Slint's testing backend and pure portal responses.
        command(Command::new("cargo").args([
            "test",
            "--locked",
            "-p",
            "eml-gui",
            "--lib",
            "--features",
            "desktop-qt",
            "--target",
            &target,
        ]))?;
        // The scoped Qt backend patch is a dependency, outside workspace test discovery.
        command(Command::new("cargo").args([
            "test",
            "--locked",
            "-p",
            "i-slint-backend-qt",
            "--lib",
            "--target",
            &target,
        ]))?;
    }
    if !rust_only && cfg!(target_os = "macos") {
        let out = tempfile::tempdir()?;
        crate::dist::build(false, true, true, None, out.path())?;
    }
    Ok(())
}
