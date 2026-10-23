//! Exact-version developer tools supplied by the caller's environment.
use crate::{Result, campaign, capture};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const CARGO_TOOLS: [(&str, &str); 4] = [
    ("deny", "cargo_deny"),
    ("about", "cargo_about"),
    ("mutants", "cargo_mutants"),
    ("fuzz", "cargo_fuzz"),
];

pub(crate) fn compiler(root: &Path) -> Result<String> {
    let authority: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml"))?)?;
    Ok(authority["toolchain"]["channel"]
        .as_str()
        .ok_or("Missing compiler authority")?
        .to_owned())
}

/// Resolve once so an isolated campaign uses the same checked executable.
pub(crate) fn path(root: &Path, name: &str) -> Result<PathBuf> {
    let pin = CARGO_TOOLS
        .iter()
        .find(|(tool, _)| *tool == name)
        .ok_or("Unknown repository tool")?
        .1;
    let expected = campaign::pin(root, pin)?;
    path_pinned(root, name, &expected)
}
pub(crate) fn path_pinned(root: &Path, name: &str, expected: &str) -> Result<PathBuf> {
    if !CARGO_TOOLS.iter().any(|(tool, _)| *tool == name) {
        return Err("Unknown pinned repository tool".into());
    }
    let filename = format!("cargo-{name}{}", std::env::consts::EXE_SUFFIX);
    let search = std::env::var_os("PATH").ok_or("PATH is missing")?;
    let candidate = std::env::split_paths(&search)
        .map(|directory| directory.join(&filename))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("Install {filename} {expected} and put it on PATH; versions are declared in xtask/tools.toml"))?
        .canonicalize()?;
    let mut query = Command::new(&candidate);
    query.current_dir(root).args([name, "--version"]);
    let actual = String::from_utf8(capture(&mut query)?)?;
    if actual.trim() != format!("cargo-{name} {expected}") {
        return Err(format!("Expected cargo-{name} {expected} at {}; install the version declared in xtask/tools.toml", candidate.display()).into());
    }
    Ok(candidate)
}

pub(crate) fn command(root: &Path, name: &str) -> Result<Command> {
    let mut command = Command::new(path(root, name)?);
    command
        .current_dir(root)
        .arg(name)
        .env("CARGO_HOME", campaign::cargo_home()?)
        .env("RUSTUP_TOOLCHAIN", compiler(root)?);
    Ok(command)
}
