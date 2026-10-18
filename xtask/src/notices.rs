//! Target-specific legal materials from independently reviewed package bindings.
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
    scope: &str,
    report: &About,
    review: &Review,
    lock: &Lock,
) -> Result<(Files, Vec<Value>)> {
    let expected: BTreeMap<_, _> = review
        .packages
        .iter()
        .filter(|package| package.scopes.iter().any(|value| value == scope))
        .map(|package| ((package.name.clone(), package.version.clone()), package))
        .collect();
    if expected.is_empty() {
        return Err("Notice scope has no reviewed package closure".into());
    }
    if expected.len()
        != review
            .packages
            .iter()
            .filter(|package| package.scopes.iter().any(|value| value == scope))
            .count()
    {
        return Err("Duplicate reviewed package identity in one component/target scope".into());
    }
    let mut actual = BTreeMap::new();
    for entry in &report.crates {
        let key = (entry.package.name.clone(), entry.package.version.clone());
        let reviewed = expected
            .get(&key)
            .ok_or("Unreviewed package in scope notice closure")?;
        if entry.license != reviewed.license_expression
            || actual.insert(key, &entry.package).is_some()
        {
            return Err("Duplicate package or changed notice license expression".into());
        }
        verify_package(root, &entry.package, reviewed, lock)?;
        if !reviewed
            .bindings
            .iter()
            .any(|binding| binding.scopes.iter().any(|value| value == scope))
        {
            return Err(
                "Reviewed package has no legal material binding for this component and target"
                    .into(),
            );
        }
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
                .ok_or("License group refers to a package outside its scope closure")?;
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
                    binding.scopes.iter().any(|value| value == scope)
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
            let mut additional = Vec::new();
            let selected = match binding.mapping.as_str() {
                "exact_per_package_legal_file" => {
                    if materials.len() != 1 || materials[0] != license.text.as_bytes() {
                        return Err(
                            "Per-package legal bytes differ from selected generator text".into(),
                        );
                    }
                    materials[0].clone()
                }
                "complete_source_license_materials" => {
                    if materials.is_empty() || materials.iter().any(Vec::is_empty) {
                        return Err("Missing complete source license material".into());
                    }
                    for material in materials.iter().skip(1) {
                        additional.push(text_file(&mut files, material.clone())?);
                    }
                    materials[0].clone()
                }
                "identified_standard_text_with_source_declarations" => {
                    let standard = binding
                        .standard_text
                        .as_ref()
                        .ok_or("Missing identified standard text")?;
                    if standard["sha256"] != hash
                        || standard["bytes"] != license.text.len()
                        || standard["license_id"] != license.id
                        || materials.is_empty()
                        || materials.iter().any(Vec::is_empty)
                    {
                        return Err(
                            "Standard template or source declarations differ from review".into(),
                        );
                    }
                    for material in &materials {
                        additional.push(text_file(&mut files, material.clone())?);
                    }
                    license.text.as_bytes().to_vec()
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
                            .find(|fact| fact.path == "legal/objc2/source.json")
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
            additional.retain(|path| path != &file);
            inventory.push(json!({"name":package.name,"version":package.version,"source":package.source,
                "checksum":reviewed.checksum,"license_expression":reviewed.license_expression,
                "license_id":license.id,"generator_text_sha256":hash,"file":file,
                "review_mapping":binding.mapping,"package_archive_identity_checked":package.source.is_some(),
                "unpacked_materials_checked_separately":true,
                "materials":binding.materials,"additional_material_files":additional,
                "source_provenance":reviewed.provenance }));
        }
    }
    // cargo-about 0.9.2 omits selected LicenseRef text groups, including Slint's
    // bundled royalty-free license. Preserve this omission as a generator fact;
    // supplement only an explicitly reviewed exact per-package source file.
    for (key, reviewed) in &expected {
        for binding in reviewed.bindings.iter().filter(|binding| {
            binding.scopes.iter().any(|value| value == scope)
                && binding.mapping == "explicit_package_license_reference_supplement"
        }) {
            if !binding.license_id.starts_with("LicenseRef-")
                || !reviewed
                    .license_expression
                    .split(|value: char| value.is_whitespace() || matches!(value, '(' | ')'))
                    .any(|value| value == binding.license_id)
                || !binding.generator_text_sha256.is_empty()
                || binding.generator_text_bytes != 0
                || binding.materials.len() != 1
                || binding.materials[0].kind != "package_file"
                || observed.iter().any(|(package, _, _)| package == key)
            {
                return Err("Invalid or redundant reviewed license-reference supplement".into());
            }
            let package = actual[key];
            let bytes = material_bytes(root, package, &binding.materials[0])?;
            if bytes.is_empty() {
                return Err("Empty license-reference supplement".into());
            }
            let file = text_file(&mut files, bytes)?;
            observed.insert((key.clone(), binding.license_id.clone(), String::new()));
            inventory.push(
                json!({"name":package.name,"version":package.version,"source":package.source,
                "checksum":reviewed.checksum,"license_expression":reviewed.license_expression,
                "license_id":binding.license_id,"generator_text_sha256":null,
                "generator_observation":"selected package present; license text group omitted",
                "file":file,"review_mapping":binding.mapping,
                "package_archive_identity_checked":package.source.is_some(),
                "unpacked_materials_checked_separately":true,"materials":binding.materials}),
            );
        }
    }
    let expected_count: usize = expected
        .values()
        .map(|package| {
            package
                .bindings
                .iter()
                .filter(|binding| binding.scopes.iter().any(|value| value == scope))
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeComponent {
    Cli,
    Gui,
    Worker,
}
impl NoticeComponent {
    fn applies_to(self, platform: &str) -> bool {
        self == Self::Cli || platform.starts_with("macos-")
    }
    fn name(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Gui => "gui",
            Self::Worker => "worker",
        }
    }
    fn package(self) -> &'static str {
        match self {
            Self::Cli => "remove-eml-attachments",
            Self::Gui => "eml-gui",
            Self::Worker => "eml-worker",
        }
    }
    fn manifest(self) -> &'static str {
        match self {
            Self::Cli => "crates/eml-cli/Cargo.toml",
            Self::Gui => "crates/eml-gui/Cargo.toml",
            Self::Worker => "crates/eml-worker/Cargo.toml",
        }
    }
    fn config(self) -> &'static str {
        if self == Self::Gui {
            "xtask/app-notices.toml"
        } else {
            "xtask/notices.toml"
        }
    }
    fn prefix(self, platform: &str) -> String {
        if self == Self::Cli {
            format!("{platform}/")
        } else {
            format!("{}/{platform}/", self.name())
        }
    }
}

struct NoticeTarget<'a> {
    target: &'a str,
    platform: &'a str,
    version: &'a str,
    component: NoticeComponent,
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
        component,
    } = selection;
    let mut arguments = vec![
        "about",
        "generate",
        "--locked",
        "--manifest-path",
        component.manifest(),
        "--target",
        target,
        "--config",
        component.config(),
        "--format",
        "json",
        "--fail",
    ];
    if component == NoticeComponent::Gui {
        arguments.extend(["--features", "macos-gui"]);
    }
    let bytes = capture(crate::tools::command(root, "about")?.args(&arguments[1..]))?;
    let report: About = serde_json::from_slice(&bytes)?;
    let (mut files, inventory) = render(
        root,
        &format!("{}@{target}", component.name()),
        &report,
        review,
        lock,
    )?;
    let mut features_arguments = vec![
        "tree",
        "--locked",
        "--package",
        component.package(),
        "--target",
        target,
        "--edges",
        "normal,build",
        "--prefix",
        "depth",
        "--format",
        "{p}|{f}",
    ];
    if component == NoticeComponent::Gui {
        features_arguments.extend(["--features", "eml-gui/macos-gui"]);
    }
    let features = String::from_utf8(capture(
        Command::new("cargo")
            .current_dir(root)
            .args(&features_arguments),
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
        "legal/rust",
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
            "legal/windows-runtime",
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
    let mut notice = format!(
        "EML Attachment Remover {version}\nProduct: {}\nTarget: {target} ({platform})\n\n",
        component.name()
    );
    notice.push_str("These materials cover the target-selected locked component normal/build dependency graph. Build and proc-macro materials are included conservatively; this list does not assert that every listed crate is linked application code.\n\n");
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
        if let Some(additional) = item["additional_material_files"].as_array() {
            for file in additional {
                notice.push_str(&format!(
                    "  Additional source/notice material: {}\n",
                    file.as_str().ok_or("Invalid accompanying material path")?
                ));
            }
        }
    }
    if inventory
        .iter()
        .any(|item| item["review_mapping"] == "identified_standard_text_with_source_declarations")
    {
        notice.push_str("\nIdentified standard license templates are not upstream copyright attributions. The corresponding complete source declarations and notices are retained separately above; no missing copyright year or owner was invented.\n");
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
            "version":version,"target":target,"platform":platform,"component":component.name(),
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

fn matches_tree(path: &Path, expected: &Files) -> Result<()> {
    if enumerate(path)? != *expected {
        return Err("Generated legal materials differ from the expected tree".into());
    }
    Ok(())
}

/// New output only: there is no replacement or migration of tracked notices.
fn write_new_tree(output: &Path, files: &Files) -> Result<()> {
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

fn generated_materials(
    root: &Path,
    selected: Option<(&str, NoticeComponent)>,
) -> Result<(PathBuf, Files)> {
    campaign::require_stock_topology(root)?;
    let tool = campaign::tool_version(root, "about", "cargo_about")?;
    let root = root.canonicalize()?;
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
    insert(
        &mut all,
        "source.json".into(),
        serde_json::to_vec_pretty(&json!({
            "head": crate::git(&root, &["rev-parse", "HEAD"])?,
            "working_tree": crate::git(&root, &["status", "--porcelain=v1", "--untracked-files=all"])?,
            "scope": "generated legal materials; not native execution or legal review evidence"
        }))?,
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
        for target in targets {
            let platform = target["platform"]
                .as_str()
                .ok_or("Missing platform identity")?;
            if selected.is_some_and(|(selected, _)| selected != platform)
                || !component.applies_to(platform)
            {
                continue;
            }
            let target = target["target"].as_str().ok_or("Missing target identity")?;
            eprintln!(
                "Generating {} legal materials: {platform} ({target})",
                component.name()
            );
            for (path, bytes) in target_materials(
                &root,
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

pub(crate) fn run(root: &Path, output: &Path) -> Result<()> {
    match fs::symlink_metadata(output) {
        Ok(_) => return Err("Notice writing requires a new output directory; use --check to inspect existing materials".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => return Err(error.into()),
    }
    let (_, all) = generated_materials(root, None)?;
    write_new_tree(output, &all)
}

fn source_identity(source: &Value, head: &str, working_tree: &str) -> Result<()> {
    if source["head"] != head || source["working_tree"] != "" || !working_tree.is_empty() {
        return Err(
            "Legal producer and native consumer must use the same clean source checkout".into(),
        );
    }
    Ok(())
}

fn checked_generated_input(root: &Path, directory: &Path) -> Result<Files> {
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
    source_identity(
        &source,
        &crate::git(root, &["rev-parse", "HEAD"])?,
        &crate::git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?,
    )?;
    all.insert(OUTPUT_MANIFEST.into(), marker);
    Ok(all)
}

fn checked_pairs(all: &Files, plan: &Value) -> Result<usize> {
    let targets = plan["native_targets"]
        .as_array()
        .ok_or("Missing native notice policy")?;
    let version = plan["version"].as_str().ok_or("Missing notice version")?;
    let mut prefixes = BTreeSet::new();
    for row in targets {
        let platform = row["platform"].as_str().ok_or("Missing notice platform")?;
        for component in [
            NoticeComponent::Cli,
            NoticeComponent::Gui,
            NoticeComponent::Worker,
        ] {
            if !component.applies_to(platform) {
                continue;
            }
            let prefix = component.prefix(platform);
            let identity: Value = serde_json::from_slice(
                all.get(&format!("{prefix}licenses/materials.json"))
                    .ok_or("Missing required notice component")?,
            )?;
            if identity["platform"] != platform
                || identity["target"] != row["target"]
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

pub(crate) fn check(root: &Path, output: Option<&Path>) -> Result<Value> {
    campaign::require_stock_topology(root)?;
    let (all, mode) = if let Some(directory) = std::env::var_os("EML_GENERATED_LEGAL_MATERIALS") {
        (
            checked_generated_input(root, Path::new(&directory))?,
            "consumer_identity",
        )
    } else {
        (
            generated_materials(root, None)?.1,
            "generated_source_material_validation",
        )
    };
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let pairs = checked_pairs(&all, &plan)?;
    if let Some(output) = output {
        matches_tree(output, &all)?;
    }
    let source: Value = serde_json::from_slice(
        all.get("source.json")
            .ok_or("Missing notice source receipt")?,
    )?;
    Ok(
        json!({"mode":mode,"profiles":pairs,"entries":all.len(),"source":source,"compared_output":output,
        "scope":if mode=="consumer_identity" {"same-clean-source generated material identity/completeness; successful same-source producer is a separate prerequisite, not attested by inventory/source SHA"} else {"current selected locked graphs and reviewed source/material bindings regenerated and validated; no native execution or external legal assurance"}}),
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
    checked_component(root, platform, NoticeComponent::Cli)
}

pub(crate) fn checked_component(
    root: &Path,
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
        checked_generated_input(root, Path::new(&directory))?
    } else {
        generated_materials(root, Some((platform, component)))?.1
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
mod tests {
    use super::*;

    #[test]
    fn generated_source_identity_rejects_stale_and_dirty_receipts() {
        let source = json!({"head":"owned-head","working_tree":""});
        source_identity(&source, "owned-head", "").unwrap();
        assert!(source_identity(&source, "different-head", "").is_err());
        assert!(source_identity(&source, "owned-head", " M source").is_err());
        for invalid in [
            json!({"head":"owned-head","working_tree":" M source"}),
            json!({"head":"owned-head"}),
            json!({"working_tree":""}),
        ] {
            assert!(source_identity(&invalid, "owned-head", "").is_err());
        }
    }

    #[test]
    fn current_policy_requires_every_notice_pair_and_rejects_extra_or_mismatched_components() {
        let plan = json!({"version":"6.0.0","native_targets":[{"platform":"macos-aarch64","target":"aarch64-apple-darwin"},{"platform":"linux-x86_64","target":"x86_64-unknown-linux-gnu"}]});
        let mut all = Files::new();
        for (prefix, platform, target, component) in [
            (
                "macos-aarch64/",
                "macos-aarch64",
                "aarch64-apple-darwin",
                "cli",
            ),
            (
                "gui/macos-aarch64/",
                "macos-aarch64",
                "aarch64-apple-darwin",
                "gui",
            ),
            (
                "worker/macos-aarch64/",
                "macos-aarch64",
                "aarch64-apple-darwin",
                "worker",
            ),
            (
                "linux-x86_64/",
                "linux-x86_64",
                "x86_64-unknown-linux-gnu",
                "cli",
            ),
        ] {
            all.insert(format!("{prefix}licenses/materials.json"), serde_json::to_vec(&json!({"platform":platform,"target":target,"component":component,"version":"6.0.0"})).unwrap());
        }
        add_directories(&mut all);
        assert_eq!(checked_pairs(&all, &plan).unwrap(), 4);
        let path = "gui/macos-aarch64/licenses/materials.json";
        let mut missing = all.clone();
        missing.remove(path);
        assert!(checked_pairs(&missing, &plan).is_err());
        let mut extra = all.clone();
        extra.insert(
            "gui/linux-x86_64/licenses/materials.json".into(),
            b"{}".to_vec(),
        );
        assert!(checked_pairs(&extra, &plan).is_err());
        for key in ["platform", "target", "component", "version"] {
            let mut invalid = all.clone();
            let mut identity: Value = serde_json::from_slice(&invalid[path]).unwrap();
            identity[key] = json!("wrong");
            invalid.insert(path.into(), serde_json::to_vec(&identity).unwrap());
            assert!(checked_pairs(&invalid, &plan).is_err(), "{key}");
        }
    }

    #[test]
    fn existing_generated_tree_check_rejects_changed_missing_and_extra_files() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("materials");
        let expected = Files::from([("license".into(), b"owned text".to_vec())]);
        write_new_tree(&output, &expected).unwrap();
        matches_tree(&output, &expected).unwrap();
        fs::write(output.join("license"), b"alter text").unwrap();
        assert!(matches_tree(&output, &expected).is_err());
        fs::remove_file(output.join("license")).unwrap();
        assert!(matches_tree(&output, &expected).is_err());
        fs::write(output.join("license"), b"owned text").unwrap();
        fs::write(output.join("extra"), b"extra").unwrap();
        assert!(matches_tree(&output, &expected).is_err());
    }

    #[test]
    fn standard_template_retains_declarations_and_refuses_changed_source_material() {
        let root = tempfile::tempdir().unwrap();
        let declaration = "Owned synthetic license declaration and attribution\n";
        fs::write(root.path().join("Cargo.toml"), declaration).unwrap();
        let standard = "Identified synthetic standard text, not an upstream attribution\n";
        let package = json!({"name":"owned-declaration","version":"1.0.0","source":null,"manifest_path":root.path().join("Cargo.toml")});
        let report: About =
            serde_json::from_value(json!({"crates":[{"package":package,"license":"MIT"}],
            "licenses":[{"id":"MIT","text":standard,"used_by":[{"crate":package}]}]}))
            .unwrap();
        let review: Review = serde_json::from_value(json!({"generator_pin":{},"source_inputs":[],"targets":[{"target":"owned-target"}],
            "packages":[{"name":"owned-declaration","version":"1.0.0","source":null,"checksum":null,"license_expression":"MIT","scopes":["owned-target"],
                "provenance":{"kind":"workspace","manifest_path":"Cargo.toml"},"bindings":[{"license_id":"MIT","generator_text_sha256":digest(standard.as_bytes()),"generator_text_bytes":standard.len(),
                "mapping":"identified_standard_text_with_source_declarations","scopes":["owned-target"],
                "standard_text":{"license_id":"MIT","sha256":digest(standard.as_bytes()),"bytes":standard.len()},
                "materials":[{"kind":"package_file","path":"Cargo.toml","bytes":declaration.len(),"sha256":digest(declaration.as_bytes())}]}]}]})).unwrap();
        let lock: Lock = serde_json::from_value(json!({"package":[{"name":"owned-declaration","version":"1.0.0","source":null,"checksum":null}]})).unwrap();
        let (files, inventory) =
            render(root.path(), "owned-target", &report, &review, &lock).unwrap();
        assert_eq!(
            files[inventory[0]["file"].as_str().unwrap()],
            standard.as_bytes()
        );
        let additional = inventory[0]["additional_material_files"]
            .as_array()
            .unwrap();
        assert_eq!(additional.len(), 1);
        assert_eq!(
            files[additional[0].as_str().unwrap()],
            declaration.as_bytes()
        );
        fs::write(root.path().join("Cargo.toml"), b"changed declaration").unwrap();
        assert!(render(root.path(), "owned-target", &report, &review, &lock).is_err());
    }

    #[test]
    fn omitted_reference_requires_explicit_material_and_preserves_generator_omission() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Cargo.toml"), b"owned manifest").unwrap();
        let text = "Synthetic owned custom license material\n";
        fs::write(root.path().join("LICENSE"), text).unwrap();
        let package = json!({"name":"owned-reference","version":"1.0.0","source":null,"manifest_path":root.path().join("Cargo.toml")});
        let mut report: About = serde_json::from_value(json!({"crates":[{"package":package,"license":"LicenseRef-Owned-Fixture"}],"licenses":[]})).unwrap();
        let mut review: Review = serde_json::from_value(json!({"generator_pin":{},"source_inputs":[],"targets":[{"target":"owned-target"}],
            "packages":[{"name":"owned-reference","version":"1.0.0","source":null,"checksum":null,"license_expression":"LicenseRef-Owned-Fixture",
                "scopes":["owned-target"],"provenance":{"kind":"workspace","manifest_path":"Cargo.toml"},"bindings":[]}]})).unwrap();
        let lock: Lock = serde_json::from_value(json!({"package":[{"name":"owned-reference","version":"1.0.0","source":null,"checksum":null}]})).unwrap();
        assert!(
            render(root.path(), "owned-target", &report, &review, &lock)
                .unwrap_err()
                .to_string()
                .contains("no legal material binding")
        );
        review.packages[0].bindings.push(serde_json::from_value(json!({"license_id":"LicenseRef-Owned-Fixture","generator_text_sha256":"","generator_text_bytes":0,
            "mapping":"explicit_package_license_reference_supplement","scopes":["owned-target"],
            "materials":[{"kind":"package_file","path":"LICENSE","bytes":text.len(),"sha256":digest(text.as_bytes())}]})).unwrap());
        let (files, inventory) =
            render(root.path(), "owned-target", &report, &review, &lock).unwrap();
        assert_eq!(
            files[inventory[0]["file"].as_str().unwrap()],
            text.as_bytes()
        );
        assert!(inventory[0]["generator_text_sha256"].is_null());
        assert_eq!(
            inventory[0]["generator_observation"],
            "selected package present; license text group omitted"
        );
        fs::write(root.path().join("LICENSE"), b"changed material").unwrap();
        assert!(render(root.path(), "owned-target", &report, &review, &lock).is_err());
        fs::write(root.path().join("LICENSE"), text).unwrap();
        report.licenses.push(
            serde_json::from_value(
                json!({"id":"LicenseRef-Owned-Fixture","text":text,"used_by":[{"crate":package}]}),
            )
            .unwrap(),
        );
        assert!(
            render(root.path(), "owned-target", &report, &review, &lock).is_err(),
            "Changed generator behavior requires review, not a duplicate supplement"
        );
    }

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
                "license_expression":"MIT","scopes":["owned-target"],
                "provenance":{"kind":"workspace","manifest_path":format!("{name}/Cargo.toml")},
                "bindings":[{"license_id":"MIT","generator_text_sha256":digest(text.as_bytes()),"generator_text_bytes":text.len(),
                    "mapping":"exact_per_package_legal_file","scopes":["owned-target"],
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
                "license_expression":"MIT OR Apache-2.0","scopes":["owned-target"],"provenance":{"kind":"registry"},
                "bindings":[{"license_id":"MIT","generator_text_sha256":digest(fallback.as_bytes()),"generator_text_bytes":fallback.len(),
                    "mapping":"complete_source_license_materials","scopes":["owned-target"],
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

    #[test]
    fn generation_refuses_an_existing_output_without_changing_it() {
        let owned = tempfile::tempdir().unwrap();
        let output = owned.path().join("generated");
        let files = BTreeMap::from([("license".into(), b"retained legal text".to_vec())]);
        write_new_tree(&output, &files).unwrap();
        fs::write(output.join("unrelated"), b"keep this").unwrap();
        assert!(write_new_tree(&output, &files).is_err());
        assert_eq!(fs::read(output.join("unrelated")).unwrap(), b"keep this");
        assert_eq!(
            fs::read(output.join("license")).unwrap(),
            b"retained legal text"
        );
    }
}
