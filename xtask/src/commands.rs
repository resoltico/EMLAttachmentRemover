//! Flat CLI commands dispatch to focused repository operations.
mod macos;
mod shared;
mod windows;
use crate::{Result, Task, initialize_controller};
use std::path::Path;

pub(crate) fn run(root: &Path, task: Task) -> Result<()> {
    match task {
        Task::CiResult {
            event,
            native,
            needs,
        } => crate::ci::run(&event, native, &needs),
        Task::Distribution => crate::distribution::run(root),
        task @ (Task::ObserveWindowsLink { .. } | Task::PrepareWindowsWorkspace) => {
            windows::run(root, task)
        }
        task @ (Task::CheckMacosSwift { .. }
        | Task::ObserveMacosPanel { .. }
        | Task::PrepareMacosPickerHost { .. }
        | Task::VerifyMacosPickerHost { .. }
        | Task::CheckMacosDesktop { .. }
        | Task::MacosIconDiagnostic { .. }
        | Task::CompileMacosIcon { .. }
        | Task::BuildMacosSources { .. }
        | Task::CheckMacosVolume { .. }
        | Task::BuildMacosApp { .. }
        | Task::SmokeMacosApp { .. }
        | Task::CheckMacosPicker { .. }) => macos::run(root, task),
        Task::WindowsRuntimeReferences { archives, out } => controlled(root, || {
            crate::windows_references::generate(root, &archives, &out)
        }),
        Task::ReleaseCheck { source, version } => {
            controlled(root, || crate::release_check::run(root, &source, &version))
        }
        Task::StageReleaseAssets { archives, out } => {
            controlled(root, || crate::release_assets::run(root, &archives, &out))
        }
        Task::CheckReleaseServer { assets, release } => controlled(root, || {
            crate::release_assets::server(root, &assets, &release)
        }),
        Task::NativeTests {
            target,
            out,
            release,
        } => controlled(root, || shared::native_tests(root, &target, &out, release)),
        Task::Check => controlled(root, || crate::check::run(root)),
        Task::CheckQuality => controlled(root, || shared::print_json(crate::quality::run(root))),
        Task::CheckShell => {
            controlled(root, || shared::print_json(crate::check::check_shell(root)))
        }
        Task::BuildCliArchive { target, out } => {
            controlled(root, || shared::build_cli_archive(root, &target, &out))
        }
        Task::CliSmokeTests { executable, out } => {
            controlled(root, || shared::cli_smoke(root, &executable, &out))
        }
        Task::Qualify { target, out, icons } => controlled(root, || {
            crate::qualify::run(root, &target, &out, icons.as_deref())
        }),
        Task::VerifyArchive { archive, target } => {
            controlled(root, || crate::archive_verify::run(root, &archive, &target))
        }
        Task::Notices { out, check, .. } => {
            controlled(root, || shared::notices(root, out.as_deref(), check))
        }
        Task::Fuzz { seconds } => controlled(root, || crate::fuzz::run(root, seconds)),
        Task::Mutants { initial, diff } => {
            controlled(root, || crate::mutants::run(root, initial, diff.as_deref()))
        }
    }
}
fn controlled(root: &Path, run: impl FnOnce() -> Result<()>) -> Result<()> {
    initialize_controller(root)?;
    run()
}
