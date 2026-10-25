//! Shell linting for authored scripts.
use crate::{Result, campaign, capture};
use serde_json::{Value, json};
use std::{path::Path, process::Command};
pub(crate) fn check_shell(root: &Path) -> Result<Value> {
    let expected = campaign::pin(root, "shellcheck")?;
    let actual = String::from_utf8(capture(Command::new("shellcheck").arg("--version"))?)?;
    if !actual
        .lines()
        .any(|line| line == format!("version: {expected}"))
    {
        return Err("Install shellcheck at the xtask/tools.toml version".into());
    }
    let paths = crate::quality::shell_sources(root)?;
    if !paths.is_empty() {
        let mut command = Command::new("shellcheck");
        command
            .current_dir(root)
            .env_remove("SHELLCHECK_OPTS")
            .arg("--rcfile")
            .arg(root.join(".shellcheckrc"))
            .arg("--")
            .args(&paths);
        capture(&mut command)?;
    }
    Ok(json!({"version":expected,"authored_scripts":paths.len()}))
}
