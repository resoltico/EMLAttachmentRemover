//! Native app assembly. Building alone is not archive/GUI/legal qualification.
use crate::macos::{self, checked_run, cli_artifact, fixture_identity};
use crate::{Result, capture};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn run(root: &Path, out: &Path) -> Result<()> {
    let build = macos::load(root)?;
    fs::create_dir(out)?; // Refuse an existing output or unrelated bundle.
    let out = out.canonicalize()?;
    let result = assemble(root, &out, &build);
    if let Err(error) = &result
        && let Err(record) = fs::write(out.join("failure.log"), format!("{error}\n"))
    {
        eprintln!("Could not retain native app build failure: {record}");
    }
    result
}

fn assemble(root: &Path, out: &Path, build: &macos::MacBuild) -> Result<()> {
    let cargo = capture(
        build
            .command(root, "cargo")
            .args([
                "build",
                "--locked",
                "--release",
                "--package",
                "remove-eml-attachments",
                "--target",
                &build.host,
                "--message-format=json",
            ])
            .env("MACOSX_DEPLOYMENT_TARGET", &build.minimum_text)
            .env("CARGO_TARGET_DIR", out.join("cargo")),
    )?;
    fs::write(out.join("cargo.jsonl"), &cargo)?;
    let helper = cli_artifact(root, &cargo)?;
    let mut overflow = Vec::new();
    for value in serde_json::Deserializer::from_slice(&cargo).into_iter::<Value>() {
        let value = value?;
        if value["reason"] == "compiler-artifact"
            && value["executable"]
                .as_str()
                .is_some_and(|path| Path::new(path) == helper)
        {
            overflow.push(value["profile"]["overflow_checks"] == true);
        }
    }
    if overflow != [true] {
        return Err("Actual release CLI must retain overflow checks".into());
    }
    fixture_identity(&fs::read(&helper)?, &build.architecture, build.minimum)?;
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
    let standalone = out.join("signed-cli");
    fs::create_dir(&standalone)?;
    let signed_cli = standalone.join("remove-eml-attachments");
    fs::copy(&helper, &signed_cli)?;
    let mut sign_helper = build.command(root, "/usr/bin/codesign");
    sign_helper
        .args(["--force", "--sign", "-", "--timestamp=none"])
        .arg(&signed_cli);
    checked_run(sign_helper, &out.join("helper-sign.log"), 60)?;
    fs::copy(&signed_cli, macos.join("remove-eml-attachments"))?;
    fs::copy(&executable, macos.join("EMLAttachmentRemover"))?;
    for name in ["Assets.car", "EML.icns"] {
        fs::copy(icons.join(name), resources.join(name))?;
    }
    fs::copy(root.join("LICENSE"), resources.join("LICENSE"))?;
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
        ("standalone", signed_cli.as_path()),
        ("app", app.as_path()),
        ("helper", macos.join("remove-eml-attachments").as_path()),
    ] {
        let mut verify = build.command(root, "/usr/bin/codesign");
        verify
            .args(["--verify", "--strict", "--verbose=2"])
            .arg(path);
        checked_run(verify, &out.join(format!("{role}-verify.log")), 60)?;
    }
    if fs::read(&signed_cli)? != fs::read(macos.join("remove-eml-attachments"))? {
        return Err("Outer signing changed the copied helper bytes".into());
    }
    let helper_identity =
        fixture_identity(&fs::read(&signed_cli)?, &build.architecture, build.minimum)?;
    let app_identity = fixture_identity(
        &fs::read(macos.join("EMLAttachmentRemover"))?,
        &build.architecture,
        build.minimum,
    )?;
    for (role, binary) in [
        ("app", macos.join("EMLAttachmentRemover")),
        ("helper", signed_cli.clone()),
    ] {
        let dependencies = String::from_utf8(capture(
            build.command(root, "/usr/bin/otool").arg("-L").arg(&binary),
        )?)?;
        fs::write(out.join(format!("{role}-dependencies.log")), &dependencies)?;
        if dependencies.lines().skip(1).any(|line| {
            !line.trim_start().starts_with("/usr/lib/")
                && !line.trim_start().starts_with("/System/Library/")
        }) {
            return Err(format!(
                "{role} has a non-system dependency requiring reviewed native runtime bundling"
            )
            .into());
        }
    }
    fs::write(
        out.join("build.json"),
        serde_json::to_vec_pretty(
            &json!({"build":build,"app":app,"signed_cli":signed_cli,"app_identity":app_identity,"helper_identity":helper_identity,"helper_signed_bytes_equal":true,"scope":"native build/signature only; complete notices, extracted archives and GUI qualification pending"}),
        )?,
    )?;
    eprintln!("Built native app for integration: {}", app.display());
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
