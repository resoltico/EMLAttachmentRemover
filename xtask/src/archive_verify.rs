//! Native inspection and actual behavior of extracted CLI archive bytes.
use crate::{Result, capture, controller, notices, smoke};
use object::{Object, ObjectKind};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command, time::Duration};
use xtask::archive::ArchiveSnapshot;

fn binary_identity(bytes: &[u8], os: &str, architecture: &str, minimum: &str) -> Result<Value> {
    let file = object::File::parse(bytes)?;
    let arch = match file.architecture() {
        object::Architecture::Aarch64 => "aarch64",
        object::Architecture::X86_64 => "x86_64",
        _ => return Err("Unsupported archive binary CPU".into()),
    };
    if arch != architecture || !file.is_64() {
        return Err("Archive executable has the wrong CPU/word size".into());
    }
    let format = match (&file, os) {
        (object::File::MachO64(_), "macos") if file.kind() == ObjectKind::Executable => {
            "thin-mach-o64"
        }
        (object::File::Elf64(_), "linux")
            if matches!(file.kind(), ObjectKind::Executable | ObjectKind::Dynamic) =>
        {
            "elf64"
        }
        (object::File::Pe64(_), "windows") if file.kind() == ObjectKind::Executable => "pe64",
        _ => return Err("Archive binary type does not match its native platform".into()),
    };
    #[cfg(target_os = "macos")]
    if os == "macos" {
        let actual = crate::macos::macho_identity(bytes)?;
        let parts: Vec<u32> = minimum
            .split('.')
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()?;
        if parts.len() > 3
            || parts.is_empty()
            || parts[0] > 65535
            || parts.iter().skip(1).any(|part| *part > 255)
        {
            return Err("Invalid macOS deployment policy".into());
        }
        let packed = parts[0] << 16
            | parts.get(1).copied().unwrap_or(0) << 8
            | parts.get(2).copied().unwrap_or(0);
        if actual.minimum != packed {
            return Err("Archive Mach-O deployment minimum differs from policy".into());
        }
        return Ok(
            json!({"format":format,"architecture":arch,"minimum":actual.minimum,"sdk":actual.sdk}),
        );
    }
    let _ = minimum;
    Ok(json!({"format":format,"architecture":arch}))
}

fn glibc_version(value: &str) -> Result<(u16, u16, u16)> {
    let values: Vec<u16> = value
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    if !(2..=3).contains(&values.len()) {
        return Err("Unreviewed GLIBC version spelling".into());
    }
    Ok((values[0], values[1], values.get(2).copied().unwrap_or(0)))
}

pub(crate) fn runtime(
    root: &Path,
    executable: &Path,
    os: &str,
    architecture: &str,
    maximum_glibc: &str,
    logs: &Path,
) -> Result<Value> {
    let run = |program: &str, args: &[&str], label: &str| -> Result<String> {
        let mut command = Command::new(program);
        command
            .current_dir(root)
            .env("LC_ALL", "C")
            .args(args)
            .arg(executable);
        let log = logs.join(format!("{label}.log"));
        let result = controller()?.run_separate_output(command, &log, Duration::from_secs(30))?;
        if !result.success() {
            return Err(format!("Native binary inspection failed; see {}", log.display()).into());
        }
        if !label.starts_with("signature") && fs::metadata(log.with_extension("stderr"))?.len() != 0
        {
            return Err(format!(
                "Native binary inspector emitted a warning/diagnostic; see {}",
                log.with_extension("stderr").display()
            )
            .into());
        }
        if fs::metadata(&log)?.len() > 4 * 1024 * 1024 {
            return Err("Native inspection output exceeds its document bound".into());
        }
        let bytes = fs::read(&log)?;
        Ok(String::from_utf8(bytes)?)
    };
    if os == "macos" {
        run(
            "/usr/bin/codesign",
            &["--verify", "--strict", "--verbose=2"],
            "signature",
        )?;
        run(
            "/usr/bin/codesign",
            &["--display", "--verbose=4"],
            "signature-details",
        )?;
        let signature_path = logs.join("signature-details.stderr");
        if fs::metadata(&signature_path)?.len() > 4 * 1024 * 1024 {
            return Err("Signature detail output exceeds its bound".into());
        }
        let signature = fs::read_to_string(signature_path)?;
        if !signature.lines().any(|line| line == "Signature=adhoc") {
            return Err("Archive executable signature is not the declared ad hoc form".into());
        }
        #[cfg(target_os = "macos")]
        let dependencies = crate::macos::inspect_system_dependencies(
            Command::new("/usr/bin/otool"),
            executable,
            &logs.join("dependencies.log"),
        )?;
        #[cfg(not(target_os = "macos"))]
        return Err("Mach-O dependency inspection requires native macOS".into());
        #[cfg(target_os = "macos")]
        return Ok(
            json!({"external_system_dependencies":dependencies,"adhoc_signature_verified":true}),
        );
    }
    if os == "linux" {
        let dynamic = run("readelf", &["--dynamic", "--wide"], "dynamic")?;
        if dynamic
            .lines()
            .any(|line| line.contains("(RPATH)") || line.contains("(RUNPATH)"))
        {
            return Err("Unreviewed embedded ELF runtime search path".into());
        }
        let mut dependencies = BTreeSet::new();
        for line in dynamic.lines().filter(|line| line.contains("(NEEDED)")) {
            let name = line
                .split_once('[')
                .and_then(|(_, tail)| tail.split_once(']'))
                .map(|(name, _)| name)
                .ok_or("Malformed ELF dependency record")?;
            if !matches!(
                name,
                "libc.so.6"
                    | "libgcc_s.so.1"
                    | "libpthread.so.0"
                    | "libdl.so.2"
                    | "libm.so.6"
                    | "librt.so.1"
                    | "ld-linux-x86-64.so.2"
                    | "ld-linux-aarch64.so.1"
            ) {
                return Err("Unreviewed non-system ELF runtime dependency".into());
            }
            dependencies.insert(name.to_owned());
        }
        if dependencies.is_empty() {
            return Err("ELF dependency discovery unexpectedly empty".into());
        }
        let headers = run(
            "readelf",
            &["--program-headers", "--wide"],
            "program-headers",
        )?;
        let expected = if architecture == "x86_64" {
            "/lib64/ld-linux-x86-64.so.2"
        } else {
            "/lib/ld-linux-aarch64.so.1"
        };
        let interpreters: Vec<_> = headers
            .lines()
            .filter_map(|line| {
                line.split_once("[Requesting program interpreter: ")
                    .and_then(|(_, tail)| tail.strip_suffix(']'))
            })
            .collect();
        if interpreters != [expected] {
            return Err("ELF interpreter differs from the reviewed GNU platform".into());
        }
        let versions = run("readelf", &["--version-info", "--wide"], "symbol-versions")?;
        let maximum = glibc_version(maximum_glibc)?;
        let mut required = BTreeSet::new();
        for token in versions.split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.'))
        }) {
            if let Some(version) = token.strip_prefix("GLIBC_") {
                if glibc_version(version)? > maximum {
                    return Err("ELF GLIBC symbol requirement exceeds declared floor".into());
                }
                required.insert(version.to_owned());
            }
        }
        if required.is_empty() {
            return Err("ELF GLIBC version discovery unexpectedly empty".into());
        }
        return Ok(
            json!({"external_system_dependencies":dependencies,"interpreter":expected,"required_glibc_versions":required,"maximum_policy":maximum_glibc}),
        );
    }
    if os == "windows" {
        let imports = crate::windows_runtime::inspect(&fs::read(executable)?, architecture)?;
        fs::write(
            logs.join("pe-imports.json"),
            serde_json::to_vec_pretty(&imports)?,
        )?;
        #[cfg(windows)]
        {
            let evidence = fs::read(
                executable
                    .parent()
                    .ok_or("Missing extracted CLI directory")?
                    .join(crate::windows_evidence::MATERIAL),
            )?;
            let target = format!("{architecture}-pc-windows-gnullvm");
            crate::windows_evidence::verify(
                root,
                &target,
                &crate::git(root, &["rev-parse", "HEAD"])?,
                &fs::read(executable)?,
                &evidence,
            )?;
            return Ok(
                json!({"pe":imports,"static_input_evidence":"reclassified against repository policy and bound to extracted executable; native qualification execution remains separate"}),
            );
        }
        #[cfg(not(windows))]
        return Err("Windows final-link verification requires its native platform".into());
    }
    Err("Unsupported runtime inspection platform".into())
}

pub(crate) fn run(root: &Path, archive: &Path, target: &str) -> Result<()> {
    verify(root, archive, target, None).map(|_| ())
}

pub(crate) fn expected(
    root: &Path,
    archive: &Path,
    target: &str,
    expected_binary: &xtask::archive::ByteIdentity,
) -> Result<xtask::archive::ByteIdentity> {
    verify(root, archive, target, Some(expected_binary))
}

fn verify(
    root: &Path,
    archive: &Path,
    target: &str,
    expected_binary: Option<&xtask::archive::ByteIdentity>,
) -> Result<xtask::archive::ByteIdentity> {
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let row = plan["native_targets"]
        .as_array()
        .ok_or("Missing native target table")?
        .iter()
        .find(|row| row["target"] == target)
        .ok_or("Unsupported archive target")?;
    let platform = row["platform"]
        .as_str()
        .ok_or("Missing platform identity")?;
    let os = row["os"].as_str().ok_or("Missing platform family")?;
    let architecture = row["architecture"].as_str().ok_or("Missing architecture")?;
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Archive basename must match a declared artifact")?;
    let artifact = plan["expected_artifacts"]
        .as_array()
        .ok_or("Missing archive identities")?
        .iter()
        .find(|artifact| artifact["name"] == name && artifact["platform"] == platform)
        .ok_or("Archive basename/version/target does not match the declared artifact set")?;
    if artifact["role"] != "cli" {
        #[cfg(target_os = "macos")]
        {
            let reference = archive.with_file_name(format!("{name}.reference.json"));
            crate::macos_archive::smoke(root, archive, &reference)?;
            return Ok(ArchiveSnapshot::read(archive)?.identity());
        }
        #[cfg(not(target_os = "macos"))]
        return Err(
            "App archive verification requires native macOS and its frozen producer reference"
                .into(),
        );
    }
    let host = crate::native_host::inspect(root, target, os, architecture)?;
    let work_parent = root.join("target/archive-verification");
    fs::create_dir_all(&work_parent)?;
    let work = tempfile::Builder::new()
        .prefix("native-")
        .tempdir_in(&work_parent)?
        .keep();
    eprintln!("Retained native archive verification: {}", work.display());
    let mut snapshot_identity = None;
    let result: Result<xtask::archive::ByteIdentity> = (|| {
        let snapshot = ArchiveSnapshot::read(archive)?;
        snapshot_identity = Some(snapshot.identity());
        fs::write(
            work.join("input.json"),
            serde_json::to_vec_pretty(
                &json!({"archive_basename":name,"snapshot":snapshot_identity,"target":target}),
            )?,
        )?;
        let checked = notices::checked_target(root, platform)?;
        let mut materials = checked.files().clone();
        let version = plan["version"]
            .as_str()
            .ok_or("Missing Cargo-derived version")?;
        let root_name = artifact["root"].as_str().ok_or("Missing archive root")?;
        let executable = artifact["executable"]
            .as_str()
            .ok_or("Missing archive executable")?;
        let readme = cli_readme(
            version,
            row["platform_text"]
                .as_str()
                .ok_or("Missing archive platform text")?,
        );
        materials.insert("README.txt".into(), readme.into_bytes());
        #[cfg(windows)]
        if os == "windows" {
            let evidence = snapshot.read_cli_zip_material(
                root_name,
                crate::windows_evidence::MATERIAL,
                crate::windows_evidence::MAX_BYTES,
            )?;
            materials.insert(crate::windows_evidence::MATERIAL.into(), evidence);
        }
        let extracted = if os == "linux" {
            snapshot.extract_cli_tar_gz(root_name, executable, &materials, &work)?
        } else {
            snapshot.extract_cli_zip(root_name, executable, &materials, &work)?
        };
        let executable = extracted.root().join(executable);
        let executable_bytes = fs::read(&executable)?;
        let actual_binary = xtask::archive::ByteIdentity::of(&executable_bytes);
        if expected_binary.is_some_and(|expected| expected != &actual_binary) {
            return Err("Extracted CLI bytes differ from the frozen signed build artifact".into());
        }
        let binary = binary_identity(
            &executable_bytes,
            os,
            architecture,
            plan["policy"]["macos_minimum"]
                .as_str()
                .ok_or("Missing deployment policy")?,
        )?;
        let runtime = runtime(
            root,
            &executable,
            os,
            architecture,
            plan["policy"]["linux_glibc_maximum"]
                .as_str()
                .ok_or("Missing ABI policy")?,
            &work,
        )?;
        let behavior = smoke::run(
            root,
            &executable,
            artifact["executable"]
                .as_str()
                .unwrap()
                .trim_end_matches(".exe"),
            version,
            extracted
                .root()
                .parent()
                .ok_or("Missing retained enclosure")?,
            &work.join("behavior"),
        )?;
        fs::write(
            work.join("result.json"),
            serde_json::to_vec_pretty(
                &json!({"archive":name,"snapshot":snapshot_identity,"target":target,"host":host,"binary":binary,"binary_bytes":actual_binary,"matched_frozen_build":expected_binary.is_some(),"runtime":runtime,"behavior":behavior,
            "scope":"native CLI archive structural/material/runtime/fixture behavior only; full source/platform/app qualification is separate"}),
            )?,
        )?;
        Ok(snapshot.identity())
    })();
    match result {
        Ok(identity) => Ok(identity),
        Err(error) => {
            let evidence = serde_json::to_vec_pretty(
                &json!({"error":error.to_string(),"snapshot":snapshot_identity,"target":target}),
            )?;
            if let Err(logging) = fs::write(work.join("failure.json"), evidence) {
                return Err(format!("Archive verification failed: {error}; evidence write also failed: {logging}; retained {}",work.display()).into());
            }
            Err(error)
        }
    }
}

pub(crate) fn cli_readme(version: &str, platform_text: &str) -> String {
    format!(
        "EML Attachment Remover {version}\n\n{platform_text}\n\nRun ./remove-eml-attachments --help (remove-eml-attachments.exe on Windows).\nCreate copies: remove-eml-attachments PATH...\nInspect without writes: remove-eml-attachments --dry-run PATH...\nOriginal messages remain unchanged; existing different copies are never replaced.\n\nSource and documentation: https://github.com/resoltico/EMLAttachmentRemover\nSoftware/artwork terms: LICENSE. Complete third-party materials: THIRD-PARTY-NOTICES.txt and licenses/.\nmacOS binaries are ad hoc signed, without Developer ID or notarization; first launch may require the system's Open Anyway choice.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glibc_floor_rejects_newer_or_unreviewed_symbol_spelling() {
        assert!(glibc_version("2.2.5").unwrap() < glibc_version("2.35").unwrap());
        assert!(glibc_version("2.36").unwrap() > glibc_version("2.35").unwrap());
        for invalid in ["PRIVATE", "ABI_DT_RELR", "2", "2.35.0.1", "2.x"] {
            assert!(glibc_version(invalid).is_err());
        }
    }

    #[test]
    fn actual_native_executable_header_passes_and_wrong_cpu_is_rejected_before_execution() {
        let bytes = fs::read(std::env::current_exe().unwrap()).unwrap();
        #[cfg(target_os = "macos")]
        let minimum = {
            let packed = crate::macos::macho_identity(&bytes).unwrap().minimum;
            format!("{}.{}.{}", packed >> 16, (packed >> 8) & 255, packed & 255)
        };
        #[cfg(not(target_os = "macos"))]
        let minimum = "15.0".to_owned();
        assert!(
            binary_identity(
                &bytes,
                std::env::consts::OS,
                std::env::consts::ARCH,
                &minimum
            )
            .is_ok()
        );
        let wrong = if std::env::consts::ARCH == "aarch64" {
            "x86_64"
        } else {
            "aarch64"
        };
        assert!(
            binary_identity(&bytes, std::env::consts::OS, wrong, &minimum)
                .unwrap_err()
                .to_string()
                .contains("wrong CPU")
        );
    }
}
