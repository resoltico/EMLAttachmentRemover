//! Native macOS task admission and focused observation handlers.
use crate::distribution::BuildPlan;
use crate::{Result, Task};
use std::path::Path;

#[cfg(target_os = "macos")]
pub(super) fn run(root: &Path, plan: &BuildPlan, task: Task) -> Result<()> {
    crate::initialize_controller(root)?;
    match task {
        Task::CheckMacosSwift { out } => {
            crate::macos::check_swift_sources(root, plan, &out).map(|_| ())
        }
        Task::ObserveMacosPanel { out } => observe_panel(root, plan, &out),
        Task::QualifyMacosPicker {
            archive,
            reference,
            out,
        } => crate::macos_picker_host::qualify(root, plan, &archive, &reference, &out),
        Task::CheckMacosDesktop { out } => crate::macos::check_desktop(root, plan, &out),
        Task::MacosIconDiagnostic { out } => crate::macos_icon::run(root, plan, &out),
        Task::CompileMacosIcon { out } => crate::macos_icon::compile(root, plan, &out),
        Task::BuildMacosSources { out } => crate::macos_app::build_sources(root, plan, &out),
        Task::CheckMacosVolume { out } => crate::macos_volume::run(root, &out),
        Task::BuildMacosApp { out, icons } => crate::macos_archive::build(root, plan, &out, &icons),
        Task::SmokeMacosApp {
            archive,
            reference,
            operation_fixture,
            operation_reference,
        } => crate::macos_archive::smoke(
            root,
            plan,
            &archive,
            &reference,
            &operation_fixture,
            &operation_reference,
        ),
        Task::CheckMacosPicker { archive, reference } => {
            crate::macos_archive::picker(root, plan, &archive, &reference)
        }
        _ => Err("Internal macOS command dispatch mismatch".into()),
    }
}
#[cfg(target_os = "macos")]
fn observe_panel(root: &Path, plan: &BuildPlan, out: &Path) -> Result<()> {
    let build = crate::macos::load(root, plan)?;
    let observed = crate::macos_panel::observe(root, &build, out)?;
    if observed["process"]["exit_code"] != 0 {
        return Err("Native AppKit panel did not complete visible owned cancellation; inspect retained diagnostic evidence".into());
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub(super) fn run(_root: &Path, _plan: &BuildPlan, task: Task) -> Result<()> {
    let message = match task {
        Task::CheckMacosSwift { .. } => {
            "Selected Xcode is required for Swift bridge/test formatting"
        }
        Task::ObserveMacosPanel { .. } => "Native AppKit panel observation requires macOS",
        Task::QualifyMacosPicker { .. } => {
            "Picker qualification requires its admitted native macOS host"
        }
        Task::CheckMacosDesktop { .. } => "Native macOS is required for desktop profile checks",
        Task::MacosIconDiagnostic { .. } => "Native macOS is required for icon observations",
        Task::CompileMacosIcon { .. } => "Native macOS is required for owner-icon compilation",
        Task::BuildMacosSources { .. } => "Native macOS is required for desktop executable builds",
        Task::CheckMacosVolume { .. } => "Owned exFAT image checks require native macOS",
        Task::BuildMacosApp { .. } => "App assembly requires native macOS",
        Task::SmokeMacosApp { .. } => "App archive smoke requires native macOS",
        Task::CheckMacosPicker { .. } => "Native picker compatibility requires native macOS",
        _ => "Internal macOS command dispatch mismatch",
    };
    Err(message.into())
}
