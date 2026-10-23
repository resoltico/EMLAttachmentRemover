//! Two-phase pinned cargo-fuzz build and pre-execution graph admission.
use crate::{
    Result,
    campaign::{Campaign, SourceHistory, pin, require_unix_campaign, run_logged, tool_version},
    capture,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Default)]
struct StableInputs(BTreeMap<PathBuf, Vec<u8>>);
impl StableInputs {
    fn add(&mut self, path: PathBuf) -> Result<()> {
        self.0.insert(path.clone(), fs::read(path)?);
        Ok(())
    }
    fn verify(&self) -> Result<()> {
        for (path, bytes) in &self.0 {
            if fs::read(path)? != *bytes {
                return Err(format!(
                    "Input drift before/during fuzz execution: {} (not restored)",
                    path.display()
                )
                .into());
            }
        }
        Ok(())
    }
}
fn metadata(campaign: &Campaign, compiler: &str, target: &str, build: &Path) -> Result<Value> {
    let mut command = campaign.command("rustup")?;
    command
        .args([
            "run",
            compiler,
            "cargo",
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--filter-platform",
            target,
            "--manifest-path",
        ])
        .arg(campaign.source.join("fuzz/Cargo.toml"))
        .env("CARGO_TARGET_DIR", build);
    Ok(serde_json::from_slice(&capture(&mut command)?)?)
}
fn execution_count(log: &str) -> Result<u64> {
    let summary = log
        .lines()
        .filter_map(|line| line.strip_prefix("stat::number_of_executed_units:"))
        .collect::<Vec<_>>();
    if summary.len() != 1
        || !log
            .lines()
            .any(|line| line.starts_with("Done ") && line.contains(" runs in "))
    {
        return Err("Fuzz run lacks its complete final execution summary".into());
    }
    let count = summary[0].trim().parse()?;
    if count == 0 {
        return Err("Empty fuzz execution is not qualification".into());
    }
    Ok(count)
}
fn regular_seeds(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err("Seed directories may contain regular fixture files only".into());
        }
        files.push(entry.path());
    }
    files.sort();
    Ok(files)
}
fn seed_names(source: &Path, files: &[PathBuf]) -> Result<Vec<String>> {
    files
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(source)?;
            let name = relative.to_str().ok_or("Fuzz seed names must be UTF-8")?;
            if name.is_empty()
                || name.contains([',', '\n', '\r'])
                || relative
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(
                    "Fuzz seed name cannot be represented by the pinned comma-list format".into(),
                );
            }
            Ok(name.to_owned())
        })
        .collect()
}

fn admit_and_execute(
    inputs: &StableInputs,
    seeds: &StableInputs,
    before: &Value,
    after: &Value,
    launch: impl FnOnce() -> Result<crate::campaign::ProcessResult>,
) -> Result<crate::campaign::ProcessResult> {
    inputs.verify()?;
    seeds.verify()?;
    if before != after {
        return Err(
            "Full locked fuzz dependency graph changed during build; harness will not execute"
                .into(),
        );
    }
    launch()
}
pub(crate) fn run(root: &Path, seconds: u64) -> Result<()> {
    require_unix_campaign()?;
    let version = tool_version(root, "fuzz", "cargo_fuzz")?;
    let compiler = pin(root, "fuzz_compiler")?;
    let campaign = Campaign::create(root, "fuzz", SourceHistory::HeadOnly)?;
    let mut rustc = campaign.command("rustup")?;
    rustc.args(["run", &compiler, "rustc", "-vV"]);
    let identity = String::from_utf8(capture(&mut rustc)?)?;
    let target = identity
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("Nightly compiler host missing")?
        .to_owned();
    fs::write(campaign.directory.join("compiler.txt"), &identity)?;
    let build = campaign.directory.join("build");
    let artifacts = campaign.directory.join("artifacts");
    fs::create_dir(&artifacts)?;
    let fixtures = campaign.source.join("tests/contract/mime/inputs");
    let edges = campaign.source.join("fuzz/seeds");
    let fixture_files = regular_seeds(&fixtures)?;
    let edge_files = regular_seeds(&edges)?;
    if fixture_files.len() != 12 || edge_files.len() < 4 {
        return Err(
            "The twelve required fixtures and structural edge seeds must be present".into(),
        );
    }
    // Cargo-fuzz0.13.2 has no --locked pass-through. All graph/input checks run
    // before harness launch, not merely after a potentially changed harness ran.
    let before = metadata(&campaign, &compiler, &target, &build)?;
    fs::write(
        campaign.directory.join("metadata-before.json"),
        serde_json::to_vec_pretty(&before)?,
    )?;
    let inputs = capture_build_inputs(&campaign, &before)?;
    let seed_files = fixture_files
        .iter()
        .chain(&edge_files)
        .cloned()
        .collect::<Vec<_>>();
    let names = seed_names(&campaign.source, &seed_files)?;
    let mut seeds = StableInputs::default();
    for file in &seed_files {
        seeds.add(file.clone())?;
    }
    let seed_identities = names.iter().zip(&seed_files).map(|(name, path)| {
        serde_json::json!({"path":name,"identity":xtask::archive::ByteIdentity::of(&seeds.0[path])})
    }).collect::<Vec<_>>();
    let seed_list = campaign.directory.join("seed-inputs.txt");
    // No positional corpus directory: libFuzzer keeps its adaptive corpus in
    // memory without copying every newly interesting input to disk. Crash and
    // timeout reproducers still use the separately owned artifact directory.
    fs::write(&seed_list, names.join(","))?;
    seeds.add(seed_list.clone())?;
    let executable = build.join(&target).join("release/preparation");
    let built = build_harness(&campaign, &compiler, &target, &build)?;
    if !built.success() {
        let image =
            if built.cleanup_complete && built.unexpected_survivors == 0 && executable.is_file() {
                Some(campaign.retain_executable(&executable)?)
            } else {
                None
            };
        let cleanup = campaign.cleanup_generated(&built);
        campaign.report(&serde_json::json!({"source":campaign.sha,"stage":"build","tool_version":version,"compiler":compiler,"host_target":target,"build_process":built.evidence(),"retained_unexecuted_image":image,"seed_inputs":seed_identities,"cleanup":cleanup,"qualified":false}))?;
        return Err(format!(
            "Fuzz build failed; inspect {}",
            campaign.directory.join("build.log").display()
        )
        .into());
    }
    match fs::symlink_metadata(&executable) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let settled =
                crate::controller().is_ok_and(|controller| controller.ownership_settled());
            return post_build_admission_failure(
                &campaign,
                &built,
                None,
                settled,
                "Pinned cargo-fuzz did not produce the expected executable".into(),
            );
        }
        Ok(metadata) if metadata.is_file() => {}
        _ => return Err("Expected fuzz image identity is uncertain; retain build storage".into()),
    }
    // Preserve the image before diagnostics that can fail after a settled build.
    // An image/address identity failure still retains all scratch conservatively.
    let retained = campaign.retain_executable(&executable)?;
    let after = (|| -> Result<Value> {
        let after = metadata(&campaign, &compiler, &target, &build)?;
        fs::write(
            campaign.directory.join("metadata-after.json"),
            serde_json::to_vec_pretty(&after)?,
        )?;
        Ok(after)
    })();
    let after = match after {
        Ok(after) => after,
        Err(error) => {
            let settled =
                crate::controller().is_ok_and(|controller| controller.ownership_settled());
            return post_build_admission_failure(
                &campaign,
                &built,
                Some(&retained),
                settled,
                error,
            );
        }
    };
    let mut seed_option = std::ffi::OsString::from("-seed_inputs=@");
    seed_option.push(&seed_list);
    let mut command = std::process::Command::new(&retained.path);
    command
        .current_dir(&campaign.source)
        .arg(seed_option)
        .args([
            format!("-max_total_time={seconds}"),
            "-max_len=4194304".into(),
            "-timeout=5".into(),
            "-rss_limit_mb=1024".into(),
            // Avoid external symbolization solely for new-function progress names.
            // Coverage accounting and sanitizer failure diagnostics remain enabled.
            "-print_funcs=0".into(),
            "-print_final_stats=1".into(),
            format!("-artifact_prefix={}/", artifacts.display()),
        ])
        .env("ASAN_OPTIONS", "detect_odr_violation=0")
        .env_remove("LLVM_PROFILE_FILE");
    writeln!(
        std::io::stderr(),
        "Admitting bounded fuzz execution for max_total_time={seconds}"
    )?;
    let mut process_launch_attempted = false;
    let launched = admit_and_execute(&inputs, &seeds, &before, &after, || {
        process_launch_attempted = true;
        run_logged(
            command,
            &campaign.directory.join("campaign.log"),
            Duration::from_secs(seconds + 30),
        )
    });
    let process = match launched {
        Ok(process) => process,
        Err(error) if !process_launch_attempted => {
            let cleanup = campaign.cleanup_generated(&built);
            campaign.report(&serde_json::json!({"source":campaign.sha,"stage":"pre_execution_admission","failure":error.to_string(),"retained_unexecuted_image":retained,"seed_inputs":seed_identities,"cleanup":cleanup,"qualified":false}))?;
            return Err(error);
        }
        Err(error) => return Err(error), // No terminal process result: retain all.
    };
    let count: Result<u64> = fs::read_to_string(campaign.directory.join("campaign.log"))
        .map_err(Into::into)
        .and_then(|log| execution_count(&log));
    let artifacts_present = fs::read_dir(&artifacts).map(|mut entries| entries.next().is_some());
    let image_stable = crate::campaign::image_file_identity(&retained.path).and_then(|actual| {
        if actual == retained.identity {
            Ok(())
        } else {
            Err("Executed fuzz image changed".into())
        }
    });
    let stable = inputs.verify().and_then(|()| seeds.verify());
    let qualified = process.success()
        && !process.timed_out
        && artifacts_present.as_ref().is_ok_and(|present| !present)
        && count.is_ok()
        && stable.is_ok()
        && image_stable.is_ok();
    let cleanup = if image_stable.is_ok() {
        campaign.cleanup_generated(&process)
    } else {
        serde_json::json!({"state":"retained","failure":"Executed image identity is unconfirmed; retain all storage"})
    };
    let qualified = qualified && cleanup["state"] == "complete";
    campaign.report(&serde_json::json!({"source":campaign.sha,"tool_version":version,"compiler":compiler,"host_target":target,
        "fixture_seeds":fixture_files.len(),"edge_seeds":edge_files.len(),"seed_inputs":seed_identities,"generated_corpus_storage":"memory_only","max_total_time":seconds,"max_len":4194304,"per_input_timeout_seconds":5,"rss_limit_mb":1024,
        "executed_units":count.as_ref().ok(),"command_exit":process.status.code(),"deadline_exceeded":process.timed_out,"elapsed_ms":process.elapsed.as_millis(),"artifacts_present":artifacts_present.as_ref().ok(),"artifact_observation_failure":artifacts_present.as_ref().err().map(ToString::to_string),"execution_summary_failure":count.as_ref().err().map(ToString::to_string),"retained_executed_image":retained,"image_identity_failure":image_stable.as_ref().err().map(ToString::to_string),"input_failure":stable.as_ref().err().map(ToString::to_string),"cleanup":cleanup,
        "pre_execution_inputs_and_graph_verified":true,"qualified":qualified}))?;
    stable?;
    image_stable?;
    artifacts_present?;
    count?;
    if !qualified {
        return Err(format!(
            "Fuzz execution failed or incomplete; retain {}",
            campaign.directory.display()
        )
        .into());
    }
    Ok(())
}

// Admission refusal does not undo a successfully settled compiler lifetime.
fn post_build_admission_failure(
    campaign: &Campaign,
    built: &crate::campaign::ProcessResult,
    retained: Option<&crate::campaign::RetainedExecutable>,
    later_processes_settled: bool,
    error: Box<dyn std::error::Error>,
) -> Result<()> {
    let cleanup = if later_processes_settled {
        campaign.cleanup_generated(built)
    } else {
        serde_json::json!({"state":"retained","failure":"Later process ownership is uncertain; retain all storage"})
    };
    if let Err(report) = campaign.report(&serde_json::json!({
        "source":campaign.sha,"stage":"post_build_admission","failure":error.to_string(),
        "build_process":built.evidence(),"retained_unexecuted_image":retained,
        "cleanup":cleanup,"qualified":false
    })) {
        eprintln!("Post-build admission failure report also failed: {report}");
    }
    Err(error)
}

fn capture_build_inputs(campaign: &Campaign, before: &Value) -> Result<StableInputs> {
    let mut inputs = StableInputs::default();
    for file in [
        "Cargo.toml",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        "xtask/tools.toml",
        "fuzz/Cargo.toml",
        "fuzz/Cargo.lock",
    ] {
        inputs.add(campaign.source.join(file))?;
    }
    for package in before["packages"]
        .as_array()
        .ok_or("Full Cargo metadata packages missing")?
    {
        inputs.add(PathBuf::from(
            package["manifest_path"]
                .as_str()
                .ok_or("Package manifest path missing")?,
        ))?;
    }
    for configuration in crate::campaign::effective_configs(&campaign.source)? {
        inputs.add(configuration)?;
    }
    Ok(inputs)
}

fn build_harness(
    campaign: &Campaign,
    compiler: &str,
    target: &str,
    build: &Path,
) -> Result<crate::campaign::ProcessResult> {
    let mut command = campaign.command("rustup")?;
    let fuzz_tool = campaign.tool_path("fuzz")?;
    command
        .args(["run", compiler])
        .arg(fuzz_tool)
        .args(["fuzz", "build", "preparation", "--fuzz-dir"])
        .arg(campaign.source.join("fuzz"))
        .args(["--target", target, "--target-dir"])
        .arg(build)
        .env("CARGO_NET_OFFLINE", "true");
    writeln!(
        std::io::stderr(),
        "Building pinned fuzz harness at {}; evidence {}",
        campaign.sha,
        campaign.directory.display()
    )?;
    let built = run_logged(
        command,
        &campaign.directory.join("build.log"),
        Duration::from_secs(10 * 60),
    )?;
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_list_keeps_relative_inputs_when_the_owned_parent_contains_commas() {
        let source = Path::new("/owned,parent/source");
        let names = seed_names(
            source,
            &[
                source.join("fixtures/first.eml"),
                source.join("edges/second.eml"),
            ],
        )
        .unwrap();
        assert_eq!(names.join(","), "fixtures/first.eml,edges/second.eml");
        for invalid in [
            source.join("edges/comma,name"),
            source.join("edges/line\nname"),
            source.join("edges/carriage\rname"),
            source.join("../escape"),
            PathBuf::from("/outside/input"),
        ] {
            assert!(seed_names(source, &[invalid]).is_err());
        }
    }

    #[test]
    fn summary_requires_real_positive_execution_and_completion() {
        assert!(
            execution_count("Done 12 runs in 60 second(s)\nstat::number_of_executed_units: 12\n")
                .is_ok()
        );
        for invalid in [
            "",
            "Done 0 runs in 60 second(s)\nstat::number_of_executed_units: 0\n",
            "stat::number_of_executed_units: 12\n",
            "Done 12 runs in 60 second(s)\n",
        ] {
            assert!(execution_count(invalid).is_err());
        }
    }
    #[test]
    fn drift_is_rejected_before_a_harness_can_be_admitted() {
        let directory = scratch();
        let input = directory.path().join("Cargo.lock");
        fs::write(&input, "admitted graph").unwrap();
        let mut stable = StableInputs::default();
        stable.add(input.clone()).unwrap();
        assert!(stable.verify().is_ok());
        fs::write(input, "different graph").unwrap();
        assert!(stable.verify().is_err());
    }
    fn scratch() -> tempfile::TempDir {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/xtask-tests");
        fs::create_dir_all(&parent).unwrap();
        tempfile::tempdir_in(parent).unwrap()
    }
    #[test]
    #[cfg(unix)]
    fn post_build_metadata_refusal_preserves_primary_error_image_and_settled_cleanup() {
        for (uncertain, later_settled) in [(false, true), (true, true), (false, false)] {
            let root = scratch();
            let campaign = crate::campaign::cleanup_tests::fixture(root.path());
            let executable = campaign.directory.join("build/preparation");
            fs::write(&executable, b"owned unexecuted image").unwrap();
            fs::write(
                campaign.directory.join("build/objects"),
                b"compiler scratch",
            )
            .unwrap();
            fs::write(campaign.directory.join("tmp/scratch"), b"temporary bytes").unwrap();
            fs::write(
                campaign.directory.join("build.log"),
                b"compiler observations",
            )
            .unwrap();
            let retained = campaign.retain_executable(&executable).unwrap();
            let status = std::process::Command::new("/usr/bin/true")
                .status()
                .unwrap();
            let built = crate::campaign::ProcessResult {
                status,
                timed_out: false,
                cancelled: false,
                unexpected_survivors: 0,
                cleanup_complete: !uncertain,
                elapsed: Duration::ZERO,
                cleanup_elapsed: Duration::ZERO,
            };
            let error = post_build_admission_failure(
                &campaign,
                &built,
                Some(&retained),
                later_settled,
                "independent metadata failure".into(),
            )
            .unwrap_err();
            assert_eq!(error.to_string(), "independent metadata failure");
            assert_eq!(fs::read(&retained.path).unwrap(), b"owned unexecuted image");
            assert_eq!(
                fs::read(campaign.directory.join("build.log")).unwrap(),
                b"compiler observations"
            );
            for name in ["build", "tmp"] {
                assert_eq!(
                    campaign.directory.join(name).exists(),
                    uncertain || !later_settled
                );
            }
            let report: Value =
                serde_json::from_slice(&fs::read(campaign.directory.join("result.json")).unwrap())
                    .unwrap();
            assert_eq!(report["qualified"], false);
            assert_eq!(report["failure"], "independent metadata failure");
            assert_eq!(
                report["cleanup"]["state"],
                if uncertain || !later_settled {
                    "retained"
                } else {
                    "complete"
                }
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn settled_build_missing_image_keeps_failure_and_logs_without_compiler_tree() {
        let root = scratch();
        let campaign = crate::campaign::cleanup_tests::fixture(root.path());
        fs::write(
            campaign.directory.join("build/objects"),
            b"compiler scratch",
        )
        .unwrap();
        fs::write(
            campaign.directory.join("build.log"),
            b"compiler observations",
        )
        .unwrap();
        let built = crate::campaign::ProcessResult {
            status: std::process::Command::new("/usr/bin/true")
                .status()
                .unwrap(),
            timed_out: false,
            cancelled: false,
            unexpected_survivors: 0,
            cleanup_complete: true,
            elapsed: Duration::ZERO,
            cleanup_elapsed: Duration::ZERO,
        };
        let error = post_build_admission_failure(
            &campaign,
            &built,
            None,
            true,
            "expected image absent".into(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "expected image absent");
        assert!(!campaign.directory.join("build").exists());
        assert!(!campaign.directory.join("tmp").exists());
        assert_eq!(
            fs::read(campaign.directory.join("build.log")).unwrap(),
            b"compiler observations"
        );
        let report: Value =
            serde_json::from_slice(&fs::read(campaign.directory.join("result.json")).unwrap())
                .unwrap();
        assert_eq!(report["qualified"], false);
        assert_eq!(report["retained_unexecuted_image"], Value::Null);
        assert_eq!(report["cleanup"]["state"], "complete");
    }

    #[test]
    #[cfg(unix)]
    fn drift_and_graph_mismatch_prevent_real_process_launch() {
        let directory = scratch();
        let input = directory.path().join("Cargo.lock");
        fs::write(&input, "admitted").unwrap();
        let mut inputs = StableInputs::default();
        inputs.add(input.clone()).unwrap();
        let seeds = StableInputs::default();
        let graph = serde_json::json!({"graph":"admitted"});
        let changed = serde_json::json!({"graph":"changed"});
        let launch = || {
            let began = std::time::Instant::now();
            let status = std::process::Command::new("/bin/sh")
                .current_dir(directory.path())
                .args(["-c", "printf executed > invoked"])
                .status()?;
            Ok(crate::campaign::ProcessResult {
                status,
                timed_out: false,
                cancelled: false,
                unexpected_survivors: 0,
                cleanup_complete: true,
                elapsed: began.elapsed(),
                cleanup_elapsed: Duration::ZERO,
            })
        };
        fs::write(&input, "drift").unwrap();
        assert!(admit_and_execute(&inputs, &seeds, &graph, &graph, launch).is_err());
        assert!(!directory.path().join("invoked").exists());
        fs::write(&input, "admitted").unwrap();
        assert!(admit_and_execute(&inputs, &seeds, &graph, &changed, launch).is_err());
        assert!(!directory.path().join("invoked").exists());
        assert!(
            admit_and_execute(&inputs, &seeds, &graph, &graph, launch)
                .unwrap()
                .success()
        );
        assert!(directory.path().join("invoked").exists());
    }
}
