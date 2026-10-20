//! Two-phase pinned cargo-fuzz build and pre-execution graph admission.
use crate::{
    Result,
    campaign::{Campaign, pin, require_unix_campaign, run_logged, tool_version},
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
    let campaign = Campaign::create(root, "fuzz")?;
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
    let corpus = campaign.directory.join("corpus");
    let artifacts = campaign.directory.join("artifacts");
    fs::create_dir(&corpus)?;
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
    let mut seeds = StableInputs::default();
    for file in fixture_files.iter().chain(&edge_files) {
        seeds.add(file.clone())?;
    }
    build_harness(&campaign, &compiler, &target, &build)?;
    let after = metadata(&campaign, &compiler, &target, &build)?;
    fs::write(
        campaign.directory.join("metadata-after.json"),
        serde_json::to_vec_pretty(&after)?,
    )?;
    let executable = build.join(&target).join("release/preparation");
    if !executable.is_file() {
        return Err("Pinned cargo-fuzz did not produce the expected executable".into());
    }
    let mut command = std::process::Command::new(&executable);
    command
        .current_dir(&campaign.source)
        .arg(&corpus)
        .arg(&fixtures)
        .arg(&edges)
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
    let process = admit_and_execute(&inputs, &seeds, &before, &after, || {
        run_logged(
            command,
            &campaign.directory.join("campaign.log"),
            Duration::from_secs(seconds + 30),
        )
    })?;
    let log = fs::read_to_string(campaign.directory.join("campaign.log"))?;
    let count = execution_count(&log);
    let artifacts_present = fs::read_dir(&artifacts)?.next().is_some();
    let stable = inputs.verify().and_then(|()| seeds.verify());
    let qualified = process.success()
        && !process.timed_out
        && !artifacts_present
        && count.is_ok()
        && stable.is_ok();
    campaign.report(&serde_json::json!({"source":campaign.sha,"tool_version":version,"compiler":compiler,"host_target":target,
        "fixture_seeds":fixture_files.len(),"edge_seeds":edge_files.len(),"max_total_time":seconds,"max_len":4194304,"per_input_timeout_seconds":5,"rss_limit_mb":1024,
        "executed_units":count.as_ref().ok(),"command_exit":process.status.code(),"deadline_exceeded":process.timed_out,"elapsed_ms":process.elapsed.as_millis(),"artifacts_present":artifacts_present,
        "pre_execution_inputs_and_graph_verified":true,"qualified":qualified}))?;
    stable?;
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

fn build_harness(campaign: &Campaign, compiler: &str, target: &str, build: &Path) -> Result<()> {
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
    if !built.success() {
        return Err(format!(
            "Fuzz build failed; inspect {}",
            campaign.directory.join("build.log").display()
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
