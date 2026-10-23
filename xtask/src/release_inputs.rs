//! Source-bound admission of immutable products and their actual native consumers.
use crate::{
    Result,
    distribution::{BuildContext, BuildPlan, OsFamily, ProductRole},
    picker_evidence::{QualificationBinding, StringMap, VerifiedPickerReceipt},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use xtask::archive::{ArchiveSnapshot, ByteIdentity};
const MAX_RECORD: usize = 16 * 1024 * 1024;
const MAX_MANIFEST: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowIdentity {
    pub(crate) run: String,
    pub(crate) attempt: String,
    pub(crate) workflow_ref: String,
    pub(crate) workflow_sha: String,
}
impl WorkflowIdentity {
    fn validate(&self) -> Result<()> {
        for number in [&self.run, &self.attempt] {
            if number.is_empty()
                || number.len() > 20
                || !number.bytes().all(|b| b.is_ascii_digit())
                || number.bytes().all(|b| b == b'0')
            {
                return Err("Invalid workflow run or attempt".into());
            }
        }
        if self.workflow_ref.is_empty()
            || self.workflow_ref.len() > 512
            || self.workflow_ref.chars().any(char::is_control)
            || self.workflow_sha.len() != 40
            || !self
                .workflow_sha
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("Invalid workflow reference or source identity".into());
        }
        Ok(())
    }
}
pub(crate) fn workflow_identity() -> Result<Option<WorkflowIdentity>> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        return Ok(None);
    }
    let identity = WorkflowIdentity {
        run: std::env::var("GITHUB_RUN_ID")?,
        attempt: std::env::var("GITHUB_RUN_ATTEMPT")?,
        workflow_ref: std::env::var("GITHUB_WORKFLOW_REF")?,
        workflow_sha: std::env::var("GITHUB_WORKFLOW_SHA")?,
    };
    identity.validate()?;
    Ok(Some(identity))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrozenRecord {
    identity: ByteIdentity,
    contents: String,
}
impl FrozenRecord {
    fn read(path: &Path) -> Result<Self> {
        let bytes = read(path, MAX_RECORD)?;
        Ok(Self {
            identity: ByteIdentity::of(&bytes),
            contents: String::from_utf8(bytes)?,
        })
    }
    fn document(&self) -> Result<Value> {
        if self.contents.len() > MAX_RECORD
            || ByteIdentity::of(self.contents.as_bytes()) != self.identity
        {
            return Err("Retained native receipt identity changed".into());
        }
        crate::picker_evidence::observation_document(self.contents.as_bytes())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductInput {
    platform: String,
    role: ProductRole,
    archive: ByteIdentity,
    archive_path: PathBuf,
    producer: FrozenRecord,
    #[serde(deserialize_with = "required_option")]
    reference: Option<FrozenRecord>,
    #[serde(deserialize_with = "required_option")]
    desktop: Option<FrozenRecord>,
    #[serde(deserialize_with = "required_option")]
    picker: Option<VerifiedPickerReceipt>,
    #[serde(deserialize_with = "required_option")]
    picker_product: Option<FrozenRecord>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerifiedInputs {
    schema: String,
    schema_version: u8,
    source: String,
    version: String,
    workflow: WorkflowIdentity,
    products: StringMap<ProductInput>,
}
fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
fn read(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(format!(
            "Release evidence must be a bounded ordinary file: {}",
            path.display()
        )
        .into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err("Release evidence grew beyond its bound".into());
    }
    Ok(bytes)
}
fn files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(
        path: &Path,
        depth: usize,
        examined: &mut usize,
        out: &mut Vec<PathBuf>,
    ) -> Result<()> {
        if depth > 32 || *examined >= 100_000 {
            return Err("Release input traversal exceeds its bound".into());
        }
        let meta = fs::symlink_metadata(path)?;
        *examined += 1;
        if meta.is_dir() {
            for entry in fs::read_dir(path)? {
                visit(&entry?.path(), depth + 1, examined, out)?;
            }
        } else if meta.is_file() {
            out.push(path.to_owned());
        } else {
            return Err("Release inputs contain a nonordinary entry".into());
        }
        Ok(())
    }
    if !fs::symlink_metadata(root)?.is_dir() {
        return Err("Release input root is not an ordinary directory".into());
    }
    let mut out = Vec::new();
    visit(root, 0, &mut 0, &mut out)?;
    out.sort();
    Ok(out)
}
fn unique_named(paths: &[PathBuf], name: &str) -> Result<PathBuf> {
    let matches: Vec<_> = paths
        .iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some(name))
        .collect();
    if matches.len() != 1 {
        return Err(format!("Expected exactly one {name} in its admitted artifact").into());
    }
    Ok(matches[0].to_owned())
}
fn record_binding(
    value: &Value,
    source: &str,
    target: &str,
    workflow: &WorkflowIdentity,
) -> Result<()> {
    if value["source"] != source
        || value["target"] != target
        || serde_json::from_value::<WorkflowIdentity>(value["workflow"].clone())? != *workflow
    {
        return Err("Native record source/target/run/attempt/workflow binding differs".into());
    }
    Ok(())
}
fn host(
    value: &Value,
    target: &crate::distribution::NativeTarget,
    major: Option<u32>,
) -> Result<()> {
    let platform = target.platform();
    let expected = platform.architecture().as_str();
    let kernel = if expected == "aarch64" {
        "arm64"
    } else {
        expected
    };
    if value["target"] != target.rust_target()
        || value["process_os"] != platform.family().as_str()
        || value["process_architecture"] != expected
    {
        return Err("Native record host OS/CPU/target differs".into());
    }
    if platform.family() == OsFamily::Macos
        && (value["kernel_machine"] != kernel
            || major.is_some_and(|major| {
                value["product_version"]
                    .as_str()
                    .and_then(|version| version.split('.').next())
                    .and_then(|major| major.parse::<u32>().ok())
                    != Some(major)
            }))
    {
        return Err("Native macOS host role differs".into());
    }
    Ok(())
}
fn check_desktop(
    document: &Value,
    source: &str,
    target: &crate::distribution::NativeTarget,
    workflow: &WorkflowIdentity,
    archive: &ByteIdentity,
    reference: &ByteIdentity,
    app_root: &str,
) -> Result<()> {
    record_binding(document, source, target.rust_target(), workflow)?;
    let desktop = target
        .desktop()
        .ok_or("Target has no desktop qualification role")?;
    host(&document["host"], target, Some(desktop.floor_major()))?;
    if document["stage"] != "desktop_qualification"
        || document["verification"]["stage"] != "desktop_qualification"
        || serde_json::from_value::<ByteIdentity>(document["verification"]["snapshot"].clone())?
            != *archive
    {
        return Err("Desktop consumer stage/archive identity differs".into());
    }
    if serde_json::from_value::<ByteIdentity>(document["reference"].clone())? != *reference {
        return Err("Desktop consumer reference identity differs".into());
    }
    let interaction = &document["verification"]["gui_interaction"];
    use crate::desktop_evidence::DesktopCase;
    for case in [
        DesktopCase::Workflow,
        DesktopCase::Pagination,
        DesktopCase::Appearance,
    ] {
        crate::desktop_evidence::validate_case(
            &interaction[case.name()],
            case,
            source,
            archive,
            app_root,
        )?;
    }
    if interaction["operation_controls"]["shipping_bundle_unchanged"] != true {
        return Err("Operation-control fixture changed the shipping bundle".into());
    }
    let cases = interaction["operation_controls"]["cases"]
        .as_array()
        .ok_or("Missing operation-control cases")?;
    let expected = DesktopCase::controlled_cases();
    let mut observed = BTreeSet::new();
    for value in cases {
        let name = value["case"]
            .as_str()
            .ok_or("Missing operation-control case name")?;
        let case = expected
            .iter()
            .find(|case| case.name() == name)
            .ok_or("Unexpected operation-control case")?;
        if !observed.insert(name) {
            return Err("Duplicate operation-control case".into());
        }
        crate::desktop_evidence::validate_case(value, *case, source, archive, app_root)?;
    }
    if observed != expected.iter().map(|case| case.name()).collect() {
        return Err("Desktop operation-control case set differs".into());
    }
    Ok(())
}
fn check_product(
    product: &ProductInput,
    name: &str,
    plan: &BuildPlan,
    source: &str,
    workflow: &WorkflowIdentity,
) -> Result<()> {
    let artifact = plan.named_artifact(name)?;
    let target = plan.lookup_platform(&product.platform)?;
    if artifact.key().platform() != target.platform() || artifact.key().role() != product.role {
        return Err("Archive name is bound to a different product/platform".into());
    }
    let producer = product.producer.document()?;
    record_binding(&producer, source, target.rust_target(), workflow)?;
    host(&producer["host"], target, None)?;
    match product.role {
        ProductRole::Cli => {
            if producer.get("qualification_failures") != Some(&serde_json::json!([])) {
                return Err(
                    "CLI producer retains a failed or missing required qualification result".into(),
                );
            }
            if serde_json::from_value::<ByteIdentity>(producer["archive"]["identity"].clone())?
                != product.archive
                || product.reference.is_some()
                || product.desktop.is_some()
                || product.picker.is_some()
                || product.picker_product.is_some()
            {
                return Err("CLI producer identity or consumer roles differ".into());
            }
            let image: ByteIdentity = serde_json::from_value(
                producer
                    .get("cli_image")
                    .ok_or("Missing frozen CLI image identity")?
                    .clone(),
            )?;
            crate::name_profiles::validate(
                producer
                    .get("name_profiles")
                    .ok_or("Missing required native naming profiles")?,
                source,
                &image,
                target.platform(),
            )?;
            if product.platform == "linux-x86_64" {
                crate::preparation_evidence::validate(
                    producer
                        .get("preparation_resources")
                        .ok_or("Missing required Linux preparation resources")?,
                    source,
                    &image,
                )?;
            } else if producer
                .get("preparation_resources")
                .is_some_and(|value| !value.is_null())
            {
                return Err(
                    "Preparation measurements cannot substitute for another native platform".into(),
                );
            }
        }
        ProductRole::DesktopApp => {
            if producer["stage"] != "native_candidate"
                || producer["verification"]["stage"] != "native_candidate"
                || serde_json::from_value::<ByteIdentity>(producer["identity"].clone())?
                    != product.archive
            {
                return Err("App producer stage/archive identity differs".into());
            }
            let reference = product
                .reference
                .as_ref()
                .ok_or("App reference is absent")?;
            let reference_document = reference.document()?;
            if serde_json::from_value::<ByteIdentity>(producer["reference"].clone())?
                != reference.identity
            {
                return Err("App producer reference identity differs".into());
            }
            record_binding(&reference_document, source, target.rust_target(), workflow)?;
            if reference_document["schema"] != 2
                || reference_document["version"] != plan.version()
                || reference_document["root"] != artifact.root()
                || reference_document["architecture"] != target.platform().architecture().as_str()
            {
                return Err("App reference schema/version differs".into());
            }
            check_desktop(
                &product
                    .desktop
                    .as_ref()
                    .ok_or("Complete desktop consumer is absent")?
                    .document()?,
                source,
                target,
                workflow,
                &product.archive,
                &reference.identity,
                artifact.root(),
            )?;
            let requires_picker = target
                .desktop()
                .and_then(|desktop| desktop.picker())
                .is_some();
            if requires_picker != product.picker.is_some()
                || requires_picker != product.picker_product.is_some()
            {
                return Err("Picker receipt role does not match the plan".into());
            }
            if let (Some(picker), Some(raw)) = (&product.picker, &product.picker_product) {
                let raw_document = raw.document()?;
                record_binding(&raw_document, source, target.rust_target(), workflow)?;
                picker.check_raw_product(
                    raw.contents.as_bytes(),
                    &binding(
                        source,
                        target.rust_target(),
                        workflow,
                        &product.archive,
                        &reference.identity,
                    ),
                )?;
                if picker.raw_product_identity() != &raw.identity {
                    return Err(
                        "Picker projection differs from the frozen actual product bytes".into(),
                    );
                }
            }
        }
    }
    Ok(())
}
fn binding(
    source: &str,
    target: &str,
    workflow: &WorkflowIdentity,
    archive: &ByteIdentity,
    reference: &ByteIdentity,
) -> QualificationBinding {
    QualificationBinding {
        source: source.into(),
        target: target.into(),
        run: workflow.run.clone(),
        attempt: workflow.attempt.clone(),
        workflow_ref: workflow.workflow_ref.clone(),
        workflow_sha: workflow.workflow_sha.clone(),
        archive: archive.clone(),
        reference: reference.clone(),
    }
}
impl VerifiedInputs {
    pub(crate) fn archive_identities(&self) -> BTreeMap<String, ByteIdentity> {
        self.products
            .iter()
            .map(|(name, product)| (name.clone(), product.archive.clone()))
            .collect()
    }
    fn validate(&self, plan: &BuildPlan, source: &str, workflow: &WorkflowIdentity) -> Result<()> {
        self.workflow.validate()?;
        if self.schema != "eml-verified-release-inputs"
            || self.schema_version != 1
            || self.source != source
            || self.version != plan.version()
            || &self.workflow != workflow
            || self.products.keys().cloned().collect::<BTreeSet<_>>() != plan.archive_names()
        {
            return Err("Verified input source/version/workflow/product set differs".into());
        }
        for (name, product) in self.products.iter() {
            check_product(product, name, plan, source, workflow)?;
        }
        Ok(())
    }
    pub(crate) fn archive_directory(&self) -> Result<&Path> {
        let mut parent = None;
        for product in self.products.values() {
            let current = product
                .archive_path
                .parent()
                .ok_or("Admitted archive has no parent")?;
            if parent.is_some_and(|parent| parent != current) {
                return Err("Admitted archive set has different frozen parents".into());
            }
            parent = Some(current);
        }
        parent.ok_or_else(|| "Admitted archive set is empty".into())
    }
    pub(crate) fn recheck_pickers(
        &self,
        root: &Path,
        plan: &BuildPlan,
        now: SystemTime,
    ) -> Result<()> {
        for product in self.products.values() {
            if let Some(receipt) = &product.picker {
                let reference = &product
                    .reference
                    .as_ref()
                    .ok_or("Picker product has no frozen reference")?
                    .identity;
                let target = plan.lookup_platform(&product.platform)?;
                crate::picker_evidence::recheck_release(
                    root,
                    plan,
                    receipt,
                    &binding(
                        &self.source,
                        target.rust_target(),
                        &self.workflow,
                        &product.archive,
                        reference,
                    ),
                    now,
                )?;
            }
        }
        Ok(())
    }
}
pub(crate) fn load(root: &Path, plan: &BuildPlan, path: &Path) -> Result<VerifiedInputs> {
    let manifest: VerifiedInputs = serde_json::from_slice(&read(path, MAX_MANIFEST)?)?;
    let workflow = workflow_identity()?
        .ok_or("Release input consumption requires exact current GitHub run identity")?;
    manifest.validate(plan, &crate::git(root, &["rev-parse", "HEAD"])?, &workflow)?;
    Ok(manifest)
}
pub(crate) fn run(
    context: &BuildContext,
    products: &Path,
    desktop: &Path,
    picker: &Path,
    out: &Path,
) -> Result<()> {
    let root = context.root();
    let plan = context.plan();
    if !crate::git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty()
        || crate::git(root, &["rev-parse", "HEAD"])? != context.source()
    {
        return Err("Release inputs require the exact clean source checkout".into());
    }
    let workflow =
        workflow_identity()?.ok_or("Release admission requires exact GitHub run identity")?;
    let all_products = files(products)?;
    let observed: BTreeSet<_> = all_products
        .iter()
        .filter_map(|path| {
            path.file_name()?
                .to_str()
                .filter(|name| name.ends_with(".zip") || name.ends_with(".tar.gz"))
                .map(str::to_owned)
        })
        .collect();
    let archive_count = all_products
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".zip") || name.ends_with(".tar.gz"))
        })
        .count();
    if observed != plan.archive_names() || archive_count != observed.len() {
        return Err("Downloaded archive set has missing, duplicate or unexpected products".into());
    }
    let mut admitted = BTreeMap::new();
    for artifact in plan.expected_artifacts() {
        let target = plan.target(artifact.key().platform());
        let platform = target.platform().to_string();
        let directory = products.join(format!(
            "native-products-{}-{platform}-r{}-a{}",
            context.source(),
            workflow.run,
            workflow.attempt
        ));
        let paths = files(&directory)?;
        let archive_path = unique_named(&paths, artifact.basename())?;
        let snapshot = ArchiveSnapshot::read(&archive_path)?;
        let producer = match artifact.key().role() {
            ProductRole::DesktopApp => {
                FrozenRecord::read(&unique_named(&paths, "app-result.json")?)?
            }
            ProductRole::Cli => FrozenRecord::read(&unique_named(&paths, "qualification.json")?)?,
        };
        let mut product = ProductInput {
            platform: platform.clone(),
            role: artifact.key().role(),
            archive: snapshot.identity(),
            archive_path,
            producer,
            reference: None,
            desktop: None,
            picker: None,
            picker_product: None,
        };
        if artifact.key().role() == ProductRole::DesktopApp {
            let reference = FrozenRecord::read(&unique_named(
                &paths,
                &format!("{}.reference.json", artifact.basename()),
            )?)?;
            let floor = desktop.join(format!(
                "floor-smoke-{}-{platform}-r{}-a{}",
                context.source(),
                workflow.run,
                workflow.attempt
            ));
            let desktop_records: Vec<_> = files(&floor)?
                .into_iter()
                .filter(|path| {
                    path.file_name().and_then(|name| name.to_str()) == Some("result.json")
                        && path
                            .parent()
                            .and_then(|parent| parent.parent())
                            .and_then(|parent| parent.file_name())
                            .and_then(|name| name.to_str())
                            == Some("app-archive-smoke")
                })
                .collect();
            if desktop_records.len() != 1 {
                return Err("Desktop product requires one exact complete consumer record".into());
            }
            product.desktop = Some(FrozenRecord::read(&desktop_records[0])?);
            if target
                .desktop()
                .and_then(|desktop| desktop.picker())
                .is_some()
            {
                let receipt_path = picker
                    .join(format!(
                        "picker-qualification-{}-{platform}-r{}-a{}",
                        context.source(),
                        workflow.run,
                        workflow.attempt
                    ))
                    .join("result.json");
                let bytes = read(&receipt_path, MAX_RECORD)?;
                product.picker_product = Some(FrozenRecord::read(
                    &receipt_path.with_file_name("product-result.json"),
                )?);
                product.picker = Some(crate::picker_evidence::verify_receipt(
                    root,
                    plan,
                    &bytes,
                    &binding(
                        context.source(),
                        target.rust_target(),
                        &workflow,
                        &product.archive,
                        &reference.identity,
                    ),
                    SystemTime::now(),
                )?);
            }
            product.reference = Some(reference);
        }
        check_product(
            &product,
            artifact.basename(),
            plan,
            context.source(),
            &workflow,
        )?;
        admitted.insert(artifact.basename().to_owned(), product);
    }
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let frozen = xtask::archive::create_artifact_directory(&out)?;
    for (name, product) in &mut admitted {
        let snapshot = ArchiveSnapshot::read(&product.archive_path)?;
        if snapshot.identity() != product.archive {
            return Err("Archive changed between admission and freezing".into());
        }
        let path = frozen.path().join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(snapshot.bytes())?;
        file.sync_all()?;
        drop(file);
        if ArchiveSnapshot::read(&path)?.identity() != product.archive {
            return Err("Frozen archive copy failed byte readback".into());
        }
        product.archive_path = path;
    }
    let manifest = VerifiedInputs {
        schema: "eml-verified-release-inputs".into(),
        schema_version: 1,
        source: context.source().into(),
        version: plan.version().into(),
        workflow,
        products: admitted.into(),
    };
    manifest.validate(plan, context.source(), &manifest.workflow)?;
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    if bytes.len() > MAX_MANIFEST {
        return Err("Verified release manifest exceeds its bound".into());
    }
    let path = out.join("verified-inputs.json");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"manifest":path,"source":context.source(),"version":plan.version(),"archives_directory":frozen.path(),"picker_receipts":manifest.products.values().filter_map(|product|product.picker.as_ref().map(|receipt|serde_json::json!({"platform":product.platform,"identity":receipt.identity(),"mode":receipt.mode()}))).collect::<Vec<_>>(),"scope":"admitted exact current-run product and native consumer identities; not permission to publish"})
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests;
