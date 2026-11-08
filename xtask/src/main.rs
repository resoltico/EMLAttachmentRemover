//! Small distribution and verification entry points. Tool versions live in tools.toml.
mod archive;
mod ci;
mod dist;
mod lint;
mod notices;
mod source;
mod tools;

use clap::{Parser, Subcommand};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
struct Arguments {
    #[command(subcommand)]
    command: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Build the release CLI archive, and optionally the universal macOS app.
    Dist {
        #[arg(long)]
        cli: bool,
        #[arg(long)]
        app: bool,
        #[arg(long)]
        universal: bool,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value = "target/dist")]
        out: PathBuf,
    },
    /// Generate notices for the selected shipped graph and actual target(s).
    Notices {
        #[arg(long)]
        app: bool,
        #[arg(long, required = true)]
        target: Vec<String>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Check source/configuration lint without building distribution archives.
    Lint,
    /// Print exact developer-tool pins as key=value lines.
    ToolVersions,
    /// Run the host merge gate. --rust-only runs the native matrix portion.
    Ci {
        #[arg(long)]
        rust_only: bool,
    },
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}
fn execute() -> Result<()> {
    std::env::set_current_dir(workspace_root(&std::env::current_dir()?)?)?;
    match Arguments::parse().command {
        Task::Dist {
            cli,
            app,
            universal,
            target,
            out,
        } => dist::build(cli, app, universal, target, &out),
        Task::Notices { app, target, out } => notices::write(app, &target, &out),
        Task::Ci { rust_only } => ci::run(rust_only),
        Task::Lint => lint::run(),
        Task::ToolVersions => tools::print(),
    }
}
fn workspace_root(from: &Path) -> Result<PathBuf> {
    // A cached executable can have been compiled in another worktree. Cargo's
    // runtime workspace owns this invocation, not an embedded source directory.
    let manifest = capture(
        Command::new("cargo")
            .args(["locate-project", "--workspace", "--message-format", "plain"])
            .current_dir(from),
    )?;
    Path::new(&manifest)
        .parent()
        .map(Path::to_owned)
        .ok_or_else(|| "Cargo did not return a workspace manifest path".into())
}

fn run(command: &mut Command) -> Result<()> {
    eprintln!("+ {command:?}");
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{command:?} exited {status}").into());
    }
    Ok(())
}
fn capture(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "{command:?} exited {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn host() -> Result<String> {
    capture(Command::new("rustc").arg("-vV"))?
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        .ok_or_else(|| "rustc did not report its host triple".into())
}
fn tool(name: &str, key: &str) -> Result<()> {
    let versions = tools::read()?;
    let expected = versions.get(key).ok_or("missing tool version")?;
    let actual = capture(Command::new(name).arg("--version"))?;
    if tools::reported_version(name, &actual) != Some(expected.as_str()) {
        return Err(format!("Install {name} {expected} (xtask/tools.toml); found {actual}").into());
    }
    Ok(())
}

#[cfg(test)]
mod workspace_tests {
    #[test]
    fn invocation_workspace_is_independent_of_the_executables_build_directory() {
        for _ in 0..2 {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("member/src");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                root.path().join("Cargo.toml"),
                "[workspace]\nmembers=[\"member\"]\nresolver=\"3\"\n",
            )
            .unwrap();
            std::fs::write(
                root.path().join("member/Cargo.toml"),
                "[package]\nname=\"workspace-root-fixture\"\nversion=\"0.0.0\"\nedition=\"2024\"\n",
            )
            .unwrap();
            std::fs::write(source.join("lib.rs"), "").unwrap();
            for invocation in [root.path(), source.as_path()] {
                assert_eq!(
                    super::workspace_root(invocation)
                        .unwrap()
                        .canonicalize()
                        .unwrap(),
                    root.path().canonicalize().unwrap()
                );
            }
        }
    }
}
