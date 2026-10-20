//! Reviewed package and retained-license provenance.
use super::*;

pub(super) fn verify_package(
    root: &Path,
    package: &Package,
    reviewed: &ReviewedPackage,
    lock: &Lock,
) -> Result<()> {
    if package.source != reviewed.source {
        return Err("Notice package source differs from review".into());
    }
    let matches: Vec<_> = lock
        .package
        .iter()
        .filter(|entry| {
            entry.name == package.name
                && entry.version == package.version
                && entry.source == package.source
        })
        .collect();
    if matches.len() != 1 || matches[0].checksum != reviewed.checksum {
        return Err(format!(
            "Locked identity differs from legal review: {}@{}",
            package.name, package.version
        )
        .into());
    }
    if package.source.is_none() {
        let path = reviewed.provenance["manifest_path"]
            .as_str()
            .ok_or("Missing workspace material provenance")?;
        let kind = reviewed.provenance["kind"].as_str();
        if !matches!(kind, Some("workspace" | "vendored"))
            || (kind == Some("vendored") && !relative(path)?.starts_with("vendor"))
            || package.manifest_path.canonicalize()? != root.join(relative(path)?).canonicalize()?
        {
            return Err("Local notice package is outside its reviewed workspace source".into());
        }
    } else {
        if package.source.as_deref()
            != Some("registry+https://github.com/rust-lang/crates.io-index")
        {
            return Err("Unreviewed registry in shipped notice graph".into());
        }
        // Cargo's standard unpack/cache layout is checked before deriving a
        // cached archive path. This hash establishes package byte identity only.
        let directory = package
            .manifest_path
            .parent()
            .ok_or("Missing registry package directory")?;
        let index = directory
            .parent()
            .ok_or("Missing registry index directory")?;
        let source = index.parent().ok_or("Missing registry source directory")?;
        if source.file_name().and_then(|name| name.to_str()) != Some("src")
            || directory.file_name().and_then(|name| name.to_str())
                != Some(&format!("{}-{}", package.name, package.version))
        {
            return Err("Unreviewed registry source/cache layout".into());
        }
        let archive = source
            .parent()
            .ok_or("Missing registry root")?
            .join("cache")
            .join(index.file_name().ok_or("Missing registry index name")?)
            .join(format!("{}-{}.crate", package.name, package.version));
        let bytes = read(&archive, 64 * 1024 * 1024)?;
        if Some(digest(&bytes)) != reviewed.checksum {
            return Err(format!(
                "Cached registry archive differs from locked identity: {}@{}",
                package.name, package.version
            )
            .into());
        }
    }
    Ok(())
}

pub(super) fn material_bytes(
    root: &Path,
    package: &Package,
    material: &Material,
) -> Result<Vec<u8>> {
    let base = match material.kind.as_str() {
        "package_file" => package
            .manifest_path
            .parent()
            .ok_or("Missing package directory")?,
        "workspace_file" | "retained_upstream_repository_file" => root,
        _ => return Err("Unreviewed legal-material kind".into()),
    };
    let bytes = checked_file(
        base,
        &FileFact {
            path: material.path.clone(),
            bytes: material.bytes,
            sha256: material.sha256.clone(),
        },
    )?;
    if let Some(upstream) = &material.upstream {
        let vcs: Value = serde_json::from_slice(&read(
            &package
                .manifest_path
                .parent()
                .ok_or("Missing package source")?
                .join(".cargo_vcs_info.json"),
            16 * 1024,
        )?)?;
        if vcs["git"]["sha1"] != upstream["revision"]
            || vcs["path_in_vcs"] != upstream["crate_path_in_vcs"]
        {
            return Err(
                "Version-bound legal supplement differs from package VCS provenance".into(),
            );
        }
    }
    Ok(bytes)
}
