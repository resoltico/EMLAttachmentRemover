//! Native window ingress around the platform-neutral Slint controller.
#![deny(unsafe_code)]
use eml_gui::{DesktopController, ingress, platform, preferences::Preferences};
use slint::ComponentHandle;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let cli = eml_app::bundled_cli(&executable)?;
    let home = dirs::home_dir().ok_or("Cannot locate your home folder")?;
    let preferences = Preferences::load(Preferences::default_path()?)?;
    let controller = DesktopController::new(cli, home, preferences)?;
    let ui = controller.window();
    ui.show()?;
    platform::install()?;
    ingress::install_native(&controller)?;
    let paths = ingress::launch_paths(std::env::args_os().skip(1), &std::env::current_dir()?)?;
    if !paths.is_empty() {
        if let Err(error) = controller.enqueue(paths) {
            controller.message(&error.to_string());
        }
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
