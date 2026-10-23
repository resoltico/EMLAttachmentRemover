//! Bounded generated legal trees and freshness verification.
use super::*;

pub(super) fn native_relative(base: &Path, path: &Path) -> Result<String> {
    let mut pieces = Vec::new();
    for component in path.strip_prefix(base)?.components() {
        let PathComponent::Normal(piece) = component else {
            return Err("Unexpected native material path component".into());
        };
        let piece = piece.to_str().ok_or("Non-UTF-8 material component")?;
        if piece.contains(['/', '\\', ':', '\0']) {
            return Err("Ambiguous native material component".into());
        }
        pieces.push(piece);
    }
    let name = pieces.join("/");
    relative(&name)?;
    Ok(name)
}

pub(super) fn enumerate(directory: &Path) -> Result<Files> {
    fn walk(
        base: &Path,
        path: &Path,
        files: &mut Files,
        depth: usize,
        total: &mut usize,
    ) -> Result<()> {
        if depth > 16 {
            return Err("Generated notice directory depth exceeds its bound".into());
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if files.len() >= 4096 {
                return Err("Generated notice inventory exceeds its bound".into());
            }
            let mut name = native_relative(base, &entry.path())?;
            let bytes = if kind.is_dir() && !kind.is_symlink() {
                name.push('/');
                Vec::new()
            } else if kind.is_file() && !kind.is_symlink() {
                let remaining = (64 * 1024 * 1024_usize)
                    .checked_sub(*total)
                    .ok_or("Notice tree exceeds its byte bound")?;
                let bytes = read(&entry.path(), MAX_LEGAL_BYTES.min(remaining as u64))?;
                *total = total
                    .checked_add(bytes.len())
                    .ok_or("Notice tree byte count overflow")?;
                bytes
            } else {
                return Err("Generated notice tree contains a symlink or special object".into());
            };
            if files.insert(name, bytes).is_some() {
                return Err("Duplicate native notice inventory key".into());
            }
            if kind.is_dir() {
                walk(base, &entry.path(), files, depth + 1, total)?;
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    if !fs::symlink_metadata(directory)?.is_dir() {
        return Err("Notice output must be an ordinary directory".into());
    }
    walk(directory, directory, &mut files, 0, &mut 0)?;
    Ok(files)
}

pub(super) fn add_directories(files: &mut Files) {
    let mut directories = BTreeSet::new();
    for path in files.keys() {
        let mut remaining = path.as_str();
        while let Some((parent, _)) = remaining.rsplit_once('/') {
            directories.insert(format!("{parent}/"));
            remaining = parent;
        }
    }
    for directory in directories {
        files.insert(directory, Vec::new());
    }
}

pub(super) fn output_manifest(files: &Files) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(
        &json!({"scope":"generated notice output identity only; source-material review is separate",
        "files":files.iter().map(|(path,bytes)| json!({"path":path,"kind":if path.ends_with('/') { "directory" } else { "file" },"bytes":bytes.len(),"sha256":digest(bytes)})).collect::<Vec<_>>() }),
    )?)
}

pub(super) fn matches_tree(path: &Path, expected: &Files) -> Result<()> {
    if enumerate(path)? != *expected {
        return Err("Generated legal materials differ from the expected tree".into());
    }
    Ok(())
}

/// New output only: there is no replacement or migration of tracked notices.
pub(super) fn write_new_tree(output: &Path, files: &Files) -> Result<()> {
    use std::io::Write;
    fs::create_dir(output)?;
    for (name, bytes) in files {
        let path = output.join(relative(name)?);
        if name.ends_with('/') {
            fs::create_dir_all(path)?;
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::File::options()
                .write(true)
                .create_new(true)
                .open(path)?;
            file.write_all(bytes)?;
            file.flush()?;
        }
    }
    matches_tree(output, files)
}

pub(super) fn source_document(root: &Path, plan: &BuildPlan) -> Result<Value> {
    Ok(json!({
        "source_commit": plan.source_commit(),
        "source_epoch": plan.source_epoch().seconds(),
        "source_provenance": plan.source_provenance(),
        "source_working_tree": plan.source_working_tree(root)?,
        "scope": "generated legal materials; not native execution or legal review evidence"
    }))
}

pub(super) fn source_identity(source: &Value, expected: &Value) -> Result<()> {
    for field in [
        "source_commit",
        "source_epoch",
        "source_provenance",
        "source_working_tree",
    ] {
        if source.get(field).is_none() || source.get(field) != expected.get(field) {
            return Err(
                "Generated legal source differs from the admitted source tuple/working tree".into(),
            );
        }
    }
    // Generating fresh notices in a dirty checkout remains useful. Consuming
    // another producer's bytes retains the clean-Git requirement; declarations
    // have no working-tree proof and never become qualified Git provenance.
    if expected["source_provenance"] == "git_commit" && expected["source_working_tree"] != "" {
        return Err("Generated legal inputs require the same clean Git source checkout".into());
    }
    Ok(())
}

pub(super) fn checked_generated_input(
    root: &Path,
    plan: &BuildPlan,
    directory: &Path,
) -> Result<Files> {
    let mut all = enumerate(directory)?;
    let marker = all
        .remove(OUTPUT_MANIFEST)
        .ok_or("Missing generated legal inventory")?;
    if output_manifest(&all)? != marker {
        return Err("Generated legal inventory differs from its actual files".into());
    }
    let source: Value = serde_json::from_slice(
        all.get("source.json")
            .ok_or("Missing legal producer source")?,
    )?;
    source_identity(&source, &source_document(root, plan)?)?;
    all.insert(OUTPUT_MANIFEST.into(), marker);
    Ok(all)
}

pub(super) fn checked_pairs(all: &Files, plan: &BuildPlan) -> Result<usize> {
    let targets = plan.targets();
    let version = plan.version();
    let mut prefixes = BTreeSet::new();
    for row in targets {
        let platform_name = row.platform().to_string();
        let platform = platform_name.as_str();
        for component in [
            NoticeComponent::Cli,
            NoticeComponent::Gui,
            NoticeComponent::Worker,
        ] {
            if !component.applies_to(row) {
                continue;
            }
            let prefix = component.prefix(platform);
            let identity: Value = serde_json::from_slice(
                all.get(&format!("{prefix}licenses/materials.json"))
                    .ok_or("Missing required notice component")?,
            )?;
            if identity["platform"] != platform
                || identity["target"] != row.rust_target()
                || identity["component"] != component.name()
                || identity["version"] != version
            {
                return Err(
                    "Notice component identity differs from current target/version policy".into(),
                );
            }
            if !prefixes.insert(prefix) {
                return Err("Duplicate notice component policy".into());
            }
        }
    }
    if prefixes.is_empty() {
        return Err("Notice component selection is empty".into());
    }
    for path in all.keys() {
        if path == "source.json" || path == OUTPUT_MANIFEST {
            continue;
        }
        if !prefixes.iter().any(|prefix| {
            path.starts_with(prefix) || (path.ends_with('/') && prefix.starts_with(path))
        }) {
            return Err("Generated legal tree has an unexpected platform/component".into());
        }
    }
    Ok(prefixes.len())
}
