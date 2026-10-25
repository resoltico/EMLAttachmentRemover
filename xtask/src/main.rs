//! Small distribution and verification entry points. Tool versions live in tools.toml.
mod archive;
mod ci;
mod dist;
mod notices;

use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::Command};
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
    std::env::set_current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap())?;
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
    }
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
    let versions: toml::Value = toml::from_str(include_str!("../tools.toml"))?;
    let expected = versions["versions"][key]
        .as_str()
        .ok_or("missing tool version")?;
    let actual = capture(Command::new(name).arg("--version"))?;
    if !actual
        .split_whitespace()
        .any(|word| word.trim_start_matches('v') == expected)
    {
        return Err(format!("Install {name} {expected} (xtask/tools.toml); found {actual}").into());
    }
    Ok(())
}
