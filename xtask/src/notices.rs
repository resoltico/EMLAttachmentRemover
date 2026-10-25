use crate::{Result, capture, tool};
use std::{fs, io::Write, path::Path, process::Command};

pub(crate) fn write(app: bool, targets: &[String], out: &Path) -> Result<()> {
    tool("cargo-about", "cargo_about")?;
    // The GUI's shipped normal/build graph uses the royalty-free Slint license.
    // cargo-about cannot supply that LicenseRef text; the upstream supplement is mandatory.
    let supplement = if app {
        let text = fs::read("legal/slint/LicenseRef-Slint-Royalty-free-2.0.md")
            .map_err(|error| format!("Required Slint license supplement legal/slint/LicenseRef-Slint-Royalty-free-2.0.md: {error}"))?;
        if !text
            .windows(b"Slint Royalty-free".len())
            .any(|s| s == b"Slint Royalty-free")
        {
            return Err("Slint license supplement is empty or invalid".into());
        }
        Some(text)
    } else {
        None
    };
    let mut texts = Vec::new();
    for package in if app {
        &["eml-cli", "eml-gui"][..]
    } else {
        &["eml-cli"][..]
    } {
        let mut command = Command::new("cargo");
        command
            .args([
                "about",
                "generate",
                "--fail",
                "--locked",
                "--config",
                "legal/about.toml",
                "--manifest-path",
            ])
            .arg(format!("crates/{package}/Cargo.toml"));
        for target in targets {
            command.args(["--target", target]);
        }
        if *package == "eml-gui" {
            command.args([
                "--no-default-features",
                "--features",
                "desktop renderer-software",
            ]);
        }
        command.arg("legal/about.hbs");
        // Keep cargo-about's diagnostics visible, including incomplete license sources.
        let result = command.stderr(std::process::Stdio::inherit()).output()?;
        if !result.status.success() {
            return Err(format!("cargo-about failed: {}", result.status).into());
        }
        if result.stdout.is_empty() {
            return Err("cargo-about produced no notices".into());
        }
        texts.push(result.stdout);
    }
    if let Some(text) = supplement {
        texts.push(text);
    }
    if app {
        // These upstream attribution files are absent from the registry packages
        // or cargo-about's selected text. Keep their complete terms with the app.
        for path in [
            "legal/upstream/accesskit/LICENSE-MIT-23f18e03dc49.txt",
            "legal/upstream/accesskit/LICENSE-APACHE-62c7a1e35f56.txt",
            "legal/upstream/objc2/LICENSE-e353f37b12ae.txt",
            "legal/objc2/LICENSE.md",
            "legal/upstream/lyon/LICENSE-MIT-80f275e90d79.txt",
            "legal/upstream/rspolib/LICENSE-4fe9b5fce8fd.txt",
            "legal/upstream/taffy/LICENSE-a7083ef3dd41.md",
        ] {
            let text = fs::read(path)
                .map_err(|error| format!("Required upstream license material {path}: {error}"))?;
            if text.is_empty() {
                return Err(format!("missing legal material: {path}").into());
            }
            texts.push(format!("Upstream legal material: {path}\n\n").into_bytes());
            texts.push(text);
        }
    }
    let sysroot = capture(Command::new("rustc").args(["--print", "sysroot"]))?;
    texts.push(fs::read(
        Path::new(&sysroot).join("share/doc/rust/COPYRIGHT.html"),
    )?);
    // Do not replace an owner's output. Dist writes into a fresh temporary stage.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    for text in texts {
        file.write_all(&text)?;
        file.write_all(b"\n\n")?;
    }
    file.sync_all()?;
    Ok(())
}
