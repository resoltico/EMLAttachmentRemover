//! Repository checks and materials; archive qualification has been retired.
#![forbid(unsafe_code)]
mod campaign;
mod check;
mod ci;
mod commands;
mod distribution;
mod fuzz;
mod mutants;
mod notices;
mod quality;
mod tools;
use clap::{Parser, Subcommand};
use std::{
    error::Error,
    io::Write,
    path::PathBuf,
    process::{Command, ExitCode},
};
static CONTROLLER: std::sync::OnceLock<xtask::process::Controller> = std::sync::OnceLock::new();
fn controller() -> Result<&'static xtask::process::Controller> {
    CONTROLLER
        .get()
        .ok_or_else(|| "Process controller is not initialized".into())
}
type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[derive(Parser)]
#[command(about = "EML Attachment Remover repository tasks", version)]
struct Arguments {
    #[command(subcommand)]
    task: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Check required event-specific CI results.
    CiResult {
        #[arg(long)]
        event: String,
        #[arg(long)]
        native: bool,
        #[arg(long)]
        needs: PathBuf,
    },
    /// Print the current distribution target policy.
    Distribution,
    /// Check authored Rust source policy.
    CheckQuality,
    /// Check authored shell scripts.
    CheckShell,
    /// Generate or check complete third-party materials.
    Notices {
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, conflicts_with = "check", required_unless_present = "check")]
        write: bool,
        #[arg(long, conflicts_with = "write")]
        check: bool,
    },
    /// Run bounded MIME fuzzing.
    Fuzz {
        #[arg(long, default_value_t=60, value_parser=clap::value_parser!(u64).range(1..=900))]
        seconds: u64,
    },
    /// Review mutations of the MIME guards.
    Mutants {
        #[arg(long, conflicts_with = "diff", required_unless_present = "diff")]
        initial: bool,
        #[arg(long, conflicts_with = "initial")]
        diff: Option<String>,
    },
}
fn capture(command: &mut Command) -> Result<Vec<u8>> {
    controller()?.capture(command)
}
fn git(root: &std::path::Path, args: &[&str]) -> Result<String> {
    Ok(
        String::from_utf8(capture(Command::new("git").current_dir(root).args(args))?)?
            .trim_end()
            .to_owned(),
    )
}
fn root() -> Result<PathBuf> {
    for path in std::env::current_dir()?.ancestors() {
        if path.join(".git").exists() {
            return Ok(path.to_owned());
        }
    }
    Err("Run repository tasks from a Git checkout".into())
}
fn initialize_controller(root: &std::path::Path) -> Result<()> {
    let controlled = xtask::process::Controller::new(root, &std::env::current_exe()?)?;
    controlled.install_signals()?;
    CONTROLLER
        .set(controlled)
        .map_err(|_| "Process controller was already initialized")?;
    Ok(())
}
fn main() -> ExitCode {
    if let Some(exit) = xtask::process::helper_entry() {
        return exit;
    }
    let result = (|| {
        let args = Arguments::parse();
        let root = root()?;
        commands::run(&root, args.task)
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notices_require_one_explicit_operation() {
        assert!(Arguments::try_parse_from(["xtask", "notices"]).is_err());
        assert!(Arguments::try_parse_from(["xtask", "notices", "--out", "owned"]).is_err());
        assert!(Arguments::try_parse_from(["xtask", "notices", "--write", "--check"]).is_err());
        for mode in ["--write", "--check"] {
            assert!(Arguments::try_parse_from(["xtask", "notices", mode]).is_ok());
            assert!(
                Arguments::try_parse_from(["xtask", "notices", mode, "--out", "owned"]).is_ok()
            );
        }
    }
}
