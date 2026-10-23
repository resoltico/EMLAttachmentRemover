//! Exact-source native CI checkout beneath an admitted user-profile namespace.
use crate::{Result, capture, git, git_command};
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
    let tool_checkout = ordinary_drive_path(&checkout)?;
    capture(&mut clone_command(root, &checkout)?)?;
    capture(git_command(&tool_checkout).args(["checkout", "--quiet", "--detach", &source]))?;
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
    let record = serde_json::to_vec(
        &serde_json::json!({"source":source,"path":ordinary_drive_path(&checkout)?,"scratch":ordinary_drive_path(&scratch)?,"scope":"new protected native qualification checkout; original workspace permissions unchanged"}),
    )?;
    fs::write(
        checkout.join("target/windows-native-workspace.json"),
        &record,
    )?;
    println!("{}", std::str::from_utf8(&record)?);
    Ok(())
}

// One production boundary for this exact local-source clone. The same command
// is exercised by the real native Git regression control below.
fn clone_command(root: &Path, destination: &Path) -> Result<Command> {
    let mut command = git_command(root);
    command
        .args(["clone", "--no-local", "--quiet", "--no-checkout"])
        .arg(ordinary_drive_path(root)?)
        .arg(ordinary_drive_path(destination)?);
    Ok(command)
}

/// Ordinary expressions for external tools accepting finite short native drive paths.
/// Keep the original native paths/handles for filesystem admission; remove only
/// the verbatim prefix at the external argument boundary, without lossy UTF16 text.
pub(crate) fn ordinary_drive_path(path: &Path) -> Result<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if units.get(..4) != Some(&[92, 92, 63, 92])
        || !units
            .get(4)
            .is_some_and(|drive| (65..=90).contains(drive) || (97..=122).contains(drive))
        || units.get(5) != Some(&58)
        || units.get(6) != Some(&92)
        || units.len() >= 260
    {
        return Err("External Windows command needs a short native drive path".into());
    }
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&units[4..])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    #[test]
    fn ordinary_drive_path_preserves_exact_utf16_and_refuses_other_path_domains() {
        let units = [92, 92, 63, 92, 67, 58, 92, 65, 32, 0xd800, 0xdc00, 0xdfff];
        let input = PathBuf::from(std::ffi::OsString::from_wide(&units));
        assert_eq!(
            ordinary_drive_path(&input)
                .unwrap()
                .as_os_str()
                .encode_wide()
                .collect::<Vec<_>>(),
            units[4..]
        );
        for path in [
            r"C:\source",
            r"\\server\share\source",
            r"\\?\UNC\server\share",
            r"\\?\1:\source",
            r"\\?\C:source",
            r"relative",
        ] {
            assert!(ordinary_drive_path(Path::new(path)).is_err(), "{path}");
        }
        let mut bound = vec![b'x' as u16; 259];
        bound[..7].copy_from_slice(&[92, 92, 63, 92, 67, 58, 92]);
        assert!(ordinary_drive_path(&PathBuf::from(std::ffi::OsString::from_wide(&bound))).is_ok());
        bound.push(b'x' as u16);
        assert!(
            ordinary_drive_path(&PathBuf::from(std::ffi::OsString::from_wide(&bound))).is_err()
        );
    }

    #[test]
    fn actual_local_git_clone_uses_canonical_source_as_local_drive_and_preserves_head_and_bytes() {
        fn observed(command: &mut std::process::Command) -> Vec<u8> {
            let output = command
                .output()
                .expect("Native Windows Git must be installed for the clone boundary control");
            assert!(
                output.status.success(),
                "Native Git failed: {:?}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        }
        let owned = tempfile::tempdir().unwrap();
        let source = owned.path().join("source λ");
        fs::create_dir(&source).unwrap();
        let source = source.canonicalize().unwrap();
        let payload = b"\0exact owned binary fixture\xff\n";
        fs::write(source.join("payload.bin"), payload).unwrap();
        observed(git_command(&source).args(["-c", "init.templateDir=", "init", "--quiet"]));
        observed(git_command(&source).args([
            "-c",
            "user.name=Owned clone control",
            "-c",
            "user.email=control@example.invalid",
            "add",
            "payload.bin",
        ]));
        let hooks = source.join(".git/owned-empty-hooks");
        fs::create_dir(&hooks).unwrap();
        let mut hooks_config = std::ffi::OsString::from("core.hooksPath=");
        hooks_config.push(ordinary_drive_path(&hooks).unwrap());
        observed(
            git_command(&source)
                .args([
                    "-c",
                    "user.name=Owned clone control",
                    "-c",
                    "user.email=control@example.invalid",
                    "-c",
                    "commit.gpgSign=false",
                    "-c",
                ])
                .arg(hooks_config)
                .args(["commit", "--quiet", "-m", "Owned local source"]),
        );
        let head = observed(git_command(&source).args(["rev-parse", "HEAD"]));
        let tree = observed(git_command(&source).args(["rev-parse", "HEAD^{tree}"]));
        let destination = owned.path().canonicalize().unwrap().join("checkout λ");
        let tool_destination = ordinary_drive_path(&destination).unwrap();
        observed(&mut clone_command(&source, &destination).unwrap());
        let head_text = std::str::from_utf8(&head).unwrap().trim();
        observed(
            git_command(&tool_destination).args(["checkout", "--quiet", "--detach", head_text]),
        );
        assert_eq!(
            observed(git_command(&tool_destination).args(["rev-parse", "HEAD"])),
            head
        );
        assert_eq!(
            observed(git_command(&tool_destination).args(["rev-parse", "HEAD^{tree}"])),
            tree
        );
        assert_eq!(fs::read(destination.join("payload.bin")).unwrap(), payload);
    }
}
