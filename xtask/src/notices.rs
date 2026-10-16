//! Target-specific legal materials from independently reviewed package bindings.
use crate::{Result, campaign, capture};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
};

type Files = BTreeMap<String, Vec<u8>>;
const MAX_LEGAL_BYTES: u64 = 16 * 1024 * 1024;
const OUTPUT_MANIFEST: &str = "generated-files.json";

#[derive(Deserialize)]
struct Review {
    generator_pin: Value,
    packages: Vec<ReviewedPackage>,
    source_inputs: Vec<FileFact>,
    targets: Vec<TargetReview>,
}
#[derive(Deserialize)]
struct TargetReview {
    target: String,
}
#[derive(Deserialize)]
struct ReviewedPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    license_expression: String,
    targets: Vec<String>,
    bindings: Vec<Binding>,
    provenance: Value,
}
#[derive(Deserialize)]
struct Binding {
    license_id: String,
    generator_text_sha256: String,
    generator_text_bytes: usize,
    mapping: String,
    materials: Vec<Material>,
    targets: Vec<String>,
    standard_text: Option<Value>,
}
#[derive(Deserialize)]
struct FileFact {
    path: String,
    bytes: usize,
    sha256: String,
}
#[derive(Deserialize, Serialize)]
struct Material {
    kind: String,
    path: String,
    bytes: usize,
    sha256: String,
    upstream: Option<Value>,
}
#[derive(Deserialize)]
struct About {
    licenses: Vec<License>,
    crates: Vec<CrateLicense>,
}
#[derive(Deserialize)]
struct License {
    id: String,
    text: String,
    used_by: Vec<UsedBy>,
}
#[derive(Deserialize)]
struct UsedBy {
    #[serde(rename = "crate")]
    package: Package,
}
#[derive(Deserialize)]
struct CrateLicense {
    package: Package,
    license: String,
}
#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    source: Option<String>,
    manifest_path: PathBuf,
}
#[derive(Deserialize)]
struct Lock {
    package: Vec<Locked>,
}
#[derive(Deserialize)]
struct Locked {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn feature_tree(text: &str, packages: &BTreeSet<(String, String)>) -> Result<Value> {
    let mut entries = Vec::new();
    let mut observed = BTreeSet::new();
    let mut previous_depth = 0;
    for line in text.lines() {
        let (identity, features) = line
            .split_once('|')
            .ok_or("Malformed activated feature record")?;
        let depth_end = identity
            .bytes()
            .position(|byte| !byte.is_ascii_digit())
            .ok_or("Missing feature-tree package identity")?;
        let depth: usize = identity[..depth_end].parse()?;
        if depth > 128
            || depth > previous_depth + 1
            || (entries.is_empty() && depth != 0)
            || (!entries.is_empty() && depth == 0)
        {
            return Err("Invalid activated feature-tree depth".into());
        }
        let mut parts = identity[depth_end..].split_whitespace();
        let name = parts.next().ok_or("Missing feature package name")?;
        let version = parts
            .next()
            .and_then(|value| value.strip_prefix('v'))
            .ok_or("Missing feature package version")?;
        if !packages.contains(&(name.to_owned(), version.to_owned())) {
            return Err(
                "Feature tree contains a package outside the reviewed target closure".into(),
            );
        }
        let proc_macro = parts.next() == Some("(proc-macro)");
        let repeated = features.ends_with(" (*)");
        let features = features.strip_suffix(" (*)").unwrap_or(features);
        let features: BTreeSet<_> = if features.is_empty() {
            BTreeSet::new()
        } else {
            features.split(',').collect()
        };
        if features
            .iter()
            .any(|feature| feature.is_empty() || feature.chars().any(char::is_whitespace))
        {
            return Err("Invalid activated feature name".into());
        }
        observed.insert((name.to_owned(), version.to_owned()));
        entries.push(json!({"depth":depth,"name":name,"version":version,"features":features,"proc_macro":proc_macro,"repeated":repeated}));
        previous_depth = depth;
    }
    if entries.is_empty() || observed != *packages {
        return Err("Activated feature tree omits reviewed target packages".into());
    }
    Ok(
        json!({"scope":"actual CLI normal/build feature tree; host paths omitted without merging distinct occurrences", "entries":entries}),
    )
}
fn relative(path: &str) -> Result<&Path> {
    let value = Path::new(path);
    let raw = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty()
        || path.len() > 1024
        || path.contains(['\\', ':', '\0'])
        || raw
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || !value
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err("Invalid legal-material relative path".into());
    }
    Ok(value)
}
fn read(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Cannot inspect required legal input {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err("Legal material must be an ordinary non-symlink file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| {
            format!(
                "Cannot open required legal input {}: {error}",
                path.display()
            )
        })?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("Legal input exceeds its document bound".into());
    }
    Ok(bytes)
}
fn checked_file(base: &Path, fact: &FileFact) -> Result<Vec<u8>> {
    let bytes = read(&base.join(relative(&fact.path)?), MAX_LEGAL_BYTES)?;
    if bytes.len() != fact.bytes || digest(&bytes) != fact.sha256 {
        return Err(format!("Reviewed legal input changed: {}", fact.path).into());
    }
    Ok(bytes)
}
fn insert(files: &mut Files, path: String, bytes: Vec<u8>) -> Result<()> {
    relative(&path)?;
    if let Some(previous) = files.get(&path) {
        if *previous != bytes {
            return Err("Conflicting generated legal-material paths".into());
        }
    } else {
        files.insert(path, bytes);
    }
    Ok(())
}
fn text_file(files: &mut Files, bytes: Vec<u8>) -> Result<String> {
    let path = format!("licenses/crates/{}.txt", digest(&bytes));
    insert(files, path.clone(), bytes)?;
    Ok(path)
}

fn verify_package(
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
        if reviewed.provenance["kind"] != "workspace"
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

fn material_bytes(root: &Path, package: &Package, material: &Material) -> Result<Vec<u8>> {
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

fn render(
    root: &Path,
    target: &str,
    report: &About,
    review: &Review,
    lock: &Lock,
) -> Result<(Files, Vec<Value>)> {
    let expected: BTreeMap<_, _> = review
        .packages
        .iter()
        .filter(|package| package.targets.iter().any(|value| value == target))
        .map(|package| ((package.name.clone(), package.version.clone()), package))
        .collect();
    if expected.is_empty() {
        return Err("Notice target has no reviewed package closure".into());
    }
    let mut actual = BTreeMap::new();
    for entry in &report.crates {
        let key = (entry.package.name.clone(), entry.package.version.clone());
        let reviewed = expected
            .get(&key)
            .ok_or("Unreviewed package in target notice closure")?;
        if entry.license != reviewed.license_expression
            || actual.insert(key, &entry.package).is_some()
        {
            return Err("Duplicate package or changed notice license expression".into());
        }
        verify_package(root, &entry.package, reviewed, lock)?;
    }
    if actual.keys().ne(expected.keys()) {
        return Err("Target notice package closure differs from review".into());
    }
    let mut files = BTreeMap::new();
    let mut observed = BTreeSet::new();
    let mut inventory = Vec::new();
    for license in &report.licenses {
        if license.used_by.is_empty() || license.text.is_empty() {
            return Err("Empty license group cannot prove complete notices".into());
        }
        let hash = digest(license.text.as_bytes());
        for used in &license.used_by {
            let key = (used.package.name.clone(), used.package.version.clone());
            let package = actual
                .get(&key)
                .ok_or("License group refers to a package outside its target closure")?;
            if used.package.source != package.source
                || used.package.manifest_path != package.manifest_path
            {
                return Err("License-group package provenance differs from graph package".into());
            }
            let reviewed = expected[&key];
            let candidates: Vec<_> = reviewed
                .bindings
                .iter()
                .filter(|binding| {
                    binding.targets.iter().any(|value| value == target)
                        && binding.license_id == license.id
                        && binding.generator_text_sha256 == hash
                        && binding.generator_text_bytes == license.text.len()
                })
                .collect();
            if candidates.len() != 1
                || !observed.insert((key.clone(), license.id.clone(), hash.clone()))
            {
                return Err(
                    "Unreviewed, ambiguous or duplicate per-package license binding".into(),
                );
            }
            let binding = candidates[0];
            let materials = binding
                .materials
                .iter()
                .map(|material| material_bytes(root, package, material))
                .collect::<Result<Vec<_>>>()?;
            let selected = match binding.mapping.as_str() {
                "exact_per_package_legal_file" => {
                    if materials.len() != 1 || materials[0] != license.text.as_bytes() {
                        return Err(
                            "Per-package legal bytes differ from selected generator text".into(),
                        );
                    }
                    materials[0].clone()
                }
                "canonical_generator_fallback_mapped_explicitly_to_actual_package_mit_file" => {
                    if materials.len() != 1 || license.id != "MIT" {
                        return Err("Invalid copyright-bearing MIT replacement binding".into());
                    }
                    materials[0].clone()
                }
                "workspace_full_license_plus_identified_standard_mpl_text"
                | "version_and_revision_bound_upstream_explanation_plus_identified_standard_mit_text" =>
                {
                    let standard = binding
                        .standard_text
                        .as_ref()
                        .ok_or("Missing identified standard license text")?;
                    if standard["sha256"] != hash
                        || standard["bytes"] != license.text.len()
                        || standard["license_id"] != license.id
                    {
                        return Err("Identified standard legal text differs from review".into());
                    }
                    if binding.mapping.starts_with("workspace_") {
                        if materials.len() != 1 {
                            return Err("Missing full workspace license material".into());
                        }
                        insert(&mut files, "LICENSE".into(), materials[0].clone())?;
                    } else {
                        if materials.len() != 1 {
                            return Err("Missing upstream licensing explanation".into());
                        }
                        insert(
                            &mut files,
                            "licenses/objc2/LICENSE.md".into(),
                            materials[0].clone(),
                        )?;
                        let source = review
                            .source_inputs
                            .iter()
                            .find(|fact| fact.path == "xtask/licenses/objc2/source.json")
                            .ok_or("Missing reviewed supplement source record")?;
                        insert(
                            &mut files,
                            "licenses/objc2/source.json".into(),
                            checked_file(root, source)?,
                        )?;
                    }
                    license.text.as_bytes().to_vec()
                }
                _ => return Err("Unknown legal-material mapping requires renewed review".into()),
            };
            let file = text_file(&mut files, selected)?;
            inventory.push(json!({"name":package.name,"version":package.version,"source":package.source,
                "checksum":reviewed.checksum,"license_expression":reviewed.license_expression,
                "license_id":license.id,"generator_text_sha256":hash,"file":file,
                "review_mapping":binding.mapping,"package_archive_identity_checked":package.source.is_some(),
                "unpacked_materials_checked_separately":true,
                "materials":binding.materials }));
        }
    }
    let expected_count: usize = expected
        .values()
        .map(|package| {
            package
                .bindings
                .iter()
                .filter(|binding| binding.targets.iter().any(|value| value == target))
                .count()
        })
        .sum();
    if observed.len() != expected_count {
        return Err("Target notices omit reviewed per-package license bindings".into());
    }
    inventory.sort_by_key(|value| serde_json::to_string(value).expect("JSON value serialization"));
    Ok((files, inventory))
}

fn bundle(
    root: &Path,
    directory: &str,
    output: &str,
    required: &[&str],
    files: &mut Files,
) -> Result<Value> {
    let base = root.join(directory);
    let source = read(&base.join("source.json"), MAX_LEGAL_BYTES)?;
    let record: Value = serde_json::from_slice(&source)?;
    let materials = record["materials"]
        .as_array()
        .ok_or("Missing retained legal-material inventory")?;
    let mut names = BTreeSet::new();
    for material in materials {
        let name = material["file"]
            .as_str()
            .ok_or("Missing retained legal filename")?;
        if relative(name)?.components().count() != 1 || !names.insert(name) {
            return Err("Invalid retained legal-material filename".into());
        }
        let bytes = read(&base.join(name), MAX_LEGAL_BYTES)?;
        if material["sha256"] != digest(&bytes) {
            return Err("Retained upstream legal bytes differ from their source record".into());
        }
        insert(files, format!("{output}/{name}"), bytes)?;
    }
    if names != required.iter().copied().collect() {
        return Err(
            "Retained upstream legal-material inventory is incomplete or unexpected".into(),
        );
    }
    insert(files, format!("{output}/source.json"), source)?;
    Ok(record)
}

struct NoticeTarget<'a> {
    target: &'a str,
    platform: &'a str,
    version: &'a str,
}

fn target_materials(
    root: &Path,
    selection: NoticeTarget<'_>,
    review: &Review,
    lock: &Lock,
    compiler: &str,
    config: &[u8],
) -> Result<Files> {
    let NoticeTarget {
        target,
        platform,
        version,
    } = selection;
    let arguments = [
        "about",
        "generate",
        "--locked",
        "--manifest-path",
        "crates/eml-cli/Cargo.toml",
        "--target",
        target,
        "--config",
        "xtask/notices.toml",
        "--format",
        "json",
        "--fail",
    ];
    let bytes = capture(Command::new("cargo").current_dir(root).args(arguments))?;
    let report: About = serde_json::from_slice(&bytes)?;
    let (mut files, inventory) = render(root, target, &report, review, lock)?;
    let features_arguments = [
        "tree",
        "--locked",
        "--package",
        "remove-eml-attachments",
        "--target",
        target,
        "--edges",
        "normal,build",
        "--prefix",
        "depth",
        "--format",
        "{p}|{f}",
    ];
    let features = String::from_utf8(capture(
        Command::new("cargo")
            .current_dir(root)
            .args(features_arguments),
    )?)?;
    let packages = report
        .crates
        .iter()
        .map(|entry| (entry.package.name.clone(), entry.package.version.clone()))
        .collect();
    let features = feature_tree(&features, &packages)?;
    insert(
        &mut files,
        "licenses/activated-features.json".into(),
        serde_json::to_vec_pretty(&features)?,
    )?;
    let rust = bundle(
        root,
        "xtask/licenses/rust",
        "licenses/rust",
        &[
            "COPYRIGHT-library.html",
            "MIT.txt",
            "Apache-2.0.txt",
            "BSD-2-Clause.txt",
            "Unicode-3.0.txt",
        ],
        &mut files,
    )?;
    let toolchain: toml::Value = toml::from_str(&String::from_utf8(read(
        &root.join("rust-toolchain.toml"),
        MAX_LEGAL_BYTES,
    )?)?)?;
    let channel = toolchain["toolchain"]["channel"]
        .as_str()
        .ok_or("Missing compiler authority")?;
    if rust["version"] != channel
        || !compiler
            .lines()
            .any(|line| line == format!("release: {channel}"))
        || !compiler
            .lines()
            .any(|line| line == format!("commit-hash: {}", rust["commit"].as_str().unwrap_or("")))
    {
        return Err(
            "Actual compiler differs from retained standard-library material identity".into(),
        );
    }
    let windows = if platform.starts_with("windows-") {
        let material = bundle(
            root,
            "xtask/licenses/windows-runtime",
            "licenses/windows-runtime",
            &[
                "mingw-COPYING",
                "mingw-COPYING-runtime.txt",
                "mingw-DISCLAIMER.PD",
                "llvm-libunwind-LICENSE.TXT",
                "mingw-pseudo-reloc-notice.txt",
                "mingw-complex-internal-notice.txt",
            ],
            &mut files,
        )?;
        if material["producer"]["rust"] != channel {
            return Err("Native runtime legal inputs differ from pinned Rust".into());
        }
        Some(material)
    } else {
        None
    };
    let mut notice = format!("EML Attachment Remover {version}\nTarget: {target} ({platform})\n\n");
    notice.push_str("These materials cover the target-selected locked CLI normal/build dependency graph. Build and proc-macro materials are included conservatively; this list does not assert that every listed crate is linked application code.\n\n");
    notice.push_str("Full Rust standard-library copyright, nested exceptions and source pointers: licenses/rust/COPYRIGHT-library.html and licenses/rust/source.json. Associated complete license texts are in that directory.\n");
    if windows.is_some() {
        notice.push_str("Windows runtime distribution legal materials: licenses/windows-runtime/. The final executable's selected native inputs require a separate link-bound review; this material bundle does not approve that trace.\n");
    }
    notice.push_str("\nComplete selected crate texts and source-specific notices:\n");
    for item in &inventory {
        notice.push_str(&format!(
            "\n{}@{} — {}\n  {}\n",
            item["name"].as_str().unwrap_or(""),
            item["version"].as_str().unwrap_or(""),
            item["license_id"].as_str().unwrap_or(""),
            item["file"].as_str().unwrap_or("")
        ));
    }
    if files.contains_key("licenses/objc2/LICENSE.md") {
        notice.push_str("\nobjc2-family upstream licensing explanation and exact revisions: licenses/objc2/LICENSE.md and source.json. The identified standard MIT template is included separately; its copyright placeholders are not an upstream attribution. The upstream Apple SDK discussion is retained unchanged.\n");
    }
    insert(
        &mut files,
        "THIRD-PARTY-NOTICES.txt".into(),
        notice.into_bytes(),
    )?;
    insert(
        &mut files,
        "licenses/materials.json".into(),
        serde_json::to_vec_pretty(&json!({
            "version":version,"target":target,"platform":platform,
            "scope":"legal-material generation only; no native execution, archive or final-link qualification",
            "generation":{"tool":review.generator_pin,"arguments":arguments,"config_sha256":digest(config),
            "feature_arguments":features_arguments,"activated_features":"licenses/activated-features.json",
                "compiler_version":channel,"compiler_commit":rust["commit"],
                "execution_host":"recorded separately by Controller/qualification; notice generation is not native execution evidence"},
            "packages":inventory,"rust":rust,"windows_runtime":windows
        }))?,
    )?;
    Ok(files)
}

fn native_relative(base: &Path, path: &Path) -> Result<String> {
    let mut pieces = Vec::new();
    for component in path.strip_prefix(base)?.components() {
        let Component::Normal(piece) = component else {
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

fn enumerate(directory: &Path) -> Result<Files> {
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

fn add_directories(files: &mut Files) {
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

fn output_manifest(files: &Files) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(
        &json!({"scope":"generated notice output identity only; source-material review is separate",
        "files":files.iter().map(|(path,bytes)| json!({"path":path,"kind":if path.ends_with('/') { "directory" } else { "file" },"bytes":bytes.len(),"sha256":digest(bytes)})).collect::<Vec<_>>() }),
    )?)
}

fn admitted_output(output: &Path) -> Result<Option<Files>> {
    if !output.try_exists()? {
        return Ok(None);
    }
    let existing = enumerate(output)?;
    let mut payload = existing.clone();
    let prior = payload
        .remove(OUTPUT_MANIFEST)
        .ok_or("Refusing to replace a notice tree without its generated-file ownership record")?;
    if output_manifest(&payload)? != prior {
        return Err("Refusing to replace notice output with unrelated edits or files".into());
    }
    Ok(Some(existing))
}

#[cfg(unix)]
fn move_without_replacement(from: &Path, to: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}
#[cfg(not(unix))]
fn move_without_replacement(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Native no-replace notice-directory update is not implemented on this host",
    ))
}

fn matches_tree(path: &Path, expected: &Files) -> Result<()> {
    if enumerate(path)? != *expected {
        return Err("Notice paths, types or bytes differ from the admitted tree".into());
    }
    Ok(())
}

fn replace_tree(
    output: &Path,
    expected: &Files,
    admitted: Option<&Files>,
    final_check: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let parent = output.parent().ok_or("Notice output has no parent")?;
    let stage = tempfile::Builder::new()
        .prefix(".notice-generation-")
        .tempdir_in(parent)?;
    let fresh = stage.path().join("fresh");
    fs::create_dir(&fresh)?;
    for (name, bytes) in expected {
        let path = fresh.join(relative(name)?);
        if name.ends_with('/') {
            fs::create_dir_all(path)?;
        } else {
            fs::create_dir_all(path.parent().ok_or("Missing generated material parent")?)?;
            fs::write(path, bytes)?;
        }
    }
    matches_tree(&fresh, expected)?;
    let previous = stage.path().join("previous");
    let mut moved_previous = false;
    let mut installed = false;
    let attempt = (|| -> Result<()> {
        if let Some(admitted) = admitted {
            move_without_replacement(output, &previous)?;
            moved_previous = true;
            // Check what was actually displaced, after staging, before any
            // deletion authority is accepted for the old tree.
            matches_tree(&previous, admitted)?;
        }
        move_without_replacement(&fresh, output)?;
        installed = true;
        final_check(output)
    })();
    if let Err(original) = attempt {
        let mut errors = vec![original.to_string()];
        if installed
            && let Err(error) =
                move_without_replacement(output, &stage.path().join("failed-output"))
        {
            errors.push(format!(
                "Could not retain failed output separately: {error}"
            ));
        }
        if moved_previous && let Err(error) = move_without_replacement(&previous, output) {
            errors.push(format!("Could not restore displaced output: {error}"));
        }
        let retained = stage.keep();
        return Err(format!("{}; retained {}", errors.join("; "), retained.display()).into());
    }
    let stage_path = stage.path().to_owned();
    stage.close().map_err(|error| {
        format!(
            "Notice output verified but owned staging cleanup failed: {error}; inspect {}",
            stage_path.display()
        )
    })?;
    Ok(())
}

fn generated_materials(root: &Path) -> Result<(PathBuf, Files)> {
    campaign::require_stock_topology(root)?;
    let tool = campaign::tool_version(root, "about", "cargo_about")?;
    let root = root.canonicalize()?;
    let review: Review = serde_json::from_slice(&read(
        &root.join("xtask/licenses/crate-materials.json"),
        MAX_LEGAL_BYTES,
    )?)?;
    if review.generator_pin["name"] != "cargo-about" || review.generator_pin["version"] != tool {
        return Err("Legal review uses another generator identity".into());
    }
    for fact in &review.source_inputs {
        if matches!(
            fact.path.as_str(),
            "Cargo.toml"
                | "crates/eml-cli/Cargo.toml"
                | "crates/eml-core/Cargo.toml"
                | "crates/eml-fs/Cargo.toml"
                | "LICENSE"
        ) {
            checked_file(&root, fact)?;
        }
    }
    let lock: Lock = toml::from_str(&String::from_utf8(read(
        &root.join("Cargo.lock"),
        MAX_LEGAL_BYTES,
    )?)?)?;
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(&root)
            .arg("distribution"),
    )?)?;
    let version = plan["version"]
        .as_str()
        .ok_or("Missing Cargo-derived version")?;
    let compiler = String::from_utf8(capture(
        Command::new("rustc").current_dir(&root).arg("-vV"),
    )?)?;
    let config = read(&root.join("xtask/notices.toml"), MAX_LEGAL_BYTES)?;
    let targets = plan["native_targets"]
        .as_array()
        .ok_or("Missing native target table")?;
    let expected: BTreeSet<_> = review
        .targets
        .iter()
        .map(|entry| entry.target.as_str())
        .collect();
    let actual: BTreeSet<_> = targets
        .iter()
        .map(|entry| entry["target"].as_str().ok_or("Missing target identity"))
        .collect::<std::result::Result<_, _>>()?;
    if actual != expected || targets.len() != expected.len() {
        return Err("Notice target set differs from reviewed native policy".into());
    }
    let mut all = BTreeMap::new();
    for target in targets {
        let platform = target["platform"]
            .as_str()
            .ok_or("Missing platform identity")?;
        let target = target["target"].as_str().ok_or("Missing target identity")?;
        eprintln!("Generating notice materials: {platform} ({target})");
        for (path, bytes) in target_materials(
            &root,
            NoticeTarget {
                target,
                platform,
                version,
            },
            &review,
            &lock,
            &compiler,
            &config,
        )? {
            insert(&mut all, format!("{platform}/{path}"), bytes)?;
        }
    }
    let marker = OUTPUT_MANIFEST;
    add_directories(&mut all);
    let manifest = output_manifest(&all)?;
    all.insert(marker.into(), manifest);
    Ok((root, all))
}

pub(crate) fn run(root: &Path, write: bool) -> Result<()> {
    let (root, all) = generated_materials(root)?;
    let output = root.join("notices");
    if !write {
        if !output.try_exists()? {
            return Err("Generated notice tree is missing; run cargo xtask notices --write and review the generated materials".into());
        }
        if enumerate(&output)? != all {
            return Err("Generated notices are missing, extra or stale; run cargo xtask notices --write and review the changes".into());
        }
        return Ok(());
    }
    let admitted = admitted_output(&output)?;
    replace_tree(&output, &all, admitted.as_ref(), |path| {
        matches_tree(path, &all)
    })
}

/// Regenerate and verify the complete tracked tree before returning one target's
/// fresh legal bytes. The caller never trusts an archive-supplied inventory.
pub(crate) struct CheckedTargetMaterials {
    files: Files,
    target: String,
    platform: String,
    version: String,
}
impl CheckedTargetMaterials {
    pub fn files(&self) -> &Files {
        &self.files
    }
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn platform(&self) -> &str {
        &self.platform
    }
    pub fn version(&self) -> &str {
        &self.version
    }
}

pub(crate) fn checked_target(root: &Path, platform: &str) -> Result<CheckedTargetMaterials> {
    if relative(platform)?.components().count() != 1 {
        return Err("Invalid notice platform component".into());
    }
    let (root, all) = generated_materials(root)?;
    matches_tree(&root.join("notices"), &all)?;
    let prefix = format!("{platform}/");
    let selected: Files = all
        .into_iter()
        .filter_map(|(path, bytes)| {
            if path.ends_with('/') {
                return None;
            }
            path.strip_prefix(&prefix)
                .map(|relative| (relative.to_owned(), bytes))
        })
        .collect();
    if selected.is_empty() {
        return Err("No generated materials for the requested native platform".into());
    }
    let identity: Value = serde_json::from_slice(
        selected
            .get("licenses/materials.json")
            .ok_or("Missing checked target material identity")?,
    )?;
    if identity["platform"] != platform {
        return Err("Checked material platform mismatch".into());
    }
    Ok(CheckedTargetMaterials {
        target: identity["target"]
            .as_str()
            .ok_or("Missing checked material target")?
            .to_owned(),
        version: identity["version"]
            .as_str()
            .ok_or("Missing checked material version")?
            .to_owned(),
        platform: platform.to_owned(),
        files: selected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activated_features_preserve_occurrences_without_host_paths() {
        let packages = BTreeSet::from([
            ("tool".into(), "1.0.0".into()),
            ("derive".into(), "2.0.0".into()),
        ]);
        let text = "0tool v1.0.0 (/owned/source with spaces)|std\n1derive v2.0.0 (proc-macro)|default (*)\n";
        let record = feature_tree(text, &packages).unwrap();
        assert!(!record.to_string().contains("/owned"));
        assert_eq!(record["entries"][1]["proc_macro"], true);
        assert_eq!(record["entries"][1]["repeated"], true);
        for invalid in [
            "",
            "0tool v1.0.0|std\n",
            "0tool v1.0.0|std\n2derive v2.0.0|\n",
            "0tool v1.0.0|std\n1foreign v2.0.0|\n",
        ] {
            assert!(feature_tree(invalid, &packages).is_err());
        }
    }

    #[test]
    fn grouped_representative_cannot_cover_another_packages_missing_file() {
        let root = tempfile::tempdir().unwrap();
        let text = "Synthetic owned license fixture\n";
        let mut packages = Vec::new();
        let mut reviewed = Vec::new();
        let mut locked = Vec::new();
        for name in ["first", "second"] {
            fs::create_dir(root.path().join(name)).unwrap();
            fs::write(
                root.path().join(name).join("Cargo.toml"),
                "owned manifest fixture",
            )
            .unwrap();
            fs::write(root.path().join(name).join("LICENSE"), text).unwrap();
            let package = json!({"name":name,"version":"1.0.0","source":null,"manifest_path":root.path().join(name).join("Cargo.toml")});
            packages.push(package);
            locked.push(json!({"name":name,"version":"1.0.0","source":null,"checksum":null}));
            reviewed.push(json!({"name":name,"version":"1.0.0","source":null,"checksum":null,
                "license_expression":"MIT","targets":["owned-target"],
                "provenance":{"kind":"workspace","manifest_path":format!("{name}/Cargo.toml")},
                "bindings":[{"license_id":"MIT","generator_text_sha256":digest(text.as_bytes()),"generator_text_bytes":text.len(),
                    "mapping":"exact_per_package_legal_file","targets":["owned-target"],
                    "materials":[{"kind":"package_file","path":"LICENSE","bytes":text.len(),"sha256":digest(text.as_bytes())}]}]}));
        }
        let mut report: About = serde_json::from_value(json!({"crates":packages.iter().map(|package| json!({"package":package,"license":"MIT"})).collect::<Vec<_>>(),
            "licenses":[{"id":"MIT","text":text,"source_path":root.path().join("first/LICENSE"),
                "used_by":packages.iter().map(|package| json!({"crate":package})).collect::<Vec<_>>() }]})).unwrap();
        let review: Review = serde_json::from_value(json!({"generator_pin":{},"packages":reviewed,"source_inputs":[],"targets":[{"target":"owned-target"}]})).unwrap();
        let lock: Lock = serde_json::from_value(json!({"package":locked})).unwrap();
        assert_eq!(
            render(root.path(), "owned-target", &report, &review, &lock)
                .unwrap()
                .1
                .len(),
            2
        );
        fs::remove_file(root.path().join("second/LICENSE")).unwrap();
        assert!(render(root.path(), "owned-target", &report, &review, &lock).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("first/LICENSE")).unwrap(),
            text
        );
        fs::write(root.path().join("second/LICENSE"), text).unwrap();
        assert!(render(root.path(), "owned-target", &report, &review, &lock).is_ok());
        assert!(render(root.path(), "another-target", &report, &review, &lock).is_err());
        report.licenses[0].text.push_str("changed selected text");
        assert!(render(root.path(), "owned-target", &report, &review, &lock).is_err());
    }

    #[test]
    fn actual_copyright_bearing_microsoft_text_replaces_generic_fallback() {
        let owned = tempfile::tempdir().unwrap();
        let directory = owned
            .path()
            .join("registry/src/owned-index/owned-binding-1.0.0");
        let cache = owned.path().join("registry/cache/owned-index");
        fs::create_dir_all(&directory).unwrap();
        fs::create_dir_all(&cache).unwrap();
        let legal = include_bytes!("../../tests/contract/notices/windows-bindings-license-mit.txt");
        fs::write(directory.join("license-mit"), legal).unwrap();
        fs::write(directory.join("Cargo.toml"), b"owned manifest fixture").unwrap();
        let archive = b"owned synthetic archive identity; not a real registry package";
        fs::write(cache.join("owned-binding-1.0.0.crate"), archive).unwrap();
        let fallback = "MIT template with <year> and <copyright holders>";
        let package = json!({"name":"owned-binding","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index","manifest_path":directory.join("Cargo.toml")});
        let report: About = serde_json::from_value(json!({"crates":[{"package":package,"license":"MIT OR Apache-2.0"}],
            "licenses":[{"id":"MIT","text":fallback,"source_path":null,"used_by":[{"crate":package}]}]})).unwrap();
        let review: Review = serde_json::from_value(json!({"generator_pin":{},"source_inputs":[],"targets":[{"target":"owned-target"}],
            "packages":[{"name":"owned-binding","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index","checksum":digest(archive),
                "license_expression":"MIT OR Apache-2.0","targets":["owned-target"],"provenance":{"kind":"registry"},
                "bindings":[{"license_id":"MIT","generator_text_sha256":digest(fallback.as_bytes()),"generator_text_bytes":fallback.len(),
                    "mapping":"canonical_generator_fallback_mapped_explicitly_to_actual_package_mit_file","targets":["owned-target"],
                    "materials":[{"kind":"package_file","path":"license-mit","bytes":legal.len(),"sha256":"c2cfccb812fe482101a8f04597dfc5a9991a6b2748266c47ac91b6a5aae15383"}]}]}]})).unwrap();
        let lock: Lock = serde_json::from_value(json!({"package":[{"name":"owned-binding","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index","checksum":digest(archive)}]})).unwrap();
        let (files, inventory) =
            render(owned.path(), "owned-target", &report, &review, &lock).unwrap();
        let selected = inventory[0]["file"].as_str().unwrap();
        assert_eq!(files[selected], legal);
        assert!(
            String::from_utf8(files[selected].clone())
                .unwrap()
                .contains("Copyright (c) Microsoft Corporation.")
        );
        assert!(!files.values().any(|bytes| bytes == fallback.as_bytes()));
    }

    #[test]
    fn empty_extra_directories_and_deep_or_oversized_trees_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("license"), b"owned").unwrap();
        let expected = enumerate(root.path()).unwrap();
        fs::create_dir(root.path().join("unrelated-empty")).unwrap();
        assert!(matches_tree(root.path(), &expected).is_err());
        fs::remove_dir(root.path().join("unrelated-empty")).unwrap();
        let mut deep = root.path().to_owned();
        for _ in 0..17 {
            deep.push("a");
            fs::create_dir(&deep).unwrap();
        }
        assert!(enumerate(root.path()).is_err());

        let large = tempfile::tempdir().unwrap();
        for index in 0..4 {
            fs::File::create(large.path().join(index.to_string()))
                .unwrap()
                .set_len(16 * 1024 * 1024)
                .unwrap();
        }
        assert_eq!(
            enumerate(large.path())
                .unwrap()
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            64 * 1024 * 1024
        );
        fs::write(large.path().join("one-more-byte"), [1]).unwrap();
        assert!(enumerate(large.path()).is_err());
    }

    #[test]
    fn all_inventory_nodes_count_toward_the_4096_entry_bound() {
        let owned = tempfile::tempdir().unwrap();
        for index in 0..4096 {
            fs::File::create(owned.path().join(format!("n-{index:04}"))).unwrap();
        }
        assert_eq!(enumerate(owned.path()).unwrap().len(), 4096);
        fs::create_dir(owned.path().join("one-more-directory")).unwrap();
        assert!(enumerate(owned.path()).is_err());
    }

    #[test]
    fn actual_output_admission_rejects_absent_tampered_and_stale_ownership_records() {
        let owned = tempfile::tempdir().unwrap();
        let output = owned.path().join("notices");
        assert!(admitted_output(&output).unwrap().is_none());
        fs::create_dir(&output).unwrap();
        fs::write(output.join("license"), b"before").unwrap();
        assert!(admitted_output(&output).is_err());
        let payload = enumerate(&output).unwrap();
        let marker = output_manifest(&payload).unwrap();
        fs::write(output.join(OUTPUT_MANIFEST), &marker).unwrap();
        assert!(admitted_output(&output).unwrap().is_some());
        fs::write(output.join(OUTPUT_MANIFEST), b"tampered record").unwrap();
        assert!(admitted_output(&output).is_err());
        fs::write(output.join(OUTPUT_MANIFEST), &marker).unwrap();
        fs::write(output.join("license"), b"after!").unwrap();
        assert!(admitted_output(&output).is_err());
        fs::write(output.join("license"), b"before").unwrap();
        fs::create_dir(output.join("unrelated-empty")).unwrap();
        assert!(admitted_output(&output).is_err());
        assert_eq!(fs::read(output.join("license")).unwrap(), b"before");
        assert!(output.join("unrelated-empty").is_dir());
    }

    #[test]
    #[cfg(unix)]
    fn native_backslash_names_are_refused_without_collapsing_aliases() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        fs::write(root.path().join("a/b"), b"nested").unwrap();
        fs::write(root.path().join("a\\b"), b"literal backslash").unwrap();
        assert!(enumerate(root.path()).is_err());
        assert_eq!(fs::read(root.path().join("a/b")).unwrap(), b"nested");
        assert_eq!(
            fs::read(root.path().join("a\\b")).unwrap(),
            b"literal backslash"
        );
    }

    #[test]
    #[cfg(unix)]
    fn generated_tree_symlinks_are_rejected_without_following_them() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("owned-target"), b"unchanged").unwrap();
        std::os::unix::fs::symlink(root.path().join("owned-target"), root.path().join("link"))
            .unwrap();
        assert!(enumerate(root.path()).is_err());
        assert_eq!(
            fs::read(root.path().join("owned-target")).unwrap(),
            b"unchanged"
        );
    }

    #[cfg(unix)]
    fn previous_tree(root: &Path) -> (PathBuf, Files) {
        let output = root.join("notices");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("license"), b"previous owned bytes").unwrap();
        let admitted = enumerate(&output).unwrap();
        (output, admitted)
    }

    #[test]
    #[cfg(unix)]
    fn post_swap_validation_failure_restores_previous_bytes() {
        let owned = tempfile::tempdir().unwrap();
        let (output, admitted) = previous_tree(owned.path());
        let expected = BTreeMap::from([("license".into(), b"next owned bytes".to_vec())]);
        let result = replace_tree(&output, &expected, Some(&admitted), |path| {
            fs::write(path.join("unexpected"), b"owned readback change")?;
            matches_tree(path, &expected)
        });
        assert!(result.is_err());
        matches_tree(&output, &admitted).unwrap();
        let stage = fs::read_dir(owned.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".notice-generation-")
            })
            .unwrap();
        assert_eq!(
            fs::read(stage.join("failed-output/license")).unwrap(),
            b"next owned bytes"
        );
        assert_eq!(
            fs::read(stage.join("failed-output/unexpected")).unwrap(),
            b"owned readback change"
        );
    }

    #[test]
    #[cfg(unix)]
    fn failed_restoration_retains_old_data_and_both_errors() {
        let owned = tempfile::tempdir().unwrap();
        let (output, admitted) = previous_tree(owned.path());
        let expected = BTreeMap::from([("license".into(), b"next owned bytes".to_vec())]);
        let mut retained = None;
        let result = replace_tree(&output, &expected, Some(&admitted), |path| {
            let stage = fs::read_dir(owned.path())?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()?
                .into_iter()
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".notice-generation-")
                })
                .ok_or("Missing owned test stage")?;
            fs::rename(stage.join("previous"), stage.join("preserved-original"))?;
            fs::remove_file(path.join("license"))?;
            retained = Some(stage);
            matches_tree(path, &expected)
        });
        let error = result.unwrap_err().to_string();
        assert!(error.contains("differ from the admitted tree"));
        assert!(error.contains("Could not restore displaced output"));
        let retained = retained.unwrap();
        assert_eq!(
            fs::read(retained.join("preserved-original/license")).unwrap(),
            b"previous owned bytes"
        );
        assert!(retained.join("failed-output").is_dir());
    }

    #[test]
    #[cfg(unix)]
    fn blocked_quarantine_and_restore_preserve_both_trees_and_errors() {
        let owned = tempfile::tempdir().unwrap();
        let (output, admitted) = previous_tree(owned.path());
        let expected = BTreeMap::from([("license".into(), b"next owned bytes".to_vec())]);
        let mut retained = None;
        let result = replace_tree(&output, &expected, Some(&admitted), |path| {
            let stage = fs::read_dir(owned.path())?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()?
                .into_iter()
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".notice-generation-")
                })
                .ok_or("Missing owned test stage")?;
            fs::create_dir(stage.join("failed-output"))?;
            fs::write(path.join("license"), b"observed changed new output")?;
            retained = Some(stage);
            matches_tree(path, &expected)
        });
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Could not retain failed output separately"));
        assert!(error.contains("Could not restore displaced output"));
        assert_eq!(
            fs::read(output.join("license")).unwrap(),
            b"observed changed new output"
        );
        assert_eq!(
            fs::read(retained.unwrap().join("previous/license")).unwrap(),
            b"previous owned bytes"
        );
    }

    #[test]
    #[cfg(unix)]
    fn commit_boundary_changes_are_restored_and_unexpected_output_is_not_replaced() {
        let owned = tempfile::tempdir().unwrap();
        let (output, admitted) = previous_tree(owned.path());
        let expected = BTreeMap::from([("license".into(), b"next owned bytes".to_vec())]);
        fs::write(output.join("license"), b"later unrelated edit").unwrap();
        assert!(
            replace_tree(&output, &expected, Some(&admitted), |path| matches_tree(
                path, &expected
            ))
            .is_err()
        );
        assert_eq!(
            fs::read(output.join("license")).unwrap(),
            b"later unrelated edit"
        );
        let unexpected = owned.path().join("unexpected");
        fs::create_dir(&unexpected).unwrap();
        assert!(
            replace_tree(&unexpected, &expected, None, |path| matches_tree(
                path, &expected
            ))
            .is_err()
        );
        assert_eq!(fs::read_dir(unexpected).unwrap().count(), 0);
    }
}
