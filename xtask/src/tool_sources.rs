//! Authenticated complete published tool sources in isolated owned scratch.
use crate::{Result, campaign};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
pub(crate) const CARGO_TOOLS: [(&str, &str); 4] = [
    ("deny", "cargo_deny"),
    ("about", "cargo_about"),
    ("mutants", "cargo_mutants"),
    ("fuzz", "cargo_fuzz"),
];

fn octal(bytes: &[u8]) -> Result<u64> {
    let text = std::str::from_utf8(bytes)?.trim_matches(['\0', ' ']);
    if text.is_empty() || !text.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
        return Err("Invalid published GNU TAR number".into());
    }
    Ok(u64::from_str_radix(text, 8)?)
}
fn terminated(bytes: &[u8]) -> Result<&[u8]> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    if bytes[end..].iter().any(|byte| *byte != 0) {
        return Err("Hidden published TAR name suffix".into());
    }
    Ok(&bytes[..end])
}
fn published_files(archive: &[u8], basename: &str) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    // All four authenticated Cargo inputs use GNU headers, regular files and
    // bounded GNU long-name records. No PAX/sparse/links/directories are needed.
    let mut gzip = flate2::bufread::GzDecoder::new(archive);
    let mut bytes = Vec::new();
    (&mut gzip)
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 || !gzip.into_inner().is_empty() {
        return Err("Published tool gzip has excess data/members".into());
    }
    let mut cursor = 0_usize;
    let mut records = 0;
    let mut long_name: Option<Vec<u8>> = None;
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    loop {
        let header = bytes
            .get(cursor..cursor.checked_add(512).ok_or("TAR cursor overflow")?)
            .ok_or("Truncated published TAR header")?;
        if header.iter().all(|byte| *byte == 0) {
            if long_name.is_some()
                || bytes.len() - cursor < 1024
                || !(bytes.len() - cursor).is_multiple_of(512)
                || bytes[cursor..].iter().any(|byte| *byte != 0)
            {
                return Err("Invalid published TAR end blocks/tail".into());
            }
            break;
        }
        records += 1;
        if records > 4096
            || &header[257..265] != b"ustar  \0"
            || header[157..257].iter().any(|byte| *byte != 0)
        {
            return Err("Unsupported published GNU TAR header".into());
        }
        let checksum: u64 = header
            .iter()
            .enumerate()
            .map(|(index, byte)| {
                if (148..156).contains(&index) {
                    32
                } else {
                    u64::from(*byte)
                }
            })
            .sum();
        if checksum != octal(&header[148..156])? {
            return Err("Published TAR checksum differs".into());
        }
        let size: usize = octal(&header[124..136])?.try_into()?;
        let start = cursor + 512;
        let end = start.checked_add(size).ok_or("TAR body overflow")?;
        let next = end
            .checked_add((512 - size % 512) % 512)
            .ok_or("TAR padding overflow")?;
        let body = bytes
            .get(start..end)
            .ok_or("Truncated published TAR body")?;
        if bytes
            .get(end..next)
            .ok_or("Truncated published TAR padding")?
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err("Nonzero published TAR padding".into());
        }
        let short_name = terminated(&header[..100])?;
        if header[156] == b'L' {
            if long_name.is_some()
                || short_name != b"././@LongLink"
                || size == 0
                || size > 1024
                || body.last() != Some(&0)
                || body[..size - 1].contains(&0)
            {
                return Err("Invalid bounded GNU long-name record".into());
            }
            long_name = Some(body[..size - 1].to_vec());
        } else {
            if header[156] != b'0' || octal(&header[100..108])? & !0o777 != 0 {
                return Err("Published tool member is not an ordinary regular file".into());
            }
            let name = long_name.take().unwrap_or_else(|| short_name.to_vec());
            let name = std::str::from_utf8(&name)?;
            let relative = name
                .strip_prefix(&format!("{basename}/"))
                .ok_or("Published TAR root differs")?;
            let components: Vec<_> = relative.split('/').collect();
            if components.len() > 32
                || components.iter().any(|part| {
                    part.is_empty()
                        || matches!(*part, "." | "..")
                        || part.contains('\\')
                        || part.contains(':')
                })
            {
                return Err("Invalid published TAR component/depth".into());
            }
            let path = PathBuf::from(relative);
            if directories.contains(&path) || files.contains_key(&path) {
                return Err("Duplicate/aliased published TAR member".into());
            }
            let mut prefix = PathBuf::new();
            for part in &components[..components.len() - 1] {
                prefix.push(part);
                if files.contains_key(&prefix) {
                    return Err("Published file/directory collision".into());
                }
                directories.insert(prefix.clone());
            }
            files.insert(path, body.to_vec());
            if files.len() + directories.len() > 4096 {
                return Err("Published TAR node bound exceeded".into());
            }
        }
        cursor = next;
    }
    Ok(files)
}

fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() || fs::metadata(path)?.len() > limit {
        return Err("Tool input is not a bounded ordinary file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("Tool input exceeds its bound".into());
    }
    Ok(bytes)
}
pub(crate) fn compiler(root: &Path) -> Result<String> {
    let authority: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml"))?)?;
    Ok(authority["toolchain"]["channel"]
        .as_str()
        .ok_or("Missing compiler authority")?
        .to_owned())
}
pub(crate) fn source(root: &Path, name: &str, pin: &str, evidence: &Path) -> Result<PathBuf> {
    let version = campaign::pin(root, pin)?;
    let home = campaign::cargo_home()?;
    let package = format!("cargo-{name}");
    let basename = format!("{package}-{version}");
    let cached = home
        .join("registry/src/index.crates.io-1949cf8c6b5b557f")
        .join(&basename);
    if !cached.is_dir() {
        crate::capture(
            Command::new("cargo")
                .current_dir(root)
                .arg(format!("+{}", compiler(root)?))
                .args([
                    "info",
                    "--locked",
                    "--registry",
                    "crates-io",
                    &format!("{package}@{version}"),
                ])
                .env("CARGO_HOME", &home),
        )?;
    }
    let mut candidates = Vec::new();
    for index in fs::read_dir(home.join("registry/src"))? {
        let index = index?;
        let source = index.path().join(&basename);
        if source.is_dir() {
            candidates.push((index.file_name(), source));
        }
    }
    if candidates.len() != 1 {
        return Err(format!("Expected one canonical cached published {basename} source after cargo info; inspect the registry/cache configuration and rerun cargo xtask tools").into());
    }
    let (index, source) = &candidates[0];
    if index != "index.crates.io-1949cf8c6b5b557f" {
        return Err(
            "Pinned Cargo tool audit requires the canonical crates.io sparse-index namespace"
                .into(),
        );
    }
    let config: Value = serde_json::from_slice(&bounded(
        &home.join("registry/index").join(index).join("config.json"),
        65_536,
    )?)?;
    if config["dl"] != "https://static.crates.io/crates" || config["api"] != "https://crates.io" {
        return Err("Tool source is not the admitted crates.io registry".into());
    }
    let index_bytes = bounded(
        &home
            .join("registry/index")
            .join(index)
            .join(".cache/ca/rg")
            .join(&package),
        4 * 1024 * 1024,
    )?;
    let records: Vec<Value> = index_bytes
        .split(|byte| *byte == 0)
        .filter_map(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .filter(|record| record["name"] == package && record["vers"] == version)
        .collect();
    if records.len() != 1 {
        return Err("Missing or ambiguous pinned tool index record".into());
    }
    let expected = records[0]["cksum"]
        .as_str()
        .ok_or("Tool index checksum missing")?;
    let archive = bounded(
        &home
            .join("registry/cache")
            .join(index)
            .join(format!("{basename}.crate")),
        64 * 1024 * 1024,
    )?;
    let identity = xtask::archive::ByteIdentity::of(&archive);
    if identity.sha256 != expected {
        return Err("Published tool archive checksum differs from the crates.io index".into());
    }
    // Outside the product workspace: preserve the complete published manifest
    // and lock rather than rewriting resolver or dependency declarations.
    let owned = tempfile::Builder::new()
        .prefix("eml-tool-source-")
        .tempdir()?;
    let directory = owned.path().canonicalize()?;
    if directory.starts_with(root.canonicalize()?) {
        return Err("Tool scratch resolved inside the product workspace".into());
    }
    if directory
        .ancestors()
        .any(|path| path.join("Cargo.toml").exists())
    {
        return Err(
            "Tool scratch inherited a Cargo workspace; refused before running Cargo".into(),
        );
    }
    let destination = directory.join(&basename);
    fs::create_dir(&destination)?;
    for (relative, bytes) in published_files(&archive, &basename)? {
        let output = destination.join(relative);
        fs::create_dir_all(output.parent().ok_or("Tool member parent missing")?)?;
        let mut file = fs::File::options()
            .write(true)
            .create_new(true)
            .open(output)?;
        file.write_all(&bytes)?;
        file.flush()?;
    }
    let manifest = destination.join("Cargo.toml");
    let manifest_bytes = bounded(&manifest, 1024 * 1024)?;
    let lock = bounded(&destination.join("Cargo.lock"), 4 * 1024 * 1024)?;
    if manifest_bytes != bounded(&source.join("Cargo.toml"), 1024 * 1024)?
        || lock != bounded(&source.join("Cargo.lock"), 4 * 1024 * 1024)?
    {
        return Err("Cached tool manifest/lock differs from authenticated archive".into());
    }
    let parsed: toml::Value = toml::from_str(std::str::from_utf8(&manifest_bytes)?)?;
    if parsed["package"]["name"].as_str() != Some(&package)
        || parsed["package"]["version"].as_str() != Some(&version)
    {
        return Err("Published tool package identity differs from pin".into());
    }
    campaign::require_stock_topology(&directory)?;
    fs::write(
        evidence.join(format!("tool-{name}-source.json")),
        serde_json::to_vec_pretty(
            &json!({"package":package,"version":version,"archive":identity,"manifest":xtask::archive::ByteIdentity::of(&manifest_bytes),"published_lock":xtask::archive::ByteIdentity::of(&lock),"owned_source":destination,"graph_cwd":directory,"checksum_reference":"cached crates.io index and archive checksum; cached root manifest/lock equal authenticated members","scope":"published source graph; no installed binary provenance"}),
        )?,
    )?;
    // Retained, named inputs support graph failure review and subsequent locked
    // rebuilds. Never remove them while process settlement might be unknown.
    let _ = owned.keep();
    eprintln!(
        "Retained authenticated tool source: {}",
        directory.display()
    );
    Ok(manifest)
}
pub(crate) fn cargo(root: &Path, manifest: &Path) -> Result<Command> {
    let directory = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or("Tool scratch parent missing")?;
    campaign::require_stock_topology(directory)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(directory)
        .env("CARGO_HOME", campaign::cargo_home()?)
        .arg(format!("+{}", compiler(root)?));
    Ok(command)
}

pub(crate) struct SelectedSource {
    pub manifest: PathBuf,
    pub input_record: Value,
}
pub(crate) fn build_source(
    root: &Path,
    name: &str,
    pin: &str,
    evidence: &Path,
) -> Result<SelectedSource> {
    let manifest = source(root, name, pin, evidence)?;
    let observed_path = evidence.join(format!("tool-{name}-source.json"));
    let mut observed: Value = serde_json::from_slice(&bounded(&observed_path, 65_536)?)?;
    let inputs: Value = serde_json::from_slice(&bounded(
        &root.join("xtask/tool-locks/build-inputs.json"),
        65_536,
    )?)?;
    let records = inputs["tools"]
        .as_array()
        .ok_or("Missing tool build input records")?;
    if inputs["schema"] != 1
        || inputs["review_status"] != "source inputs reviewed"
        || records.len() != CARGO_TOOLS.len()
    {
        return Err("Tool build inputs require their explicit source-input review".into());
    }
    let records: Vec<_> = records
        .iter()
        .filter(|record| record["package"] == observed["package"])
        .collect();
    if records.len() != 1 {
        return Err("Missing/duplicate tool build source record".into());
    }
    let record = records[0];
    for key in [
        "version",
        "published_archive",
        "published_manifest",
        "published_lock",
    ] {
        let observed_key = match key {
            "published_archive" => "archive",
            "published_manifest" => "manifest",
            other => other,
        };
        if record[key] != observed[observed_key] {
            return Err(format!("Tool build source binding differs: {key}").into());
        }
    }
    let filename = format!("cargo-{name}.Cargo.lock");
    if record["build_lock"]["path"] != filename {
        return Err("Tool build lock name differs from package".into());
    }
    let lock = bounded(
        &root.join("xtask/tool-locks").join(filename),
        4 * 1024 * 1024,
    )?;
    let identity = xtask::archive::ByteIdentity::of(&lock);
    if record["build_lock"]["sha256"] != identity.sha256
        || record["build_lock"]["byte_length"] != identity.byte_length
    {
        return Err("Reviewed tool build lock bytes differ".into());
    }
    let lock_path = manifest
        .parent()
        .ok_or("Missing tool source parent")?
        .join("Cargo.lock");
    fs::write(&lock_path, &lock)?;
    if bounded(&lock_path, 4 * 1024 * 1024)? != lock {
        return Err("Selected tool lock readback differs".into());
    }
    observed["selected_build_lock"] = serde_json::to_value(&identity)?;
    observed["scope"] = json!(
        "authenticated unchanged published source with selected reviewed build lock; original published-lock findings remain separate; no installed binary provenance"
    );
    fs::write(observed_path, serde_json::to_vec_pretty(&observed)?)?;
    Ok(SelectedSource {
        manifest,
        input_record: record.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }
    fn gnu(name: &str, body: &[u8]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(body.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, name, body).unwrap();
        tar.into_inner().unwrap()
    }
    #[test]
    fn published_gnu_long_names_are_bounded_and_require_the_next_regular_file() {
        let relative = format!("{}source.rs", "folder/".repeat(11));
        let tar = gnu(&format!("cargo-fixture-1.0.0/{relative}"), b"owned source");
        let files = published_files(&gzip(&tar), "cargo-fixture-1.0.0").unwrap();
        assert_eq!(files[Path::new(&relative)], b"owned source");
        let first_size = octal(&tar[124..136]).unwrap() as usize;
        let next = 512 + first_size.div_ceil(512) * 512;
        let mut orphan = tar[..next].to_vec();
        orphan.extend([0; 1024]);
        assert!(published_files(&gzip(&orphan), "cargo-fixture-1.0.0").is_err());
        let mut repeated = tar[..next].to_vec();
        repeated.extend_from_slice(&tar);
        assert!(published_files(&gzip(&repeated), "cargo-fixture-1.0.0").is_err());
        let deep = gnu(
            &format!("cargo-fixture-1.0.0/{}source.rs", "folder/".repeat(33)),
            b"source",
        );
        assert!(published_files(&gzip(&deep), "cargo-fixture-1.0.0").is_err());
    }
    #[test]
    fn published_profile_rejects_extensions_specials_and_bad_tar_gzip_framing() {
        let baseline = gnu("cargo-fixture-1.0.0/Cargo.toml", b"owned source");
        assert_eq!(
            published_files(&gzip(&baseline), "cargo-fixture-1.0.0")
                .unwrap()
                .len(),
            1
        );
        for kind in b"xgS125" {
            let mut altered = baseline.clone();
            altered[156] = *kind;
            altered[148..156].fill(b' ');
            let checksum: u64 = altered[..512].iter().map(|byte| u64::from(*byte)).sum();
            altered[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
            assert!(published_files(&gzip(&altered), "cargo-fixture-1.0.0").is_err());
        }
        let mut bad_padding = baseline.clone();
        bad_padding[512 + 12] = 1;
        let mut bad_tail = baseline.clone();
        *bad_tail.last_mut().unwrap() = 1;
        for malformed in [
            baseline[..700].to_vec(),
            baseline[..baseline.len() - 512].to_vec(),
            bad_padding,
            bad_tail,
        ] {
            assert!(published_files(&gzip(&malformed), "cargo-fixture-1.0.0").is_err());
        }
        let compressed = gzip(&baseline);
        let mut crc = compressed.clone();
        let index = crc.len() - 8;
        crc[index] ^= 1;
        let mut multi = compressed.clone();
        multi.extend_from_slice(&compressed);
        let mut trailing = compressed.clone();
        trailing.push(1);
        for malformed in [
            crc,
            multi,
            trailing,
            compressed[..compressed.len() - 1].to_vec(),
        ] {
            assert!(published_files(&malformed, "cargo-fixture-1.0.0").is_err());
        }
    }
}
