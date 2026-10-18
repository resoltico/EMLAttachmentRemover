//! Rust-only repository verification and distribution commands.
#![forbid(unsafe_code)]
mod archive_verify;
mod campaign;
mod check;
mod ci;
mod cli_build;
mod distribution;
mod fuzz;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_app;
#[cfg(target_os = "macos")]
mod macos_icon;
mod mutants;
mod native_host;
mod notices;
mod qualify;
mod smoke;
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
    /// Check the actual macOS Slint profile and owned native child fixtures.
    CheckMacosDesktop {
        #[arg(long)]
        out: PathBuf,
    },
    /// Execute the exact PR M workspace/core integration on native Linux x64.
    CiIntegration(ci::IntegrationArguments),
    /// Bind event H/S/Q and the real PR merge checkout to live ref observations.
    CiContext(ci::ContextArguments),
    /// Recheck source/role and live refs before running or accepting CI work.
    CiCheckout {
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        live: PathBuf,
        #[arg(long)]
        role: String,
        #[arg(long)]
        run_id: u64,
        #[arg(long)]
        attempt: u32,
        #[arg(long)]
        workflow_sha: String,
    },
    /// Reject failed/missing required GitHub jobs; no artifact qualification claim.
    CiNeeds {
        #[arg(long)]
        event: String,
        #[arg(long)]
        needs: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Run independent repository checks, retaining every failed section.
    Check,
    /// Build and verify one native CLI archive; remaining qualification is separate.
    BuildCliArchive {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Exercise an owned native CLI and real negative controls; no archive verdict.
    CliSmokeTests {
        #[arg(long)]
        executable: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Run native release/build/archive checks; unfinished required gates fail.
    Qualify {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Inspect and exercise extracted native archive bytes; missing gates fail.
    VerifyArchive {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        target: String,
    },
    /// Generate complete per-target legal materials into a new output directory.
    Notices {
        #[arg(long)]
        out: PathBuf,
    },
    /// Observe exactly two pinned icon compilations; never qualifies an app.
    MacosIconDiagnostic {
        #[arg(long)]
        out: PathBuf,
    },
    /// Build and ad hoc sign native GUI/worker executables; no app/archive verdict.
    BuildMacosSources {
        #[arg(long)]
        out: PathBuf,
    },
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
            Task::CheckMacosDesktop { out } => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos::check_desktop(&root, &out)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = out;
                    Err("Native macOS is required for desktop profile checks".into())
                }
            }
            Task::CiIntegration(arguments) => {
                initialize_controller(&root)?;
                ci::integration(&root, arguments)
            }
            Task::CiContext(arguments) => {
                initialize_controller(&root)?;
                ci::establish(&root, arguments)
            }
            Task::CiCheckout {
                context,
                live,
                role,
                run_id,
                attempt,
                workflow_sha,
            } => {
                initialize_controller(&root)?;
                ci::recheck(
                    &root,
                    &context,
                    &live,
                    &role,
                    run_id,
                    attempt,
                    &workflow_sha,
                )
                .map(|_| ())
            }
            Task::CiNeeds { event, needs, out } => ci::needs(&event, &needs, &out),
            Task::Check => {
                initialize_controller(&root)?;
                check::run(&root)
            }
            Task::BuildCliArchive { target, out } => {
                initialize_controller(&root)?;
                std::fs::create_dir(&out)?;
                let out = out.canonicalize()?;
                let cli = cli_build::build(&root, &target, &out.join("cli-build"))?;
                let materials = notices::checked_target(&root, cli.platform())?;
                let archive = qualify::staged_cli(&root, &cli, &materials, &out)?;
                std::fs::write(
                    out.join("cli-result.json"),
                    serde_json::to_vec_pretty(
                        &serde_json::json!({"archive":archive,"scope":"native CLI build and archive readback only; full qualification remains separate"}),
                    )?,
                )?;
                Ok(())
            }
            Task::CliSmokeTests { executable, out } => {
                initialize_controller(&root)?;
                std::fs::create_dir(&out)?;
                let out = out.canonicalize()?;
                let owned = xtask::archive::create_artifact_directory(&out)?;
                let result = smoke::run(
                    &root,
                    &executable.canonicalize()?,
                    "remove-eml-attachments",
                    env!("CARGO_PKG_VERSION"),
                    owned.path(),
                    &out.join("behavior"),
                )?;
                std::fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
                Ok(())
            }
            Task::Qualify { target, out } => {
                initialize_controller(&root)?;
                qualify::run(&root, &target, &out)
            }
            Task::VerifyArchive { archive, target } => {
                initialize_controller(&root)?;
                archive_verify::run(&root, &archive, &target)
            }
            Task::Notices { out } => {
                initialize_controller(&root)?;
                notices::run(&root, &out)
            }
            Task::MacosIconDiagnostic { out } => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos_icon::run(&root, &out)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = out;
                    Err("Native macOS is required for icon observations".into())
                }
            }
            Task::BuildMacosSources { out } => {
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
