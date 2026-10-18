//! Exact release asset assembly; native qualification/provenance remain separate.
use crate::{Result, capture};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, io::Write, path::Path, process::Command};
use xtask::archive::{ArchiveSnapshot, ByteIdentity};

fn verify_copy(path: &Path, identity: &ByteIdentity) -> Result<()> {
    if ArchiveSnapshot::read(path)?.identity() != *identity {
        return Err("Copied release archive differs from its frozen input".into());
    }
    Ok(())
}

fn check_server(
    names: &BTreeSet<String>,
    assets: &Path,
    release: &Value,
    source: &str,
    version: &str,
) -> Result<Value> {
    if release["draft"] != true
        || release["tag_name"] != format!("v{version}")
        || release["target_commitish"] != source
        || !release["id"].as_u64().is_some_and(|id| id > 0)
    {
        return Err("Server release is not the exact-source unpublished draft".into());
    }
    if !fs::symlink_metadata(assets)?.is_dir() {
        return Err("Release assets must be an ordinary directory".into());
    }
    let mut expected = names.clone();
    expected.insert("SHA256SUMS".into());
    let mut local = BTreeSet::new();
    for entry in fs::read_dir(assets)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err("Release assets contain a nonordinary file".into());
        }
        local.insert(
            entry
                .file_name()
                .into_string()
                .map_err(|_| "Invalid asset name")?,
        );
    }
    if names.len() != 8 || local != expected {
        return Err("Local release assets differ from the exact nine-file set".into());
    }
    let mut identities = serde_json::Map::new();
    let mut sums = String::new();
    for name in names {
        let identity = ArchiveSnapshot::read(&assets.join(name))?.identity();
        sums.push_str(&format!("{}  {name}\n", identity.sha256));
        identities.insert(name.clone(), serde_json::to_value(identity)?);
    }
    let manifest = ArchiveSnapshot::read(&assets.join("SHA256SUMS"))?;
    if manifest.bytes() != sums.as_bytes() {
        return Err("Local checksum manifest differs from actual archive bytes".into());
    }
    identities.insert(
        "SHA256SUMS".into(),
        serde_json::to_value(manifest.identity())?,
    );
    let server = release["assets"]
        .as_array()
        .ok_or("Missing server asset array")?;
    if server.len() != expected.len() {
        return Err("Server asset count differs from the exact nine-file set".into());
    }
    let mut observed = BTreeSet::new();
    let mut asset_ids = BTreeSet::new();
    for asset in server {
        let name = asset["name"].as_str().ok_or("Missing server asset name")?;
        if !observed.insert(name.to_owned()) || !expected.contains(name) {
            return Err("Duplicate or unexpected server release asset".into());
        }
        let identity = &identities[name];
        let asset_id = asset["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or("Missing positive server asset ID")?;
        if !asset_ids.insert(asset_id) {
            return Err("Duplicate server asset ID".into());
        }
        if asset["size"] != identity["byte_length"]
            || asset["digest"]
                != format!(
                    "sha256:{}",
                    identity["sha256"].as_str().ok_or("Missing local digest")?
                )
            || asset["state"] != "uploaded"
        {
            return Err(format!("Server release asset identity/state differs: {name}").into());
        }
    }
    Ok(
        json!({"source":source,"version":version,"release_id":release["id"],"assets":identities,
        "scope":"exact unpublished server asset names, sizes, digests and local checksum readback; not native qualification or authorization to publish"}),
    )
}

pub(crate) fn server(root: &Path, assets: &Path, release: &Path) -> Result<()> {
    if !crate::git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty() {
        return Err("Release server checking requires a clean committed checkout".into());
    }
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let names = plan["expected_artifacts"]
        .as_array()
        .ok_or("Missing archive policy")?
        .iter()
        .map(|artifact| {
            artifact["name"]
                .as_str()
                .map(str::to_owned)
                .ok_or("Missing archive name")
        })
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    let metadata = fs::symlink_metadata(release)?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("Server release JSON must be an ordinary bounded file".into());
    }
    let bytes = fs::read(release)?;
    if bytes.len() > 1024 * 1024 {
        return Err("Server release JSON grew beyond its bound".into());
    }
    let result = check_server(
        &names,
        assets,
        &serde_json::from_slice(&bytes)?,
        &crate::git(root, &["rev-parse", "HEAD"])?,
        plan["version"].as_str().ok_or("Missing version")?,
    )?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn assemble(names: &BTreeSet<String>, archives: &Path, out: &Path) -> Result<Value> {
    if names.len() != 8
        || names.iter().any(|name| {
            name.is_empty()
                || name == "."
                || name == ".."
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
    {
        return Err("Release assembly requires eight distinct plain archive basenames".into());
    }
    if !fs::symlink_metadata(archives)?.is_dir() {
        return Err("Release archive input must be an ordinary directory".into());
    }
    let mut observed = BTreeSet::new();
    for entry in fs::read_dir(archives)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err("Release archive inputs must be ordinary non-symlink files".into());
        }
        observed.insert(
            entry
                .file_name()
                .into_string()
                .map_err(|_| "Non-UTF8 release asset name")?,
        );
    }
    if &observed != names {
        return Err("Release input set differs from the exact eight expected archives".into());
    }
    fs::create_dir(out)?;
    let enclosure = xtask::archive::create_artifact_directory(out)?;
    let mut identities = serde_json::Map::new();
    let mut sums = String::new();
    for name in names {
        // One bounded immutable snapshot supplies both copied bytes and digest.
        let snapshot = ArchiveSnapshot::read(&archives.join(name))?;
        let identity = snapshot.identity();
        let destination = enclosure.path().join(name);
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        output.write_all(snapshot.bytes())?;
        output.sync_all()?;
        drop(output);
        verify_copy(&destination, &identity)?;
        sums.push_str(&format!("{}  {name}\n", identity.sha256));
        identities.insert(name.clone(), serde_json::to_value(identity)?);
    }
    let mut manifest = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(enclosure.path().join("SHA256SUMS"))?;
    manifest.write_all(sums.as_bytes())?;
    manifest.sync_all()?;
    drop(manifest);
    if fs::read(enclosure.path().join("SHA256SUMS"))? != sums.as_bytes() {
        return Err("Checksum manifest differs from assembled archive identities".into());
    }
    Ok(
        json!({"assets_directory":enclosure.path(),"archives":identities,"checksum_manifest":ByteIdentity::of(sums.as_bytes()),
        "scope":"exact eight-archive byte assembly and sorted checksum manifest only; no archive content, build provenance, native execution, attestation or publication qualification"}),
    )
}

pub(crate) fn run(root: &Path, archives: &Path, out: &Path) -> Result<()> {
    if !crate::git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty() {
        return Err("Release asset assembly requires a clean committed checkout".into());
    }
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let artifacts = plan["expected_artifacts"]
        .as_array()
        .ok_or("Missing artifact policy")?;
    let names = artifacts
        .iter()
        .map(|artifact| {
            artifact["name"]
                .as_str()
                .map(str::to_owned)
                .ok_or("Missing archive name")
        })
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    if artifacts.len() != 8 {
        return Err("Release policy must specify exactly eight archives".into());
    }
    let mut result = assemble(&names, archives, out)?;
    result["assembly_source"] = json!(crate::git(root, &["rev-parse", "HEAD"])?);
    result["version"] = plan["version"].clone();
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(root: &Path) -> (std::path::PathBuf, BTreeSet<String>) {
        let input = root.join("inputs");
        fs::create_dir(&input).unwrap();
        let names = (0..8)
            .map(|n| format!("owned-{n}.zip"))
            .collect::<BTreeSet<_>>();
        // Synthetic identity-assembly inputs, not package/native test evidence.
        for name in &names {
            fs::write(input.join(name), b"abc").unwrap();
        }
        (input, names)
    }

    #[test]
    fn exact_set_preserves_bytes_and_produces_sorted_independent_checksum_lines() {
        let root = tempfile::tempdir().unwrap();
        let (input, names) = inputs(root.path());
        let output = root.path().join("assembled");
        let result = assemble(&names, &input, &output).unwrap();
        let assets = Path::new(result["assets_directory"].as_str().unwrap());
        assert_eq!(fs::read_dir(assets).unwrap().count(), 9);
        let expected = names
            .iter()
            .map(|name| {
                format!(
                    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  {name}\n"
                )
            })
            .collect::<String>();
        assert_eq!(
            fs::read_to_string(assets.join("SHA256SUMS")).unwrap(),
            expected
        );
        for name in &names {
            assert_eq!(fs::read(assets.join(name)).unwrap(), b"abc");
        }
        assert!(assemble(&names, &input, &output).is_err());
        assert_eq!(
            fs::read_to_string(assets.join("SHA256SUMS")).unwrap(),
            expected
        );
    }

    #[test]
    fn server_guard_rejects_published_wrong_source_and_changed_or_duplicate_assets() {
        let root = tempfile::tempdir().unwrap();
        let (input, names) = inputs(root.path());
        let assembly = assemble(&names, &input, &root.path().join("assembled")).unwrap();
        let assets = Path::new(assembly["assets_directory"].as_str().unwrap());
        let mut rows = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                json!({"id":index+1,"name":name,"size":3,"state":"uploaded",
            "digest":"sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"})
            })
            .collect::<Vec<_>>();
        let checksum = fs::read(assets.join("SHA256SUMS")).unwrap();
        rows.push(
            json!({"id":9,"name":"SHA256SUMS","size":checksum.len(),"state":"uploaded",
            "digest":format!("sha256:{}",ByteIdentity::of(&checksum).sha256)}),
        );
        let good = json!({"id":42,"draft":true,"tag_name":"v6.0.0","target_commitish":"source","assets":rows});
        check_server(&names, assets, &good, "source", "6.0.0").unwrap();
        for case in [
            "published",
            "source",
            "digest",
            "size",
            "state",
            "duplicate",
            "duplicate-id",
            "extra",
            "missing",
        ] {
            let mut bad = good.clone();
            match case {
                "published" => bad["draft"] = json!(false),
                "source" => bad["target_commitish"] = json!("another-source"),
                "digest" => bad["assets"][0]["digest"] = json!("sha256:wrong"),
                "size" => bad["assets"][0]["size"] = json!(4),
                "state" => bad["assets"][0]["state"] = json!("new"),
                "duplicate" => bad["assets"][1] = bad["assets"][0].clone(),
                "duplicate-id" => bad["assets"][1]["id"] = bad["assets"][0]["id"].clone(),
                "extra" => bad["assets"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"name":"extra.zip"})),
                "missing" => {
                    bad["assets"].as_array_mut().unwrap().pop();
                }
                _ => unreachable!(),
            }
            assert!(
                check_server(&names, assets, &bad, "source", "6.0.0").is_err(),
                "{case}"
            );
        }
        fs::write(assets.join("SHA256SUMS"), b"changed manifest\n").unwrap();
        assert!(check_server(&names, assets, &good, "source", "6.0.0").is_err());
    }

    #[test]
    fn missing_extra_empty_and_nonordinary_inputs_cannot_assemble() {
        for case in ["missing", "extra", "empty", "directory"] {
            let root = tempfile::tempdir().unwrap();
            let (input, names) = inputs(root.path());
            match case {
                "missing" => fs::remove_file(input.join("owned-0.zip")).unwrap(),
                "extra" => fs::write(input.join("unreviewed.zip"), b"extra").unwrap(),
                "empty" => {
                    for name in &names {
                        fs::remove_file(input.join(name)).unwrap();
                    }
                }
                "directory" => {
                    fs::remove_file(input.join("owned-0.zip")).unwrap();
                    fs::create_dir(input.join("owned-0.zip")).unwrap();
                }
                _ => unreachable!(),
            }
            let output = root.path().join("assembled");
            assert!(assemble(&names, &input, &output).is_err(), "{case}");
            assert!(!output.exists());
        }
    }

    #[test]
    fn changed_copied_bytes_fail_the_actual_readback_guard() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("owned.zip");
        fs::write(&destination, b"abc").unwrap();
        let identity = ArchiveSnapshot::read(&destination).unwrap().identity();
        verify_copy(&destination, &identity).unwrap();
        fs::write(&destination, b"changed owned bytes").unwrap();
        assert!(verify_copy(&destination, &identity).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_archive_is_not_an_ordinary_release_input() {
        let root = tempfile::tempdir().unwrap();
        let (input, names) = inputs(root.path());
        fs::remove_file(input.join("owned-0.zip")).unwrap();
        std::os::unix::fs::symlink("owned-1.zip", input.join("owned-0.zip")).unwrap();
        assert!(assemble(&names, &input, &root.path().join("assembled")).is_err());
    }
}
