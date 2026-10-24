//! Frozen shipping and nonshipping operation-fixture app references; no native authority.
use crate::{
    Result,
    picker_evidence::{ProcessObservation, observation_document, record},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use xtask::archive::{ByteIdentity, PackageEpoch};

pub(crate) const MAIN: &str = "Contents/MacOS/EMLAttachmentRemover";
pub(crate) const WORKER: &str = "Contents/Helpers/eml-worker";
pub(crate) const FEATURES: &str =
    "Contents/Resources/licenses/worker/licenses/activated-features.json";
pub(crate) const MATERIALS: &str = "Contents/Resources/licenses/worker/licenses/materials.json";
pub(crate) const ROOT: &str = "Observed Operations.app";
#[cfg(target_os = "macos")]
pub(crate) const ARCHIVE: &str = "operation-fixture.zip";
#[cfg(target_os = "macos")]
pub(crate) const REFERENCE: &str = "operation-fixture.reference.json";
pub(crate) type Files = BTreeMap<String, (ByteIdentity, u32)>;
record!(IdentityObject {
    byte_length: usize,
    sha256: String
});
fn identity<'de, A: serde::de::MapAccess<'de>>(
    map: &mut A,
) -> std::result::Result<ByteIdentity, A::Error> {
    let v: IdentityObject = map.next_value()?;
    Ok(ByteIdentity {
        byte_length: v.byte_length,
        sha256: v.sha256,
    })
}
record!(AppReference {schema:u8,workflow:Option<crate::release_inputs::WorkflowIdentity>,source:String,source_provenance:String,source_epoch:PackageEpoch,version:String,target:String,architecture:String,minimum:u32,root:String,files:Files});
record!(ReferenceDocument {identity:ByteIdentity=>self::identity,contents:String});
record!(OperationFixtureEnvelope {archive:ByteIdentity=>self::identity,reference:ReferenceDocument});
record!(SignatureObservation {
    identifier: String,
    flags: u32,
    entitlements: Value,
    details_process: ProcessObservation,
    entitlements_process: ProcessObservation
});
record!(GuiBinding {shipping_signed:ByteIdentity=>self::identity,fixture_signed:ByteIdentity=>self::identity,shipping_stripped:ByteIdentity=>self::identity,fixture_stripped:ByteIdentity=>self::identity,shipping_signature:SignatureObservation,fixture_signature:SignatureObservation,shipping_strip_process:ProcessObservation,fixture_strip_process:ProcessObservation});
record!(WorkerProfile {
    opt_level: String,
    debuginfo: Value,
    debug_assertions: bool,
    overflow_checks: bool,
    test: bool
});
record!(WorkerToolchain {
    rust_identity: String,
    xcode_identity: String,
    sdk_version: String,
    target: String,
    architecture: String,
    minimum: u32
});
record!(WorkerBuild {compiled_image:ByteIdentity=>self::identity,features:Vec<String>,profile:WorkerProfile,toolchain:WorkerToolchain,process:ProcessObservation,shipping_signature:SignatureObservation,fixture_signature:SignatureObservation});
record!(LegalBinding {
    shipping_features: ReferenceDocument,
    fixture_features: ReferenceDocument,
    shipping_materials: ReferenceDocument,
    fixture_materials: ReferenceDocument
});
record!(OperationFixtureReference {schema:u8,shipping_archive:ByteIdentity=>self::identity,shipping_reference:ByteIdentity=>self::identity,app:AppReference,worker:WorkerBuild,gui:GuiBinding,legal:LegalBinding,changed_paths:Vec<String>});
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
fn valid_identity(v: &ByteIdentity, maximum: u64) -> bool {
    v.byte_length > 0
        && u64::try_from(v.byte_length).is_ok_and(|length| length <= maximum)
        && v.sha256.len() == 64
        && v.sha256
            .bytes()
            .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x))
}
fn success(p: &ProcessObservation) -> Result<()> {
    require(p.clean()? == 0, "Operation-fixture native tool failed")
}
fn inventory_shape(value: &Value) -> Result<()> {
    require(
        value.is_object() && (value["workflow"].is_null() || value["workflow"].is_object()),
        "App reference/context is not an object",
    )?;
    let files = value["files"]
        .as_object()
        .ok_or("App file inventory is not an object")?;
    for entry in files.values() {
        let pair = entry
            .as_array()
            .ok_or("App inventory entry is not a pair")?;
        require(
            pair.len() == 2 && pair[0].is_object(),
            "App byte identity is not an object",
        )?;
    }
    Ok(())
}
pub(crate) fn parse_reference(bytes: &[u8]) -> Result<AppReference> {
    let value = observation_document(bytes)?;
    inventory_shape(&value)?;
    Ok(serde_json::from_value(value)?)
}
impl ReferenceDocument {
    #[cfg(target_os = "macos")]
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self> {
        Ok(Self {
            identity: ByteIdentity::of(&bytes),
            contents: String::from_utf8(bytes)?,
        })
    }
    pub(crate) fn document(&self) -> Result<Value> {
        require(
            self.contents.len() <= 4 * 1024 * 1024
                && ByteIdentity::of(self.contents.as_bytes()) == self.identity,
            "Frozen operation-fixture document identity differs",
        )?;
        observation_document(self.contents.as_bytes())
    }
}
fn signature_equal(a: &SignatureObservation, b: &SignatureObservation) -> Result<()> {
    for s in [a, b] {
        success(&s.details_process)?;
        success(&s.entitlements_process)?;
        require(
            !s.identifier.is_empty() && s.identifier.len() <= 512 && s.flags & 2 != 0,
            "Operation-fixture signature lacks bounded ad-hoc identity",
        )?;
        require(
            s.entitlements.is_null() || s.entitlements.is_object(),
            "Invalid operation-fixture entitlements",
        )?;
    }
    require(
        a.identifier == b.identifier && a.flags == b.flags && a.entitlements == b.entitlements,
        "Operation-fixture signing security metadata differs",
    )
}

impl WorkerToolchain {
    /// Match retained build observations to the caller's held producer/build context.
    /// SDK headers encode a packed version, while the original observed text is retained.
    pub(crate) fn validate_observed(
        &self,
        rust_identity: &str,
        xcode_identity: &str,
        sdk: u32,
    ) -> Result<()> {
        require(
            !rust_identity.is_empty() && self.rust_identity == rust_identity,
            "Operation-fixture Rust identity differs from its producer observation",
        )?;
        require(
            !xcode_identity.is_empty() && self.xcode_identity == xcode_identity,
            "Operation-fixture Xcode identity differs from its selected build context",
        )?;
        require(
            sdk != 0 && packed_version(&self.sdk_version)? == sdk,
            "Operation-fixture SDK differs from its native image/build observation",
        )
    }
}

pub(crate) fn validate_legal(legal: &LegalBinding) -> Result<()> {
    let before = legal.shipping_features.document()?;
    let after = legal.fixture_features.document()?;
    let mut m = legal.shipping_materials.document()?;
    let mut n = legal.fixture_materials.document()?;
    let packages = m["packages"]
        .as_array()
        .ok_or("Missing reviewed legal packages")?;
    let mut reviewed_packages = BTreeSet::new();
    for package in packages {
        reviewed_packages.insert((
            package["name"]
                .as_str()
                .ok_or("Missing reviewed package name")?,
            package["version"]
                .as_str()
                .ok_or("Missing reviewed package version")?,
        ));
    }
    let mut observed_packages = BTreeSet::new();
    let a = before
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("Missing shipping feature entries")?;
    let b = after
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("Missing fixture feature entries")?;
    require(
        before["scope"] == after["scope"] && a.len() == b.len() && !a.is_empty(),
        "Worker feature graph shape differs",
    )?;
    let expected = BTreeSet::from([
        "eml-worker",
        "eml-app",
        "eml-core",
        "eml-fs",
        "eml-macos",
        "eml-protocol",
    ]);
    let mut seen = BTreeSet::new();
    for (left, right) in a.iter().zip(b) {
        let mut l = left
            .as_object()
            .ok_or("Feature entry is not an object")?
            .clone();
        let mut r = right
            .as_object()
            .ok_or("Feature entry is not an object")?
            .clone();
        let lf = l.remove("features").ok_or("Missing feature set")?;
        let rf = r.remove("features").ok_or("Missing fixture feature set")?;
        require(l == r, "Worker external package identity/context differs")?;
        let name = left["name"]
            .as_str()
            .ok_or("Missing feature package name")?;
        let version = left["version"]
            .as_str()
            .ok_or("Missing feature package version")?;
        require(
            reviewed_packages.contains(&(name, version)),
            "Feature graph contains a package outside reviewed legal materials",
        )?;
        observed_packages.insert((name, version));
        if expected.contains(name) {
            let entries: Vec<_> = packages
                .iter()
                .filter(|p| p["name"] == name && p["version"] == version)
                .collect();
            require(
                !entries.is_empty()
                    && version == m["version"].as_str().ok_or("Missing legal version")?
                    && entries.iter().all(|p| {
                        p.get("source") == Some(&Value::Null)
                            && p.get("checksum") == Some(&Value::Null)
                    }),
                "Fixture feature delta is not a reviewed workspace package",
            )?;
            let old: Vec<String> = serde_json::from_value(lf)?;
            let new: Vec<String> = serde_json::from_value(rf)?;
            let mut wanted = old.clone();
            require(
                !wanted.iter().any(|x| x == "native-fixtures"),
                "Shipping worker contains fixture feature",
            )?;
            wanted.push("native-fixtures".into());
            wanted.sort();
            require(
                new == wanted,
                "Workspace feature delta is not the declared fixture feature",
            )?;
            seen.insert(name);
        } else {
            require(lf == rf, "External worker activated features differ")?;
        }
    }
    require(
        seen == expected,
        "Fixture graph omits a declared workspace feature",
    )?;
    require(
        observed_packages == reviewed_packages,
        "Feature graph omits a reviewed legal package",
    )?;
    let args = m["generation"]["feature_arguments"]
        .as_array()
        .ok_or("Missing original feature command")?
        .clone();
    let mut wanted = args.clone();
    wanted.extend([
        Value::String("--features".into()),
        Value::String("eml-worker/native-fixtures".into()),
    ]);
    require(
        n["generation"]["feature_arguments"] == Value::Array(wanted),
        "Fixture material generation command differs",
    )?;
    m["generation"]["feature_arguments"] = Value::Null;
    n["generation"]["feature_arguments"] = Value::Null;
    require(
        m == n,
        "Fixture legal materials changed beyond actual feature command",
    )
}
impl OperationFixtureEnvelope {
    pub(crate) fn validate(
        &self,
        shipping_archive: &ByteIdentity,
        shipping_reference: &[u8],
    ) -> Result<OperationFixtureReference> {
        require(
            valid_identity(&self.archive, xtask::archive::MAX_ARCHIVE_BYTES),
            "Invalid operation-fixture archive identity",
        )?;
        let normal = parse_reference(shipping_reference)?;
        let document = self.reference.document()?;
        inventory_shape(&document["app"])?;
        let v: OperationFixtureReference = serde_json::from_value(document)?;
        require(
            v.schema == 1
                && v.shipping_archive == *shipping_archive
                && v.shipping_reference == ByteIdentity::of(shipping_reference),
            "Operation-fixture shipping binding differs",
        )?;
        let app = &v.app;
        for reference in [&normal, app] {
            require(
                [
                    MAIN,
                    WORKER,
                    "Contents/_CodeSignature/CodeResources",
                    FEATURES,
                    MATERIALS,
                ]
                .iter()
                .all(|path| reference.files.contains_key(*path)),
                "App reference omits a required operation-fixture path",
            )?;
        }
        require(
            app.schema == 2
                && normal.schema == 2
                && app.root == ROOT
                && app.source == normal.source
                && app.source_provenance == normal.source_provenance
                && app.source_epoch == normal.source_epoch
                && app.version == normal.version
                && app.target == normal.target
                && app.architecture == normal.architecture
                && app.minimum == normal.minimum
                && app.workflow == normal.workflow,
            "Operation-fixture source/native context differs",
        )?;
        require(
            app.files.keys().eq(normal.files.keys()),
            "Operation-fixture bundle paths differ",
        )?;
        let allowed = BTreeSet::from([
            MAIN,
            WORKER,
            "Contents/_CodeSignature/CodeResources",
            FEATURES,
            MATERIALS,
        ]);
        let mut changed = Vec::new();
        for (name, (bytes, mode)) in &app.files {
            let (original, original_mode) = &normal.files[name];
            require(
                valid_identity(bytes, MAX_FILE_BYTES) && mode == original_mode,
                "Operation-fixture image/mode differs",
            )?;
            if bytes != original {
                require(
                    allowed.contains(name.as_str()),
                    "Unexpected operation-fixture resource change",
                )?;
                changed.push(name.clone());
            }
        }
        require(
            changed == v.changed_paths
                && [WORKER, FEATURES, MATERIALS]
                    .iter()
                    .all(|p| changed.iter().any(|x| x == p)),
            "Operation-fixture changed-path proof differs",
        )?;
        let gui = &v.gui;
        require(
            gui.shipping_signed == normal.files[MAIN].0
                && gui.fixture_signed == app.files[MAIN].0
                && valid_identity(&gui.shipping_stripped, MAX_FILE_BYTES)
                && gui.shipping_stripped == gui.fixture_stripped,
            "Operation-fixture GUI whole-byte identity differs",
        )?;
        success(&gui.shipping_strip_process)?;
        success(&gui.fixture_strip_process)?;
        signature_equal(&gui.shipping_signature, &gui.fixture_signature)?;
        signature_equal(&v.worker.shipping_signature, &v.worker.fixture_signature)?;
        let w = &v.worker;
        success(&w.process)?;
        require(
            valid_identity(&w.compiled_image, MAX_FILE_BYTES)
                && w.features == ["native-fixtures"]
                && w.profile.opt_level == "3"
                && w.profile.debuginfo == 0
                && !w.profile.debug_assertions
                && w.profile.overflow_checks
                && !w.profile.test,
            "Operation-fixture worker build selection differs",
        )?;
        require(
            w.toolchain.target == app.target
                && w.toolchain.architecture == app.architecture
                && w.toolchain.minimum == app.minimum
                && [
                    &w.toolchain.rust_identity,
                    &w.toolchain.xcode_identity,
                    &w.toolchain.sdk_version,
                ]
                .iter()
                .all(|s| !s.is_empty() && s.len() <= 4096),
            "Operation-fixture worker toolchain differs",
        )?;
        validate_legal(&v.legal)?;
        require(
            v.legal.shipping_features.identity == normal.files[FEATURES].0
                && v.legal.fixture_features.identity == app.files[FEATURES].0
                && v.legal.shipping_materials.identity == normal.files[MATERIALS].0
                && v.legal.fixture_materials.identity == app.files[MATERIALS].0,
            "Operation-fixture embedded legal identity differs",
        )?;
        Ok(v)
    }
}

pub(crate) fn packed_version(value: &str) -> Result<u32> {
    let parts: Vec<u32> = value
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    if !(2..=3).contains(&parts.len())
        || parts[0] > 65_535
        || parts[1] > 255
        || parts.get(2).is_some_and(|value| *value > 255)
    {
        return Err("Invalid macOS version".into());
    }
    Ok((parts[0] << 16) | (parts[1] << 8) | parts.get(2).copied().unwrap_or(0))
}

#[cfg(test)]
pub(crate) use tests::test_fixture;
#[cfg(test)]
mod tests;
