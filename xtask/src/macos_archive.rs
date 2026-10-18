//! Native app assembly, frozen-input ditto readback and extracted execution.
use crate::{Result, capture, controller, macos, macos_app, notices};
use eml_app::{RequestSnapshot, RunQueue, WorkerControl, WorkerObservation, WorkerRun};
use eml_macos::{ChildState, StopSignal};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use xtask::archive::{ArchiveSnapshot, ByteIdentity};
const MAIN: &str = "Contents/MacOS/EMLAttachmentRemover";
const WORKER: &str = "Contents/Helpers/eml-worker";
type Files = BTreeMap<String, (ByteIdentity, u32)>;
fn artifact(root: &Path, target: &str) -> Result<(String, String, String)> {
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let row = plan["native_targets"]
        .as_array()
        .ok_or("Missing native target policy")?
        .iter()
        .find(|row| row["target"] == target && row["app"] == true)
        .ok_or("Target has no native app artifact")?;
    let platform = row["platform"].as_str().ok_or("Missing app platform")?;
    let app = plan["expected_artifacts"]
        .as_array()
        .ok_or("Missing artifact policy")?
        .iter()
        .find(|app| app["platform"] == platform && app["role"] == "app")
        .ok_or("Missing expected app archive")?;
    Ok((
        platform.to_owned(),
        app["name"]
            .as_str()
            .ok_or("Missing app archive name")?
            .to_owned(),
        app["root"].as_str().ok_or("Missing app root")?.to_owned(),
    ))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    schema: u8,
    source: String,
    version: String,
    target: String,
    architecture: String,
    minimum: u32,
    root: String,
    files: Files,
}
fn read(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err("App input is not a bounded ordinary file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("App input exceeds its byte bound".into());
    }
    Ok(bytes)
}
fn write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    fs::create_dir_all(path.parent().ok_or("Missing app output parent")?)?;
    let mut file = fs::File::options()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()?;
    Ok(())
}
fn tree(root: &Path, normalize: bool) -> Result<Files> {
    fn visit(
        base: &Path,
        path: &Path,
        depth: usize,
        normalize: bool,
        files: &mut Files,
        total: &mut u64,
    ) -> Result<()> {
        if depth > 8 || files.len() > 4096 {
            return Err("App tree exceeds its depth/entry bound".into());
        }
        let metadata = fs::symlink_metadata(path)?;
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.file_type().is_symlink()
        {
            return Err("App tree ownership/type is not the current owned fixture".into());
        }
        if metadata.is_dir() {
            if normalize {
                fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
            } else if metadata.mode() & 0o7777 != 0o755 {
                return Err("App directory permissions changed".into());
            }
            for entry in fs::read_dir(path)? {
                visit(base, &entry?.path(), depth + 1, normalize, files, total)?;
            }
        } else if metadata.is_file() {
            if metadata.nlink() != 1 {
                return Err("App input has unexpected hard links".into());
            }
            let relative = path
                .strip_prefix(base)?
                .to_str()
                .ok_or("App tree path is not UTF8")?
                .to_owned();
            xtask::archive::validate_member_name(&relative)?;
            let mode = if matches!(relative.as_str(), MAIN | WORKER) {
                0o755
            } else {
                0o644
            };
            if normalize {
                fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
            } else if metadata.mode() & 0o7777 != mode {
                return Err("App file permissions changed".into());
            }
            let bytes = read(path, 256 * 1024 * 1024)?;
            *total = total
                .checked_add(bytes.len() as u64)
                .ok_or("App tree size overflow")?;
            if *total > 1024 * 1024 * 1024
                || files
                    .insert(relative, (ByteIdentity::of(&bytes), mode))
                    .is_some()
            {
                return Err("App tree size or unique-file bound failed".into());
            }
        } else {
            return Err("App tree contains a nonordinary entry".into());
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, 0, normalize, &mut files, &mut 0)?;
    Ok(files)
}
pub(crate) fn bundle_identity(app: &Path) -> Result<Value> {
    Ok(serde_json::to_value(tree(app, false)?)?)
}

fn inspect_images(root: &Path, app: &Path, build: &macos::MacBuild, out: &Path) -> Result<Value> {
    fs::create_dir(out)?;
    let files = tree(app, false)?;
    let mut images = Vec::new();
    for (relative, (identity, _)) in &files {
        let path = app.join(relative);
        let bytes = read(&path, 256 * 1024 * 1024)?;
        if matches!(relative.as_str(), MAIN | WORKER) {
            let image = macos::fixture_identity(&bytes, &build.architecture, build.minimum)?;
            let label = if relative == MAIN { "gui" } else { "worker" };
            let logs = out.join(label);
            fs::create_dir(&logs)?;
            let runtime = crate::archive_verify::runtime(
                root,
                &path,
                "macos",
                &build.architecture,
                "",
                &logs,
            )?;
            let mut vtool = build.command(root, "/usr/bin/xcrun");
            vtool.args(["vtool", "-show-build"]).arg(&path);
            macos::checked_run(vtool, &logs.join("vtool.log"), 30)?;
            let entitlement_path = logs.join("entitlements.plist");
            let mut entitlements = build.command(root, "/usr/bin/codesign");
            entitlements
                .args(["--display", "--entitlements", "-", "--xml"])
                .arg(&path);
            controller()?
                .run_separate_output(entitlements, &entitlement_path, Duration::from_secs(30))?
                .require_success(&entitlement_path)?;
            let entitlement_bytes = read(&entitlement_path, 1024 * 1024)?;
            let entitlements = if entitlement_bytes.is_empty() {
                Value::Null
            } else {
                let mut convert = build.command(root, "/usr/bin/plutil");
                convert
                    .args(["-convert", "json", "-o", "-"])
                    .arg(&entitlement_path);
                serde_json::from_slice::<Value>(&crate::capture(&mut convert)?)?
            };
            eprintln!(
                "Verified {relative}: thin {}, minos {}.{}.{}; actual vtool output retained",
                image.architecture,
                image.minimum >> 16,
                (image.minimum >> 8) & 255,
                image.minimum & 255
            );
            images.push(json!({"path":relative,"image":image,"bytes":identity,"runtime":runtime,"entitlements":entitlements}));
        } else if bytes.get(..4).is_some_and(|magic| {
            matches!(
                magic,
                [0xcf, 0xfa, 0xed, 0xfe]
                    | [0xfe, 0xed, 0xfa, 0xcf]
                    | [0xce, 0xfa, 0xed, 0xfe]
                    | [0xfe, 0xed, 0xfa, 0xce]
                    | [0xca, 0xfe, 0xba, 0xbe]
                    | [0xbe, 0xba, 0xfe, 0xca]
                    | [0xca, 0xfe, 0xba, 0xbf]
                    | [0xbf, 0xba, 0xfe, 0xca]
            )
        }) {
            return Err("Unexpected Mach-O/fat image in app bundle".into());
        }
    }
    if images.len() != 2 {
        return Err("App must contain exactly its native GUI and private worker images".into());
    }
    let mut verify = build.command(root, "/usr/bin/codesign");
    verify
        .args(["--verify", "--strict", "--deep", "--verbose=2"])
        .arg(app);
    macos::checked_run(verify, &out.join("bundle-signature.log"), 30)?;
    Ok(
        json!({"images":images,"bundle_signature":"ad_hoc; inner images independently checked","notarized":false}),
    )
}
fn worker_smoke(worker: &Path, version: &str, out: &Path) -> Result<Value> {
    fs::create_dir(out)?;
    let source = out.join("message.eml");
    let input = include_bytes!("../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    let golden = include_bytes!("../../tests/contract/mime/golden/01-gmail-attachment.eml");
    write(&source, input, 0o600)?;
    let mut observations = Vec::new();
    for expected in ["created", "up_to_date"] {
        let mut queue = RunQueue::default();
        queue.enqueue(RequestSnapshot::new(vec![source.clone()], None)?)?;
        let (id, request) = queue
            .start_next()?
            .ok_or("Missing admitted worker request")?;
        let mut run = WorkerRun::start(
            worker.to_owned(),
            version.to_owned(),
            request,
            WorkerControl::new(id),
        )?;
        let report = run.report();
        let deadline = Instant::now() + Duration::from_secs(15);
        let completion = loop {
            if let WorkerObservation::Settled(result) = run.poll() {
                break result;
            }
            if Instant::now() >= deadline || !controller()?.admits_work() {
                run.control()
                    .confirm_shutdown(Instant::now() - Duration::from_secs(12))?;
                let cleanup = Instant::now() + Duration::from_secs(5);
                loop {
                    if let WorkerObservation::Settled(result) = run.poll() {
                        if !result.settled {
                            return Err(
                                "Extracted worker lifetime remains unverified after cleanup intent"
                                    .into(),
                            );
                        }
                        break;
                    }
                    if Instant::now() >= cleanup {
                        return Err("Extracted worker lifetime remains unverified; preserve owned resources and abort later work".into());
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                return Err(
                    "Extracted worker exceeded its smoke allowance; Kill intent settled its actors"
                        .into(),
                );
            }
            thread::sleep(Duration::from_millis(5));
        };
        if !completion.settled
            || !completion.reusable
            || completion.failure.is_some()
            || completion.exit_code != Some(0)
        {
            return Err(format!("Extracted worker did not qualify: {completion:?}").into());
        }
        let exported: Value = serde_json::from_slice(&report.export_bytes()?)?;
        if exported["items"][0]["status"] != expected
            || exported["summary"]["exit_code"] != 0
            || !exported["protocol_failure"].is_null()
            || read(&source, 4 * 1024 * 1024)? != input
            || read(&out.join("message.no-attachments.eml"), 4 * 1024 * 1024)? != golden
        {
            return Err("Extracted private worker outcome/golden/original check failed".into());
        }
        observations.push(json!({"expected":expected,"completion":completion,"report":exported}));
    }
    Ok(
        json!({"runs":observations,"scope":"actual extracted private worker through the production supervisor and Core; independent golden and unchanged original"}),
    )
}
fn settle(child: &eml_macos::OwnedChild) -> Result<eml_macos::ChildExit> {
    if let ChildState::Exited(exit) = child.poll()? {
        return Ok(exit);
    }
    child.signal(StopSignal::Kill)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let ChildState::Exited(exit) = child.poll()? {
            return Ok(exit);
        }
        if Instant::now() >= deadline {
            return Err("Owned GUI termination/reap remains unverified".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn drain(mut reader: std::io::PipeReader) -> Result<Vec<u8>> {
    let flags = rustix::fs::fcntl_getfl(&reader)?;
    rustix::fs::fcntl_setfl(&reader, flags | rustix::fs::OFlags::NONBLOCK)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut kept = Vec::new();
    let mut buffer = [0u8; 4096];
    let result = (|| -> Result<()> {
        loop {
            if Instant::now() >= deadline {
                return Err("GUI diagnostic reader did not reach EOF".into());
            }
            match reader.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(count) => {
                    let take = count.min(65536usize.saturating_sub(kept.len()));
                    kept.extend_from_slice(&buffer[..take]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
    })();
    let closed = eml_macos::close_reader(reader);
    result?;
    closed?;
    Ok(kept)
}
fn gui_startup(root: &Path, app: &Path, build: &macos::MacBuild, out: &Path) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("GUI startup smoke requires an owned fresh hosted runner; saved owner preferences must remain unchanged".into());
    }
    fs::create_dir(out)?;
    let observer = out.join("AppWindowObservation");
    let mut compile = build.command(root, &build.swiftc);
    compile
        .args([
            "-parse-as-library",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-target",
            &build.swift_target(),
            "-sdk",
        ])
        .arg(&build.sdk)
        .args(["-framework", "CoreGraphics"])
        .arg(root.join("tests/macos/AppWindowObservation.swift"))
        .arg("-o")
        .arg(&observer);
    macos::checked_run(compile, &out.join("observer-build.log"), 120)?;
    macos::fixture_identity(
        &read(&observer, 16 * 1024 * 1024)?,
        &build.architecture,
        build.minimum,
    )?;
    let observed = eml_macos::spawn(&app.join(MAIN), &[])?;
    let child = observed.child;
    let input = eml_macos::close_writer(observed.pipes.input);
    let observation = (|| -> Result<Value> {
        input?;
        if !observed.issues.is_empty() {
            return Err(format!("GUI spawn setup failed: {:?}", observed.issues).into());
        }
        let child = child.as_ref().ok_or("GUI was not spawned")?;
        let pid = child
            .observation_pid()?
            .ok_or("GUI authority unavailable")?;
        let mut command = build.command(root, &observer);
        command.arg(pid.to_string());
        macos::checked_run(command, &out.join("window.json"), 25)?;
        if child.poll()? != ChildState::Running {
            return Err("GUI terminated during actual window observation".into());
        }
        let window: Value = serde_json::from_slice(&read(&out.join("window.json"), 65536)?)?;
        Ok(window)
    })();
    let termination = child.as_ref().map(settle).transpose();
    let stdout = drain(observed.pipes.output);
    let stderr = drain(observed.pipes.errors);
    if let Some(control) = observed.pipes.control {
        eml_macos::close_reader(control)?;
    }
    let errors = stderr.as_ref().ok().cloned().unwrap_or_default();
    fs::write(out.join("gui.stderr"), &errors)?;
    let cleanup = json!({"terminal":termination.as_ref().ok().and_then(|x|x.as_ref()).map(|exit|json!({"code":exit.code,"signal":exit.signal})),"termination_error":termination.as_ref().err().map(ToString::to_string),"stdout_settled":stdout.is_ok(),"stderr_settled":stderr.is_ok(),"scope":"no requests supplied; exact owned GUI stopped by harness after startup observation; not UI Quit evidence"});
    fs::write(
        out.join("cleanup.json"),
        serde_json::to_vec_pretty(&cleanup)?,
    )?;
    let window = observation?;
    termination?;
    stdout?;
    stderr?;
    Ok(json!({"window":window,"cleanup":cleanup}))
}
fn verify_snapshot(
    root: &Path,
    snapshot: &ArchiveSnapshot,
    reference: &Reference,
    build: &macos::MacBuild,
    out: &Path,
) -> Result<Value> {
    let (_, _, app_root) = artifact(root, &build.host)?;
    if reference.schema != 1
        || reference.source != crate::git(root, &["rev-parse", "HEAD"])?
        || reference.version != build.version
        || reference.target != build.host
        || reference.architecture != build.architecture
        || reference.minimum != build.minimum
        || reference.root != app_root
        || !crate::git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty()
    {
        return Err("App reference does not match the current source/native build policy".into());
    }
    snapshot.inspect_ditto_app(&reference.root, &reference.files)?;
    let enclosure = xtask::archive::create_artifact_directory(out)?;
    let copy = enclosure.path().join("inspected-input.zip");
    write(&copy, snapshot.bytes(), 0o600)?;
    let mut extract = build.command(root, "/usr/bin/ditto");
    extract
        .args(["-x", "-k", "--norsrc", "--noextattr", "--noqtn", "--noacl"])
        .arg(&copy)
        .arg(enclosure.path());
    macos::checked_run(extract, &out.join("ditto-extract.log"), 120)?;
    let app = enclosure.path().join(&reference.root);
    if tree(&app, false)? != reference.files {
        return Err("Extracted app tree differs from frozen producer reference".into());
    }
    let images = inspect_images(root, &app, build, &out.join("extracted-images"))?;
    let worker = worker_smoke(
        &app.join(WORKER),
        &build.version,
        &enclosure.path().join("worker-fixture"),
    )?;
    let gui = if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        gui_startup(root, &app, build, &out.join("gui-startup"))?
    } else {
        json!({"outcome":"not_run","reason":"requires an owned fresh hosted runner; saved owner preferences protected"})
    };
    let interaction = if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        use crate::macos_interaction::{DesktopCase, run_case};
        // Exercise the shipping bundle before registering copied fixture bundles.
        // The earlier startup observation remains a separate launch.
        let workflow = run_case(
            root,
            &app,
            build,
            &out.join("gui-interaction"),
            DesktopCase::Workflow,
        )?;
        let publication =
            crate::macos_publication_ui::run(root, &app, build, &out.join("gui-publication"))?;
        // Native Settings changes belong to the final phase on this disposable
        // host. Failure stops qualification with restoration uncertainty retained.
        let appearance = run_case(
            root,
            &app,
            build,
            &out.join("gui-interaction/appearance"),
            DesktopCase::Appearance,
        )?;
        json!({"workflow":workflow,"publication":publication,"appearance":appearance})
    } else {
        json!({"outcome":"not_run","reason":"requires a fresh hosted Mac; local source compilation is not GUI interaction evidence"})
    };
    if tree(&app, false)? != reference.files {
        return Err("Native app/worker execution changed the extracted bundle".into());
    }
    Ok(
        json!({"snapshot":snapshot.identity(),"images":images,"worker":worker,"gui_startup":gui,"gui_interaction":interaction,"scope":"bundle structure/signatures, actual extracted Core-backed worker and scoped native GUI interaction; full GUI acceptance remains separate"}),
    )
}
pub(crate) fn build(root: &Path, out: &Path, icons: &Path) -> Result<()> {
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let build = macos::load(root)?;
    if !crate::git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty() {
        return Err("App artifacts require a clean committed source checkout".into());
    }
    let source = crate::git(root, &["rev-parse", "HEAD"])?;
    let icon_record: Value =
        serde_json::from_slice(&read(&icons.join("result.json"), 1024 * 1024)?)?;
    if icon_record["source"] != source
        || icon_record["source_working_tree"] != ""
        || icon_record["outputs_validated"] != true
        || icon_record["build"]["minimum_text"] != build.minimum_text
        || icon_record["build"]["xcode_identity"] != build.xcode_identity
    {
        return Err("Shared icon input is not the validated same-source pinned producer".into());
    }
    let icon_dir = icons.join("icon-assets");
    let partial = macos_app::icon_outputs(root, &icon_dir, &build)?;
    for name in ["Assets.car", "EML.icns", "icon-partial.plist"] {
        let identity = ByteIdentity::of(&read(&icon_dir.join(name), 64 * 1024 * 1024)?);
        if serde_json::to_value(identity)? != icon_record["materials"][name] {
            return Err("Shared icon bytes differ from the selected producer".into());
        }
    }
    let (platform, name, app_root) = artifact(root, &build.host)?;
    let stage = xtask::archive::create_artifact_directory(&out)?;
    let gui = notices::checked_component(root, &platform, notices::NoticeComponent::Gui)?;
    let worker = notices::checked_component(root, &platform, notices::NoticeComponent::Worker)?;
    for materials in [&gui, &worker] {
        if materials.target() != build.host || materials.version() != build.version {
            return Err("App legal material identity differs from native build".into());
        }
    }
    let sources = out.join("sources");
    macos_app::build_sources(root, &sources)?;
    let app = stage.path().join(&app_root);
    let worker_bytes = read(&sources.join("eml-worker"), 256 * 1024 * 1024)?;
    write(
        &app.join(MAIN),
        &read(&sources.join("EMLAttachmentRemover"), 256 * 1024 * 1024)?,
        0o755,
    )?;
    write(&app.join(WORKER), &worker_bytes, 0o755)?;
    let mut metadata = macos_app::app_metadata(&build.version, &build.minimum_text);
    metadata
        .as_object_mut()
        .ok_or("Invalid app metadata")?
        .extend(partial.as_object().ok_or("Invalid icon metadata")?.clone());
    let info = app.join("Contents/Info.plist");
    write(&info, &serde_json::to_vec(&metadata)?, 0o644)?;
    let mut plist = build.command(root, "/usr/bin/plutil");
    plist.args(["-convert", "xml1"]).arg(&info);
    macos::checked_run(plist, &out.join("info-plist.log"), 30)?;
    for name in ["Assets.car", "EML.icns"] {
        write(
            &app.join("Contents/Resources").join(name),
            &read(&icon_dir.join(name), 64 * 1024 * 1024)?,
            0o644,
        )?;
    }
    let resources = app.join("Contents/Resources");
    write(
        &resources.join("LICENSE"),
        &read(&root.join("LICENSE"), 1024 * 1024)?,
        0o644,
    )?;
    for (component, materials) in [("gui", &gui), ("worker", &worker)] {
        for (name, bytes) in materials.files() {
            write(
                &resources.join("licenses").join(component).join(name),
                bytes,
                0o644,
            )?;
        }
    }
    write(&resources.join("THIRD-PARTY-NOTICES.txt"),b"Complete macOS app interface legal materials: licenses/gui/THIRD-PARTY-NOTICES.txt and its accompanying licenses/.\nComplete private Core-backed worker legal materials: licenses/worker/THIRD-PARTY-NOTICES.txt and its accompanying licenses/.\nEach component's paths are relative to its own directory. Software and owner artwork terms: LICENSE.\n",0o644)?;
    write(&resources.join("README.txt"),format!("EML Attachment Remover {}\nmacOS {}+, native {}.\nGUI and CLI are separate products using the same Rust business libraries. This app uses its private same-CPU worker.\nAd hoc signed only; no Developer ID or notarization. First launch may require macOS app-specific Open Anyway approval.\nSource: https://github.com/resoltico/EMLAttachmentRemover/tree/{}\nNo v6 release/tag/publication is claimed.\n",build.version,build.minimum_text,build.architecture,source).as_bytes(),0o644)?;
    let mut sign = build.command(root, "/usr/bin/codesign");
    sign.args(["--force", "--sign", "-", "--timestamp=none"])
        .arg(&app);
    macos::checked_run(sign, &out.join("app-adhoc-sign.log"), 60)?;
    let files = tree(&app, true)?;
    if read(&app.join(WORKER), 256 * 1024 * 1024)? != worker_bytes {
        return Err("Outer ad hoc signing changed the frozen signed private worker".into());
    }
    let images = inspect_images(root, &app, &build, &out.join("producer-images"))?;
    let reference = Reference {
        schema: 1,
        source,
        version: build.version.clone(),
        target: build.host.clone(),
        architecture: build.architecture.clone(),
        minimum: build.minimum,
        root: app_root,
        files,
    };
    let staged = stage.path().join(&name);
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
    .arg(&app)
    .arg(&staged);
    macos::checked_run(pack, &out.join("ditto-create.log"), 120)?;
    let snapshot = ArchiveSnapshot::read(&staged)?;
    let verification = verify_snapshot(root, &snapshot, &reference, &build, &out)?;
    let destination = out.join(&name);
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &staged,
        rustix::fs::CWD,
        &destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    if ArchiveSnapshot::read(&destination)?.identity() != snapshot.identity() {
        return Err("Published app archive differs from the verified snapshot".into());
    }
    write(
        &out.join(format!("{name}.reference.json")),
        &serde_json::to_vec_pretty(&reference)?,
        0o644,
    )?;
    fs::write(
        out.join("app-result.json"),
        serde_json::to_vec_pretty(
            &json!({"archive":destination,"identity":snapshot.identity(),"producer_images":images,"verification":verification,"source":reference.source,"scope":"native app bundle/archive and actual extracted checks; full GUI/source/platform qualification remains separate"}),
        )?,
    )?;
    Ok(())
}
pub(crate) fn smoke(root: &Path, archive: &Path, reference_path: &Path) -> Result<()> {
    let build = macos::load(root)?;
    let (_, name, _) = artifact(root, &build.host)?;
    if archive.file_name() != Some(std::ffi::OsStr::new(&name)) {
        return Err("App archive basename does not match its native target/version".into());
    }
    let reference: Reference = serde_json::from_slice(&read(reference_path, 4 * 1024 * 1024)?)?;
    let parent = root.join("target/app-archive-smoke");
    fs::create_dir_all(&parent)?;
    let out = tempfile::Builder::new()
        .prefix("native-")
        .tempdir_in(parent)?
        .keep();
    let host = crate::native_host::inspect(root, &build.host, "macos", &build.architecture)?;
    let snapshot = ArchiveSnapshot::read(archive)?;
    let verification = verify_snapshot(root, &snapshot, &reference, &build, &out)?;
    fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(
            &json!({"host":host,"source":reference.source,"verification":verification}),
        )?,
    )?;
    Ok(())
}
