//! Independent desktop fixtures and exact native helper compilation.
use super::*;

pub(super) fn prepare_case_fixture(fixture: &Path, case: DesktopCase) -> Result<()> {
    let original = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    for name in ["one.eml", "two.eml"] {
        fs::write(fixture.join(name), original)?;
    }
    for (directory, name) in [
        ("folder-input", "folder.eml"),
        ("service-input", "日本語-مرحبا-\u{202e}report.eml"),
    ] {
        let directory = fixture.join(directory);
        fs::create_dir(&directory)?;
        fs::write(directory.join(name), original)?;
    }
    fs::write(
        fixture.join("report-collision.json"),
        b"Owned report collision sentinel\n",
    )?;
    if case == DesktopCase::Workflow {
        fs::create_dir(fixture.join("chosen-destination"))?;
    }
    if case == DesktopCase::ExportWriterBlock {
        let directory = fixture.join("writer-input");
        fs::create_dir(&directory)?;
        for index in 0..1024 {
            fs::write(directory.join(format!("{index:04}.eml")), original)?;
        }
    }
    if case == DesktopCase::Pagination {
        let directory = fixture.join("pagination-input");
        fs::create_dir(&directory)?;
        // This hand-authored MIME oracle is independent of Core serialization.
        let prefix = "From: synthetic@example.test\r\nSubject: Pagination\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=pages\r\n\r\n--pages\r\nContent-Type: text/plain\r\n\r\nOwned pagination body.\r\n";
        let mut message = prefix.to_owned();
        for index in 0..65 {
            message.push_str(&format!("--pages\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"part-{index:03}.bin\"\r\n\r\nOwned attachment {index}.\r\n"));
        }
        message.push_str("--pages--\r\n");
        fs::write(
            fixture.join("pagination-golden.eml"),
            format!("{prefix}--pages--\r\n"),
        )?;
        for index in 0..65 {
            fs::write(
                directory.join(format!("{index:03}.eml")),
                if index == 0 {
                    message.as_bytes()
                } else {
                    original
                },
            )?;
        }
    }
    Ok(())
}
pub(super) fn prepare_case_input(
    root: &Path,
    app: &Path,
    fixture: &Path,
    case: DesktopCase,
) -> Result<std::path::PathBuf> {
    let input = root.join("target").join(if case == DesktopCase::Workflow {
        "desktop-interaction".to_owned()
    } else {
        format!("desktop-interaction-{}", case.name())
    });
    fs::create_dir(&input)?;
    let mut invocation =
        json!({"app":app,"fixture":fixture,"host_scope":"fresh-github-hosted-runner"});
    if case.uses_native_pickers() {
        invocation["fixture_output"] = json!("true");
        invocation["scenario"] = json!(case.name());
    }
    if case == DesktopCase::ExportWriterBlock {
        invocation["writer_session"] = json!("true");
    }
    if case == DesktopCase::Appearance {
        invocation["appearance_session"] = json!("true");
    }
    if case == DesktopCase::Pagination {
        invocation["pagination_session"] = json!("true");
    }
    if case.is_controlled() {
        use base64::Engine;
        use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};
        let control = fixture.join("operation-control");
        fs::create_dir(&control)?;
        fs::set_permissions(&control, fs::Permissions::from_mode(0o700))?;
        if case.is_publication() {
            fs::write(
                control.join("observation"),
                if case == DesktopCase::PublishedError {
                    b"processing:after-publication:0\n".as_slice()
                } else {
                    b"processing:publication:1\n".as_slice()
                },
            )?;
        }
        let destination = fixture.join("publication-output");
        fs::create_dir(&destination)?;
        if case == DesktopCase::BlockedExportPreflight {
            for (lane, recipe) in [
                ("processing", "processing:preflight:0\n"),
                ("export", "export:preflight\n"),
            ] {
                let path = control.join(lane);
                fs::create_dir(&path)?;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                fs::write(path.join("observation"), recipe)?;
            }
        }
        if case == DesktopCase::BlockedProcessingReadback {
            fs::write(
                destination.join("two.ctpjzw7urreuwuroqgjcbuwt3m.no-attachments.eml"),
                include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml"),
            )?;
        }
        invocation["scenario"] = json!(case.name());
        invocation["child_control"] = json!("true");
        invocation["observation"] = json!(control);
        invocation["destination"] = json!(
            base64::engine::general_purpose::STANDARD.encode(destination.as_os_str().as_bytes())
        );
    }
    fs::write(
        input.join("input.json"),
        serde_json::to_vec_pretty(&invocation)?,
    )?;
    Ok(input)
}
pub(super) fn compile_preference_observers(
    root: &Path,
    build: &MacBuild,
    out: &Path,
) -> Result<std::path::PathBuf> {
    let observer = out.join("DesktopPreferenceSnapshot");
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
        .arg(root.join("tests/macos/DesktopPreferenceSnapshot.swift"))
        .arg("-o")
        .arg(&observer);
    crate::macos::checked_run(compile, &out.join("preference-observer-build.log"), 120)?;
    let observation_tests = out.join("DesktopPreferenceSnapshotTests");
    let mut compile = build.command(root, &build.swiftc);
    compile
        .args([
            "-parse-as-library",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-D",
            "PREFERENCE_OBSERVATION_TESTS",
            "-target",
            &build.swift_target(),
            "-sdk",
        ])
        .arg(&build.sdk)
        .arg(root.join("tests/macos/DesktopPreferenceSnapshot.swift"))
        .arg(root.join("tests/macos/DesktopPreferenceSnapshotTests.swift"))
        .arg("-o")
        .arg(&observation_tests);
    crate::macos::checked_run(
        compile,
        &out.join("preference-observation-tests-build.log"),
        120,
    )?;
    crate::macos::checked_run(
        build.command(root, &observation_tests),
        &out.join("preference-observation-tests.log"),
        30,
    )?;
    Ok(observer)
}
pub(super) fn compile_interaction_owner(
    root: &Path,
    build: &MacBuild,
    out: &Path,
    case: DesktopCase,
) -> Result<std::path::PathBuf> {
    let owner = out.join("DesktopInteractionOwner");
    if case.uses_native_pickers() {
        let tests = out.join("DesktopFixtureTests");
        let mut compile = build.command(root, &build.swiftc);
        compile
            .args([
                "-parse-as-library",
                "-swift-version",
                "6",
                "-warnings-as-errors",
                "-D",
                "DESKTOP_FIXTURE_TESTS",
                "-target",
                &build.swift_target(),
                "-sdk",
            ])
            .arg(&build.sdk)
            .arg(root.join("tests/macos/DesktopInteractionOwner.swift"))
            .arg(root.join("tests/macos/DesktopFixtureOutput.swift"))
            .arg(root.join("tests/macos/DesktopFixtureTests.swift"))
            .arg("-o")
            .arg(&tests);
        crate::macos::checked_run(compile, &out.join("desktop-fixture-tests-build.log"), 120)?;
        crate::macos::checked_run(
            build.command(root, &tests),
            &out.join("desktop-fixture-tests.log"),
            30,
        )?;
    }
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
        .arg(root.join("tests/macos/DesktopInteractionOwner.swift"))
        .arg(root.join("tests/macos/DesktopFixtureOutput.swift"))
        .arg("-o")
        .arg(&owner);
    crate::macos::checked_run(compile, &out.join("interaction-owner-build.log"), 120)?;
    Ok(owner)
}
