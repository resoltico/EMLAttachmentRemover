//! Independent repository checks with retained failures and actual tool graphs.
use crate::distribution::BuildPlan;
use crate::{Result, campaign, capture, controller};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

struct Checks {
    directory: PathBuf,
    results: Vec<Value>,
}
impl Checks {
    fn section(&mut self, name: &str, run: impl FnOnce() -> Result<Value>) {
        if controller().is_ok_and(|controller| !controller.admits_work()) {
            self.results.push(json!({"section":name,"outcome":"not_run","reason":"process admission closed after cancellation or uncertain control/lifetime"}));
            return;
        }
        eprintln!("Repository check: {name}");
        self.results.push(match run() {
            Ok(evidence) => json!({"section":name,"outcome":if evidence["passed"] == false {"failed"} else {"passed"},"evidence":evidence}),
            Err(error) => {
                eprintln!("Repository check {name} failed: {error}");
                json!({"section":name,"outcome":"failed","error":error.to_string(),"command":error.downcast_ref::<xtask::process::CommandFailure>().map(|failure|json!({"log":failure.log,"process":failure.process,"diagnostic_error":failure.diagnostic_error})),"process_admission_open":controller().is_ok_and(|controller|controller.admits_work())})
            }
        });
    }
    fn command(&mut self, name: &str, command: Command, seconds: u64) {
        let log = self.directory.join(format!("{name}.log"));
        self.section(name, || {
            let result = controller()?.run(command, &log, Duration::from_secs(seconds))?;
            Ok(json!({"log":log,"passed":result.success(),"process":result.evidence()}))
        });
    }
}
fn cargo(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("cargo");
    command.current_dir(root).args(args);
    command
}

pub(crate) fn run(root: &Path, plan: &BuildPlan) -> Result<()> {
    campaign::require_stock_topology(root)?;
    let parent = root.join("target/repository-checks");
    fs::create_dir_all(&parent)?;
    let directory = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(parent)?
        .keep();
    eprintln!("Retained repository checks: {}", directory.display());
    let mut checks = Checks {
        directory,
        results: Vec::new(),
    };
    checks.section("repository-config", || {
        Ok(serde_json::to_value(crate::repository_config::check(
            root,
        )?)?)
    });
    compiler_check(&mut checks, root);
    rust_checks(&mut checks, root, plan);
    dependency_checks(&mut checks, root, plan);
    tool_checks(&mut checks, root, plan);
    #[cfg(target_os = "macos")]
    native_checks(&mut checks, root, plan);
    finish(checks)
}
fn compiler_check(checks: &mut Checks, root: &Path) {
    checks.section("compiler", || {
        let authority: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml"))?)?;
        let version = authority["toolchain"]["channel"]
            .as_str()
            .ok_or("Missing Rust authority")?;
        let actual =
            String::from_utf8(capture(Command::new("rustc").current_dir(root).arg("-vV"))?)?;
        if !actual
            .lines()
            .any(|line| line == format!("release: {version}"))
        {
            return Err("Rust compiler differs from rust-toolchain.toml".into());
        }
        campaign::tool_version(root, "deny", "cargo_deny")?;
        Ok(json!({"rustc":actual}))
    });
}

fn rust_checks(checks: &mut Checks, root: &Path, plan: &BuildPlan) {
    checks.command("rust-format", cargo(root, &["fmt", "--all", "--check"]), 60);
    for (name, manifest) in [
        ("fuzz", "fuzz/Cargo.toml"),
        ("filesystem-probe", "tests/native-filesystem/Cargo.toml"),
    ] {
        checks.command(
            &format!("rust-format-{name}"),
            cargo(
                root,
                &["fmt", "--manifest-path", manifest, "--all", "--check"],
            ),
            60,
        );
        checks.command(
            &format!("rust-clippy-{name}"),
            cargo(
                root,
                &[
                    "clippy",
                    "--locked",
                    "--manifest-path",
                    manifest,
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            ),
            600,
        );
    }
    checks.command(
        "rust-check",
        cargo(root, &["check", "--locked", "--workspace", "--all-targets"]),
        600,
    );
    checks.command(
        "rust-clippy",
        cargo(
            root,
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        600,
    );
    let test_directory = checks.directory.clone();
    checks.section("rust-tests", || {
        crate::qualify::checked_tests(root, plan, None, false, &test_directory)
    });
    let mut docs = cargo(root, &["doc", "--locked", "--workspace", "--no-deps"]);
    docs.env("RUSTDOCFLAGS", "-D warnings");
    checks.command("rust-docs", docs, 600);
    let mut python = Command::new("python3");
    python.current_dir(root).args([
        "-m",
        "unittest",
        "discover",
        "-s",
        "tools",
        "-p",
        "test_*.py",
        "-v",
    ]);
    checks.command("python-tool-tests", python, 120);
}

fn dependency_checks(checks: &mut Checks, root: &Path, plan: &BuildPlan) {
    let out = checks.directory.join("dependencies");
    checks.section("dependencies", || {
        crate::dependencies::run(root, plan, &out)
    });
}

fn tool_checks(checks: &mut Checks, root: &Path, plan: &BuildPlan) {
    checks.section("actionlint", || {
        let expected = campaign::pin(root, "actionlint")?;
        let actual = String::from_utf8(capture(Command::new("actionlint").arg("--version"))?)?;
        if actual.lines().next() != Some(expected.as_str()) {
            return Err("Install actionlint at the xtask/tools.toml version".into());
        }
        capture(Command::new("actionlint").current_dir(root))?;
        Ok(json!({"version":expected}))
    });
    checks.section("shellcheck", || check_shell(root));
    checks.section("notices", || crate::notices::check(root, plan, None));
}

#[cfg(target_os = "macos")]
fn native_checks(checks: &mut Checks, root: &Path, plan: &BuildPlan) {
    #[cfg(target_os = "macos")]
    let swift_checks = checks.directory.join("swift-format");
    #[cfg(target_os = "macos")]
    checks.section("swift-format", || {
        crate::macos::check_swift_sources(root, plan, &swift_checks)
    });
    #[cfg(target_os = "macos")]
    let desktop_checks = checks.directory.join("desktop");
    #[cfg(target_os = "macos")]
    checks.section("macos-desktop", || {
        crate::macos::check_desktop(root, plan, &desktop_checks)?;
        Ok(json!({"scope":"pinned native Slint profile and owned child tests; no GUI acceptance"}))
    });
}

fn finish(checks: Checks) -> Result<()> {
    let failed = checks
        .results
        .iter()
        .filter(|record| record["outcome"] != "passed")
        .count();
    let report =
        serde_json::to_vec_pretty(&json!({"sections":checks.results,"failed_sections":failed}))?;
    if let Err(error) = fs::write(checks.directory.join("result.json"), &report) {
        eprintln!(
            "Collected repository sections: {}",
            String::from_utf8_lossy(&report)
        );
        return Err(
            format!("{failed} failed/unrun sections; summary write failed: {error}").into(),
        );
    }
    if failed != 0 {
        return Err(format!(
            "{failed} repository check sections failed; retained {}",
            checks.directory.display()
        )
        .into());
    }
    Ok(())
}

pub(crate) fn check_shell(root: &Path) -> Result<Value> {
    let expected = campaign::pin(root, "shellcheck")?;
    let actual = String::from_utf8(capture(Command::new("shellcheck").arg("--version"))?)?;
    if !actual
        .lines()
        .any(|line| line == format!("version: {expected}"))
    {
        return Err("Install shellcheck at the xtask/tools.toml version".into());
    }
    let paths = crate::source_files::shells(root)?;
    if paths.is_empty() {
        return Err(
            "Authored shell discovery is empty; investigate the retained script selection".into(),
        );
    }
    {
        let mut command = Command::new("shellcheck");
        command
            .current_dir(root)
            .env_remove("SHELLCHECK_OPTS")
            .arg("--rcfile")
            .arg(root.join(".shellcheckrc"))
            .arg("--")
            .args(&paths);
        capture(&mut command)?;
    }
    Ok(json!({"version":expected,"authored_scripts":paths.len()}))
}
