//! Isolated native adapter probe: record typed requests, then execute real core.
#![deny(unsafe_code)]
use eml_core::{InvalidRequest, RunControl, RunMode, RunOutcome, RunRequest};
use std::{
    fs::{self, OpenOptions},
    io::Write,
};
fn record_core(
    request: &RunRequest,
    control: &RunControl,
    sink: &mut eml_gui::RunSink,
) -> Result<RunOutcome, InvalidRequest> {
    if request.mode != RunMode::DryRun {
        return Err(InvalidRequest::DryRunModeRequired);
    }
    let result = eml_core::run(request, control, sink);
    let executable = std::env::current_exe().expect("native probe executable");
    let root = executable
        .ancestors()
        .nth(4)
        .expect("isolated probe bundle layout");
    let mut paths = Vec::new();
    for path in &request.paths {
        paths.extend_from_slice(path.as_os_str().as_encoded_bytes());
        paths.push(0);
    }
    fs::write(root.join("request.paths"), paths).expect("record native request paths");
    fs::write(root.join("owner.pid"), std::process::id().to_string()).expect("record native owner");
    fs::write(root.join("mode"), b"dry-run").expect("record read-only mode");
    OpenOptions::new()
        .write(true)
        .open(root.join("arrival"))
        .expect("native probe arrival pipe")
        .write_all(b"1")
        .expect("notify native observer");
    result
}
fn main() {
    if let Err(error) = eml_gui::run_desktop(record_core) {
        eprintln!("Native document probe: {error}");
        std::process::exit(1);
    }
}
