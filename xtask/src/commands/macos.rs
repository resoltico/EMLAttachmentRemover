//! Native macOS task admission and focused observation handlers.
use crate::{Result, Task};
use std::path::Path;

#[cfg(target_os = "macos")]
pub(super) fn run(root: &Path, task: Task) -> Result<()> {
    if matches!(task, Task::PrepareMacosPickerHost { .. }) {
        crate::macos_panel::require_picker_preparation_context()?;
    }
    crate::initialize_controller(root)?;
    match task {
        Task::CheckMacosSwift { out } => crate::macos::check_swift_sources(root, &out).map(|_| ()),
        Task::ObserveMacosPanel { out } => observe_panel(root, &out),
        Task::PrepareMacosPickerHost {
            archive,
            reference,
            normal_failure,
            out,
        } => {
            let build = crate::macos::load(root)?;
            crate::macos_panel::prepare_picker_host(
                root,
                &build,
                &archive,
                &reference,
                &normal_failure,
                &out,
            )
        }
        Task::CheckMacosDesktop { out } => crate::macos::check_desktop(root, &out),
        Task::MacosIconDiagnostic { out } => crate::macos_icon::run(root, &out),
        Task::CompileMacosIcon { out } => crate::macos_icon::compile(root, &out),
        Task::BuildMacosSources { out } => crate::macos_app::build_sources(root, &out),
        Task::CheckMacosVolume { out } => crate::macos_volume::run(root, &out),
        Task::BuildMacosApp { out, icons } => crate::macos_archive::build(root, &out, &icons),
        Task::SmokeMacosApp { archive, reference } => {
            crate::macos_archive::smoke(root, &archive, &reference)
        }
        Task::CheckMacosPicker { archive, reference } => {
            crate::macos_archive::picker(root, &archive, &reference)
        }
        _ => Err("Internal macOS command dispatch mismatch".into()),
    }
}
#[cfg(target_os = "macos")]
fn observe_panel(root: &Path, out: &Path) -> Result<()> {
    let build = crate::macos::load(root)?;
    let observed = crate::macos_panel::observe(root, &build, out)?;
    if observed["process"]["exit_code"] != 0 {
        return Err("Native AppKit panel did not complete visible owned cancellation; inspect retained diagnostic evidence".into());
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub(super) fn run(_root: &Path, task: Task) -> Result<()> {
    let message = match task {
        Task::CheckMacosSwift { .. } => {
            "Selected Xcode is required for Swift bridge/test formatting"
        }
        Task::ObserveMacosPanel { .. } => "Native AppKit panel observation requires macOS",
        Task::PrepareMacosPickerHost { .. } => {
            "Controlled picker preparation requires native Intel macOS26"
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
