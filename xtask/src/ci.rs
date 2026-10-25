use crate::{Result, host, run as command, tool};
use std::process::Command;

pub(crate) fn run(rust_only: bool) -> Result<()> {
    if !rust_only {
        command(Command::new("cargo").args(["fmt", "--all", "--check"]))?;
        tool("cargo-deny", "cargo_deny")?;
        tool("actionlint", "actionlint")?;
        tool("shellcheck", "shellcheck")?;
        command(Command::new("cargo").args(["deny", "--locked", "check"]))?;
        command(&mut Command::new("actionlint"))?;
        command(Command::new("bash").args(["-c", "git ls-files -z '*.sh' | xargs -0 shellcheck"]))?;
        command(Command::new("bash").arg("scripts/check-budget.sh"))?;
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
    if !cfg!(windows) {
        clippy.args(["--features", "eml-gui/desktop"]);
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
    if !rust_only && cfg!(target_os = "macos") {
        let out = tempfile::tempdir()?;
        crate::dist::build(false, true, true, None, out.path())?;
    }
    Ok(())
}
