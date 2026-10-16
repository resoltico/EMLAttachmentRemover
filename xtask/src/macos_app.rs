//! Native app assembly. Building alone is not archive/GUI/legal qualification.
use crate::macos::{self, checked_run, fixture_identity};
use crate::{Result, capture};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn run(root: &Path, out: &Path) -> Result<()> {
    fs::create_dir(out)?; // Refuse an existing output or unrelated bundle.
    let out = out.canonicalize()?;
    let result = (|| {
        let target = format!("{}-apple-darwin", std::env::consts::ARCH);
        let cli = crate::cli_build::build(root, &target, &out.join("cli-build"))?;
        let materials = crate::notices::checked_target(root, cli.platform())?;
        let archive = crate::qualify::staged_cli(root, &cli, &materials, &out)?;
        fs::write(
            out.join("cli-archive.json"),
            serde_json::to_vec_pretty(
                &json!({"archive":archive,"frozen_cli":cli.identity(),"scope":"independently verified native CLI archive; app and full qualification gates remain separate"}),
            )?,
        )?;
        assemble(root, &out, &cli, &materials)
    })();
    if let Err(error) = &result
        && let Err(record) = fs::write(out.join("failure.log"), format!("{error}\n"))
    {
        eprintln!("Could not retain native app build failure: {record}");
    }
    result
}

pub(crate) fn assemble(
    root: &Path,
    out: &Path,
    cli: &crate::cli_build::FrozenCli,
    materials: &crate::notices::CheckedTargetMaterials,
) -> Result<()> {
    if materials.target() != cli.target()
        || materials.platform() != cli.platform()
        || materials.version() != cli.version()
    {
        return Err("App legal materials do not match frozen CLI target/version".into());
    }
    let build = cli.mac();
    let signed_cli = cli.path();
    if fs::read(signed_cli)? != cli.bytes() {
        return Err("Owned signed CLI path no longer matches frozen bytes".into());
    }
    let swift = build
        .developer
        .join("Toolchains/XcodeDefault.xctoolchain/usr/bin/swift");
    let scratch = out.join("swift");
    let mut compile = build.command(root, &swift);
    compile
        .args([
            "build",
            "--configuration",
            "release",
            "--arch",
            &build.swift_arch,
            "--package-path",
        ])
        .arg(root.join("macos-app"))
        .arg("--scratch-path")
        .arg(&scratch)
        .args([
            "-Xswiftc",
            "-warnings-as-errors",
            "-Xswiftc",
            "-sdk",
            "-Xswiftc",
        ])
        .arg(&build.sdk);
    checked_run(compile, &out.join("swift-build.log"), 600)?;
    let path = String::from_utf8(capture(
        build
            .command(root, &swift)
            .args([
                "build",
                "--configuration",
                "release",
                "--arch",
                &build.swift_arch,
                "--show-bin-path",
                "--package-path",
            ])
            .arg(root.join("macos-app"))
            .arg("--scratch-path")
            .arg(&scratch),
    )?)?;
    let lines: Vec<_> = path.lines().filter(|line| !line.is_empty()).collect();
    if lines.len() != 1 {
        return Err("SwiftPM must return one actual native binary directory".into());
    }
    let binary_dir = PathBuf::from(lines[0]).canonicalize()?;
    if !binary_dir.starts_with(scratch.canonicalize()?) {
        return Err("SwiftPM binary directory escaped owned scratch".into());
    }
    let executable = binary_dir.join("EMLAttachmentRemover");
    fixture_identity(&fs::read(&executable)?, &build.architecture, build.minimum)?;

    let (icon, icons) = icon_command(root, out, build)?;
    checked_run(icon, &out.join("icon-build.log"), 180)?;
    let partial = icon_outputs(root, &icons, build)?;
    let mut metadata = app_metadata(&build.version, &build.minimum_text);
    for (key, value) in partial
        .as_object()
        .ok_or("Icon metadata must be a dictionary")?
    {
        metadata
            .as_object_mut()
            .ok_or("App metadata must be a dictionary")?
            .insert(key.clone(), value.clone());
    }
    let app = out.join("EML Attachment Remover.app");
    let macos = app.join("Contents/MacOS");
    let resources = app.join("Contents/Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir(&resources)?;
    let helper_path = macos.join("remove-eml-attachments");
    let mut helper_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&helper_path)?;
    std::io::Write::write_all(&mut helper_file, cli.bytes())?;
    use std::os::unix::fs::PermissionsExt;
    helper_file.set_permissions(fs::Permissions::from_mode(0o755))?;
    drop(helper_file);
    fs::copy(&executable, macos.join("EMLAttachmentRemover"))?;
    let mut resource_bytes = materials.files().clone();
    for name in ["Assets.car", "EML.icns"] {
        resource_bytes.insert(name.to_owned(), fs::read(icons.join(name))?);
    }
    for (name, bytes) in &resource_bytes {
        xtask::archive::validate_member_name(name)?;
        let destination = resources.join(name);
        fs::create_dir_all(destination.parent().ok_or("Missing resource parent")?)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.set_permissions(fs::Permissions::from_mode(0o644))?;
    }
    for directory in resource_directories(&resource_bytes) {
        fs::set_permissions(resources.join(directory), fs::Permissions::from_mode(0o755))?;
    }
    verify_resources(&resources, &resource_bytes)?;
    let input = out.join("Info.json");
    fs::write(&input, serde_json::to_vec_pretty(&metadata)?)?;
    let mut convert = build.command(root, "/usr/bin/plutil");
    convert
        .args(["-convert", "xml1", "-o"])
        .arg(app.join("Contents/Info.plist"))
        .arg(&input);
    checked_run(convert, &out.join("plist.log"), 60)?;
    let readback: Value = serde_json::from_slice(&capture(
        build
            .command(root, "/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(app.join("Contents/Info.plist")),
    )?)?;
    if readback != metadata {
        return Err("Built app plist does not match generated metadata".into());
    }
    let mut sign_app = build.command(root, "/usr/bin/codesign");
    sign_app
        .args(["--force", "--sign", "-", "--timestamp=none"])
        .arg(&app);
    checked_run(sign_app, &out.join("app-sign.log"), 60)?;
    for (role, path) in [
        ("standalone", signed_cli),
        ("app", app.as_path()),
        ("helper", macos.join("remove-eml-attachments").as_path()),
    ] {
        let mut verify = build.command(root, "/usr/bin/codesign");
        verify
            .args(["--verify", "--strict", "--verbose=2"])
            .arg(path);
        checked_run(verify, &out.join(format!("{role}-verify.log")), 60)?;
    }
    if cli.bytes() != fs::read(macos.join("remove-eml-attachments"))? {
        return Err("Outer signing changed the copied helper bytes".into());
    }
    let helper_identity = fixture_identity(cli.bytes(), &build.architecture, build.minimum)?;
    let app_identity = fixture_identity(
        &fs::read(macos.join("EMLAttachmentRemover"))?,
        &build.architecture,
        build.minimum,
    )?;
    for (role, binary) in [
        ("app", macos.join("EMLAttachmentRemover")),
        ("helper", macos.join("remove-eml-attachments")),
    ] {
        macos::inspect_system_dependencies(
            build.command(root, "/usr/bin/otool"),
            &binary,
            &out.join(format!("{role}-dependencies.log")),
        )?;
    }
    fs::write(
        out.join("build.json"),
        serde_json::to_vec_pretty(
            &json!({"build":build,"app":app,"signed_cli":signed_cli,"frozen_cli_identity":cli.identity(),"app_identity":app_identity,"helper_identity":helper_identity,"helper_signed_bytes_equal":true,"scope":"native build/signature and reviewed legal materials; extracted archives, actual GUI and full platform qualification pending"}),
        )?,
    )?;
    eprintln!("Built native app for integration: {}", app.display());
    Ok(())
}

fn resource_directories(
    expected: &std::collections::BTreeMap<String, Vec<u8>>,
) -> std::collections::BTreeSet<String> {
    let mut directories = std::collections::BTreeSet::from([String::new()]);
    for name in expected.keys() {
        let mut path = name.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            directories.insert(parent.to_owned());
            path = parent;
        }
    }
    directories
}

fn verify_resources(
    resources: &Path,
    expected: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let directories = resource_directories(expected);
    if fs::metadata(resources)?.permissions().mode() & 0o7777 != 0o755 {
        return Err("App resource root mode differs".into());
    }
    let mut pending = vec![resources.to_owned()];
    let mut observed = std::collections::BTreeSet::new();
    let mut count = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            count += 1;
            if count > 4096 {
                return Err("App resource count exceeds its bound".into());
            }
            let kind = entry.file_type()?;
            let path = entry.path();
            let relative = path
                .strip_prefix(resources)?
                .to_str()
                .ok_or("Non-UTF-8 app resource")?;
            xtask::archive::validate_member_name(relative)?;
            if kind.is_dir() && !kind.is_symlink() {
                if !directories.contains(relative)
                    || entry.metadata()?.permissions().mode() & 0o7777 != 0o755
                {
                    return Err("Unexpected app resource directory/type/mode".into());
                }
                pending.push(path);
            } else if kind.is_file() && !kind.is_symlink() {
                let bytes = expected
                    .get(relative)
                    .ok_or("Unexpected app resource file")?;
                if entry.metadata()?.len() != bytes.len() as u64
                    || entry.metadata()?.permissions().mode() & 0o7777 != 0o644
                    || fs::read(&path)? != *bytes
                    || !observed.insert(relative.to_owned())
                {
                    return Err("App resource bytes/type/mode differ from frozen inputs".into());
                }
            } else {
                return Err("App resource must be an ordinary file/directory".into());
            }
        }
    }
    if observed.len() != expected.len() {
        return Err("App resource inventory is incomplete".into());
    }
    Ok(())
}

pub(crate) fn icon_command(
    root: &Path,
    out: &Path,
    build: &macos::MacBuild,
) -> Result<(std::process::Command, PathBuf)> {
    let icons = out.join("icon-assets");
    fs::create_dir(&icons)?;
    let actool = String::from_utf8(capture(build.command(root, "/usr/bin/xcrun").args([
        "--sdk",
        "macosx",
        "--toolchain",
        "XcodeDefault",
        "--find",
        "actool",
    ]))?)?;
    let actool = PathBuf::from(actool.trim_end()).canonicalize()?;
    if !actool.starts_with(build.developer.canonicalize()?) {
        return Err("actool is outside selected Xcode".into());
    }
    let mut icon = build.command(root, &actool);
    // actool launches a host AssetRuntime agent; do not give that runtime the
    // build SDK root. Compiler SDK selection remains explicit and unchanged.
    icon.env_remove("SDKROOT");
    // Remove the whole inherited fallback variable, including any pre-Cargo
    // entries. dyld defaults remain; this is not an established causal fix.
    icon.env_remove("DYLD_FALLBACK_LIBRARY_PATH");
    let loader_presence: serde_json::Map<String, Value> = [
        "SDKROOT",
        "DYLD_ROOT_PATH",
        "DYLD_LIBRARY_PATH",
        "DYLD_FRAMEWORK_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "DYLD_FALLBACK_FRAMEWORK_PATH",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            Value::Bool(std::env::var_os(name).is_some()),
        )
    })
    .collect();
    fs::write(
        out.join("icon-environment.json"),
        serde_json::to_vec_pretty(
            &json!({"inherited_presence_only":loader_presence,"actool_sdkroot_removed":true,"actool_fallback_library_path_removed":true,"developer":build.developer,"actool":actool}),
        )?,
    )?;
    icon.arg(root.join("macos-app/Artwork/EML.icon"))
        .arg("--compile")
        .arg(&icons)
        .args([
            "--output-format",
            "human-readable-text",
            "--notices",
            "--warnings",
            "--errors",
            "--output-partial-info-plist",
        ])
        .arg(icons.join("icon-partial.plist"))
        .args([
            "--app-icon",
            "EML",
            "--include-all-app-icons",
            "--enable-on-demand-resources",
            "NO",
            "--development-region",
            "en",
            "--target-device",
            "mac",
            "--minimum-deployment-target",
            &build.minimum_text,
            "--platform",
            "macosx",
        ]);
    Ok((icon, icons))
}

pub(crate) fn icon_outputs(root: &Path, icons: &Path, build: &macos::MacBuild) -> Result<Value> {
    for file in ["Assets.car", "EML.icns", "icon-partial.plist"] {
        if fs::metadata(icons.join(file))?.len() == 0 {
            return Err("actool omitted a required icon output".into());
        }
    }
    let partial: Value = serde_json::from_slice(&capture(
        build
            .command(root, "/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(icons.join("icon-partial.plist")),
    )?)?;
    let object = partial
        .as_object()
        .ok_or("Icon metadata must be a dictionary")?;
    if object.keys().any(|key| {
        !["CFBundleIconName", "CFBundleIconFile", "CFBundleIcons"].contains(&key.as_str())
    }) {
        return Err("Unexpected icon metadata key".into());
    }
    Ok(partial)
}

fn app_metadata(version: &str, minimum: &str) -> Value {
    json!({"CFBundleIdentifier":"io.github.resoltico.emlattachmentremover","CFBundleName":"EML Attachment Remover","CFBundleDisplayName":"EML Attachment Remover","CFBundleExecutable":"EMLAttachmentRemover","CFBundlePackageType":"APPL","CFBundleDevelopmentRegion":"en","LSApplicationCategoryType":"public.app-category.utilities","CFBundleShortVersionString":version,"CFBundleVersion":version,"LSMinimumSystemVersion":minimum,"NSHighResolutionCapable":true,"CFBundleIconName":"EML","NSServices":[{"NSMenuItem":{"default":"Create EML Copies Without Attachments"},"NSMessage":"createEMLCopies","NSPortName":"EMLAttachmentRemover","NSSendTypes":["public.file-url"],"NSSendFileTypes":["public.email-message","public.folder"],"NSRestricted":true,"NSRequiredContext":{}}],"CFBundleDocumentTypes":[{"CFBundleTypeName":"EML message","CFBundleTypeExtensions":["eml"],"LSItemContentTypes":["public.email-message"],"CFBundleTypeRole":"Viewer","LSHandlerRank":"None"}]})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_bundle_identity_versions_and_file_service_are_exact() {
        let value = app_metadata("7.2.1", "15.0");
        assert_eq!(
            value["CFBundleIdentifier"],
            "io.github.resoltico.emlattachmentremover"
        );
        assert_eq!(value["CFBundleShortVersionString"], "7.2.1");
        assert_eq!(value["CFBundleVersion"], "7.2.1");
        assert_eq!(value["LSMinimumSystemVersion"], "15.0");
        assert_eq!(value["NSServices"][0]["NSMessage"], "createEMLCopies");
        assert_eq!(
            value["NSServices"][0]["NSMenuItem"]["default"],
            "Create EML Copies Without Attachments"
        );
        assert_eq!(
            value["NSServices"][0]["NSSendTypes"],
            json!(["public.file-url"])
        );
    }
}
