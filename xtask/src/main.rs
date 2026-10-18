//! Rust-only repository verification and distribution commands.
#![forbid(unsafe_code)]
mod archive_verify;
mod campaign;
mod check;
mod ci;
mod cli_build;
#[cfg(any(target_os = "macos", test))]
mod desktop_preferences;
mod distribution;
mod fuzz;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_app;
#[cfg(target_os = "macos")]
mod macos_archive;
#[cfg(target_os = "macos")]
mod macos_icon;
#[cfg(target_os = "macos")]
mod macos_interaction;
#[cfg(target_os = "macos")]
mod macos_volume;
mod mutants;
mod native_host;
mod notices;
mod qualify;
mod release_assets;
mod release_check;
mod smoke;
mod tools;
#[cfg(any(windows, test))]
mod windows_evidence;
#[cfg(any(windows, test))]
mod windows_input_review;
#[cfg(windows)]
mod windows_link;
mod windows_references;
mod windows_runtime;
#[cfg(windows)]
mod windows_workspace;
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
    /// Admit exact event-specific CI results, rejecting absent or skipped required jobs.
    CiResult {
        #[arg(long)]
        event: String,
        #[arg(long)]
        native: bool,
        #[arg(long)]
        needs: PathBuf,
    },
    /// Derive byte references from pinned official Windows component archives; not approval.
    WindowsRuntimeReferences {
        #[arg(long)]
        archives: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Capture an actual native Windows CLI link; static-input review is separate.
    ObserveWindowsLink {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Prepare an exact-source hosted Windows checkout in protected user storage.
    PrepareWindowsWorkspace,
    /// Read-only source, version, dated changelog and remote release admission.
    ReleaseCheck {
        #[arg(long)]
        source: String,
        #[arg(long)]
        version: String,
    },
    /// Assemble exactly eight archive bytes and SHA256SUMS; no qualification or publication.
    StageReleaseAssets {
        #[arg(long)]
        archives: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Check the actual macOS Slint profile and owned native child fixtures.
    CheckMacosDesktop {
        #[arg(long)]
        out: PathBuf,
    },
    /// Discover and execute the locked native workspace selection without ignored/filtered cases.
    NativeTests {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        release: bool,
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
    /// Compile and validate one shared owner-icon asset set with pinned Xcode.
    CompileMacosIcon {
        #[arg(long)]
        out: PathBuf,
    },
    /// Build and ad hoc sign native GUI/worker executables; no app/archive verdict.
    BuildMacosSources {
        #[arg(long)]
        out: PathBuf,
    },
    /// Exercise an actual exclusively owned exFAT image on a fresh native CI host.
    CheckMacosVolume {
        #[arg(long)]
        out: PathBuf,
    },
    /// Assemble, ad hoc sign, read back and exercise one native app archive.
    BuildMacosApp {
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        icons: PathBuf,
    },
    /// Exercise the same frozen app archive on a matching native smoke host.
    SmokeMacosApp {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        reference: PathBuf,
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
    #[cfg(windows)]
    if let Some(exit) = windows_link::helper_entry() {
        return exit;
    }
    if let Some(exit) = xtask::process::helper_entry() {
        return exit;
    }
    let result = (|| {
        let args = Arguments::parse();
        let root = root()?;
        match args.task {
            Task::CiResult {
                event,
                native,
                needs,
            } => ci::run(&event, native, &needs),
            Task::WindowsRuntimeReferences { archives, out } => {
                initialize_controller(&root)?;
                windows_references::generate(&root, &archives, &out)
            }
            Task::ObserveWindowsLink { target, out } => {
                #[cfg(windows)]
                {
                    initialize_controller(&root)?;
                    std::fs::create_dir(&out)?;
                    let out = out.canonicalize()?;
                    let cli = cli_build::build(&root, &target, &out.join("cli-build"))?;
                    let unknown_input = cli_build::unknown_input_control(
                        &root,
                        &target,
                        &cli,
                        &out.join("unknown-input-control"),
                    )?;
                    let materials = notices::checked_target(&root, cli.platform())?;
                    let archive = qualify::staged_cli(&root, &cli, &materials, &out)?;
                    std::fs::write(
                        out.join("result.json"),
                        serde_json::to_vec_pretty(
                            &serde_json::json!({"target":cli.target(),"host":cli.host(),"image":cli.identity(),"unknown_input_control":unknown_input,"archive":archive,"scope":"native compiler/final-link/PE/input admission, actual unknown-object rejection and extracted CLI archive behavior; full qualification remains separate"}),
                        )?,
                    )?;
                    Ok(())
                }
                #[cfg(not(windows))]
                {
                    let _ = (target, out);
                    Err("Final Windows link observation requires native Windows".into())
                }
            }
            Task::PrepareWindowsWorkspace => {
                #[cfg(windows)]
                {
                    initialize_controller(&root)?;
                    windows_workspace::prepare(&root)
                }
                #[cfg(not(windows))]
                {
                    Err("Protected Windows checkout preparation requires native Windows".into())
                }
            }
            Task::ReleaseCheck { source, version } => {
                initialize_controller(&root)?;
                release_check::run(&root, &source, &version)
            }
            Task::StageReleaseAssets { archives, out } => {
                initialize_controller(&root)?;
                release_assets::run(&root, &archives, &out)
            }
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
            Task::NativeTests {
                target,
                out,
                release,
            } => {
                initialize_controller(&root)?;
                let host = native_host::inspect_target(&root, &target)?;
                std::fs::create_dir(&out)?;
                let out = out.canonicalize()?;
                let mut result = qualify::checked_tests(&root, Some(&target), release, &out)?;
                result["host"] = host;
                std::fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
                Ok(())
            }
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
            Task::CompileMacosIcon { out } => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos_icon::compile(&root, &out)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = out;
                    Err("Native macOS is required for owner-icon compilation".into())
                }
            }
            Task::BuildMacosSources { out } => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos_app::build_sources(&root, &out)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = out;
                    Err("Native macOS is required for desktop executable builds".into())
                }
            }
            Task::Distribution => distribution::run(&root),
            Task::CheckMacosVolume { out } => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos_volume::run(&root, &out)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = out;
                    Err("Owned exFAT image checks require native macOS".into())
                }
            }
            Task::BuildMacosApp { out, icons } => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos_archive::build(&root, &out, &icons)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (out, icons);
                    Err("App assembly requires native macOS".into())
                }
            }
            Task::SmokeMacosApp { archive, reference } => {
                #[cfg(target_os = "macos")]
                {
                    initialize_controller(&root)?;
                    macos_archive::smoke(&root, &archive, &reference)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (archive, reference);
                    Err("App archive smoke requires native macOS".into())
                }
            }
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
