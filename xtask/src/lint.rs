//! Source/configuration checks shared by local CI and the hosted lint job.
use crate::{Result, run as command, tool};
use std::process::Command;
pub(crate) fn run() -> Result<()> {
    command(Command::new("cargo").args(["fmt", "--all", "--check"]))?;
    for (name, key) in [
        ("cargo-deny", "cargo_deny"),
        ("actionlint", "actionlint"),
        ("shellcheck", "shellcheck"),
    ] {
        tool(name, key)?;
    }
    command(Command::new("cargo").args(["deny", "--all-features", "--locked", "check"]))?;
    command(Command::new("cargo").args([
        "deny",
        "--manifest-path",
        "fuzz/Cargo.toml",
        "--locked",
        "check",
    ]))?;
    command(&mut Command::new("actionlint"))?;
    command(Command::new("bash").arg("scripts/check-shell.sh"))?;
    command(Command::new("bash").arg("scripts/check-budget.sh"))
}
