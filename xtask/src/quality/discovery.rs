//! Checkout discovery includes pending authored sources and referenced-source validation.
use crate::{Result, capture};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

pub(super) fn sources(root: &Path) -> Result<BTreeSet<String>> {
    let listing = capture(Command::new("git").current_dir(root).args([
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    ]))?;
    let canonical = root.canonicalize()?;
    let mut paths = BTreeSet::new();
    for raw in listing
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(raw)?;
        let input = root.join(path);
        let metadata = match fs::symlink_metadata(&input) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file() {
            if metadata.file_type().is_symlink() && is_source(path) {
                return Err(
                    format!("Authored source must be an ordinary checkout file: {path}").into(),
                );
            }
            continue;
        }
        if !input.canonicalize()?.starts_with(&canonical) {
            return Err(format!("Source escaped checkout: {path}").into());
        }
        if is_source(path) || is_shell(&input)? {
            paths.insert(path.to_owned());
        }
    }
    for extension in ["rs", "swift", "slint", "sh"] {
        if !paths
            .iter()
            .any(|path| path.ends_with(&format!(".{extension}")))
        {
            return Err(format!("Authored {extension} discovery is empty").into());
        }
    }
    Ok(paths)
}
fn is_source(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|value| value.to_str()),
        Some("rs" | "swift" | "slint" | "sh" | "ps1")
    )
}
fn is_shell(path: &Path) -> Result<bool> {
    use std::io::Read;
    let mut prefix = [0; 128];
    let length = fs::File::open(path)?.read(&mut prefix)?;
    let first = String::from_utf8_lossy(&prefix[..length]);
    let line = first.lines().next().unwrap_or("");
    Ok(line.starts_with("#!")
        && line.split_whitespace().any(|word| {
            matches!(
                word.rsplit('/').next(),
                Some("bash" | "sh" | "dash" | "ksh" | "zsh")
            )
        }))
}
pub(super) fn shells(root: &Path) -> Result<Vec<String>> {
    sources(root)?
        .into_iter()
        .filter_map(|path| match is_shell(&root.join(&path)) {
            Ok(true) => Some(Ok(path)),
            Ok(false) if path.ends_with(".sh") => Some(Ok(path)),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}
