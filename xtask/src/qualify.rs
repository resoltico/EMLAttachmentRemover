//! Native release build/package orchestration; incomplete platform gates fail.
use crate::distribution::BuildPlan;
use crate::{Result, archive_verify, cli_build, controller, notices};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
#[cfg(target_os = "macos")]
const MACOS_RELEASE_TEST_FEATURES: &str = "eml-gui/macos-gui,eml-gui/native-fixtures,eml-worker/native-fixtures,eml-macos/native-fixtures,eml-core/native-fixtures";

#[cfg(target_os = "linux")]
const LINUX_NATIVE_TEST_FEATURES: &str = "eml-core/native-fixtures";
// These finite AC030 obligations must execute, not merely appear as an empty
// feature-gated Cargo suite in an otherwise successful ordinary selection.
#[cfg(any(target_os = "linux", test))]
fn require_linux_storage_cases(discovery: &str, execution: &str) -> Result<Value> {
    let discovered = test_names(discovery, true);
    let executed = test_names(execution, false);
    let mut observations = Vec::new();
    for name in [
        "actual_create_existing_and_check_have_distinct_finite_boundaries",
        "changed_requested_parent_before_actual_edge_refuses_without_renaming_or_publication",
        "stop_at_stage_write_or_file_sync_preserves_real_unattempted_storage_receipts",
    ] {
        let found = discovered.get(name).copied().unwrap_or(0);
        let passed = executed.get(name).copied().unwrap_or(0);
        if found != 1 || passed != 1 {
            return Err(format!("Required Linux native storage fixture was not discovered and passed exactly once: {name}; discovered={found}, executed={passed}").into());
        }
        observations.push(json!({"case":name,"discovered":found,"executed":passed}));
    }
    Ok(json!(observations))
}

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
    plan: &BuildPlan,
    target: Option<&str>,
    release: bool,
    out: &Path,
) -> Result<Value> {
    if let Some(target) = target {
        plan.lookup_target(target)?;
    }
    #[cfg(target_os = "macos")]
    let mac = if release {
        Some(crate::macos::load(root, plan)?)
    } else {
        None
    };
    let target_directory = out.join("native-test-build");
    // Release discovery/execution precedes shipping builds. Cargo fingerprints
    // feature sets separately; shipping artifacts still reject native-fixtures
    // and inspect their actual release profile.
    #[cfg(target_os = "macos")]
    let target_directory = mac
        .as_ref()
        .map_or_else(|| target_directory, |build| build.cargo_directory(root));
    #[cfg(target_os = "macos")]
    let private_build = mac.is_none();
    #[cfg(not(target_os = "macos"))]
    let private_build = true;
    let private_owner = if private_build {
        // Existing output is never adopted for later recursive removal.
        fs::create_dir(&target_directory)?;
        Some(eml_fs::BoundDirectory::open(&target_directory)?)
    } else {
        None
    };
    let command = |listing: bool| {
        let mut command = Command::new("cargo");
        command
            .current_dir(root)
            .args([
                "test",
                "--color",
                "never",
                "--locked",
                "--workspace",
                "--no-fail-fast",
            ])
            .env("CARGO_TARGET_DIR", &target_directory);
        if private_build {
            // Discovery and execution reuse Cargo's ordinary artifacts, but a
            // fresh per-run tree has no cross-run incremental state to reuse.
            command.env("CARGO_INCREMENTAL", "0");
            command.env("CARGO_BUILD_BUILD_DIR", &target_directory);
        }
        if release {
            command.arg("--release");
        }
        if let Some(target) = target {
            command.args(["--target", target]);
        }
        #[cfg(target_os = "macos")]
        if release {
            command.args(["--features", MACOS_RELEASE_TEST_FEATURES]);
        }
        #[cfg(target_os = "linux")]
        command.args(["--features", LINUX_NATIVE_TEST_FEATURES]);
        #[cfg(windows)]
        {
            command.args([
                "--features",
                "eml-core/native-fixtures,eml-windows/native-fixtures,emlattachmentremover/native-fixtures",
            ]);
            if release {
                command.env(
                    "EML_WINDOWS_EVIDENCE_DIR",
                    out.join("windows-creation-evidence"),
                );
            }
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
    let selection = checked_commands(out, command);
    #[cfg(windows)]
    let selection = selection.and_then(|mut counts| {
        // The workflow consumes this real listing after private build cleanup.
        // Reuse the exact discovery selector, then ask the harness for ignored
        // cases before discarding any of its compiled test executables.
        let mut ignored = command(true);
        ignored.arg("--ignored");
        let log = out.join("native-test-ignored.log");
        let process =
            controller()?.run_separate_output(ignored, &log, Duration::from_secs(20 * 60))?;
        process.require_success(&log)?;
        for path in [&log, &log.with_extension("stderr")] {
            if fs::metadata(path)?.len() > 16 * 1024 * 1024 {
                return Err("Native ignored-test listing exceeds its document bound".into());
            }
        }
        let mut observation = validate_ignored_listing(
            &fs::read_to_string(out.join("native-test-discovery.stderr"))?,
            &fs::read_to_string(&log)?,
            &fs::read_to_string(log.with_extension("stderr"))?,
        )?;
        observation["process"] = process.evidence();
        counts["ignored_listing"] = observation;
        Ok(counts)
    });
    let cleanup = if private_build {
        cleanup_private_test_build(
            root,
            &target_directory,
            out,
            controller()?.ownership_settled(),
            private_owner.as_ref().unwrap(),
        )
    } else {
        Ok(
            json!({"removed":false,"scope":"shared native-cargo build retained for later shipping/native profile consumers"}),
        )
    };
    let (counts, cleanup) = match selection {
        Ok(counts) => (counts, cleanup?),
        Err(primary) => {
            if let Err(error) = cleanup {
                eprintln!("Private test build cleanup also failed: {error}");
                let record = json!({"failure":error.to_string(),"primary_error_preserved":true,"scope":"cleanup diagnostic only; original command/lifetime failure returned unchanged"});
                match serde_json::to_vec_pretty(&record) {
                    Ok(bytes) => {
                        if let Err(write) =
                            fs::write(out.join("native-test-build-cleanup-failure.json"), bytes)
                        {
                            eprintln!("Could not retain cleanup diagnostic: {write}");
                        }
                    }
                    Err(write) => eprintln!("Could not encode cleanup diagnostic: {write}"),
                }
            }
            return Err(primary);
        }
    };

    let result = json!({"discovery":counts,"profile":if release {"release"} else {"test"},"target":target,"cargo_target_directory":target_directory,"test_build_cleanup":cleanup,"scope":"suite identities and aggregate case multiplicities; no per-suite case association"});
    #[cfg(target_os = "linux")]
    let result = {
        let mut result = result;
        // checked_commands settled both processes and bounded these exact logs.
        let discovery = fs::read_to_string(out.join("native-test-discovery.log"))?;
        let execution = fs::read_to_string(out.join("native-tests.log"))?;
        result["requested_features"] = json!(LINUX_NATIVE_TEST_FEATURES);
        result["linux_storage_cases"] = require_linux_storage_cases(&discovery, &execution)?;
        result
    };
    #[cfg(target_os = "macos")]
    let result = {
        let mut result = result;
        if release {
            result["requested_features"] = json!(MACOS_RELEASE_TEST_FEATURES);
        }
        result
    };
    Ok(result)
}

#[cfg(any(windows, test))]
fn validate_ignored_listing(discovery_stderr: &str, stdout: &str, stderr: &str) -> Result<Value> {
    let suites = test_suites(discovery_stderr)?;
    if test_suites(stderr)? != suites {
        return Err("Ignored-test selector omitted or changed discovered suites".into());
    }
    let summaries: Vec<_> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if summaries.len() != suites.len()
        || summaries
            .iter()
            .any(|line| *line != "0 tests, 0 benchmarks")
    {
        return Err(
            "Ignored-test listing is incomplete, malformed or contains disabled cases".into(),
        );
    }
    Ok(
        json!({"ignored":0,"observed_suites":suites,"empty_summaries":summaries.len(),"scope":"actual identical-selector --list --ignored before private build cleanup"}),
    )
}

/// Remove only this selection's fresh build after the actual controlled Cargo
/// processes settled. Keep all transcripts and exact launched-test identities.
pub(crate) fn cleanup_private_test_build(
    root: &Path,
    build: &Path,
    out: &Path,
    ownership_settled: bool,
    owner: &eml_fs::BoundDirectory,
) -> Result<Value> {
    let receipt = out.join("native-test-build-cleanup.json");
    if !ownership_settled {
        let retained = json!({"removed":false,"directory":build,"reason":"actual controlled process ownership/cleanup is uncertain"});
        fs::write(receipt, serde_json::to_vec_pretty(&retained)?)?;
        return Ok(retained);
    }
    let check_owner = || -> Result<()> {
        if !fs::symlink_metadata(build)?.is_dir()
            || eml_fs::BoundDirectory::open(build)?.identity() != owner.identity()
        {
            return Err("Private test build directory address changed; preserving storage".into());
        }
        Ok(())
    };
    check_owner()?;
    let bound = build.canonicalize()?;
    let mut executables = std::collections::BTreeSet::new();
    for name in ["native-test-discovery.stderr", "native-tests.stderr"] {
        let log = out.join(name);
        if !log.exists() {
            continue;
        }
        if fs::metadata(&log)?.len() > 16 * 1024 * 1024 {
            return Err(
                "Retaining private build: executable-identity transcript exceeds its bound".into(),
            );
        }
        for line in fs::read_to_string(log)?.lines() {
            let Some((_, path)) = line
                .find("Running ")
                .and_then(|offset| line[offset..].split_once(" ("))
            else {
                continue;
            };
            let Some(path) = path.strip_suffix(')') else {
                continue;
            };
            let path = root.join(path);
            let resolved = path.canonicalize()?;
            if !resolved.starts_with(&bound) || !fs::symlink_metadata(&path)?.is_file() {
                return Err("Launched test executable is outside the owned build tree".into());
            }
            executables.insert(resolved);
        }
    }
    let identities: Vec<_> = executables
        .into_iter()
        .map(|path| -> Result<Value> {
            Ok(json!({"path":path,"identity":xtask::archive::ByteIdentity::of(&fs::read(&path)?)}))
        })
        .collect::<Result<_>>()?;
    let mut record = json!({"directory":build,"removed":false,"processes_settled":true,"executable_identities":identities,"incremental":false,"scope":"only generated private build intermediates; discovery/execution logs and result evidence retained"});
    fs::write(&receipt, serde_json::to_vec_pretty(&record)?)?;
    check_owner()?;
    fs::remove_dir_all(build)?;
    record["removed"] = json!(true);
    fs::write(receipt, serde_json::to_vec_pretty(&record)?)?;
    Ok(record)
}

pub(crate) fn checked_commands(out: &Path, command: impl Fn(bool) -> Command) -> Result<Value> {
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
    validate_test_transcripts(&transcripts)
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
    plan: &BuildPlan,
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
    let row = plan.lookup_target(cli.target())?;
    if row.platform().to_string() != cli.platform() {
        return Err("Frozen CLI platform differs from the build plan".into());
    }
    let artifact = plan.lookup_artifact(row.platform(), crate::distribution::ProductRole::Cli)?;
    let mut materials = checked.files().clone();
    if let Some(evidence) = cli.windows_evidence() {
        materials.insert(
            "licenses/windows-runtime/link-evidence.json".into(),
            evidence.to_vec(),
        );
    }
    materials.insert(
        "README.txt".into(),
        archive_verify::cli_readme(cli.version(), row.platform_text()).into_bytes(),
    );
    let stage = xtask::archive::create_artifact_directory(out)?;
    let name = artifact.basename();
    let path = stage.path().join(name);
    let writer = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let writer = if artifact.format() == crate::distribution::ArchiveFormat::TarGz {
        xtask::archive::encode_cli_tar_gz(
            writer,
            artifact.root(),
            artifact.executable(),
            cli.bytes(),
            &materials,
            plan.source_epoch(),
        )?
    } else {
        xtask::archive::encode_cli_zip(
            writer,
            artifact.root(),
            artifact.executable(),
            cli.bytes(),
            &materials,
            plan.source_epoch(),
        )?
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        writer.set_permissions(fs::Permissions::from_mode(0o644))?;
    }
    writer.sync_all()?;
    drop(writer);
    let verified = archive_verify::expected(root, plan, &path, cli.target(), cli.identity())?;
    let output = stage
        .path()
        .parent()
        .ok_or("Missing admitted publication parent")?
        .join(name);
    publish_verified_cli(&path, output, verified)
}

pub(crate) fn run(
    root: &Path,
    plan: &BuildPlan,
    target: &str,
    out: &Path,
    icons: Option<&Path>,
) -> Result<()> {
    crate::campaign::require_stock_topology(root)?;
    #[cfg(target_os = "macos")]
    let icons = icons
        .ok_or("macOS qualification requires --icons from the same-source validated producer")?;
    #[cfg(target_os = "macos")]
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("macOS qualification requires a fresh hosted runner; saved owner preferences and desktop are protected".into());
    }
    #[cfg(not(target_os = "macos"))]
    if icons.is_some() {
        return Err("Icon inputs apply only to native macOS app qualification".into());
    }
    plan.require_qualified_source(root)?;
    crate::native_host::inspect_target(root, plan, target)?;
    let source = plan.source_commit().to_owned();
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let result: Result<()> = (|| {
        let tests = checked_tests(root, plan, Some(target), true, &out)?;
        #[cfg(not(target_os = "macos"))]
        let primitives = native_primitives(root, target, &out)?;
        let cli = cli_build::build(root, plan, target, &out.join("cli-build"))?;
        #[cfg(windows)]
        cli_build::unknown_input_control(
            root,
            plan,
            target,
            &cli,
            &out.join("unknown-input-control"),
        )?;
        let materials = notices::checked_target(root, plan, cli.platform())?;
        let archive = staged_cli(root, plan, &cli, &materials, &out)?;
        let name_profile_attempt = crate::name_profiles::run(
            root,
            &cli,
            &out.join("name-profiles"),
            plan.lookup_target(target)?.platform(),
        );
        let mut qualification_failures = Vec::new();
        let name_profiles = match name_profile_attempt {
            Ok(observed) => observed,
            Err(error) => {
                qualification_failures.push(format!("Native naming profiles: {error}"));
                fs::read(out.join("name-profiles/result.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .unwrap_or_else(|| json!({"failure":error.to_string()}))
            }
        };
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let resources =
            match crate::preparation_resources::run(root, &cli, &out.join("preparation-resources"))
            {
                Ok(observed) => Some(observed),
                Err(error) => {
                    qualification_failures.push(format!("Preparation resources: {error}"));
                    fs::read(out.join("preparation-resources/result.json"))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                }
            };
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        let resources: Option<Value> = None;
        fs::write(
            out.join("cli-result.json"),
            serde_json::to_vec_pretty(
                &json!({"source":source.trim(),"source_epoch":plan.source_epoch(),"source_provenance":plan.source_provenance(),"target":target,"workflow":crate::release_inputs::workflow_identity()?,"host":cli.host(),"release_tests":tests,
            "frozen_cli":cli.identity(),"verified_cli_archive":archive,"preparation_resources":resources,"name_profiles":name_profiles,"qualification_failures":qualification_failures,"scope":"CLI build/archive behavior only; required remaining platform/source gates are not waived"}),
            )?,
        )?;
        #[cfg(target_os = "macos")]
        {
            let app_stage = out.join("app-build");
            let volume = out.join("owned-exfat-observation");
            crate::macos_volume::run(root, &volume)?;
            crate::macos_archive::build(root, plan, &app_stage, icons)?;
            let app: Value = serde_json::from_slice(&fs::read(app_stage.join("app-result.json"))?)?;
            fs::write(
                out.join("qualification.json"),
                serde_json::to_vec_pretty(&json!({
                    "stage":"native_candidate","source":source.trim(),"source_epoch":plan.source_epoch(),"source_provenance":plan.source_provenance(),"target":target,"workflow":crate::release_inputs::workflow_identity()?,"host":cli.host(),"release_tests":tests,
                    "cli_image":cli.identity(),"archive":archive,"app":app,"volume_evidence":volume,"name_profiles":name_profiles,"qualification_failures":qualification_failures,
                    "scope":"native release tests, owned exFAT observations, frozen CLI and native app candidate structure/signature/worker verification; desktop and picker compatibility are separate required consumers"
                }))?,
            )?;
            if qualification_failures.is_empty() {
                Ok(())
            } else {
                Err(qualification_failures.join("; ").into())
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            fs::write(
                out.join("qualification.json"),
                serde_json::to_vec_pretty(&json!({
                    "source":source.trim(),"source_epoch":plan.source_epoch(),"source_provenance":plan.source_provenance(),"target":target,"workflow":crate::release_inputs::workflow_identity()?,"host":cli.host(),"release_tests":tests,
                    "native_primitives":primitives,"cli_image":cli.identity(),"archive":archive,"preparation_resources":resources,"name_profiles":name_profiles,"qualification_failures":qualification_failures,
                    "scope":"native CLI release tests, real namespace/filesystem controls, final executable/runtime inspection and extracted archive behavior; repository-wide and other-platform qualification is separate"
                }))?,
            )?;
            if qualification_failures.is_empty() {
                Ok(())
            } else {
                Err(qualification_failures.join("; ").into())
            }
        }
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

#[cfg(not(target_os = "macos"))]
fn native_primitives(root: &Path, target: &str, out: &Path) -> Result<Value> {
    let build = out.join("native-primitives-build");
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args([
            "run",
            "--locked",
            "--release",
            "--manifest-path",
            "tests/native-filesystem/Cargo.toml",
            "--target",
            target,
        ])
        .env("CARGO_TARGET_DIR", &build)
        .env("EXPECTED_TARGET", target);
    let log = out.join("native-filesystem.log");
    let process = controller()?.run(command, &log, Duration::from_secs(600))?;
    process.require_success(&log)?;
    #[cfg(target_os = "linux")]
    let namespace = {
        let mut compile = Command::new("cargo");
        compile
            .current_dir(root)
            .args([
                "build",
                "--locked",
                "--release",
                "--package",
                "eml-fs",
                "--example",
                "namespace-credentials",
                "--target",
                target,
            ])
            .env("CARGO_TARGET_DIR", &build);
        let log = out.join("namespace-build.log");
        controller()?
            .run(compile, &log, Duration::from_secs(600))?
            .require_success(&log)?;
        let mut ids = Vec::new();
        for flag in ["-u", "-g"] {
            let value = String::from_utf8(crate::capture(
                Command::new("id").current_dir(root).arg(flag),
            )?)?;
            ids.push(value.trim().parse::<u32>()?.to_string());
        }
        let mut command = Command::new("sudo");
        command
            .current_dir(root)
            .arg("-n")
            .arg(
                build
                    .join(target)
                    .join("release/examples/namespace-credentials"),
            )
            .args(&ids);
        let log = out.join("namespace-credentials.log");
        let process = controller()?.run(command, &log, Duration::from_secs(120))?;
        process.require_success(&log)?;
        json!({"process":process.evidence(),"victim_uid":ids[0],"victim_gid":ids[1]})
    };
    #[cfg(not(target_os = "linux"))]
    let namespace = json!({"scope":"Windows distinct-account ACL and containment cases are required by the release workspace test selection"});
    Ok(json!({"filesystem":process.evidence(),"namespace":namespace}))
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

fn publish_verified_cli(
    path: &Path,
    output: std::path::PathBuf,
    verified: xtask::archive::ByteIdentity,
) -> Result<PublishedCliArchive> {
    #[cfg(unix)]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            path,
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
    #[cfg(windows)]
    {
        eml_fs::rename_artifact_noreplace(path, &output)?;
        if xtask::archive::ArchiveSnapshot::read(&output)?.identity() != verified {
            return Err(
                "Published Windows CLI archive differs from its verified immutable snapshot".into(),
            );
        }
        Ok(PublishedCliArchive {
            path: output,
            identity: verified,
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (verified, output);
        Err("Native artifact publication is unsupported on this host".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ignored_listing_requires_actual_matching_selector_and_zero_disabled_cases() {
        let suites = "Running unittests src/lib.rs (target/owned)\nDoc-tests eml_core\n";
        let empty = "0 tests, 0 benchmarks\n\n0 tests, 0 benchmarks\n";
        assert_eq!(
            validate_ignored_listing(suites, empty, suites).unwrap()["ignored"],
            0
        );
        for invalid in [
            "",
            "0 tests, 0 benchmarks\n",
            "disabled: test\n1 test, 0 benchmarks\n0 tests, 0 benchmarks\n",
            "0 tests, 1 benchmark\n0 tests, 0 benchmarks\n",
            "0 tests, 0 benchmarks\n0 tests, 0 benchmarks\n0 tests, 0 benchmarks\n",
            "test result: ok. 0 passed; 0 failed; 1 ignored\n0 tests, 0 benchmarks\n",
        ] {
            assert!(validate_ignored_listing(suites, invalid, suites).is_err());
        }
        for mismatch in [
            "Running unittests src/lib.rs (target/other-profile)\nDoc-tests eml_core\n",
            "Running unittests src/lib.rs (target/owned)\n",
            "Running unittests src/lib.rs (target/owned)\nDoc-tests eml_core\nDoc-tests eml_core\n",
            "",
        ] {
            assert!(validate_ignored_listing(suites, empty, mismatch).is_err());
        }
    }
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
    fn linux_storage_obligations_require_actual_unique_discovery_and_passing_execution() {
        let discovery = "actual_create_existing_and_check_have_distinct_finite_boundaries: test\nchanged_requested_parent_before_actual_edge_refuses_without_renaming_or_publication: test\nstop_at_stage_write_or_file_sync_preserves_real_unattempted_storage_receipts: test\n";
        let execution = "test actual_create_existing_and_check_have_distinct_finite_boundaries ... ok\ntest changed_requested_parent_before_actual_edge_refuses_without_renaming_or_publication ... ok\ntest stop_at_stage_write_or_file_sync_preserves_real_unattempted_storage_receipts ... ok\n";
        assert_eq!(
            require_linux_storage_cases(discovery, execution)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            3
        );
        for (discovered,executed) in [
            (String::new(),String::new()),
            (discovery.replace("changed_requested_parent_before_actual_edge_refuses_without_renaming_or_publication: test\n", ""), execution.to_owned()),
            (discovery.to_owned(),execution.replace("test changed_requested_parent_before_actual_edge_refuses_without_renaming_or_publication ... ok\n", "")),
            (format!("{discovery}actual_create_existing_and_check_have_distinct_finite_boundaries: test\n"),execution.to_owned()),
            (discovery.to_owned(),format!("{execution}test actual_create_existing_and_check_have_distinct_finite_boundaries ... ok\n")),
            (discovery.to_owned(),execution.replace(" ... ok", " ... ignored")),
        ] {
            assert!(require_linux_storage_cases(&discovered,&executed).is_err());
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

#[cfg(test)]
mod private_build_tests {
    use super::*;
    #[test]
    fn settled_private_build_removes_intermediates_and_retains_exact_test_identity_and_logs() {
        let root = tempfile::Builder::new()
            .prefix("project (owned)-")
            .tempdir()
            .unwrap();
        // Operation capabilities take a canonical native address, just as the
        // real fresh build selection does; retain TempDir for fixture ownership.
        let directory = root.path().canonicalize().unwrap();
        let build = directory.join("native-test-build");
        fs::create_dir(&build).unwrap();
        let build = build.canonicalize().unwrap();
        let owner = eml_fs::BoundDirectory::open(&build).unwrap_or_else(|error| {
            panic!(
                "Cannot bind canonical owned fixture {build:?} (temporary root {:?}): {error}",
                root.path()
            )
        });
        let executable = build.join("independent-test-executable");
        fs::write(&executable, b"independently specified executed bytes").unwrap();
        let stderr = format!("Running unittests src/lib.rs ({})\n", executable.display());
        fs::write(directory.join("native-test-discovery.stderr"), &stderr).unwrap();
        fs::write(directory.join("native-tests.stderr"), &stderr).unwrap();
        fs::write(
            directory.join("native-tests.log"),
            b"retained failure or success evidence",
        )
        .unwrap();
        let receipt =
            cleanup_private_test_build(&directory, &build, &directory, true, &owner).unwrap();
        assert_eq!(receipt["removed"], true);
        assert!(!build.exists());
        assert_eq!(
            receipt["executable_identities"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            receipt["executable_identities"][0]["identity"],
            serde_json::to_value(xtask::archive::ByteIdentity::of(
                b"independently specified executed bytes"
            ))
            .unwrap()
        );
        assert_eq!(
            fs::read(directory.join("native-tests.log")).unwrap(),
            b"retained failure or success evidence"
        );
        assert!(directory.join("native-test-build-cleanup.json").is_file());
    }
    #[test]
    fn closed_ownership_or_outside_executable_keeps_the_private_build() {
        let root = tempfile::tempdir().unwrap();
        // Operation capabilities take a canonical native address, just as the
        // real fresh build selection does; retain TempDir for fixture ownership.
        let directory = root.path().canonicalize().unwrap();
        let build = directory.join("native-test-build");
        fs::create_dir(&build).unwrap();
        let build = build.canonicalize().unwrap();
        let owner = eml_fs::BoundDirectory::open(&build).unwrap_or_else(|error| {
            panic!(
                "Cannot bind canonical owned fixture {build:?} (temporary root {:?}): {error}",
                root.path()
            )
        });
        let outside = directory.join("outside-executable");
        fs::write(&outside, b"not build-owned").unwrap();
        fs::write(
            directory.join("native-tests.stderr"),
            format!("Running unittests src/lib.rs ({})\n", outside.display()),
        )
        .unwrap();
        assert!(cleanup_private_test_build(&directory, &build, &directory, true, &owner).is_err());
        assert!(build.is_dir());
        let retained =
            cleanup_private_test_build(&directory, &build, &directory, false, &owner).unwrap();
        assert_eq!(retained["removed"], false);
        assert!(build.is_dir());
        assert_eq!(fs::read(outside).unwrap(), b"not build-owned");
    }
    #[test]
    fn replaced_private_build_is_not_adopted_or_removed() {
        let root = tempfile::tempdir().unwrap();
        // Operation capabilities take a canonical native address, just as the
        // real fresh build selection does; retain TempDir for fixture ownership.
        let directory = root.path().canonicalize().unwrap();
        let build = directory.join("native-test-build");
        fs::create_dir(&build).unwrap();
        let build = build.canonicalize().unwrap();
        let owner = eml_fs::BoundDirectory::open(&build).unwrap_or_else(|error| {
            panic!(
                "Cannot bind canonical owned fixture {build:?} (temporary root {:?}): {error}",
                root.path()
            )
        });
        fs::rename(&build, directory.join("original-build")).unwrap();
        fs::create_dir(&build).unwrap();
        fs::write(build.join("sentinel"), b"unowned replacement").unwrap();
        assert!(cleanup_private_test_build(&directory, &build, &directory, true, &owner).is_err());
        assert_eq!(
            fs::read(build.join("sentinel")).unwrap(),
            b"unowned replacement"
        );
        assert!(directory.join("original-build").is_dir());
    }
}
