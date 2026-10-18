//! Native desktop sources and owner icon inputs; full app qualification is separate.
use crate::macos::{self, checked_run, fixture_identity};
use crate::{Result, capture, controller};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) fn run(root: &Path, out: &Path) -> Result<()> {
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let result = (|| -> Result<()> {
        let build = macos::load(root)?;
        let host = crate::native_host::inspect(root, &build.host, "macos", &build.architecture)?;
        let target_directory = out.join("cargo");
        let mut compile = build.command(root, "cargo");
        compile
            .args([
                "build",
                "--locked",
                "--release",
                "--package",
                "eml-gui",
                "--package",
                "eml-worker",
                "--features",
                "eml-gui/macos-gui",
                "--target",
                &build.host,
                "--message-format=json",
            ])
            .env("CARGO_TARGET_DIR", &target_directory)
            .env("MACOSX_DEPLOYMENT_TARGET", &build.minimum_text)
            .env("SLINT_COMPILER_DENY_WARNINGS", "1");
        let log = out.join("desktop-build.jsonl");
        let observed =
            controller()?.run_separate_output(compile, &log, Duration::from_secs(1200))?;
        observed.require_success(&log)?;
        if fs::metadata(&log)?.len() > 16 * 1024 * 1024 {
            return Err("Desktop build output exceeds its document bound".into());
        }
        let output = fs::read(&log)?;
        let mut artifacts = Vec::new();
        for (package, name) in [
            ("eml-gui", "EMLAttachmentRemover"),
            ("eml-worker", "eml-worker"),
        ] {
            let artifact = crate::cli_build::named_artifact(
                root,
                &output,
                &format!("crates/{package}/Cargo.toml"),
                &format!("crates/{package}/src/main.rs"),
                name,
            )?
            .canonicalize()?;
            if !artifact.starts_with(target_directory.canonicalize()?)
                || !fs::symlink_metadata(&artifact)?.is_file()
                || fs::metadata(&artifact)?.len() > 256 * 1024 * 1024
            {
                return Err("Native desktop executable escaped its bounded owned build".into());
            }
            let profile: Vec<_> = serde_json::Deserializer::from_slice(&output)
                .into_iter::<Value>()
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|value| {
                    value["reason"] == "compiler-artifact"
                        && value["executable"]
                            .as_str()
                            .is_some_and(|path| Path::new(path) == artifact)
                })
                .map(|value| value["profile"].clone())
                .collect();
            if profile.len() != 1
                || profile[0]["test"] != false
                || profile[0]["overflow_checks"] != true
                || profile[0]["opt_level"] != "3"
            {
                return Err("Desktop release artifact profile differs from required overflow-checked release".into());
            }
            let bytes = fs::read(&artifact)?;
            fixture_identity(&bytes, &build.architecture, build.minimum)?;
            let owned = out.join(name);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&owned)?;
            file.write_all(&bytes)?;
            file.flush()?;
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o755))?;
            file.sync_all()?;
            drop(file);
            let mut sign = build.command(root, "/usr/bin/codesign");
            sign.args(["--force", "--sign", "-", "--timestamp=none"])
                .arg(&owned);
            checked_run(sign, &out.join(format!("{package}-adhoc-sign.log")), 60)?;
            let mut verify = build.command(root, "/usr/bin/codesign");
            verify
                .args(["--verify", "--strict", "--verbose=2"])
                .arg(&owned);
            checked_run(verify, &out.join(format!("{package}-adhoc-verify.log")), 60)?;
            let signed = fs::read(&owned)?;
            let image = fixture_identity(&signed, &build.architecture, build.minimum)?;
            macos::inspect_system_dependencies(
                build.command(root, "/usr/bin/otool"),
                &owned,
                &out.join(format!("{package}-dependencies.log")),
            )?;
            artifacts.push(json!({"package":package,"name":name,"profile":profile[0],"image":image,"main_image_uuid":macos::main_image_uuid(&signed)?,"owned_bytes":xtask::archive::ByteIdentity::of(&signed),"signature":"ad_hoc","notarized":false}));
        }
        fs::write(
            out.join("desktop-sources.json"),
            serde_json::to_vec_pretty(
                &json!({"host":host,"build":build,"artifacts":artifacts,"planned_bundle_metadata":app_metadata(&build.version,&build.minimum_text),"scope":"native desktop executable source/build/ad-hoc-signature evidence only; app bundle, complete GUI legal closure, extracted archive and GUI acceptance remain incomplete"}),
            )?,
        )?;
        Err("Native Slint/worker sources built; required app bundle/legal/archive/GUI qualification remains incomplete".into())
    })();
    if let Err(error) = &result
        && let Err(record) = fs::write(out.join("failure.log"), format!("{error}\n"))
    {
        eprintln!("Could not retain desktop build failure: {record}");
    }
    result
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
            &json!({"inherited_presence_only":loader_presence,"effective_child_loader":macos::loader_environment(&icon),"actool_sdkroot_removed":true,"actool_fallback_library_path_removed":true,"developer":build.developer,"actool":actool}),
        )?,
    )?;
    icon.arg(root.join("artwork/EML.icon"))
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
