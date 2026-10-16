//! Native release build/package orchestration; incomplete platform gates fail.
use crate::{Result, archive_verify, capture, cli_build, controller, notices};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn test_suites(stderr: &str) -> Result<std::collections::BTreeSet<String>> {
    let mut suites = std::collections::BTreeSet::new();
    for raw in stderr.lines() {
        // A test's stderr need not end in LF before Cargo prints its next marker.
        let line = ["Running ", "Doc-tests "]
            .into_iter()
            .filter_map(|marker| raw.find(marker).map(|index| &raw[index..]))
            .min_by_key(|line| std::cmp::Reverse(line.len()));
        let Some(line) = line else { continue };
        if !(line.starts_with("Running ") && line.contains(" (") && line.ends_with(')')
            || line.strip_prefix("Doc-tests ").is_some_and(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            }))
        {
            return Err("Malformed native Cargo test suite identity".into());
        }
        if !suites.insert(line.to_owned()) {
            return Err("Duplicate native Cargo test suite identity".into());
        }
    }
    if suites.is_empty() {
        return Err("Native Cargo test suite discovery is empty".into());
    }
    Ok(suites)
}
fn test_names(text: &str, listing: bool) -> std::collections::BTreeMap<String, usize> {
    let mut names = std::collections::BTreeMap::new();
    for line in text.lines() {
        let name = if listing {
            line.strip_suffix(": test")
        } else {
            line.strip_prefix("test ")
                .and_then(|line| line.strip_suffix(" ... ok"))
        };
        if let Some(name) = name {
            *names.entry(name.to_owned()).or_default() += 1;
        }
    }
    names
}
pub(crate) fn checked_tests(
    root: &Path,
    target: Option<&str>,
    release: bool,
    out: &Path,
) -> Result<Value> {
    #[cfg(target_os = "macos")]
    let mac = if release {
        Some(crate::macos::load(root)?)
    } else {
        None
    };
    let command = |listing: bool| {
        let mut command = Command::new("cargo");
        command
            .current_dir(root)
            .args(["test", "--color", "never", "--locked", "--workspace"])
            .env("CARGO_TARGET_DIR", out.join("native-test-build"));
        if release {
            command.arg("--release");
        }
        if let Some(target) = target {
            command.args(["--target", target]);
        }
        command.args(["--", "--color", "never"]);
        if listing {
            command.arg("--list");
        }
        #[cfg(target_os = "macos")]
        if let Some(mac) = &mac {
            command
                .env("DEVELOPER_DIR", &mac.developer)
                .env("SDKROOT", &mac.sdk)
                .env("MACOSX_DEPLOYMENT_TARGET", &mac.minimum_text);
        }
        command
    };
    let mut transcripts = Vec::new();
    for (listing, label) in [(true, "native-test-discovery"), (false, "native-tests")] {
        let log = out.join(format!("{label}.log"));
        let result = controller()?.run_separate_output(
            command(listing),
            &log,
            Duration::from_secs(20 * 60),
        )?;
        result.require_success(&log)?;
        for path in [&log, &log.with_extension("stderr")] {
            if fs::metadata(path)?.len() > 16 * 1024 * 1024 {
                return Err("Native test output exceeds its document bound".into());
            }
        }
        transcripts.push((
            fs::read_to_string(&log)?,
            fs::read_to_string(log.with_extension("stderr"))?,
        ));
    }
    let counts = validate_test_transcripts(&transcripts)?;
    Ok(
        json!({"discovery":counts,"profile":if release {"release"} else {"test"},"target":target,"scope":"suite identities and aggregate case multiplicities; no per-suite case association"}),
    )
}

fn validate_test_transcripts(transcripts: &[(String, String)]) -> Result<Value> {
    if transcripts.len() != 2 {
        return Err("Expected discovery and execution transcripts".into());
    }
    let suites = test_suites(&transcripts[0].1)?;
    if test_suites(&transcripts[1].1)? != suites {
        return Err("Native test execution omitted/changed discovered suites".into());
    }
    let names = test_names(&transcripts[0].0, true);
    if names.is_empty() || test_names(&transcripts[1].0, false) != names {
        return Err("Native test execution omitted/duplicated discovered test cases".into());
    }
    let mut passes = 0_u64;
    let mut positive = 0;
    let mut empty = 0;
    for line in transcripts[1]
        .0
        .lines()
        .filter(|line| line.starts_with("test result:"))
    {
        let count = summary_passes(line)?;
        passes += count;
        if count == 0 {
            empty += 1;
        } else {
            positive += 1;
        }
    }
    if positive + empty != suites.len() || passes != names.values().sum::<usize>() as u64 {
        return Err("Native test summary/discovery counts disagree".into());
    }
    Ok(
        json!({"passes":passes,"positive_summaries":positive,"empty_summaries":empty,"discovered_suites":suites,"discovered_case_count":names.values().sum::<usize>()}),
    )
}

#[derive(serde::Serialize)]
pub(crate) struct PublishedCliArchive {
    path: PathBuf,
    identity: xtask::archive::ByteIdentity,
}

pub(crate) fn staged_cli(
    root: &Path,
    cli: &cli_build::FrozenCli,
    checked: &notices::CheckedTargetMaterials,
    out: &Path,
) -> Result<PublishedCliArchive> {
    if checked.target() != cli.target()
        || checked.platform() != cli.platform()
        || checked.version() != cli.version()
    {
        return Err("CLI legal materials do not match frozen build identity".into());
    }
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let artifact = plan["expected_artifacts"]
        .as_array()
        .ok_or("Missing artifact table")?
        .iter()
        .find(|artifact| artifact["platform"] == cli.platform() && artifact["role"] == "cli")
        .ok_or("Missing native CLI archive identity")?;
    let row = plan["native_targets"]
        .as_array()
        .ok_or("Missing target table")?
        .iter()
        .find(|row| row["target"] == cli.target())
        .ok_or("Missing target identity")?;
    let mut materials = checked.files().clone();
    materials.insert(
        "README.txt".into(),
        archive_verify::cli_readme(
            cli.version(),
            row["platform_text"]
                .as_str()
                .ok_or("Missing platform text")?,
        )
        .into_bytes(),
    );
    let stage = xtask::archive::create_artifact_directory(out)?;
    let name = artifact["name"].as_str().ok_or("Missing archive name")?;
    let path = stage.path().join(name);
    let writer = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let writer = if row["os"] == "linux" {
        xtask::archive::encode_cli_tar_gz(
            writer,
            artifact["root"].as_str().ok_or("Missing CLI root")?,
            artifact["executable"]
                .as_str()
                .ok_or("Missing CLI executable")?,
            cli.bytes(),
            &materials,
        )?
    } else {
        xtask::archive::encode_cli_zip(
            writer,
            artifact["root"].as_str().ok_or("Missing CLI root")?,
            artifact["executable"]
                .as_str()
                .ok_or("Missing CLI executable")?,
            cli.bytes(),
            &materials,
        )?
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        writer.set_permissions(fs::Permissions::from_mode(0o644))?;
    }
    writer.sync_all()?;
    drop(writer);
    let verified = archive_verify::expected(root, &path, cli.target(), cli.identity())?;
    let output = stage
        .path()
        .parent()
        .ok_or("Missing admitted publication parent")?
        .join(name);
    #[cfg(unix)]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            &path,
            rustix::fs::CWD,
            &output,
            rustix::fs::RenameFlags::NOREPLACE,
        )?;
        let published = xtask::archive::ArchiveSnapshot::read(&output)?.identity();
        if published != verified {
            return Err(
                "Published CLI archive differs from the verified immutable snapshot".into(),
            );
        }
        Ok(PublishedCliArchive {
            path: output,
            identity: verified,
        })
    }
    #[cfg(not(unix))]
    {
        Err("Native no-replace archive publication remains unavailable on Windows".into())
    }
}

pub(crate) fn run(root: &Path, target: &str, out: &Path) -> Result<()> {
    crate::campaign::require_stock_topology(root)?;
    let changes = capture(Command::new("git").current_dir(root).args([
        "status",
        "--porcelain",
        "--untracked-files=normal",
    ]))?;
    if !changes.is_empty() {
        return Err("Native qualification requires a clean committed checkout; preserve/commit intended work first".into());
    }
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let row = plan["native_targets"]
        .as_array()
        .ok_or("Missing native target policy")?
        .iter()
        .find(|row| row["target"] == target)
        .ok_or("Unsupported native target")?;
    crate::native_host::inspect(
        root,
        target,
        row["os"].as_str().ok_or("Missing native OS")?,
        row["architecture"].as_str().ok_or("Missing native CPU")?,
    )?;
    let source = String::from_utf8(capture(
        Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"]),
    )?)?;
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let result: Result<()> = (|| {
        let tests = checked_tests(root, Some(target), true, &out)?;
        let cli = cli_build::build(root, target, &out.join("cli-build"))?;
        let materials = notices::checked_target(root, cli.platform())?;
        let archive = staged_cli(root, &cli, &materials, &out)?;
        fs::write(
            out.join("cli-result.json"),
            serde_json::to_vec_pretty(
                &json!({"source":source.trim(),"target":target,"host":cli.host(),"release_tests":tests,
            "frozen_cli":cli.identity(),"verified_cli_archive":archive,"scope":"CLI build/archive behavior only; required remaining platform/source gates are not waived"}),
            )?,
        )?;
        #[cfg(target_os = "macos")]
        {
            let app_stage = out.join("app-build");
            fs::create_dir(&app_stage)?;
            crate::macos_app::assemble(root, &app_stage, &cli, &materials)?;
            Err("CLI archive and native app assembly completed; required ditto extracted-app/complete GUI/volume qualification remains incomplete".into())
        }
        #[cfg(not(target_os="macos"))]
        Err("CLI archive completed; remaining native privilege/console/final-link and complete qualification gates remain incomplete".into())
    })();
    if let Err(error) = result {
        let evidence = format!("source={} target={target}\n{error}\n", source.trim());
        if let Err(record) = fs::write(out.join("failure.log"), evidence) {
            return Err(format!(
                "Native qualification failed: {error}; evidence write failed: {record}"
            )
            .into());
        }
        return Err(error);
    }
    Ok(())
}

fn summary_passes(line: &str) -> Result<u64> {
    let body = line
        .strip_prefix("test result: ok. ")
        .ok_or("Native test summary did not pass")?;
    let fields: Vec<_> = body.split("; ").collect();
    if fields.len() < 5
        || fields[1] != "0 failed"
        || fields[2] != "0 ignored"
        || fields[3] != "0 measured"
        || fields[4] != "0 filtered out"
    {
        return Err("Native tests have failures/ignores/filters or an unknown summary".into());
    }
    Ok(fields[0]
        .strip_suffix(" passed")
        .ok_or("Missing native test pass count")?
        .parse()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_discovery_requires_suites_cases_and_summary_totals() {
        let suites =
            "Running unittests src/lib.rs (target/owned)\ndiagnostic   Doc-tests eml_core\n";
        let discovery = "alpha: test\nbeta: test\n";
        let execution = "test alpha ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\ntest beta ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let baseline = vec![
            (discovery.into(), suites.into()),
            (execution.into(), suites.into()),
        ];
        assert!(validate_test_transcripts(&baseline).is_ok());
        for (output, stderr) in [
            (
                execution.to_owned(),
                suites.replace("diagnostic   Doc-tests eml_core\n", ""),
            ),
            (
                execution.to_owned(),
                format!("{suites}Doc-tests eml_core\n"),
            ),
            (execution.replace("test beta ... ok\n", ""), suites.into()),
            (format!("{execution}test alpha ... ok\n"), suites.into()),
            (execution.replacen("1 passed", "2 passed", 1), suites.into()),
        ] {
            assert!(
                validate_test_transcripts(&[(discovery.into(), suites.into()), (output, stderr)])
                    .is_err()
            );
        }
    }
    #[test]
    fn test_summary_gate_preserves_empty_and_positive_counts_but_rejects_ignored_or_filtered() {
        let valid = "test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s";
        assert_eq!(summary_passes(valid).unwrap(), 19);
        assert_eq!(
            summary_passes(&valid.replace("19 passed", "0 passed")).unwrap(),
            0
        );
        for changed in [
            valid.replace("0 failed", "1 failed"),
            valid.replace("0 ignored", "1 ignored"),
            valid.replace("0 filtered out", "1 filtered out"),
            valid.replace("ok.", "FAILED."),
        ] {
            assert!(summary_passes(&changed).is_err());
        }
    }
}
