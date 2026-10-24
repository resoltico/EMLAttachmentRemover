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
use xtask::archive::{ArchiveSnapshot, ByteIdentity, PackageEpoch};
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
    source_epoch: PackageEpoch,
    source_provenance: String,
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
fn read_picker_records(receipt_path: &Path) -> Result<(Vec<u8>, FrozenRecord)> {
    let bytes = read(receipt_path, MAX_RECORD).map_err(|error| {
        format!(
            "Read required picker qualification {}: {error}",
            receipt_path.display()
        )
    })?;
    let product_path = receipt_path.with_file_name("product-result.json");
    let product = FrozenRecord::read(&product_path).map_err(|error| {
        format!(
            "Read required picker product {}: {error}",
            product_path.display()
        )
    })?;
    Ok((bytes, product))
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
fn source_epoch_binding(value: &Value, plan: &BuildPlan) -> Result<()> {
    let epoch: PackageEpoch = serde_json::from_value(
        value
            .get("source_epoch")
            .ok_or("Missing source epoch")?
            .clone(),
    )?;
    if epoch != plan.source_epoch() || value["source_provenance"] != plan.source_provenance() {
        return Err(
            "Native record source epoch/provenance differs from the admitted source".into(),
        );
    }
    Ok(())
}

fn bundle_timestamps(verification: &Value, reference: &Value, plan: &BuildPlan) -> Result<()> {
    source_epoch_binding(verification, plan)?;
    let files: BTreeMap<String, (ByteIdentity, u32)> = serde_json::from_value(
        reference
            .get("files")
            .ok_or("App reference lacks its file inventory")?
            .clone(),
    )?;
    if files.is_empty() || files.len() > 4096 {
        return Err("Invalid app reference file inventory".into());
    }
    let mut tree = BTreeMap::from([(String::new(), "directory")]);
    let mut names = BTreeSet::new();
    for (name, (identity, mode)) in files {
        xtask::archive::validate_member_name(&name)?;
        if name.ends_with('/')
            || !matches!(mode, 0o644 | 0o755)
            || identity.byte_length > 256 * 1024 * 1024
            || identity.sha256.len() != 64
            || !identity
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !names.insert(name.to_ascii_lowercase())
            || tree.insert(name.clone(), "file").is_some()
        {
            return Err("Invalid app reference file identity, mode or path collision".into());
        }
        let mut path = name.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            if tree
                .insert(parent.to_owned(), "directory")
                .is_some_and(|kind| kind != "directory")
            {
                return Err("App reference file is also a directory".into());
            }
            path = parent;
        }
    }
    if tree.len() > 4096
        || tree
            .keys()
            .map(|name| name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>()
            .len()
            != tree.len()
    {
        return Err("App timestamp tree exceeds its bound or contains case aliases".into());
    }
    let times = verification
        .get("bundle_timestamps")
        .and_then(Value::as_object)
        .ok_or("App timestamp observation is absent")?;
    let keys: BTreeSet<_> = times.keys().map(String::as_str).collect();
    if keys != BTreeSet::from(["source_epoch", "zip_epoch_seconds", "precision", "entries"])
        || times["source_epoch"].as_u64() != Some(u64::from(plan.source_epoch().seconds()))
        || times["zip_epoch_seconds"].as_u64()
            != Some(u64::from(plan.source_epoch().floor_seconds()))
        || times["precision"] != "UTC DOS two-second floor"
    {
        return Err("App timestamp epoch/precision differs from the source profile".into());
    }
    let entries = times["entries"]
        .as_object()
        .ok_or("App timestamp entries are not an object")?;
    if entries.keys().collect::<BTreeSet<_>>() != tree.keys().collect() {
        return Err("App timestamp tree differs from the exact reference inventory".into());
    }
    for (name, kind) in tree {
        let row = entries[&name]
            .as_object()
            .ok_or("App timestamp entry is not an object")?;
        if row.keys().map(String::as_str).collect::<BTreeSet<_>>()
            != BTreeSet::from(["kind", "mtime_seconds", "mtime_nanoseconds"])
            || row["kind"] != kind
            || row["mtime_seconds"].as_u64() != Some(u64::from(plan.source_epoch().floor_seconds()))
            || row["mtime_nanoseconds"].as_u64() != Some(0)
        {
            return Err("App timestamp entry kind or UTC precision differs".into());
        }
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
fn check_operation_verification(
    value: &Value,
    fixture: &crate::app_reference::OperationFixtureEnvelope,
    reference: &crate::app_reference::OperationFixtureReference,
    plan: &BuildPlan,
    sdk: u32,
) -> Result<()> {
    let app = serde_json::to_value(&reference.app)?;
    source_epoch_binding(value, plan)?;
    if value["stage"] != "operation_fixture"
        || serde_json::from_value::<ByteIdentity>(value["snapshot"].clone())? != fixture.archive
    {
        return Err("Controlled companion verification stage/archive differs".into());
    }
    bundle_timestamps(value, &app, plan)?;
    let images = value["images"]["images"]
        .as_array()
        .ok_or("Controlled companion lacks native image observations")?;
    let mut paths = BTreeSet::new();
    for image in images {
        let path = image["path"]
            .as_str()
            .ok_or("Missing companion image path")?;
        let entitlements = match path {
            crate::app_reference::MAIN => &reference.gui.fixture_signature.entitlements,
            crate::app_reference::WORKER => &reference.worker.fixture_signature.entitlements,
            _ => return Err("Unexpected controlled companion native image".into()),
        };
        if !paths.insert(path)
            || image.get("entitlements") != Some(entitlements)
            || image["bytes"] != app["files"][path][0]
            || image["image"]["architecture"] != app["architecture"]
            || image["image"]["minimum"] != app["minimum"]
            || image["image"]["sdk"].as_u64() != Some(u64::from(sdk))
            || image["runtime"]["adhoc_signature_verified"] != true
        {
            return Err(
                "Controlled companion native image differs from its frozen reference".into(),
            );
        }
    }
    if paths != BTreeSet::from([crate::app_reference::MAIN, crate::app_reference::WORKER]) {
        return Err("Controlled companion native image set differs".into());
    }
    let runs = value["worker"]["runs"]
        .as_array()
        .ok_or("Controlled companion lacks extracted-worker observations")?;
    if runs.len() != 2 {
        return Err("Controlled companion extracted-worker case set differs".into());
    }
    for (run, expected) in runs.iter().zip(["created", "up_to_date"]) {
        let completion = &run["completion"];
        let report = &run["report"];
        let items = report["items"]
            .as_array()
            .ok_or("Controlled companion report lacks its one input result")?;
        let expected_counts = serde_json::json!({
            "created": u8::from(expected == "created"),
            "up_to_date": u8::from(expected == "up_to_date"),
            "planned": 0, "failed": 0, "not_processed": 0
        });
        // Each fixed smoke must retain one complete report1 envelope.
        if run
            .as_object()
            .map(|v| v.keys().map(String::as_str).collect::<BTreeSet<_>>())
            != Some(BTreeSet::from(["expected", "completion", "report"]))
            || report
                .as_object()
                .map(|v| v.keys().map(String::as_str).collect::<BTreeSet<_>>())
                != Some(BTreeSet::from([
                    "schema",
                    "schema_version",
                    "version",
                    "build",
                    "run_id",
                    "start",
                    "items",
                    "summary",
                    "protocol_failure",
                    "worker",
                    "terminal",
                ]))
            || report["schema"] != "eml-technical-report"
            || report["schema_version"].as_u64() != Some(1)
            || report["run_id"].as_u64().is_none_or(|id| id == 0)
            || report["run_id"] != completion["operation_id"]
            || items.len() != 1
            || report["start"]["total"].as_u64() != Some(1)
            || report["start"]["version"] != plan.version()
            || report["start"]["program"] != eml_core::product::CLI_NAME
            || report["start"]["protocol"].as_u64() != Some(1)
            || report["start"]["dry_run"] != false
            || report["start"]["collection_complete"] != true
            || report["summary"]["collection_complete"] != true
            || report["summary"]["counts"] != expected_counts
            || report["summary"]["interrupted"] != false
            || report["summary"].get("run_error") != Some(&Value::Null)
            || report["terminal"] != "completed"
            || report["worker"] != *completion
            || items[0]["index"].as_u64() != Some(0)
            || items[0].get("error") != Some(&Value::Null)
            || run["expected"] != expected
            || completion["settled"] != true
            || completion["reusable"] != true
            || completion["exit_code"].as_i64() != Some(0)
            || completion.get("failure") != Some(&Value::Null)
            || completion.get("signal") != Some(&Value::Null)
            || report["items"][0]["status"] != expected
            || report["summary"]["exit_code"].as_i64() != Some(0)
            || report.get("protocol_failure") != Some(&Value::Null)
            || report["version"] != plan.version()
            || report["build"] != eml_core::product::BUILD_NUMBER
        {
            return Err("Controlled companion extracted-worker observation did not qualify".into());
        }
    }
    Ok(())
}
fn check_desktop(
    document: &Value,
    plan: &BuildPlan,
    target: &crate::distribution::NativeTarget,
    workflow: &WorkflowIdentity,
    archive: &ByteIdentity,
    reference: &FrozenRecord,
    fixture_document: &Value,
) -> Result<()> {
    let source = plan.source_commit();
    let shipping_reference = reference.document()?;
    let app_root = shipping_reference["root"]
        .as_str()
        .ok_or("Shipping reference lacks its bundle root")?;
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
    if serde_json::from_value::<ByteIdentity>(document["reference"].clone())? != reference.identity
    {
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
    if interaction["operation_controls"].get("operation_fixture") != Some(fixture_document) {
        return Err("Controlled companion differs from the frozen producer metadata".into());
    }
    let fixture: crate::app_reference::OperationFixtureEnvelope =
        serde_json::from_value(fixture_document.clone())?;
    let fixture_reference = fixture.validate(archive, reference.contents.as_bytes())?;
    let fixture_app = serde_json::to_value(&fixture_reference.app)?;
    record_binding(&fixture_app, source, target.rust_target(), workflow)?;
    source_epoch_binding(&fixture_app, plan)?;
    if fixture_app["version"] != plan.version()
        || fixture_app["architecture"] != target.platform().architecture().as_str()
        || fixture_app["root"] != "Observed Operations.app"
    {
        return Err("Controlled companion source profile differs from the admitted product".into());
    }
    check_operation_verification(
        &interaction["operation_controls"]["verification"],
        &fixture,
        &fixture_reference,
        plan,
        crate::app_reference::packed_version(&fixture_reference.worker.toolchain.sdk_version)?,
    )?;
    if interaction["operation_controls"].get("observed_bundle") != Some(&fixture_app["files"]) {
        return Err(
            "Observed controlled bundle differs from the complete frozen fixture tree".into(),
        );
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
        crate::desktop_evidence::validate_case(
            value,
            *case,
            source,
            &fixture.archive,
            &fixture_reference.app.root,
        )?;
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
    source_epoch_binding(&producer, plan)?;
    host(&producer["host"], target, None)?;
    crate::publication_evidence::validate_summary(
        producer
            .get("release_tests")
            .and_then(|v| v.get("native_publication"))
            .ok_or("Missing actual native publication/cleanup evidence")?,
        source,
        u64::from(plan.source_epoch().seconds()),
        target.rust_target(),
        target.platform().family().as_str(),
        target.platform().architecture().as_str(),
        "release",
    )?;
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
            if target.platform().family() == OsFamily::Windows {
                crate::windows_sync_evidence::validate(
                    producer
                        .get("release_tests")
                        .and_then(|v| v.get("windows_existing_sync"))
                        .ok_or("Missing actual Windows existing-copy synchronization evidence")?,
                    source,
                    u64::from(plan.source_epoch().seconds()),
                    target.rust_target(),
                    "release",
                )?;
                crate::ntfs_query_evidence::validate_summary(
                    producer
                        .get("release_tests")
                        .and_then(|v| v.get("ntfs_query"))
                        .ok_or("Missing actual NTFS locality/query lifetime evidence")?,
                    target.platform().architecture().as_str(),
                )?;
                let expected_query_cases: Vec<_> = crate::ntfs_query_evidence::REQUIRED_TESTS
                    .iter()
                    .map(|name| serde_json::json!({"name":name,"discovered":1,"executed":1}))
                    .collect();
                if producer["release_tests"].get("ntfs_query_test_cases")
                    != Some(&serde_json::json!(expected_query_cases))
                {
                    return Err("NTFS query fixtures lack exact actual discovery/execution".into());
                }
            } else if [
                "windows_existing_sync",
                "ntfs_query",
                "ntfs_query_test_cases",
            ]
            .iter()
            .any(|key| {
                producer
                    .get("release_tests")
                    .and_then(|v| v.get(key))
                    .is_some_and(|v| !v.is_null())
            }) {
                return Err(
                    "Windows synchronization evidence cannot qualify another native platform"
                        .into(),
                );
            }
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
            if producer.get("operation_fixture_failure") != Some(&Value::Null) {
                return Err(
                    "App producer retains a failed or missing controlled-companion result".into(),
                );
            }
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
            source_epoch_binding(&reference_document, plan)?;
            bundle_timestamps(&producer["verification"], &reference_document, plan)?;
            if reference_document["schema"] != 2
                || reference_document["version"] != plan.version()
                || reference_document["root"] != artifact.root()
                || reference_document["architecture"] != target.platform().architecture().as_str()
            {
                return Err("App reference schema/version differs".into());
            }
            let fixture_document = producer
                .get("operation_fixture")
                .ok_or("App producer lacks its frozen controlled companion")?;
            let fixture: crate::app_reference::OperationFixtureEnvelope =
                serde_json::from_value(fixture_document.clone())?;
            let fixture_reference =
                fixture.validate(&product.archive, reference.contents.as_bytes())?;
            let toolchain = &fixture_reference.worker.toolchain;
            let rust_pin: toml::Value = toml::from_str(include_str!("../../rust-toolchain.toml"))?;
            let release = rust_pin["toolchain"]["channel"]
                .as_str()
                .ok_or("Missing pinned Rust compiler release")?;
            if toolchain
                .rust_identity
                .lines()
                .filter(|line| *line == format!("release: {release}"))
                .count()
                != 1
                || toolchain
                    .rust_identity
                    .lines()
                    .filter(|line| *line == format!("host: {}", target.rust_target()))
                    .count()
                    != 1
                || toolchain.minimum
                    != crate::app_reference::packed_version(plan.floors().macos_minimum())?
            {
                return Err("Controlled companion worker toolchain differs from the admitted producer policy".into());
            }
            let shipping_images = producer["verification"]["images"]["images"]
                .as_array()
                .ok_or("Shipping producer lacks its native image SDK observation")?;
            let shipping_gui: Vec<_> = shipping_images
                .iter()
                .filter(|image| image["path"] == crate::app_reference::MAIN)
                .collect();
            if shipping_gui.len() != 1
                || shipping_gui[0]["bytes"]
                    != reference_document["files"][crate::app_reference::MAIN][0]
            {
                return Err(
                    "Controlled companion SDK differs from the frozen shipping GUI image".into(),
                );
            }
            let sdk: u32 = shipping_gui[0]["image"]["sdk"]
                .as_u64()
                .ok_or("Shipping GUI lacks its observed SDK")?
                .try_into()?;
            toolchain.validate_observed(
                producer["host"]["compiler"]
                    .as_str()
                    .ok_or("Producer compiler observation missing")?,
                &format!(
                    "Xcode {}\nBuild version {}\n",
                    plan.xcode().version(),
                    plan.xcode().build()
                ),
                sdk,
            )?;
            check_operation_verification(
                &producer["operation_fixture_verification"],
                &fixture,
                &fixture_reference,
                plan,
                sdk,
            )?;
            let desktop = product
                .desktop
                .as_ref()
                .ok_or("Complete desktop consumer is absent")?
                .document()?;
            source_epoch_binding(&desktop, plan)?;
            bundle_timestamps(&desktop["verification"], &reference_document, plan)?;
            check_desktop(
                &desktop,
                plan,
                target,
                workflow,
                &product.archive,
                reference,
                fixture_document,
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
                source_epoch_binding(&raw_document, plan)?;
                bundle_timestamps(&raw_document["verification"], &reference_document, plan)?;
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
    fn validate(&self, plan: &BuildPlan, workflow: &WorkflowIdentity) -> Result<()> {
        self.workflow.validate()?;
        if self.schema != "eml-verified-release-inputs"
            || self.schema_version != 2
            || self.source != plan.source_commit()
            || self.source_epoch != plan.source_epoch()
            || self.source_provenance != plan.source_provenance()
            || self.source_provenance != "git_commit"
            || self.version != plan.version()
            || &self.workflow != workflow
            || self.products.keys().cloned().collect::<BTreeSet<_>>() != plan.archive_names()
        {
            return Err("Verified input source/version/workflow/product set differs".into());
        }
        for (name, product) in self.products.iter() {
            check_product(product, name, plan, plan.source_commit(), workflow)?;
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
    plan.require_qualified_source(root)?;
    manifest.validate(plan, &workflow)?;
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
    plan.require_qualified_source(root)?;
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
                let (bytes, picker_product) = read_picker_records(&receipt_path)?;
                product.picker_product = Some(picker_product);
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
        schema_version: 2,
        source: plan.source_commit().into(),
        source_epoch: plan.source_epoch(),
        source_provenance: plan.source_provenance().into(),
        version: plan.version().into(),
        workflow,
        products: admitted.into(),
    };
    manifest.validate(plan, &manifest.workflow)?;
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
            &serde_json::json!({"manifest":path,"source":plan.source_commit(),"source_epoch":plan.source_epoch(),"source_provenance":plan.source_provenance(),"version":plan.version(),"archives_directory":frozen.path(),"picker_receipts":manifest.products.values().filter_map(|product|product.picker.as_ref().map(|receipt|serde_json::json!({"platform":product.platform,"identity":receipt.identity(),"mode":receipt.mode()}))).collect::<Vec<_>>(),"scope":"admitted exact current-run product and native consumer identities; not permission to publish"})
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests;
