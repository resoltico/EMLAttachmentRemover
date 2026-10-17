//! Native command-line adapter. Business processing remains in eml-core.
#![forbid(unsafe_code)]
mod cli;
mod human;
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(cli::entry())
}
