//! Stable Unix executable for CLI-boundary tests; scenarios are ordinary script data.
#![cfg(unix)]
use std::{os::unix::process::CommandExt, process::Command};

fn main() {
    // The supervisor supplies the scenario directory and normal public CLI args.
    // Exec preserves the owned PID and native signal/exit behavior under test.
    let error = Command::new("/bin/sh")
        .arg("fixture.sh")
        .args(std::env::args_os().skip(1))
        .exec();
    eprintln!("Cannot execute CLI fixture: {error}");
    std::process::exit(127);
}
