//! Producer-frozen nonshipping operation fixture; floor consumers never construct a SUT.
use crate::app_reference::{
    self, AppReference, GuiBinding, OperationFixtureEnvelope, OperationFixtureReference,
    ReferenceDocument, SignatureObservation, WorkerBuild, WorkerProfile, WorkerToolchain,
};
use crate::{
    Result, controller,
    distribution::BuildPlan,
    macos::{self, MacBuild},
    macos_archive,
    picker_evidence::ProcessObservation,
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration};
use xtask::archive::{ArchiveSnapshot, ByteIdentity};

pub(crate) struct FrozenOperationFixture {
    pub(crate) snapshot: ArchiveSnapshot,
    pub(crate) envelope: OperationFixtureEnvelope,
    pub(crate) reference: OperationFixtureReference,
}
pub(crate) struct VerifiedOperationFixture<'a> {
    pub(crate) app: &'a Path,
    pub(crate) envelope: &'a OperationFixtureEnvelope,
    pub(crate) verification: &'a Value,
    pub(crate) reference: &'a OperationFixtureReference,
}
pub(crate) fn read(
    archive: &Path,
    reference: &Path,
    shipping_archive: &ByteIdentity,
    shipping_reference: &[u8],
) -> Result<FrozenOperationFixture> {
    if archive.file_name() != Some(std::ffi::OsStr::new(app_reference::ARCHIVE))
        || reference.file_name() != Some(std::ffi::OsStr::new(app_reference::REFERENCE))
    {
        return Err("Operation-fixture input filenames differ".into());
    }
    let snapshot = ArchiveSnapshot::read(archive)?;
    let envelope = OperationFixtureEnvelope {
        archive: snapshot.identity(),
        reference: ReferenceDocument::new(macos_archive::read(reference, 4 * 1024 * 1024)?)?,
    };
    let reference = envelope.validate(shipping_archive, shipping_reference)?;
    Ok(FrozenOperationFixture {
        snapshot,
        envelope,
        reference,
    })
}
fn checked(command: Command, log: &Path, seconds: u64) -> Result<ProcessObservation> {
    let process = controller()?.run_separate_output(command, log, Duration::from_secs(seconds))?;
    process.require_success(log)?;
    ProcessObservation::from_process(&process)
}
fn signature(
    root: &Path,
    build: &MacBuild,
    image: &Path,
    out: &Path,
) -> Result<SignatureObservation> {
    fs::create_dir(out)?;
    let mut details = build.command(root, "/usr/bin/codesign");
    details.args(["--display", "--verbose=4"]).arg(image);
    let details_process = checked(details, &out.join("details.log"), 30)?;
    let text = String::from_utf8(macos_archive::read(
        &out.join("details.stderr"),
        1024 * 1024,
    )?)?;
    let ids: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix("Identifier="))
        .collect();
    let flags: Vec<_> = text
        .lines()
        .filter(|line| line.starts_with("CodeDirectory "))
        .filter_map(|line| {
            line.split_once("flags=0x")
                .map(|(_, value)| value.split('(').next().unwrap_or(value))
        })
        .collect();
    if ids.len() != 1 || flags.len() != 1 || !text.lines().any(|line| line == "Signature=adhoc") {
        return Err("Operation-fixture code signature metadata is ambiguous or not ad hoc".into());
    }
    let flags = u32::from_str_radix(flags[0], 16)?;
    let mut command = build.command(root, "/usr/bin/codesign");
    command
        .args(["--display", "--entitlements", "-", "--xml"])
        .arg(image);
    let entitlement_path = out.join("entitlements.plist");
    let entitlements_process = checked(command, &entitlement_path, 30)?;
    let bytes = macos_archive::read(&entitlement_path, 1024 * 1024)?;
    let entitlements = if bytes.is_empty() {
        Value::Null
    } else {
        let mut command = build.command(root, "/usr/bin/plutil");
        command
            .args(["-convert", "json", "-o", "-"])
            .arg(&entitlement_path);
        let path = out.join("entitlements-json.log");
        checked(command, &path, 30)?;
        crate::picker_evidence::observation_document(&macos_archive::read(&path, 1024 * 1024)?)?
    };
    Ok(SignatureObservation {
        identifier: ids[0].to_owned(),
        flags,
        entitlements,
        details_process,
        entitlements_process,
    })
}
fn sign_copied_app(root: &Path, build: &MacBuild, path: &Path, out: &Path) -> Result<()> {
    let mut sign = build.command(root, "/usr/bin/codesign");
    sign.args([
        "--force",
        "--sign",
        "-",
        "--timestamp=none",
        "--preserve-metadata=identifier,entitlements,flags",
    ])
    .arg(path);
    checked(sign, &out.join("app-sign.log"), 30)?;
    let mut verify = build.command(root, "/usr/bin/codesign");
    verify
        .args(["--verify", "--strict", "--verbose=2"])
        .arg(path);
    checked(verify, &out.join("app-verify.log"), 30)?;
    Ok(())
}
fn sign_compiled_worker(
    root: &Path,
    build: &MacBuild,
    path: &Path,
    shipping_identifier: &str,
    out: &Path,
) -> Result<()> {
    let mut sign = build.command(root, "/usr/bin/codesign");
    sign.args([
        "--force",
        "--sign",
        "-",
        "--timestamp=none",
        "--identifier",
        shipping_identifier,
    ])
    .arg(path);
    checked(sign, &out.join("worker-sign.log"), 30)?;
    let mut verify = build.command(root, "/usr/bin/codesign");
    verify
        .args(["--verify", "--strict", "--verbose=2"])
        .arg(path);
    checked(verify, &out.join("worker-verify.log"), 30)?;
    Ok(())
}
struct CompiledWorker {
    bytes: Vec<u8>,
    profile: WorkerProfile,
    features: Vec<String>,
    process: ProcessObservation,
    source: xtask::archive::ArtifactDirectory,
    held: eml_fs::BoundDirectory,
}
fn compile_worker(
    root: &Path,
    plan: &BuildPlan,
    build: &MacBuild,
    out: &Path,
) -> Result<CompiledWorker> {
    plan.require_qualified_source(root)?;
    let target = build.cargo_directory(root);
    let mut command = build.command(root, "cargo");
    command
        .args([
            "build",
            "--locked",
            "--release",
            "--package",
            "eml-worker",
            "--bin",
            "eml-worker",
            "--features",
            "eml-worker/native-fixtures",
            "--target",
            &build.host,
            "--message-format=json",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("CARGO_INCREMENTAL", "0")
        .env("MACOSX_DEPLOYMENT_TARGET", &build.minimum_text);
    let log = out.join("worker-build.jsonl");
    let process = checked(command, &log, 1200)?;
    let bytes = macos_archive::read(&log, 16 * 1024 * 1024)?;
    let executable = crate::cli_build::named_artifact(
        root,
        &bytes,
        "crates/eml-worker/Cargo.toml",
        "crates/eml-worker/src/main.rs",
        "eml-worker",
    )?
    .canonicalize()?;
    if !fs::symlink_metadata(&executable)?.is_file()
        || !executable.starts_with(target.canonicalize()?)
    {
        return Err("Operation-fixture worker escaped the existing native build directory".into());
    }
    let image = macos_archive::read(&executable, 256 * 1024 * 1024)?;
    let source = xtask::archive::create_artifact_directory(out)?;
    let held = eml_fs::BoundDirectory::open(source.path())?;
    macos_archive::write(&source.path().join("compiled-worker"), &image, 0o755)?;
    fs::write(
        out.join("compiled-worker.json"),
        serde_json::to_vec_pretty(
            &json!({"image":ByteIdentity::of(&image),"source_artifact":executable,"retained_copy":source.path().join("compiled-worker"),"scope":"actual admitted compiler image before profile/SDK/signature checks; owned copy removed only after successful producer verification"}),
        )?,
    )?;
    let mut selected = Vec::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: Value = serde_json::from_slice(line)?;
        if value["reason"] == "compiler-artifact"
            && value["executable"]
                .as_str()
                .is_some_and(|p| Path::new(p).canonicalize().is_ok_and(|p| p == executable))
        {
            selected.push(value);
        }
    }
    if selected.len() != 1 {
        return Err("Missing unique operation-fixture worker artifact".into());
    }
    let artifact = selected.remove(0);
    let profile: WorkerProfile = serde_json::from_value(artifact["profile"].clone())?;
    let features: Vec<String> = serde_json::from_value(artifact["features"].clone())?;
    let actual = macos::fixture_identity(&image, &build.architecture, build.minimum)?;
    if actual.sdk != macos::packed_version(&build.sdk_version)? {
        return Err("Operation-fixture worker SDK differs from selected native SDK".into());
    }
    plan.require_qualified_source(root)?;
    Ok(CompiledWorker {
        bytes: image,
        profile,
        features,
        process,
        source,
        held,
    })
}
fn gui_binding(
    root: &Path,
    build: &MacBuild,
    shipping: &Path,
    fixture: &Path,
    out: &Path,
) -> Result<GuiBinding> {
    fs::create_dir(out)?;
    let enclosure = xtask::archive::create_artifact_directory(out)?;
    let held = eml_fs::BoundDirectory::open(enclosure.path())?;
    let original = macos_archive::read(&shipping.join(app_reference::MAIN), 256 * 1024 * 1024)?;
    let controlled = macos_archive::read(&fixture.join(app_reference::MAIN), 256 * 1024 * 1024)?;
    let a = enclosure.path().join("shipping-gui");
    let b = enclosure.path().join("fixture-gui");
    macos_archive::write(&a, &original, 0o755)?;
    macos_archive::write(&b, &controlled, 0o755)?;
    let shipping_signature = signature(
        root,
        build,
        &shipping.join(app_reference::MAIN),
        &out.join("shipping-signature"),
    )?;
    let fixture_signature = signature(
        root,
        build,
        &fixture.join(app_reference::MAIN),
        &out.join("fixture-signature"),
    )?;
    let strip = |label: &str, path: &Path| -> Result<ProcessObservation> {
        let mut command = build.command(root, "/usr/bin/codesign");
        command.args(["--remove-signature"]).arg(path);
        checked(command, &out.join(format!("{label}-strip.log")), 30)
    };
    let shipping_strip_process = strip("shipping", &a)?;
    let fixture_strip_process = strip("fixture", &b)?;
    let clear_a = macos_archive::read(&a, 256 * 1024 * 1024)?;
    let clear_b = macos_archive::read(&b, 256 * 1024 * 1024)?;
    if clear_a != clear_b {
        return Err("Operation-fixture GUI differs beyond its embedded signature".into());
    }
    if macos_archive::read(&shipping.join(app_reference::MAIN), 256 * 1024 * 1024)? != original
        || macos_archive::read(&fixture.join(app_reference::MAIN), 256 * 1024 * 1024)? != controlled
    {
        return Err("Analytical comparison changed an input GUI image".into());
    }
    let result = GuiBinding {
        shipping_signed: ByteIdentity::of(&original),
        fixture_signed: ByteIdentity::of(&controlled),
        shipping_stripped: ByteIdentity::of(&clear_a),
        fixture_stripped: ByteIdentity::of(&clear_b),
        shipping_signature,
        fixture_signature,
        shipping_strip_process,
        fixture_strip_process,
    };
    remove_newly_owned(enclosure.path(), &held, "analytical", out)?;
    Ok(result)
}

fn remove_newly_owned(
    path: &Path,
    held: &eml_fs::BoundDirectory,
    label: &str,
    out: &Path,
) -> Result<()> {
    let matches = || -> Result<bool> {
        let metadata = fs::symlink_metadata(path)?;
        Ok(metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && eml_fs::BoundDirectory::open(path)?.identity() == held.identity())
    };
    if !matches()? {
        return Err("New operation-fixture scratch address was replaced".into());
    }
    let mut pending = vec![(path.to_owned(), 0_usize)];
    let mut files = 0_usize;
    let mut entries = 0_usize;
    let mut logical_bytes = 0_u64;
    while let Some((item, depth)) = pending.pop() {
        entries += 1;
        if depth > 16 || entries > 8192 {
            return Err("Operation-fixture scratch inventory exceeds its bound".into());
        }
        let metadata = fs::symlink_metadata(&item)?;
        if metadata.file_type().is_symlink() {
            return Err("Operation-fixture scratch contains a symlink".into());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&item)? {
                pending.push((entry?.path(), depth + 1));
            }
        } else if metadata.is_file() {
            files += 1;
            logical_bytes = logical_bytes
                .checked_add(metadata.len())
                .ok_or("Scratch usage overflow")?;
        } else {
            return Err("Operation-fixture scratch has an unexpected entry kind".into());
        }
    }
    if !matches()? {
        return Err("New operation-fixture scratch changed during inventory".into());
    }
    fs::remove_dir_all(path)?;
    fs::write(
        out.join(format!("{label}-cleanup.json")),
        serde_json::to_vec_pretty(
            &json!({"removed":true,"entries":entries,"files":files,"logical_bytes":logical_bytes,"scope":"new producer-only disposable enclosure; all launched native tools and worker operations settled, frozen archive/reference/image facts retained"}),
        )?,
    )?;
    Ok(())
}

pub(crate) fn produce(
    root: &Path,
    plan: &BuildPlan,
    shipping: &Path,
    shipping_archive: &ArchiveSnapshot,
    shipping_reference: &[u8],
    build: &MacBuild,
    parent: &Path,
) -> Result<(OperationFixtureEnvelope, Value)> {
    plan.require_qualified_source(root)?;
    let normal = app_reference::parse_reference(shipping_reference)?;
    if macos_archive::tree(shipping, false)? != normal.files {
        return Err(
            "Operation-fixture source bundle differs from frozen shipping reference".into(),
        );
    }
    let out = parent.join("operation-fixture");
    fs::create_dir(&out)?;
    let logs = parent.join("operation-fixture-build");
    fs::create_dir(&logs)?;
    let stage = xtask::archive::create_artifact_directory(&out)?;
    let stage_held = eml_fs::BoundDirectory::open(stage.path())?;
    let app = stage.path().join(app_reference::ROOT);
    let mut copy = build.command(root, "/usr/bin/ditto");
    copy.args(["--norsrc", "--noextattr", "--noqtn", "--noacl"])
        .arg(shipping)
        .arg(&app);
    checked(copy, &logs.join("bundle-copy.log"), 120)?;
    if macos_archive::tree(&app, false)? != normal.files {
        return Err("Operation-fixture bundle copy differs from shipping".into());
    }
    let legal = crate::notices::operation_fixture_legal(
        root,
        &build.host,
        macos_archive::read(&app.join(app_reference::FEATURES), 4 * 1024 * 1024)?,
        macos_archive::read(&app.join(app_reference::MATERIALS), 4 * 1024 * 1024)?,
        &logs,
    )?;
    fs::write(
        app.join(app_reference::FEATURES),
        legal.fixture_features.contents.as_bytes(),
    )?;
    fs::write(
        app.join(app_reference::MATERIALS),
        legal.fixture_materials.contents.as_bytes(),
    )?;
    let compiled = compile_worker(root, plan, build, &logs)?;
    let shipping_signature = signature(
        root,
        build,
        &shipping.join(app_reference::WORKER),
        &logs.join("shipping-worker-signature"),
    )?;
    fs::write(app.join(app_reference::WORKER), &compiled.bytes)?;
    fs::set_permissions(
        app.join(app_reference::WORKER),
        fs::Permissions::from_mode(0o755),
    )?;
    sign_compiled_worker(
        root,
        build,
        &app.join(app_reference::WORKER),
        &shipping_signature.identifier,
        &logs,
    )?;
    sign_copied_app(root, build, &app, &logs)?;
    let worker = WorkerBuild {
        compiled_image: ByteIdentity::of(&compiled.bytes),
        features: compiled.features,
        profile: compiled.profile,
        toolchain: WorkerToolchain {
            rust_identity: build.rust_identity.clone(),
            xcode_identity: build.xcode_identity.clone(),
            sdk_version: build.sdk_version.clone(),
            target: build.host.clone(),
            architecture: build.architecture.clone(),
            minimum: build.minimum,
        },
        process: compiled.process,
        shipping_signature,
        fixture_signature: signature(
            root,
            build,
            &app.join(app_reference::WORKER),
            &logs.join("fixture-worker-signature"),
        )?,
    };
    worker.toolchain.validate_observed(
        &build.rust_identity,
        &build.xcode_identity,
        macos::packed_version(&build.sdk_version)?,
    )?;
    let gui = gui_binding(root, build, shipping, &app, &logs.join("gui-binding"))?;
    let files = macos_archive::tree(&app, false)?;
    let changed_paths = files
        .iter()
        .filter(|(name, value)| normal.files.get(*name) != Some(*value))
        .map(|(name, _)| name.clone())
        .collect();
    let reference = OperationFixtureReference {
        schema: 1,
        shipping_archive: shipping_archive.identity(),
        shipping_reference: ByteIdentity::of(shipping_reference),
        app: AppReference {
            root: app_reference::ROOT.to_owned(),
            files,
            ..normal.clone()
        },
        worker,
        gui,
        legal,
        changed_paths,
    };
    let reference_bytes = serde_json::to_vec_pretty(&reference)?;
    macos_archive::bundle_timestamps(&app, build.source_epoch, true)?;
    let staged = stage.path().join(app_reference::ARCHIVE);
    let mut pack = build.command(root, "/usr/bin/ditto");
    pack.args([
        "-c",
        "-k",
        "--keepParent",
        "--norsrc",
        "--noextattr",
        "--noqtn",
        "--noacl",
    ])
    .env("TZ", "UTC")
    .arg(&app)
    .arg(&staged);
    checked(pack, &logs.join("ditto-create.log"), 120)?;
    let snapshot = ArchiveSnapshot::read(&staged)?;
    let envelope = OperationFixtureEnvelope {
        archive: snapshot.identity(),
        reference: ReferenceDocument::new(reference_bytes)?,
    };
    envelope.validate(&shipping_archive.identity(), shipping_reference)?;
    let verification_out = logs.join("archive-verification");
    fs::create_dir(&verification_out)?;
    let verified = macos_archive::verify_operation_snapshot(
        root,
        plan,
        &snapshot,
        &reference.app,
        build,
        &verification_out,
    )?;
    let verification = verified.verification.clone();
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &staged,
        rustix::fs::CWD,
        out.join(app_reference::ARCHIVE),
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    if ArchiveSnapshot::read(&out.join(app_reference::ARCHIVE))?.identity() != envelope.archive {
        return Err("Published operation-fixture archive changed".into());
    }
    macos_archive::write(
        &out.join(app_reference::REFERENCE),
        envelope.reference.contents.as_bytes(),
        0o644,
    )?;
    if macos_archive::tree(shipping, false)? != normal.files {
        return Err("Operation-fixture construction changed frozen shipping bytes".into());
    }
    plan.require_qualified_source(root)?;
    remove_newly_owned(stage.path(), &stage_held, "staging", &logs)?;
    remove_newly_owned(
        verified._enclosure.path(),
        &verified._held,
        "verification-extraction",
        &logs,
    )?;
    remove_newly_owned(
        compiled.source.path(),
        &compiled.held,
        "compiled-worker",
        &logs,
    )?;
    macos_archive::write(
        &out.join("result.json"),
        &serde_json::to_vec_pretty(
            &json!({"stage":"operation_fixture_producer","source":normal.source,"source_epoch":normal.source_epoch,"source_provenance":normal.source_provenance,"workflow":normal.workflow,"target":normal.target,"operation_fixture":envelope,"verification":verification,"scope":"nonshipping frozen producer artifact; no floor GUI execution or shipping release asset"}),
        )?,
        0o644,
    )?;
    Ok((envelope, verification))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("operation scratch (owned)-")
            .tempdir()
            .unwrap()
    }
    #[test]
    fn settled_new_producer_scratch_is_removed_with_actual_usage_receipt() {
        let parent = root();
        let base = parent.path().canonicalize().unwrap();
        let logs = base.join("logs");
        fs::create_dir(&logs).unwrap();
        let owned = xtask::archive::create_artifact_directory(&base).unwrap();
        let held = eml_fs::BoundDirectory::open(owned.path()).unwrap();
        fs::create_dir(owned.path().join("nested")).unwrap();
        fs::write(owned.path().join("one"), b"one").unwrap();
        fs::write(owned.path().join("nested/two"), b"two2").unwrap();
        remove_newly_owned(owned.path(), &held, "success", &logs).unwrap();
        assert!(!owned.path().exists());
        let receipt: Value =
            serde_json::from_slice(&fs::read(logs.join("success-cleanup.json")).unwrap()).unwrap();
        assert_eq!(receipt["removed"], true);
        assert_eq!(receipt["files"], 2);
        assert_eq!(receipt["logical_bytes"], 7);
    }
    #[test]
    fn replaced_scratch_address_preserves_both_original_and_replacement() {
        let parent = root();
        let base = parent.path().canonicalize().unwrap();
        let logs = base.join("logs");
        fs::create_dir(&logs).unwrap();
        let owned = xtask::archive::create_artifact_directory(&base).unwrap();
        let held = eml_fs::BoundDirectory::open(owned.path()).unwrap();
        fs::write(owned.path().join("sentinel"), b"original").unwrap();
        let moved = base.join("moved-original");
        fs::rename(owned.path(), &moved).unwrap();
        fs::create_dir(owned.path()).unwrap();
        fs::write(owned.path().join("sentinel"), b"replacement").unwrap();
        assert!(remove_newly_owned(owned.path(), &held, "replaced", &logs).is_err());
        assert_eq!(fs::read(moved.join("sentinel")).unwrap(), b"original");
        assert_eq!(
            fs::read(owned.path().join("sentinel")).unwrap(),
            b"replacement"
        );
        assert!(!logs.join("replaced-cleanup.json").exists());
    }
    #[test]
    fn nested_symlink_refusal_preserves_owned_and_outside_sentinels() {
        let parent = root();
        let base = parent.path().canonicalize().unwrap();
        let logs = base.join("logs");
        fs::create_dir(&logs).unwrap();
        let outside = base.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside").unwrap();
        let owned = xtask::archive::create_artifact_directory(&base).unwrap();
        let held = eml_fs::BoundDirectory::open(owned.path()).unwrap();
        fs::write(owned.path().join("sentinel"), b"owned").unwrap();
        std::os::unix::fs::symlink(&outside, owned.path().join("outside-link")).unwrap();
        assert!(remove_newly_owned(owned.path(), &held, "symlink", &logs).is_err());
        assert_eq!(fs::read(owned.path().join("sentinel")).unwrap(), b"owned");
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
        assert!(
            fs::symlink_metadata(owned.path().join("outside-link"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!logs.join("symlink-cleanup.json").exists());
    }
}
