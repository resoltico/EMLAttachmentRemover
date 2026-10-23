//! Cargo manifest ownership and lint inheritance; language compilers own source.
use crate::{Result, capture};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
    process::Command,
};
#[derive(Serialize)]
pub(crate) struct ConfigEvidence {
    passed: bool,
    workspace_members: BTreeSet<String>,
    standalone_manifests: BTreeSet<String>,
    path_patch_roots: BTreeSet<String>,
}
fn manifest(root: &Path, relative: &str) -> Result<toml::Value> {
    ordinary(root, Path::new(relative))?;
    Ok(toml::from_str(&fs::read_to_string(root.join(relative))?)?)
}
fn ordinary(root: &Path, relative: &Path) -> Result<()> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Manifest ownership requires contained ordinary relative paths".into());
    }
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(format!(
                "Manifest ownership cannot follow a symlink: {}",
                relative.display()
            )
            .into());
        }
    }
    if !current.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err("Manifest ownership escaped the checkout".into());
    }
    Ok(())
}
pub(crate) fn path_patch_roots(root: &Path) -> Result<BTreeSet<String>> {
    patch_roots(root, &manifest(root, "Cargo.toml")?)
}
pub(crate) fn standalone_manifests(root: &Path) -> Result<BTreeSet<String>> {
    let authority = manifest(root, "Cargo.toml")?;
    let patches = patch_roots(root, &authority)?;
    let exclusions = authority
        .get("workspace")
        .and_then(|workspace| workspace.get("exclude"))
        .and_then(toml::Value::as_array)
        .ok_or("Workspace exclusions are absent")?;
    let mut manifests = BTreeSet::new();
    for value in exclusions {
        let directory = value.as_str().ok_or("Workspace exclusion must be a path")?;
        if patches.contains(directory) {
            continue;
        }
        let path = format!("{directory}/Cargo.toml");
        ordinary(root, Path::new(&path))?;
        if !root.join(&path).is_file() || !manifests.insert(path) {
            return Err("Invalid or duplicate authored standalone graph".into());
        }
    }
    Ok(manifests)
}
fn patch_roots(root: &Path, authority: &toml::Value) -> Result<BTreeSet<String>> {
    let mut roots = BTreeSet::new();
    let mut canonical_roots = BTreeSet::new();
    let Some(registries) = authority.get("patch").and_then(toml::Value::as_table) else {
        return Ok(roots);
    };
    for patches in registries.values() {
        for entry in patches
            .as_table()
            .ok_or("Invalid Cargo patch table")?
            .values()
        {
            let Some(path) = entry.get("path").and_then(toml::Value::as_str) else {
                continue;
            };
            ordinary(root, Path::new(path))?;
            if !root.join(path).is_dir()
                || !root.join(path).join("Cargo.toml").is_file()
                || (!roots.insert(path.to_owned())
                    || !canonical_roots.insert(root.join(path).canonicalize()?))
            {
                return Err(format!("Invalid or duplicate path-patch ownership: {path}").into());
            }
        }
    }
    Ok(roots)
}
fn lints(path: &str, document: &toml::Value, expected: &toml::Value, member: bool) -> Result<()> {
    if member {
        let table = document
            .get("lints")
            .and_then(toml::Value::as_table)
            .ok_or("Workspace member has no lint inheritance")?;
        if table.len() != 1 || table.get("workspace").and_then(toml::Value::as_bool) != Some(true) {
            return Err(
                format!("{path}: workspace member must inherit the complete lint table").into(),
            );
        }
    } else if document.get("lints") != Some(expected) {
        return Err(
            format!("{path}: standalone lint table differs from workspace authority").into(),
        );
    }
    Ok(())
}
fn inspect(root: &Path, metadata: &Value, paths: &BTreeSet<String>) -> Result<ConfigEvidence> {
    let authority = manifest(root, "Cargo.toml")?;
    let workspace = authority.get("workspace").ok_or("Missing root workspace")?;
    let expected = workspace
        .get("lints")
        .ok_or("Missing authoritative workspace lint table")?;
    let patches = patch_roots(root, &authority)?;
    let members = metadata["workspace_members"]
        .as_array()
        .ok_or("Cargo metadata has no workspace members")?;
    if members.is_empty() {
        return Err("Cargo metadata workspace is empty".into());
    }
    let mut known = BTreeSet::from(["Cargo.toml".to_owned()]);
    let mut workspace_members = BTreeSet::new();
    let canonical = root.canonicalize()?;
    for id in members {
        let package = metadata["packages"]
            .as_array()
            .ok_or("Cargo metadata has no packages")?
            .iter()
            .find(|p| p["id"] == *id)
            .ok_or("Workspace member has no metadata package")?;
        let path = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("Package manifest path is absent")?,
        );
        let relative = path
            .strip_prefix(&canonical)?
            .components()
            .map(|c| c.as_os_str().to_str().ok_or("Manifest path must be UTF-8"))
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("/");
        let relative = relative.as_str();
        if patches.iter().any(|p| Path::new(relative).starts_with(p)) {
            return Err("A path patch overlaps authored workspace membership".into());
        }
        let document = manifest(root, relative)?;
        lints(relative, &document, expected, true)?;
        known.insert(relative.to_owned());
        workspace_members.insert(relative.to_owned());
    }
    let mut standalone = BTreeSet::new();
    let exclusions = workspace
        .get("exclude")
        .and_then(toml::Value::as_array)
        .ok_or("Workspace exclusions must declare standalone and patched roots")?;
    for excluded in exclusions {
        let directory = excluded
            .as_str()
            .ok_or("Workspace exclusions must be paths")?;
        if patches.contains(directory) {
            continue;
        }
        let path = format!("{directory}/Cargo.toml");
        if workspace_members.contains(&path) {
            return Err("Standalone root overlaps workspace membership".into());
        }
        let document = manifest(root, &path)?;
        lints(&path, &document, expected, false)?;
        known.insert(path.clone());
        standalone.insert(path);
    }
    for patch in &patches {
        if !exclusions.iter().any(|v| v.as_str() == Some(patch)) {
            return Err(
                format!("Path patch must be excluded from authored workspace: {patch}").into(),
            );
        }
    }
    for path in paths {
        if !known.contains(path)
            && !patches
                .iter()
                .any(|patch| Path::new(path).starts_with(patch))
        {
            return Err(format!("Unclassified authored Cargo manifest: {path}").into());
        }
    }
    Ok(ConfigEvidence {
        passed: true,
        workspace_members,
        standalone_manifests: standalone,
        path_patch_roots: patches,
    })
}
pub(crate) fn check(root: &Path) -> Result<ConfigEvidence> {
    let metadata = capture(Command::new("cargo").current_dir(root).args([
        "metadata",
        "--locked",
        "--offline",
        "--no-deps",
        "--format-version",
        "1",
    ]))?;
    if metadata.len() > 16 * 1024 * 1024 {
        return Err("Cargo metadata exceeds its bound".into());
    }
    let paths = crate::source_files::checkout_files(root)?
        .into_iter()
        .filter(|p| Path::new(p).file_name() == Some(std::ffi::OsStr::new("Cargo.toml")))
        .collect();
    inspect(root, &serde_json::from_slice(&metadata)?, &paths)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Value, BTreeSet<String>) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("member")).unwrap();
        fs::create_dir(root.path().join("probe")).unwrap();
        fs::create_dir(root.path().join("vendor")).unwrap();
        fs::write(root.path().join("Cargo.toml"),"[workspace]\nmembers=['member']\nexclude=['probe','vendor']\n[workspace.lints.rust]\nwarnings='deny'\n[patch.crates-io]\nforeign={path='vendor'}\n").unwrap();
        fs::write(
            root.path().join("member/Cargo.toml"),
            "[lints]\nworkspace=true\n",
        )
        .unwrap();
        fs::write(
            root.path().join("probe/Cargo.toml"),
            "[lints.rust]\nwarnings='deny'\n",
        )
        .unwrap();
        fs::write(
            root.path().join("vendor/Cargo.toml"),
            "[package]\nname='foreign'\nversion='1.0.0'\n",
        )
        .unwrap();
        let metadata = serde_json::json!({"workspace_members":["member"],"packages":[{"id":"member","manifest_path":root.path().canonicalize().unwrap().join("member/Cargo.toml")}]});
        let paths = BTreeSet::from([
            "Cargo.toml".into(),
            "member/Cargo.toml".into(),
            "probe/Cargo.toml".into(),
            "vendor/Cargo.toml".into(),
        ]);
        (root, metadata, paths)
    }
    #[test]
    fn manifest_ownership_and_lint_refusals_have_independent_negative_controls() {
        let (root, metadata, paths) = fixture();
        assert!(inspect(root.path(), &metadata, &paths).is_ok());
        fs::write(
            root.path().join("member/Cargo.toml"),
            "[lints.rust]\nwarnings='deny'\n",
        )
        .unwrap();
        assert!(inspect(root.path(), &metadata, &paths).is_err());
        let (root, metadata, paths) = fixture();
        fs::write(
            root.path().join("probe/Cargo.toml"),
            "[lints.rust]\nwarnings='allow'\n",
        )
        .unwrap();
        assert!(inspect(root.path(), &metadata, &paths).is_err());
        let (root, metadata, mut paths) = fixture();
        paths.insert("unclassified/Cargo.toml".into());
        assert!(inspect(root.path(), &metadata, &paths).is_err());
        let (root, mut metadata, paths) = fixture();
        metadata["packages"][0]["manifest_path"] = serde_json::json!(
            root.path()
                .canonicalize()
                .unwrap()
                .join("vendor/Cargo.toml")
        );
        assert!(inspect(root.path(), &metadata, &paths).is_err());
    }
}
