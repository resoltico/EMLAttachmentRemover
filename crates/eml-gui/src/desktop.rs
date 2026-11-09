//! Native desktop startup and event-loop ownership shared with adapter probes.
use crate::{DesktopController, ingress, platform, preferences::Preferences};
use slint::ComponentHandle;
use std::io::Write;
/// Run the native desktop, keeping Slint on its calling thread and core work on workers.
/// Production supplies `eml_core::run`; native adapter probes use the same runner seam.
pub fn run_desktop(
    runner: impl Fn(
        &eml_core::RunRequest,
        &eml_core::RunControl,
        &mut crate::RunSink,
    ) -> Result<eml_core::RunOutcome, eml_core::InvalidRequest>
    + Send
    + Sync
    + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && (arguments[0] == "--version" || arguments[0] == "-V") {
        writeln!(
            std::io::stdout().lock(),
            "emlattachmentremover-gui {}",
            eml_core::product::VERSION_WITH_REVISION.as_str()
        )?;
        return Ok(());
    }
    let home = dirs::home_dir().ok_or(crate::HOME_UNAVAILABLE)?;
    let preferences = Preferences::load(Preferences::default_path()?);
    let controller = DesktopController::new(runner, home, preferences)?;
    let ui = controller.window();
    ui.show()?;
    platform::install()?;
    ingress::install_native(&controller)?;
    let paths = ingress::launch_paths(arguments, &std::env::current_dir()?)?;
    if !paths.is_empty() {
        controller.open_documents(paths);
    }
    let timer = slint::Timer::default();
    let weak = std::rc::Rc::downgrade(&controller);
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            if let Some(controller) = weak.upgrade() {
                controller.tick();
                if controller.should_exit() {
                    let _ = slint::quit_event_loop();
                }
            }
        },
    );
    slint::run_event_loop()?;
    Ok(())
}
