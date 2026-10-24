//! Flat CLI commands dispatch to focused repository operations.
mod macos;
mod shared;
mod windows;
use crate::{
    Result, Task,
    distribution::{BuildContext, BuildPlan},
    initialize_controller,
};
use std::path::Path;

pub(crate) fn run(root: &Path, task: Task) -> Result<()> {
    let context = if matches!(
        &task,
        Task::CiResult { .. }
            | Task::CheckConfig
            | Task::CheckShell
            | Task::WindowsRuntimeReferences { .. }
            | Task::Fuzz { .. }
            | Task::Mutants { .. }
            | Task::CliSmokeTests { .. }
            | Task::CheckHostIncidents { .. }
    ) {
        None
    } else {
        Some(BuildContext::load(root)?)
    };
    let plan = || -> Result<&BuildPlan> {
        context
            .as_ref()
            .map(BuildContext::plan)
            .ok_or_else(|| "This command requires an admitted build context".into())
    };
    match task {
        Task::CiResult {
            event,
            native,
            needs,
        } => crate::ci::run(&event, native, &needs),
        Task::Distribution { github_output } => crate::distribution::run(
            context.as_ref().ok_or("Missing build context")?,
            github_output.as_deref(),
            &mut std::io::stdout().lock(),
        ),
        task @ (Task::ObserveWindowsLink { .. } | Task::PrepareWindowsWorkspace) => {
            windows::run(root, plan()?, task)
        }
        task @ (Task::CheckMacosSwift { .. }
        | Task::ObserveMacosPanel { .. }
        | Task::QualifyMacosPicker { .. }
        | Task::CheckMacosDesktop { .. }
        | Task::MacosIconDiagnostic { .. }
        | Task::CompileMacosIcon { .. }
        | Task::BuildMacosSources { .. }
        | Task::CheckMacosVolume { .. }
        | Task::BuildMacosApp { .. }
        | Task::SmokeMacosApp { .. }
        | Task::CheckMacosPicker { .. }) => macos::run(root, plan()?, task),
        Task::WindowsRuntimeReferences { archives, out } => controlled(root, || {
            crate::windows_references::generate(root, &archives, &out)
        }),
        Task::ReleaseCheck { source, version } => controlled(root, || {
            crate::release_check::run(root, plan()?, &source, &version)
        }),
        Task::StageReleaseAssets { inputs, out } => controlled(root, || {
            crate::release_assets::run(root, plan()?, &inputs, &out)
        }),
        Task::CheckReleaseServer {
            inputs,
            assets,
            release,
        } => controlled(root, || {
            crate::release_assets::server(root, plan()?, &assets, &release, &inputs)
        }),
        Task::NativeTests {
            target,
            out,
            release,
        } => controlled(root, || {
            shared::native_tests(root, plan()?, &target, &out, release)
        }),
        Task::Check => controlled(root, || crate::check::run(root, plan()?)),
        Task::CheckDependencies { out } => controlled(root, || {
            crate::dependencies::run(root, plan()?, &out).map(|_| ())
        }),
        Task::VerifyReleaseInputs {
            products,
            desktop,
            picker,
            out,
        } => controlled(root, || {
            crate::release_inputs::run(
                context.as_ref().ok_or("Missing build context")?,
                &products,
                &desktop,
                &picker,
                &out,
            )
        }),
        Task::CheckHostIncidents { out } => {
            controlled(root, || crate::host_incidents::check(root, &out))
        }
        Task::CheckProductIdentity { previous_info } => controlled(root, || {
            shared::print_json(crate::release_identity::check(
                &previous_info,
                plan()?.version(),
                eml_core::product::BUILD_NUMBER,
            ))
        }),
        Task::CheckConfig => controlled(root, || {
            shared::print_json(
                crate::repository_config::check(root)
                    .and_then(|value| Ok(serde_json::to_value(value)?)),
            )
        }),
        Task::CheckShell => {
            controlled(root, || shared::print_json(crate::check::check_shell(root)))
        }
        Task::BuildCliArchive { target, out } => controlled(root, || {
            shared::build_cli_archive(root, plan()?, &target, &out)
        }),
        Task::CliSmokeTests { executable, out } => {
            controlled(root, || shared::cli_smoke(root, &executable, &out))
        }
        Task::Qualify { target, out, icons } => controlled(root, || {
            crate::qualify::run(root, plan()?, &target, &out, icons.as_deref())
        }),
        Task::VerifyArchive { archive, target } => controlled(root, || {
            crate::archive_verify::run(root, plan()?, &archive, &target)
        }),
        Task::Notices { out, check, .. } => controlled(root, || {
            shared::notices(root, plan()?, out.as_deref(), check)
        }),
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
