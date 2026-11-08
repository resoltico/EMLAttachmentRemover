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
    // Native backends are platform selections, not an all-features union.
    // Headless covers every target; native passes include both CPU architectures.
    for (features, targets) in [
        ("eml-gui/renderer-software", &[][..]),
        (
            "eml-gui/desktop-winit,eml-gui/renderer-software",
            &["aarch64-apple-darwin", "x86_64-apple-darwin"][..],
        ),
        (
            "eml-gui/desktop-winit,eml-gui/renderer-skia",
            &["aarch64-apple-darwin", "x86_64-apple-darwin"][..],
        ),
        (
            "eml-gui/desktop-winit,eml-gui/renderer-software,eml-gui/renderer-skia",
            &["aarch64-apple-darwin", "x86_64-apple-darwin"][..],
        ),
        (
            "eml-gui/desktop-qt",
            &["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"][..],
        ),
        (
            "eml-gui/desktop-qt,eml-gui/renderer-software",
            &["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"][..],
        ),
    ] {
        if targets.is_empty() {
            dependency_graph(features, None)?;
        }
        for target in targets {
            dependency_graph(features, Some(target))?;
        }
    }
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

fn dependency_graph(features: &str, target: Option<&str>) -> Result<()> {
    let mut deny = Command::new("cargo");
    deny.args([
        "deny",
        "--locked",
        "--no-default-features",
        "--features",
        features,
    ]);
    if let Some(target) = target {
        deny.args(["--target", target]);
    }
    command(deny.arg("check"))
}
