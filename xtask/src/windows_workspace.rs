//! Exact-source native CI checkout beneath an admitted user-profile namespace.
use crate::{Result, capture, git};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
pub(crate) fn prepare(root: &Path) -> Result<()> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err(
            "Protected Windows verification checkout requires a fresh hosted runner".into(),
        );
    }
    let source = git(root, &["rev-parse", "HEAD"])?;
    if !git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty() {
        return Err("Commit source before preparing native verification checkout".into());
    }
    let parent =
        PathBuf::from(std::env::var_os("USERPROFILE").ok_or("Missing native runner user profile")?)
            .join("AppData/Local/Temp");
    let enclosure = xtask::archive::create_artifact_directory(&parent)?;
    let checkout = enclosure.path().join("source");
    let tool_checkout = transport(&checkout)?;
    capture(
        Command::new("git")
            .current_dir(root)
            .args(["clone", "--no-local", "--quiet", "--no-checkout"])
            .arg(root)
            .arg(&tool_checkout),
    )?;
    capture(
        Command::new("git")
            .current_dir(&tool_checkout)
            .args(["checkout", "--quiet", "--detach", &source]),
    )?;
    if git(&tool_checkout, &["rev-parse", "HEAD"])? != source
        || !git(
            &tool_checkout,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?
        .is_empty()
    {
        return Err("Protected checkout differs from its exact clean source".into());
    }
    let scratch = checkout.join("target/native-scratch");
    fs::create_dir_all(&scratch)?;
    // The fresh selected profile path is short. Remove only the native verbatim
    // drive prefix for Git/GitHub/.NET transport, preserving UTF16. Native
    // filesystem admission retains the original canonical handles/address.
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    fn transport(path: &Path) -> Result<PathBuf> {
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.get(..4) != Some(&[92, 92, 63, 92])
            || units.get(5) != Some(&58)
            || units.len() >= 260
        {
            return Err("Protected checkout needs a short native drive path".into());
        }
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&units[4..])))
    }
    let record = serde_json::to_vec(
        &serde_json::json!({"source":source,"path":transport(&checkout)?,"scratch":transport(&scratch)?,"scope":"new protected native qualification checkout; original workspace permissions unchanged"}),
    )?;
    fs::write(
        checkout.join("target/windows-native-workspace.json"),
        &record,
    )?;
    println!("{}", std::str::from_utf8(&record)?);
    Ok(())
}
