//! Rust-only repository verification and distribution commands.
#![forbid(unsafe_code)]
mod campaign;
mod fuzz;
mod mutants;
use clap::{Parser, Subcommand};
use std::{
    error::Error,
    io::Write,
    path::PathBuf,
    process::{Command, ExitCode},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[derive(Parser)]
#[command(about = "EML Attachment Remover repository tasks", version)]
struct Arguments {
    #[command(subcommand)]
    task: Task,
}
#[derive(Subcommand)]
enum Task {
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
    let result = command.output()?;
    if !result.stderr.is_empty() {
        std::io::stderr().write_all(&result.stderr)?;
    }
    if !result.status.success() {
        return Err(format!("Command {command:?} failed with {}", result.status).into());
    }
    Ok(result.stdout)
}
fn git(root: &std::path::Path, args: &[&str]) -> Result<String> {
    Ok(
        String::from_utf8(capture(Command::new("git").current_dir(root).args(args))?)?
            .trim_end()
            .to_owned(),
    )
}
fn root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        String::from_utf8(capture(
            Command::new("git").args(["rev-parse", "--show-toplevel"]),
        )?)?
        .trim_end(),
    ))
}
fn main() -> ExitCode {
    let result = (|| {
        let args = Arguments::parse();
        let root = root()?;
        match args.task {
            Task::Fuzz { seconds } => fuzz::run(&root, seconds),
            Task::Mutants { initial, diff } => mutants::run(&root, initial, diff.as_deref()),
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
