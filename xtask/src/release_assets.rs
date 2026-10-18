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
