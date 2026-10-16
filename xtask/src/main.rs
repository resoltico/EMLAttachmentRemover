//! Rust-only repository verification and distribution commands.
#![forbid(unsafe_code)]
mod campaign;
mod distribution;
mod fuzz;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_app;
#[cfg(target_os = "macos")]
mod macos_process;
mod mutants;
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
    /// Assemble a native signed app for integration; not shipping qualification.
    BuildMacosApp {
        #[arg(long)]
        out: PathBuf,
    },
    /// Run real native macOS helper/pipe/ownership fixtures with the pinned Xcode.
    MacosProcessTests,
    /// Print native target policy and expected archive identities; build nothing.
    Distribution,
    /// Run one bounded pure-preparation fuzz campaign with owned corpus storage.
    Fuzz {
        #[arg(long,default_value_t=60,value_parser=clap::value_parser!(u64).range(1..=900))]
        seconds: u64,
    },
    /// Completely test the two specified MIME guards, never a whole workspace campaign.
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
        match args.task {
            Task::BuildMacosApp { out } => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos_app::run(&root, &out)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = out;
                    Err("Native macOS is required for app assembly".into())
                }
            }
            Task::MacosProcessTests => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos_process::run(&root)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    Err("Native macOS is required for these process fixtures".into())
                }
            }
            Task::Distribution => distribution::run(&root),
            Task::Fuzz { seconds } => {
                initialize_controller(&root)?;
                fuzz::run(&root, seconds)
            }
            Task::Mutants { initial, diff } => {
                initialize_controller(&root)?;
                mutants::run(&root, initial, diff.as_deref())
            }
        }
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            ExitCode::FAILURE
        }
    }
}
