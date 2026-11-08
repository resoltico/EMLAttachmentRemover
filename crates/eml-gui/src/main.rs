//! Native window ingress around the platform-neutral Slint controller.
#![deny(unsafe_code)]
use eml_gui::{DesktopController, ingress, platform, preferences::Preferences};
use slint::ComponentHandle;
use std::io::Write;
fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {error}", eml_core::product::DISPLAY_NAME);
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && (arguments[0] == "--version" || arguments[0] == "-V") {
        writeln!(
            std::io::stdout().lock(),
            "emlattachmentremover-gui {}",
            eml_core::product::VERSION_WITH_REVISION.as_str()
        )?;
        return Ok(());
    }
    let executable = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    let cli = eml_app::bundled_cli(&executable)?;
    #[cfg(target_os = "linux")]
    let cli = executable.with_file_name(eml_core::product::CLI_NAME);
    platform::validate_launch(&executable, &cli)?;
    let home = dirs::home_dir().ok_or(eml_gui::HOME_UNAVAILABLE)?;
    let preferences = Preferences::load(Preferences::default_path()?);
    let controller = DesktopController::new(cli, home, preferences)?;
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
