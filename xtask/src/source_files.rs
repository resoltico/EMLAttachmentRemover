//! Contained tracked and pending checkout files for actual language tools.
use crate::{Result, capture};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
    process::Command,
};
pub(crate) fn checkout_files(root: &Path) -> Result<BTreeSet<String>> {
    let listing = capture(Command::new("git").current_dir(root).args([
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    ]))?;
    let canonical = root.canonicalize()?;
    let mut paths = BTreeSet::new();
    for raw in listing.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let relative = std::str::from_utf8(raw)?;
        if Path::new(relative)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("Checkout file path is not ordinary relative spelling".into());
        }
        let path = root.join(relative);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if !metadata.is_file() {
            if metadata.file_type().is_symlink() {
                return Err(
                    format!("Tool input must be an ordinary checkout file: {relative}").into(),
                );
            }
            continue;
        }
        if !path.canonicalize()?.starts_with(&canonical) {
            return Err(format!("Tool input escaped checkout: {relative}").into());
        }
        paths.insert(relative.to_owned());
    }
    Ok(paths)
}
fn shell(path: &Path) -> Result<bool> {
    use std::io::Read;
    let mut bytes = [0; 128];
    let size = fs::File::open(path)?.read(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes[..size]);
    let line = text.lines().next().unwrap_or("");
    Ok(line.starts_with("#!")
        && line.split_whitespace().any(|word| {
            matches!(
                word.rsplit('/').next(),
                Some("bash" | "sh" | "dash" | "ksh" | "zsh")
            )
        }))
}
fn authored(root: &Path) -> Result<BTreeSet<String>> {
    let patches = crate::repository_config::path_patch_roots(root)?;
    Ok(checkout_files(root)?
        .into_iter()
        .filter(|path| {
            !patches
                .iter()
                .any(|patch| Path::new(path).starts_with(patch))
        })
        .collect())
}
pub(crate) fn shells(root: &Path) -> Result<Vec<String>> {
    authored(root)?
        .into_iter()
        .filter_map(|path| match shell(&root.join(&path)) {
            Ok(true) => Some(Ok(path)),
            Ok(false) if path.ends_with(".sh") => Some(Ok(path)),
            Ok(false) => None,
            Err(e) => Some(Err(e)),
        })
        .collect()
}
#[cfg(target_os = "macos")]
pub(crate) fn swift(root: &Path) -> Result<Vec<String>> {
    Ok(authored(root)?
        .into_iter()
        .filter(|path| path.ends_with(".swift"))
        .collect())
}
