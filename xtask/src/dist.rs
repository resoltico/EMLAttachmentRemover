use crate::{Result, archive, host, notices, run};
use eml_core::product::{CLI_NAME, DISPLAY_NAME, VERSION};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn build(
    cli: bool,
    app: bool,
    universal: bool,
    target: Option<String>,
    out: &Path,
) -> Result<()> {
    let host = host()?;
    let mac = host.ends_with("apple-darwin");
    if (app || universal) && !mac {
        return Err("--app and --universal require a macOS build host".into());
    }
    if target.is_some() && (app || universal) {
        return Err("--target cannot be combined with --app or --universal".into());
    }
    let targets = if mac && (app || universal || target.is_none()) {
        vec![
            "aarch64-apple-darwin".to_owned(),
            "x86_64-apple-darwin".to_owned(),
        ]
    } else {
        vec![target.unwrap_or_else(|| host.clone())]
    };
    let include_cli = cli || !app;
    let revision = crate::source::revision()?;
    let stamp = archive::Timestamp::for_revision(&revision)?;
    let bundle_version = app.then(|| native_bundle_version(VERSION)).transpose()?;
    let cargo_target = target_directory(&mut Command::new("cargo"))?;
    fs::create_dir_all(out)?;
    let scratch = tempfile::tempdir()?;
    let binaries = scratch.path().join("binaries");
    fs::create_dir(&binaries)?;
    for target in &targets {
        validate_target(target)?;
        let mut command = Command::new("cargo");
        command.args([
            "build",
            "--profile",
            "dist",
            "--locked",
            "--target",
            target,
            "-p",
            "emlattachmentremover",
            "--bin",
            "emlattachmentremover",
        ]);
        command.env("EML_SOURCE_REVISION", &revision);
        if app {
            command.args([
                "-p",
                "eml-gui",
                "--bin",
                "emlattachmentremover-gui",
                "--features",
                "eml-gui/desktop",
            ]);
        }
        if mac {
            command.env("MACOSX_DEPLOYMENT_TARGET", "15.0");
        }
        run(&mut command)?;
    }
    let extension = if targets[0].ends_with("windows-msvc") {
        ".exe"
    } else {
        ""
    };
    let executable = format!("{CLI_NAME}{extension}");
    for name in if app {
        vec![executable.as_str(), "emlattachmentremover-gui"]
    } else {
        vec![executable.as_str()]
    } {
        let input = |target: &str| cargo_target.join(target).join("dist").join(name);
        if targets.len() == 2 {
            run(Command::new("lipo")
                .arg("-create")
                .args(targets.iter().map(|target| input(target)))
                .arg("-output")
                .arg(binaries.join(name)))?;
        } else {
            fs::copy(input(&targets[0]), binaries.join(name))?;
        }
    }
    if targets.contains(&host) {
        for (name, command_name) in if app {
            vec![
                (executable.as_str(), CLI_NAME),
                ("emlattachmentremover-gui", "emlattachmentremover-gui"),
            ]
        } else {
            vec![(executable.as_str(), CLI_NAME)]
        } {
            let actual = crate::capture(Command::new(binaries.join(name)).arg("--version"))?;
            let expected = format!("{command_name} {VERSION} ({revision})");
            if actual != expected {
                return Err(format!(
                    "Built {name} identity differs: expected {expected:?}, found {actual:?}"
                )
                .into());
            }
        }
    }
    let suffix = if targets.len() == 2 {
        "macos-universal".to_owned()
    } else {
        platform(&targets[0])?
    };
    if include_cli {
        let stage = scratch.path().join("cli");
        fs::create_dir(&stage)?;
        fs::copy(binaries.join(&executable), stage.join(&executable))?;
        fs::copy("LICENSE", stage.join("LICENSE"))?;
        notices::write(false, &targets, &stage.join("THIRD-PARTY-NOTICES.txt"))?;
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
        for directory in ["MacOS", "Helpers", "Resources"] {
            fs::create_dir_all(contents.join(directory))?;
        }
        let helper = contents.join("Helpers/emlattachmentremover");
        fs::copy(binaries.join(&executable), &helper)?;
        fs::copy(
            binaries.join("emlattachmentremover-gui"),
            contents.join("MacOS/EMLAttachmentRemover"),
        )?;
        fs::copy(
            "artwork/app-icon/EML.icns",
            contents.join("Resources/EML.icns"),
        )?;
        fs::copy("LICENSE", contents.join("Resources/LICENSE"))?;
        notices::write(
            true,
            &targets,
            &contents.join("Resources/THIRD-PARTY-NOTICES.txt"),
        )?;
        fs::write(contents.join("Info.plist"), plist(bundle_version))?;
        run(Command::new("codesign")
            .args(["--force", "-s", "-"])
            .arg(&helper))?;
        run(Command::new("codesign")
            .args(["--force", "-s", "-"])
            .arg(&bundle))?;
        run(Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle))?;
        archive::write(
            &stage,
            &out.join(format!("{CLI_NAME}-gui-{VERSION}-macos-universal.zip")),
            &stamp,
        )?;
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
</dict></plist>
"#
    )
}
#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(!plist.contains("CFBundleDocumentTypes"));
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
