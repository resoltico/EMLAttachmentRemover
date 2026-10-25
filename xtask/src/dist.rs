use crate::{Result, archive, host, notices, run};
use eml_core::product::{BUILD_NUMBER, CLI_NAME, VERSION};
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
        vec![target.unwrap_or(host)]
    };
    let include_cli = cli || !app;
    let stamp = archive::Timestamp::commit()?;
    fs::create_dir_all(out)?;
    let scratch = tempfile::tempdir()?;
    let binaries = scratch.path().join("binaries");
    fs::create_dir(&binaries)?;
    for target in &targets {
        validate_target(target)?;
        let mut command = Command::new("cargo");
        command.args([
            "build",
            "--release",
            "--locked",
            "--target",
            target,
            "-p",
            "emlattachmentremover",
            "--bin",
            "emlattachmentremover",
        ]);
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
    let cargo_target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    for name in if app {
        vec![executable.as_str(), "emlattachmentremover-gui"]
    } else {
        vec![executable.as_str()]
    } {
        let input = |target: &str| cargo_target.join(target).join("release").join(name);
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
    if app {
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
        fs::copy("artwork/EML.icns", contents.join("Resources/EML.icns"))?;
        fs::copy("LICENSE", contents.join("Resources/LICENSE"))?;
        notices::write(
            true,
            &targets,
            &contents.join("Resources/THIRD-PARTY-NOTICES.txt"),
        )?;
        fs::write(contents.join("Info.plist"), plist())?;
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
fn plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>io.github.resoltico.emlattachmentremover</string>
<key>CFBundleName</key><string>EML Attachment Remover</string>
<key>CFBundleDisplayName</key><string>EML Attachment Remover</string>
<key>CFBundleExecutable</key><string>EMLAttachmentRemover</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{VERSION}</string>
<key>CFBundleVersion</key><string>{BUILD_NUMBER}</string>
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
        let plist = plist();
        assert!(!plist.contains("NSServices"));
        assert!(!plist.contains("CFBundleDocumentTypes"));
        assert!(plist.contains("<string>EMLAttachmentRemover</string>"));
    }
}
