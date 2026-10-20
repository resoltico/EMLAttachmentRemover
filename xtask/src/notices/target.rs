//! Target/component notice selection and complete material assembly.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeComponent {
    Cli,
    Gui,
    Worker,
}
impl NoticeComponent {
    pub(super) fn applies_to(self, platform: &str) -> bool {
        self == Self::Cli || platform.starts_with("macos-")
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Gui => "gui",
            Self::Worker => "worker",
        }
    }
    pub(super) fn package(self) -> &'static str {
        match self {
            Self::Cli => "remove-eml-attachments",
            Self::Gui => "eml-gui",
            Self::Worker => "eml-worker",
        }
    }
    pub(super) fn manifest(self) -> &'static str {
        match self {
            Self::Cli => "crates/eml-cli/Cargo.toml",
            Self::Gui => "crates/eml-gui/Cargo.toml",
            Self::Worker => "crates/eml-worker/Cargo.toml",
        }
    }
    pub(super) fn config(self) -> &'static str {
        if self == Self::Gui {
            "xtask/app-notices.toml"
        } else {
            "xtask/notices.toml"
        }
    }
    pub(super) fn prefix(self, platform: &str) -> String {
        if self == Self::Cli {
            format!("{platform}/")
        } else {
            format!("{}/{platform}/", self.name())
        }
    }
}

pub(super) struct NoticeTarget<'a> {
    pub(super) target: &'a str,
    pub(super) platform: &'a str,
    pub(super) version: &'a str,
    pub(super) component: NoticeComponent,
}

pub(super) fn target_materials(
    root: &Path,
    selection: NoticeTarget<'_>,
    review: &Review,
    lock: &Lock,
    compiler: &str,
    config: &[u8],
) -> Result<Files> {
    let NoticeTarget {
        target,
        platform,
        version,
        component,
    } = selection;
    let mut arguments = vec![
        "about",
        "generate",
        "--locked",
        "--manifest-path",
        component.manifest(),
        "--target",
        target,
        "--config",
        component.config(),
        "--format",
        "json",
        "--fail",
    ];
    if component == NoticeComponent::Gui {
        arguments.extend(["--features", "macos-gui"]);
    }
    let bytes = capture(crate::tools::command(root, "about")?.args(&arguments[1..]))?;
    let report: About = serde_json::from_slice(&bytes)?;
    let (mut files, inventory) = render(
        root,
        &format!("{}@{target}", component.name()),
        &report,
        review,
        lock,
    )?;
    let (features_arguments, features) = activated_features(root, target, component, &report)?;
    insert(
        &mut files,
        "licenses/activated-features.json".into(),
        serde_json::to_vec_pretty(&features)?,
    )?;
    let (rust, channel) = standard_library_materials(root, compiler, &mut files)?;
    let windows = windows_materials(root, platform, &channel, &mut files)?;
    let notice = notice_text(
        NoticeTarget {
            target,
            platform,
            version,
            component,
        },
        &inventory,
        windows.is_some(),
        &files,
    )?;
    insert(
        &mut files,
        "THIRD-PARTY-NOTICES.txt".into(),
        notice.into_bytes(),
    )?;
    insert(
        &mut files,
        "licenses/materials.json".into(),
        serde_json::to_vec_pretty(&json!({
            "version":version,"target":target,"platform":platform,"component":component.name(),
            "scope":"legal-material generation only; no native execution, archive or final-link qualification",
            "generation":{"tool":review.generator_pin,"arguments":arguments,"config_sha256":digest(config),
            "feature_arguments":features_arguments,"activated_features":"licenses/activated-features.json",
                "compiler_version":channel,"compiler_commit":rust["commit"],
                "execution_host":"recorded separately by Controller/qualification; notice generation is not native execution evidence"},
            "packages":inventory,"rust":rust,"windows_runtime":windows
        }))?,
    )?;
    Ok(files)
}

fn activated_features<'a>(
    root: &Path,
    target: &'a str,
    component: NoticeComponent,
    report: &About,
) -> Result<(Vec<&'a str>, Value)> {
    let mut features_arguments = vec![
        "tree",
        "--locked",
        "--package",
        component.package(),
        "--target",
        target,
        "--edges",
        "normal,build",
        "--prefix",
        "depth",
        "--format",
        "{p}|{f}",
    ];
    if component == NoticeComponent::Gui {
        features_arguments.extend(["--features", "eml-gui/macos-gui"]);
    }
    let features = String::from_utf8(capture(
        Command::new("cargo")
            .current_dir(root)
            .args(&features_arguments),
    )?)?;
    let packages = report
        .crates
        .iter()
        .map(|entry| (entry.package.name.clone(), entry.package.version.clone()))
        .collect();
    let features = feature_tree(&features, &packages)?;
    Ok((features_arguments, features))
}
fn standard_library_materials(
    root: &Path,
    compiler: &str,
    files: &mut Files,
) -> Result<(Value, String)> {
    let rust = bundle(
        root,
        "legal/rust",
        "licenses/rust",
        &[
            "COPYRIGHT-library.html",
            "MIT.txt",
            "Apache-2.0.txt",
            "BSD-2-Clause.txt",
            "Unicode-3.0.txt",
        ],
        files,
    )?;
    let toolchain: toml::Value = toml::from_str(&String::from_utf8(read(
        &root.join("rust-toolchain.toml"),
        MAX_LEGAL_BYTES,
    )?)?)?;
    let channel = toolchain["toolchain"]["channel"]
        .as_str()
        .ok_or("Missing compiler authority")?;
    if rust["version"] != channel
        || !compiler
            .lines()
            .any(|line| line == format!("release: {channel}"))
        || !compiler
            .lines()
            .any(|line| line == format!("commit-hash: {}", rust["commit"].as_str().unwrap_or("")))
    {
        return Err(
            "Actual compiler differs from retained standard-library material identity".into(),
        );
    }
    Ok((rust, channel.to_owned()))
}
fn windows_materials(
    root: &Path,
    platform: &str,
    channel: &str,
    files: &mut Files,
) -> Result<Option<Value>> {
    let windows = if platform.starts_with("windows-") {
        let material = bundle(
            root,
            "legal/windows-runtime",
            "licenses/windows-runtime",
            &[
                "mingw-COPYING",
                "mingw-COPYING-runtime.txt",
                "mingw-DISCLAIMER.PD",
                "llvm-libunwind-LICENSE.TXT",
                "mingw-pseudo-reloc-notice.txt",
                "mingw-complex-internal-notice.txt",
            ],
            files,
        )?;
        if material["producer"]["rust"] != channel {
            return Err("Native runtime legal inputs differ from pinned Rust".into());
        }
        Some(material)
    } else {
        None
    };
    Ok(windows)
}
fn notice_text(
    selection: NoticeTarget<'_>,
    inventory: &[Value],
    has_windows: bool,
    files: &Files,
) -> Result<String> {
    let NoticeTarget {
        target,
        platform,
        version,
        component,
    } = selection;
    let mut notice = format!(
        "EML Attachment Remover {version}\nProduct: {}\nTarget: {target} ({platform})\n\n",
        component.name()
    );
    notice.push_str("These materials cover the target-selected locked component normal/build dependency graph. Build and proc-macro materials are included conservatively; this list does not assert that every listed crate is linked application code.\n\n");
    notice.push_str("Full Rust standard-library copyright, nested exceptions and source pointers: licenses/rust/COPYRIGHT-library.html and licenses/rust/source.json. Associated complete license texts are in that directory.\n");
    if has_windows {
        notice.push_str("Windows runtime distribution legal materials: licenses/windows-runtime/. The final executable's selected native inputs require a separate link-bound review; this material bundle does not approve that trace.\n");
    }
    notice.push_str("\nComplete selected crate texts and source-specific notices:\n");
    for item in inventory {
        notice.push_str(&format!(
            "\n{}@{} — {}\n  {}\n",
            item["name"].as_str().unwrap_or(""),
            item["version"].as_str().unwrap_or(""),
            item["license_id"].as_str().unwrap_or(""),
            item["file"].as_str().unwrap_or("")
        ));
        if let Some(additional) = item["additional_material_files"].as_array() {
            for file in additional {
                notice.push_str(&format!(
                    "  Additional source/notice material: {}\n",
                    file.as_str().ok_or("Invalid accompanying material path")?
                ));
            }
        }
    }
    if inventory
        .iter()
        .any(|item| item["review_mapping"] == "identified_standard_text_with_source_declarations")
    {
        notice.push_str("\nIdentified standard license templates are not upstream copyright attributions. The corresponding complete source declarations and notices are retained separately above; no missing copyright year or owner was invented.\n");
    }
    if files.contains_key("licenses/objc2/LICENSE.md") {
        notice.push_str("\nobjc2-family upstream licensing explanation and exact revisions: licenses/objc2/LICENSE.md and source.json. The identified standard MIT template is included separately; its copyright placeholders are not an upstream attribution. The upstream Apple SDK discussion is retained unchanged.\n");
    }
    Ok(notice)
}
