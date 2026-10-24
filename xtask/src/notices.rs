//! Target-specific legal materials from independently reviewed package bindings.
use crate::distribution::BuildPlan;
use crate::{Result, campaign, capture};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component as PathComponent, Path, PathBuf},
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
    scopes: Vec<String>,
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
    scopes: Vec<String>,
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
        json!({"scope":"actual selected component normal/build feature tree; host paths omitted without merging distinct occurrences", "entries":entries}),
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
            .all(|part| matches!(part, PathComponent::Normal(_)))
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

mod material;
mod render;
mod target;
mod tree;
use material::*;
use render::*;
pub(crate) use target::NoticeComponent;
use target::*;
use tree::*;

fn generated_materials(
    root: &Path,
    plan: &BuildPlan,
    selected: Option<(&str, NoticeComponent)>,
) -> Result<(PathBuf, Files)> {
    campaign::require_stock_topology(root)?;
    let tool = plan.tools().version("cargo_about")?;
    let about = crate::tools::path_pinned(root, "about", tool)?;
    let root = root.canonicalize()?;
    let review = checked_review(&root, tool)?;
    let lock: Lock = toml::from_str(&String::from_utf8(read(
        &root.join("Cargo.lock"),
        MAX_LEGAL_BYTES,
    )?)?)?;
    let version = plan.version();
    let compiler = String::from_utf8(capture(
        Command::new("rustc").current_dir(&root).arg("-vV"),
    )?)?;
    let targets: Vec<_> = plan.targets().collect();
    let expected: BTreeSet<_> = review
        .targets
        .iter()
        .map(|entry| entry.target.as_str())
        .collect();
    let actual: BTreeSet<_> = targets.iter().map(|entry| entry.rust_target()).collect();
    if actual != expected || targets.len() != expected.len() {
        return Err("Notice target set differs from reviewed native policy".into());
    }
    let mut all = BTreeMap::new();
    insert(
        &mut all,
        "source.json".into(),
        serde_json::to_vec_pretty(&source_document(&root, plan)?)?,
    )?;
    for component in [
        NoticeComponent::Cli,
        NoticeComponent::Gui,
        NoticeComponent::Worker,
    ] {
        if selected.is_some_and(|(_, selected)| selected != component) {
            continue;
        }
        let config = read(&root.join(component.config()), MAX_LEGAL_BYTES)?;
        for target in &targets {
            let platform_name = target.platform().to_string();
            let platform = platform_name.as_str();
            if selected.is_some_and(|(selected, _)| selected != platform)
                || !component.applies_to(target)
            {
                continue;
            }
            let target = target.rust_target();
            eprintln!(
                "Generating {} legal materials: {platform} ({target})",
                component.name()
            );
            for (path, bytes) in target_materials(
                &root,
                &about,
                NoticeTarget {
                    target,
                    platform,
                    version,
                    component,
                },
                &review,
                &lock,
                &compiler,
                &config,
            )? {
                insert(
                    &mut all,
                    format!("{}{path}", component.prefix(platform)),
                    bytes,
                )?;
            }
        }
    }
    let marker = OUTPUT_MANIFEST;
    add_directories(&mut all);
    let manifest = output_manifest(&all)?;
    all.insert(marker.into(), manifest);
    Ok((root, all))
}

pub(crate) fn run(root: &Path, plan: &BuildPlan, output: &Path) -> Result<()> {
    match fs::symlink_metadata(output) {
        Ok(_) => return Err("Notice writing requires a new output directory; use --check to inspect existing materials".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => return Err(error.into()),
    }
    let (_, all) = generated_materials(root, plan, None)?;
    write_new_tree(output, &all)
}

pub(crate) fn check(root: &Path, plan: &BuildPlan, output: Option<&Path>) -> Result<Value> {
    campaign::require_stock_topology(root)?;
    let (all, mode) = if let Some(directory) = std::env::var_os("EML_GENERATED_LEGAL_MATERIALS") {
        (
            checked_generated_input(root, plan, Path::new(&directory))?,
            "consumer_identity",
        )
    } else {
        (
            generated_materials(root, plan, None)?.1,
            "generated_source_material_validation",
        )
    };
    let pairs = checked_pairs(&all, plan)?;
    if let Some(output) = output {
        matches_tree(output, &all)?;
    }
    let source: Value = serde_json::from_slice(
        all.get("source.json")
            .ok_or("Missing notice source receipt")?,
    )?;
    Ok(
        json!({"mode":mode,"profiles":pairs,"entries":all.len(),"source":source,"compared_output":output,
        "scope":if mode=="consumer_identity" {"same-source tuple and generated material identity/completeness; Git inputs additionally require a clean checkout; declared archives remain unverified; successful same-source producer is a separate prerequisite, not attested by inventory/source SHA"} else {"current selected locked graphs and reviewed source/material bindings regenerated and validated; no native execution or external legal assurance"}}),
    )
}

/// Fresh target-selected legal bytes, generated from reviewed upstream inputs.
pub(crate) struct CheckedTargetMaterials {
    files: Files,
    target: String,
    platform: String,
    version: String,
}
impl CheckedTargetMaterials {
    pub(crate) fn files(&self) -> &Files {
        &self.files
    }
    pub(crate) fn target(&self) -> &str {
        &self.target
    }
    pub(crate) fn platform(&self) -> &str {
        &self.platform
    }
    pub(crate) fn version(&self) -> &str {
        &self.version
    }
}

pub(crate) fn checked_target(
    root: &Path,
    plan: &BuildPlan,
    platform: &str,
) -> Result<CheckedTargetMaterials> {
    checked_component(root, plan, platform, NoticeComponent::Cli)
}

pub(crate) fn checked_component(
    root: &Path,
    plan: &BuildPlan,
    platform: &str,
    component: NoticeComponent,
) -> Result<CheckedTargetMaterials> {
    if relative(platform)?.components().count() != 1 {
        return Err("Invalid notice platform component".into());
    }
    // cargo-about has no Intel macOS prebuilt at the pinned version. Native
    // builds may consume the target-selected output of the same CI run's Linux
    // producer. Identity checks bind that input, without treating it as review.
    let all = if let Some(directory) = std::env::var_os("EML_GENERATED_LEGAL_MATERIALS") {
        checked_generated_input(root, plan, Path::new(&directory))?
    } else {
        generated_materials(root, plan, Some((platform, component)))?.1
    };
    let prefix = component.prefix(platform);
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
    if identity["platform"] != platform || identity["component"] != component.name() {
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
mod tests;

fn checked_review(root: &Path, tool: &str) -> Result<Review> {
    let review: Review = serde_json::from_slice(&read(
        &root.join("legal/crate-materials.json"),
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
                | "crates/eml-protocol/Cargo.toml"
                | "crates/eml-core/Cargo.toml"
                | "crates/eml-fs/Cargo.toml"
                | "crates/eml-app/Cargo.toml"
                | "crates/eml-gui/Cargo.toml"
                | "crates/eml-worker/Cargo.toml"
                | "crates/eml-macos/Cargo.toml"
                | "vendor/winit/Cargo.toml"
                | "vendor/winit/EML-PATCH.md"
                | "vendor/i-slint-core/Cargo.toml"
                | "vendor/parley/Cargo.toml"
                | "vendor/typography.md"
                | "vendor/i-slint-core/textlayout/sharedparley.rs"
                | "vendor/i-slint-core/textlayout/sharedparley/shaping.rs"
                | "vendor/parley/src/shape/mod.rs"
                | "vendor/parley/src/bidi.rs"
                | "LICENSE"
        ) {
            checked_file(root, fact)?;
        }
    }
    Ok(review)
}

/// Same reviewed worker packages, with only the declared workspace fixture features.
#[cfg(target_os = "macos")]
pub(crate) fn operation_fixture_legal(
    root: &Path,
    target: &str,
    shipping_features: Vec<u8>,
    shipping_materials: Vec<u8>,
    out: &Path,
) -> Result<crate::app_reference::LegalBinding> {
    use crate::app_reference::{LegalBinding, ReferenceDocument};
    let mut materials = crate::picker_evidence::observation_document(&shipping_materials)?;
    let expected = feature_arguments(target, NoticeComponent::Worker);
    let mut args: Vec<String> =
        serde_json::from_value(materials["generation"]["feature_arguments"].clone())?;
    if args != expected {
        return Err("Shipping worker material feature command differs".into());
    }
    args.extend([
        "--features".to_owned(),
        "eml-worker/native-fixtures".to_owned(),
    ]);
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args(&args)
        .env("CARGO_INCREMENTAL", "0");
    let log = out.join("fixture-feature-tree.log");
    let process = crate::controller()?.run_separate_output(
        command,
        &log,
        std::time::Duration::from_secs(120),
    )?;
    process.require_success(&log)?;
    let packages = materials["packages"]
        .as_array()
        .ok_or("Missing worker material package inventory")?
        .iter()
        .map(|v| -> Result<(String, String)> {
            Ok((
                v["name"]
                    .as_str()
                    .ok_or("Missing material package name")?
                    .to_owned(),
                v["version"]
                    .as_str()
                    .ok_or("Missing material package version")?
                    .to_owned(),
            ))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let text = String::from_utf8(read(&log, 4 * 1024 * 1024)?)?;
    let fixture_features = serde_json::to_vec_pretty(&feature_tree(&text, &packages)?)?;
    materials["generation"]["feature_arguments"] = serde_json::to_value(args)?;
    let legal = LegalBinding {
        shipping_features: ReferenceDocument::new(shipping_features)?,
        fixture_features: ReferenceDocument::new(fixture_features)?,
        shipping_materials: ReferenceDocument::new(shipping_materials)?,
        fixture_materials: ReferenceDocument::new(serde_json::to_vec_pretty(&materials)?)?,
    };
    crate::app_reference::validate_legal(&legal)?;
    Ok(legal)
}
