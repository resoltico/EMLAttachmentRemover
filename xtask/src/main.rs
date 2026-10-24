//! Repository verification and distribution commands.
#![forbid(unsafe_code)]
mod app_reference;
mod archive_verify;
mod campaign;
mod check;
mod ci;
mod cli_build;
mod commands;
mod dependencies;
mod desktop_evidence;
mod desktop_preferences;
mod distribution;
mod fuzz;
mod host_incidents;
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
mod macos_operation_fixture;
#[cfg(target_os = "macos")]
mod macos_operation_ui;
#[cfg(target_os = "macos")]
mod macos_panel;
mod macos_picker_host;
#[cfg(target_os = "macos")]
mod macos_volume;
mod mutants;
mod name_profiles;
mod native_host;
mod notices;
mod ntfs_query_evidence;
mod picker_evidence;
mod preparation_evidence;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod preparation_resources;
mod publication_evidence;
mod qualify;
mod release_assets;
mod release_check;
mod release_identity;
mod release_inputs;
mod repository_config;
mod smoke;
mod source_files;
mod tools;
#[cfg(any(windows, test))]
mod windows_evidence;
#[cfg(any(windows, test))]
mod windows_input_review;
#[cfg(windows)]
mod windows_link;
mod windows_references;
mod windows_runtime;
mod windows_sync_evidence;
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
    /// Assemble admitted archive bytes and SHA256SUMS; no qualification or publication.
    StageReleaseAssets {
        #[arg(long)]
        inputs: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare actual local asset bytes with a retained unpublished GitHub draft response.
    CheckReleaseServer {
        #[arg(long)]
        inputs: PathBuf,
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        release: PathBuf,
    },
    /// Strictly lint retained Swift bridge/test sources with selected Xcode defaults.
    CheckMacosSwift {
        #[arg(long)]
        out: PathBuf,
    },
    /// Observe an ordinary owned AppKit panel on a fresh native hosted Mac.
    ObserveMacosPanel {
        #[arg(long)]
        out: PathBuf,
    },
    /// Qualify the frozen picker product once in the admitted native host state.
    QualifyMacosPicker {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Independently inspect incident policy validity without mutating host services.
    CheckHostIncidents {
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
    /// Check Cargo manifest ownership and complete lint inheritance.
    CheckConfig,
    /// Compare the current product revision with an authenticated prior app's metadata.
    CheckProductIdentity {
        #[arg(long)]
        previous_info: PathBuf,
    },
    /// Strictly check all discovered authored shell scripts, including shebang files.
    CheckShell,
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
    /// Run native release/filesystem/archive checks; desktop consumers are separate.
    Qualify {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out: PathBuf,
        /// Validated same-source icon producer directory, required on macOS.
        #[arg(long)]
        icons: Option<PathBuf>,
    },
    /// Inspect and exercise extracted native CLI archive bytes; missing gates fail.
    VerifyArchive {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        target: String,
    },
    /// Write new legal materials or check source/material and optional generated-tree freshness.
    Notices {
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, conflicts_with = "check", required_unless_present = "check")]
        write: bool,
        #[arg(long, conflicts_with = "write")]
        check: bool,
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
    /// Assemble and verify a native app candidate; desktop qualification is separate.
    BuildMacosApp {
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        icons: PathBuf,
    },
    /// Qualify complete desktop cases against frozen shipping and operation-fixture app archives.
    SmokeMacosApp {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        operation_fixture: PathBuf,
        #[arg(long)]
        operation_reference: PathBuf,
    },
    /// Test actual native picker compatibility against a frozen app candidate.
    CheckMacosPicker {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        reference: PathBuf,
    },
    /// Print native target policy and expected archive identities; build nothing.
    Distribution {
        #[arg(long)]
        github_output: Option<PathBuf>,
    },
    /// Check every authored and declared desktop dependency graph using pinned tools.
    CheckDependencies {
        #[arg(long)]
        out: PathBuf,
    },
    /// Admit current-run frozen products and required native consumer evidence.
    VerifyReleaseInputs {
        #[arg(long)]
        products: PathBuf,
        #[arg(long)]
        desktop: PathBuf,
        #[arg(long)]
        picker: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
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
fn git_command(root: &std::path::Path) -> Command {
    let mut command = Command::new("git");
    // Source observations belong to this workspace, even when the caller is
    // running under another Git invocation or has inherited a linked checkout.
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
    ] {
        command.env_remove(variable);
    }
    command
        .current_dir(root)
        .args(["--no-pager", "--no-replace-objects"]);
    command
}
fn git(root: &std::path::Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(capture(git_command(root).args(args))?)?
        .trim_end()
        .to_owned())
}

fn root() -> Result<PathBuf> {
    for path in std::env::current_dir()?.ancestors() {
        if path.join("Cargo.toml").is_file()
            && path.join("xtask/distribution.toml").is_file()
            && path.join("xtask/tools.toml").is_file()
        {
            return Ok(path.canonicalize()?);
        }
    }
    Err("Run repository tasks from the EML Cargo workspace (a source archive requires explicit source identity inputs)".into())
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
        commands::run(&root, args.task)
    })();
    let mut failed = false;
    if let Err(error) = result {
        let _ = writeln!(std::io::stderr(), "{error}");
        failed = true;
    }
    if let Some(controller) = CONTROLLER.get()
        && let Err(error) = controller.finish_helper()
    {
        let _ = writeln!(std::io::stderr(), "Process helper cleanup: {error}");
        failed = true;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
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
