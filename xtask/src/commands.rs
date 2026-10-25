//! Dispatch independent checks and material generation.
use crate::{Result, Task, initialize_controller};
use std::path::Path;
fn print_json(result: Result<serde_json::Value>) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&result?)?);
    Ok(())
}
pub(crate) fn run(root: &Path, task: Task) -> Result<()> {
    if let Task::CiResult {
        event,
        native,
        needs,
    } = task
    {
        return crate::ci::run(&event, native, &needs);
    }
    if matches!(task, Task::Distribution) {
        return crate::distribution::run(root);
    }
    initialize_controller(root)?;
    match task {
        Task::CheckQuality => print_json(crate::quality::run(root)),
        Task::CheckShell => print_json(crate::check::check_shell(root)),
        Task::Notices { out, check, .. } => {
            if check {
                print_json(crate::notices::check(root, out.as_deref()))
            } else {
                crate::notices::run(
                    root,
                    &out.unwrap_or_else(|| root.join("target/generated-legal-materials")),
                )
            }
        }
        Task::Fuzz { seconds } => crate::fuzz::run(root, seconds),
        Task::Mutants { initial, diff } => crate::mutants::run(root, initial, diff.as_deref()),
        Task::CiResult { .. } | Task::Distribution => {
            unreachable!("handled before controller initialization")
        }
    }
}
