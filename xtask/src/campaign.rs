//! Source-isolated owned campaigns and exact process exit/deadline handling.
use crate::{Result, capture, git};
use serde::Serialize;
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(crate) struct Campaign {
    pub directory: PathBuf,
    pub source: PathBuf,
    pub sha: String,
}
impl Campaign {
    pub fn create(root: &Path, role: &str) -> Result<Self> {
        require_stock_topology(root)?;
        let dirty = git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
        if !dirty.is_empty() {
            return Err("Commit the current workspace before a source-isolated campaign; dirty/new inputs must not be omitted".into());
        }
        let sha = git(root, &["rev-parse", "HEAD"])?;
        let parent = root.join("target").join(role);
        fs::create_dir_all(&parent)?;
        let directory = tempfile::Builder::new()
            .prefix("run-")
            .tempdir_in(&parent)?
            .keep();
        let source = directory.join("source");
        capture(
            Command::new("git")
                .args(["clone", "--no-local", "--quiet"])
                .arg(root)
                .arg(&source),
        )?;
        if git(&source, &["rev-parse", "HEAD"])? != sha {
            return Err("Campaign source clone identity differs from admitted HEAD".into());
        }
        // Cargo resolves configuration from the actual CWD and all ancestors,
        // including ignored target/.cargo paths between root and snapshot.
        require_stock_topology(&source)?;
        fs::create_dir(directory.join("tmp"))?;
        Ok(Self {
            directory,
            source,
            sha,
        })
    }
    pub fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(&self.source)
            .env("TMPDIR", self.directory.join("tmp"))
            .env("TMP", self.directory.join("tmp"))
            .env("TEMP", self.directory.join("tmp"));
        command.env("CARGO_TARGET_DIR", self.directory.join("build"));
        command
    }
    pub fn report(&self, value: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        File::options()
            .write(true)
            .create_new(true)
            .open(self.directory.join("result.json"))?
            .write_all(&bytes)?;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&bytes)?;
        stdout.write_all(b"\n")?;
        Ok(())
    }
}
pub(crate) fn pin(root: &Path, name: &str) -> Result<String> {
    let table: toml::Value = toml::from_str(&fs::read_to_string(root.join("xtask/tools.toml"))?)?;
    let version = table
        .get("versions")
        .and_then(|value| value.get(name))
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or_else(|| format!("Missing or invalid tool pin {name}"))?;
    Ok(version.to_owned())
}
pub(crate) fn tool_version(root: &Path, tool: &str, pin_name: &str) -> Result<String> {
    let expected = pin(root, pin_name)?;
    let actual = String::from_utf8(capture(
        Command::new("cargo")
            .current_dir(root)
            .args([tool, "--version"]),
    )?)?;
    if actual.trim() != format!("cargo-{tool} {expected}") {
        return Err(format!(
            "Expected cargo-{tool} {expected}; install that version with cargo install --locked"
        )
        .into());
    }
    Ok(expected)
}
pub(crate) type ProcessResult = xtask::process::ProcessResult;
pub(crate) fn run_logged(
    command: Command,
    log: &Path,
    deadline: Duration,
) -> Result<ProcessResult> {
    crate::controller()?.run(command, log, deadline)
}
pub(crate) fn require_unix_campaign() -> Result<()> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err("These bounded local campaigns require a Unix host for owned process-group deadline cleanup".into())
    }
}

pub(crate) fn effective_configs(root: &Path) -> Vec<PathBuf> {
    let mut folders = root
        .ancestors()
        .map(|path| path.join(".cargo"))
        .collect::<Vec<_>>();
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".cargo")));
    if let Some(folder) = cargo_home {
        folders.push(folder);
    }
    let mut files = Vec::new();
    for folder in folders {
        let legacy = folder.join("config");
        let current = folder.join("config.toml");
        let file = if legacy.is_file() { legacy } else { current };
        if file.is_file() && !files.contains(&file) {
            files.push(file);
        }
    }
    files
}
fn hook_name(name: &str) -> bool {
    [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTC",
        "RUSTDOC",
        "CC",
        "CXX",
        "HOST_CC",
        "HOST_CXX",
        "TARGET_CC",
        "TARGET_CXX",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTC",
        "CARGO_BUILD_RUSTDOC",
        "CARGO_BUILD_RUSTFLAGS",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
    ]
    .contains(&name)
        || (name.starts_with("CARGO_TARGET_")
            && (name.ends_with("_RUNNER")
                || name.ends_with("_LINKER")
                || name.ends_with("_RUSTFLAGS")))
}
fn config_hooks(value: &toml::Value) -> bool {
    if value
        .get("build")
        .and_then(|build| build.get("rustflags"))
        .is_some_and(|flags| flags.as_array().is_none_or(|flags| !flags.is_empty()))
    {
        return true;
    }
    if value
        .get("build")
        .and_then(toml::Value::as_table)
        .is_some_and(|table| {
            [
                "rustc",
                "rustdoc",
                "rustc-wrapper",
                "rustc-workspace-wrapper",
            ]
            .iter()
            .any(|key| {
                table
                    .get(*key)
                    .is_some_and(|value| value.as_str() != Some(""))
            })
        })
    {
        return true;
    }
    if value
        .get("target")
        .and_then(toml::Value::as_table)
        .is_some_and(|targets| {
            targets.iter().any(|(name, target)| {
                if target.get("runner").is_some() || target.get("linker").is_some() {
                    return true;
                }
                let Some(flags) = target.get("rustflags") else {
                    return false;
                };
                if flags.as_array().is_some_and(Vec::is_empty) {
                    return false;
                }
                let approved = [
                    "-C",
                    "linker-flavor=ld.lld",
                    "-C",
                    "link-self-contained=yes",
                    "-C",
                    "target-feature=+crt-static",
                ];
                !["x86_64-pc-windows-gnullvm", "aarch64-pc-windows-gnullvm"]
                    .contains(&name.as_str())
                    || flags.as_array().is_none_or(|flags| {
                        flags.len() != approved.len()
                            || flags
                                .iter()
                                .zip(approved)
                                .any(|(flag, expected)| flag.as_str() != Some(expected))
                    })
            })
        })
    {
        return true;
    }
    value
        .get("env")
        .and_then(toml::Value::as_table)
        .is_some_and(|table| {
            table
                .iter()
                .any(|(key, value)| hook_name(key) && value.as_str() != Some(""))
        })
}
fn require_stock_topology(root: &Path) -> Result<()> {
    for (key, value) in std::env::vars_os() {
        if key.to_str().is_some_and(hook_name) && !value.is_empty() {
            return Err("Compiler/runner/wrapper overrides require explicit topology review before a bounded campaign".into());
        }
    }
    for file in effective_configs(root) {
        let text = fs::read_to_string(file)?;
        let parsed: toml::Value = toml::from_str(&text)
            .map_err(|_| "Cannot parse effective Cargo configuration; review it locally")?;
        if config_hooks(&parsed) {
            return Err(
                "Effective Cargo configuration contains an unreviewed compiler/runner/wrapper hook"
                    .into(),
            );
        }
    }
    Ok(())
}
#[cfg(test)]
mod topology_tests {
    use super::*;
    #[test]
    fn wrappers_and_runners_require_review_without_exposing_values() {
        for config in [
            "[build]\nrustc-wrapper='cache'",
            "[target.host]\nrunner='daemon'",
            "[target.host]\nrustflags=['-C','linker=/wrapper']",
            "[build]\nrustflags=['-C','linker=/wrapper']",
            "[env]\nRUSTC_WRAPPER='cache'",
        ] {
            assert!(config_hooks(&toml::from_str(config).unwrap()));
        }
        assert!(!config_hooks(&toml::from_str("[target.x86_64-pc-windows-gnullvm]\nrustflags=['-C','linker-flavor=ld.lld','-C','link-self-contained=yes','-C','target-feature=+crt-static']").unwrap()));
    }
}
