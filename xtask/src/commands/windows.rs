//! Native Windows command admission precedes any process-controller setup.
use crate::{Result, Task};
use std::path::Path;

#[cfg(windows)]
pub(super) fn run(root: &Path, task: Task) -> Result<()> {
    crate::initialize_controller(root)?;
    match task {
        Task::PrepareWindowsWorkspace => crate::windows_workspace::prepare(root),
        Task::ObserveWindowsLink { target, out } => observe(root, &target, &out),
        _ => Err("Internal Windows command dispatch mismatch".into()),
    }
}
#[cfg(not(windows))]
pub(super) fn run(_root: &Path, task: Task) -> Result<()> {
    match task {
        Task::PrepareWindowsWorkspace => {
            Err("Protected Windows checkout preparation requires native Windows".into())
        }
        Task::ObserveWindowsLink { .. } => {
            Err("Final Windows link observation requires native Windows".into())
        }
        _ => Err("Internal Windows command dispatch mismatch".into()),
    }
}
#[cfg(windows)]
fn observe(root: &Path, target: &str, out: &Path) -> Result<()> {
    std::fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let cli = crate::cli_build::build(root, target, &out.join("cli-build"))?;
    let unknown_input = crate::cli_build::unknown_input_control(
        root,
        target,
        &cli,
        &out.join("unknown-input-control"),
    )?;
    let materials = crate::notices::checked_target(root, cli.platform())?;
    let archive = crate::qualify::staged_cli(root, &cli, &materials, &out)?;
    std::fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"target":cli.target(),"host":cli.host(),"image":cli.identity(),"unknown_input_control":unknown_input,"archive":archive,"scope":"native compiler/final-link/PE/input admission, actual unknown-object rejection and extracted CLI archive behavior; full qualification remains separate"}),
        )?,
    )?;
    Ok(())
}
