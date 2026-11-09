use crate::{Result, archive, host, notices, run};
use eml_core::product::{CLI_NAME, DISPLAY_NAME, VERSION};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn build(cli: bool, app: bool, target: Option<String>, out: &Path) -> Result<()> {
    let host = host()?;
    let target = distribution_target(&host, app, target)?;
    let include_cli = cli || !app;
    let revision = crate::source::revision()?;
    let stamp = archive::Timestamp::for_revision(&revision)?;
    let bundle_version = app.then(|| native_bundle_version(VERSION)).transpose()?;
    let cargo_target = target_directory(&mut Command::new("cargo"))?;
    fs::create_dir_all(out)?;
    let scratch = tempfile::tempdir()?;
    let extension = if target.ends_with("windows-msvc") {
        ".exe"
    } else {
        ""
    };
    let executable = format!("{CLI_NAME}{extension}");
    let input = |name: &str| cargo_target.join(&target).join("dist").join(name);
    let mut products = Vec::new();
    if include_cli {
        products.push(("emlattachmentremover", executable.as_str(), CLI_NAME));
    }
    if app {
        products.push((
            "eml-gui",
            "emlattachmentremover-gui",
            "emlattachmentremover-gui",
        ));
    }
    // Cargo unifies dependency features across selected packages. Build each
    // product alone so its features match its standalone shipped-notices graph.
    for &(package, name, command_name) in &products {
        let mut command = Command::new("cargo");
        command.args([
            "build",
            "--profile",
            "dist",
            "--locked",
            "--target",
            &target,
            "-p",
            package,
            "--bin",
            command_name,
        ]);
        command.env("EML_SOURCE_REVISION", &revision);
        if package == "eml-gui" {
            command.args(["--features", "eml-gui/desktop"]);
        }
        let macos = target.ends_with("apple-darwin");
        if macos {
            command.env("MACOSX_DEPLOYMENT_TARGET", "15.0");
        }
        run(&mut command)?;
        if macos {
            let architectures =
                crate::capture(Command::new("lipo").arg("-archs").arg(input(name)))?;
            verify_macos_architecture(&target, &architectures)?;
        }
        if target == host {
            let actual = crate::capture(Command::new(input(name)).arg("--version"))?;
            let expected = format!("{command_name} {VERSION} ({revision})");
            if actual != expected {
                return Err(format!(
                    "Built {name} identity differs: expected {expected:?}, found {actual:?}"
                )
                .into());
            }
        }
    }
    let suffix = platform(&target)?;
    if include_cli {
        let stage = scratch.path().join("cli");
        fs::create_dir(&stage)?;
        fs::copy(input(&executable), stage.join(&executable))?;
        fs::copy("LICENSE", stage.join("LICENSE"))?;
        notices::write(false, &target, &stage.join("THIRD-PARTY-NOTICES.txt"))?;
        let format = if extension.is_empty() {
            "tar.gz"
        } else {
            "zip"
        };
        archive::write(
            &stage,
            &out.join(format!("{CLI_NAME}-{VERSION}-{suffix}.{format}")),
            &stamp,
        )?;
    }
    if let Some(bundle_version) = bundle_version {
        let stage = scratch.path().join("gui");
        let bundle = stage.join("EML Attachment Remover.app");
        let contents = bundle.join("Contents");
        for directory in ["MacOS", "Resources"] {
            fs::create_dir_all(contents.join(directory))?;
        }
        fs::copy(
            input("emlattachmentremover-gui"),
            contents.join("MacOS/EMLAttachmentRemover"),
        )?;
        fs::copy(
            "artwork/app-icon/EML.icns",
            contents.join("Resources/EML.icns"),
        )?;
        fs::copy("LICENSE", contents.join("Resources/LICENSE"))?;
        notices::write(
            true,
            &target,
            &contents.join("Resources/THIRD-PARTY-NOTICES.txt"),
        )?;
        fs::write(contents.join("Info.plist"), plist(bundle_version))?;
        run(Command::new("codesign")
            .args(["--force", "-s", "-"])
            .arg(&bundle))?;
        run(Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle))?;
        archive::write(
            &stage,
            &out.join(format!("{CLI_NAME}-gui-{VERSION}-{suffix}.zip")),
            &stamp,
        )?;
    }
    Ok(())
}
fn distribution_target(host: &str, app: bool, target: Option<String>) -> Result<String> {
    let target = target.unwrap_or_else(|| host.to_owned());
    validate_target(&target)?;
    if app && (!host.ends_with("apple-darwin") || !target.ends_with("apple-darwin")) {
        return Err("--app requires a macOS build host and macOS target".into());
    }
    Ok(target)
}
fn verify_macos_architecture(target: &str, actual: &str) -> Result<()> {
    let expected = match target {
        "aarch64-apple-darwin" => "arm64",
        "x86_64-apple-darwin" => "x86_64",
        _ => return Err(format!("Not a supported macOS target: {target}").into()),
    };
    let mut architectures = actual.split_whitespace();
    if architectures.next() != Some(expected) || architectures.next().is_some() {
        return Err(format!(
            "Built binary for {target} must contain only {expected}; found {actual:?}"
        )
        .into());
    }
    Ok(())
}
fn validate_target(target: &str) -> Result<()> {
    platform(target).map(|_| ())
}
fn platform(target: &str) -> Result<String> {
    let (arch, platform) = target
        .split_once('-')
        .ok_or("missing target architecture")?;
    if !["x86_64", "aarch64"].contains(&arch) {
        return Err(format!("unsupported architecture {arch}").into());
    }
    let os = match platform {
        "apple-darwin" => "macos",
        "unknown-linux-gnu" => "linux",
        "pc-windows-msvc" => "windows",
        _ => return Err(format!("unsupported target {target}").into()),
    };
    Ok(format!("{os}-{arch}"))
}
fn target_directory(cargo: &mut Command) -> Result<PathBuf> {
    let metadata =
        crate::capture(cargo.args(["metadata", "--no-deps", "--locked", "--format-version", "1"]))?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
    Ok(PathBuf::from(metadata["target_directory"].as_str().ok_or(
        "Cargo metadata did not report its target_directory",
    )?))
}

fn native_bundle_version(version: &str) -> Result<u32> {
    let parts = version
        .split('.')
        .map(|part| {
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err("Native packaging requires X.Y.Z without suffixes".into());
            }
            part.parse::<u32>().map_err(Into::into)
        })
        .collect::<Result<Vec<_>>>()?;
    let [major, minor, patch] = parts.as_slice() else {
        return Err("Native packaging requires X.Y.Z without suffixes".into());
    };
    if *minor > 99 || *patch > 99 {
        return Err("The native build mapping requires minor and patch at most 99".into());
    }
    major
        .checked_mul(10_000)
        .and_then(|value| value.checked_add(minor * 100))
        .and_then(|value| value.checked_add(*patch))
        .ok_or_else(|| "Native bundle version overflow".into())
}

fn plist(bundle_version: u32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>io.github.resoltico.emlattachmentremover</string>
<key>CFBundleName</key><string>{DISPLAY_NAME}</string>
<key>CFBundleDisplayName</key><string>{DISPLAY_NAME}</string>
<key>CFBundleExecutable</key><string>EMLAttachmentRemover</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{VERSION}</string>
<key>CFBundleVersion</key><string>{bundle_version}</string>
<key>CFBundleIconFile</key><string>EML</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>CFBundleDocumentTypes</key><array>
<dict><key>CFBundleTypeRole</key><string>Viewer</string><key>LSHandlerRank</key><string>Alternate</string><key>LSItemContentTypes</key><array><string>com.apple.mail.email</string></array></dict>
<dict><key>CFBundleTypeRole</key><string>Viewer</string><key>LSHandlerRank</key><string>Alternate</string><key>LSItemContentTypes</key><array><string>public.folder</string></array></dict>
</array>
</dict></plist>
"#
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distributions_select_one_native_or_explicit_target() {
        for (target, suffix) in [
            ("aarch64-apple-darwin", "macos-aarch64"),
            ("x86_64-apple-darwin", "macos-x86_64"),
            ("aarch64-unknown-linux-gnu", "linux-aarch64"),
            ("x86_64-unknown-linux-gnu", "linux-x86_64"),
            ("aarch64-pc-windows-msvc", "windows-aarch64"),
            ("x86_64-pc-windows-msvc", "windows-x86_64"),
        ] {
            assert_eq!(distribution_target(target, false, None).unwrap(), target);
            assert_eq!(platform(target).unwrap(), suffix);
        }
        for host in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
            assert_eq!(distribution_target(host, true, None).unwrap(), host);
            for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
                assert_eq!(
                    distribution_target(host, true, Some(target.into())).unwrap(),
                    target
                );
            }
        }
        for (host, target) in [
            ("aarch64-apple-darwin", "x86_64-unknown-linux-gnu"),
            ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin"),
            ("x86_64-pc-windows-msvc", "x86_64-pc-windows-msvc"),
        ] {
            assert!(distribution_target(host, true, Some(target.into())).is_err());
        }
    }

    #[test]
    fn macos_archives_reject_the_wrong_architecture_and_multi_slice_binaries() {
        for (target, expected, other) in [
            ("aarch64-apple-darwin", "arm64", "x86_64"),
            ("x86_64-apple-darwin", "x86_64", "arm64"),
        ] {
            verify_macos_architecture(target, expected).unwrap();
            for actual in ["", other, "arm64 x86_64", "x86_64 arm64"] {
                assert!(
                    verify_macos_architecture(target, actual).is_err(),
                    "{target}: {actual}"
                );
            }
        }
        assert!(verify_macos_architecture("x86_64-unknown-linux-gnu", "x86_64").is_err());
    }

    #[test]
    fn targets_reject_old_windows_toolchain_and_unknown_platforms() {
        for target in [
            "aarch64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
        ] {
            validate_target(target).unwrap();
        }
        for target in [
            "aarch64-pc-windows-gnullvm",
            "x86_64-unknown-freebsd",
            "riscv64-unknown-linux-gnu",
        ] {
            assert!(validate_target(target).is_err());
        }
        let plist = plist(native_bundle_version(VERSION).unwrap());
        assert!(!plist.contains("NSServices"));
        assert!(plist.contains("CFBundleDocumentTypes"));
        assert!(plist.contains("com.apple.mail.email"));
        assert!(plist.contains("public.folder"));
        assert_eq!(
            plist
                .matches("<key>LSHandlerRank</key><string>Alternate</string>")
                .count(),
            2
        );
        assert_eq!(
            plist
                .matches("<key>CFBundleTypeRole</key><string>Viewer</string>")
                .count(),
            2
        );
        assert!(plist.contains("<string>EMLAttachmentRemover</string>"));
    }

    #[test]
    fn native_build_mapping_preserves_version_order_and_rejects_collisions() {
        assert_eq!(native_bundle_version("7.0.0").unwrap(), 70_000);
        assert_eq!(native_bundle_version("7.0.1").unwrap(), 70_001);
        assert_eq!(native_bundle_version("7.99.99").unwrap(), 79_999);
        assert_eq!(native_bundle_version("8.0.0").unwrap(), 80_000);
        for version in [
            "7.100.0",
            "7.0.100",
            "7.0",
            "07.0.0",
            "7.0.0-rc.1",
            "7.0.0+source",
            "429496730.0.0",
        ] {
            assert!(native_bundle_version(version).is_err(), "{version}");
        }
    }

    #[test]
    fn cargo_config_target_directory_is_used_for_artifact_lookup() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".cargo")).unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::create_dir(root.path().join("artifacts")).unwrap();
        fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"artifact-lookup\"\nversion = \"0.0.0\"\n[workspace]\n",
        )
        .unwrap();
        fs::write(
            root.path().join(".cargo/config.toml"),
            "[build]\ntarget-dir = \"artifacts\"\n",
        )
        .unwrap();
        assert_eq!(
            target_directory(
                Command::new("cargo")
                    .current_dir(root.path())
                    .env_remove("CARGO_TARGET_DIR")
                    .env_remove("CARGO_BUILD_TARGET_DIR")
            )
            .unwrap()
            .canonicalize()
            .unwrap(),
            root.path().join("artifacts").canonicalize().unwrap()
        );
    }
}
