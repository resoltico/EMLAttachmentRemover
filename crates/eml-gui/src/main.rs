//! macOS Slint view over the reusable desktop application model.
#[allow(
    unsafe_code,
    unreachable_pub,
    clippy::cognitive_complexity,
    reason = "quality:slint-generated-view"
)]
mod view {
    slint::include_modules!();
}
use controller::Action;
use slint::ComponentHandle;
use slint::winit_030::winit::platform::macos::WindowAttributesExtMacOS;
#[allow(unsafe_code, reason = "quality:native-application-boundary")]
mod application;
mod controller;
#[allow(unsafe_code, reason = "quality:native-desktop-boundary")]
mod native;
#[allow(unsafe_code, reason = "quality:native-preferences-boundary")]
mod preferences;
mod presentation;
fn main() -> std::process::ExitCode {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        native::panic_observation(info);
        previous_hook(info);
    }));
    match start() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("EML Attachment Remover: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn start() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("SLINT_SOFTWARE_RENDERER_PARLEY_DISABLED").is_some() {
        return Err("Unset SLINT_SOFTWARE_RENDERER_PARLEY_DISABLED: this app requires the desktop system-font shaping path".into());
    }
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attributes| {
            attributes.with_file_url_drop_receiver(application::receive_drop)
        })
        .select()?;
    let lifecycle = application::ApplicationHooks::install()?;
    let (interface_font_family, interface_font_size) = native::system_font()?;
    let window = view::MainWindow::new()?;
    window.set_interface_font_family(interface_font_family.clone());
    window.set_interface_font_size(interface_font_size);
    install_native_edit(&window);
    let (about, _licenses) =
        secondary_windows(&window, interface_font_family, interface_font_size)?;
    let _lifecycle_timer = install_event_pump(&window, &about, lifecycle)?;
    window.show()?;
    native::lifecycle_observation("Main window shown; source-location panic hook installed");
    slint::run_event_loop_until_quit()?;
    native::lifecycle_observation("Main event loop returned successfully");
    Ok(())
}

fn install_native_edit(window: &view::MainWindow) {
    // Native text editing is synchronous: queuing Select All could let the
    // next typed character arrive before AppKit changes the selection.
    let weak = window.as_weak();
    window.on_native_copy(move || {
        if weak
            .upgrade()
            .is_some_and(|window| window.get_native_dialog_active())
            && let Err(error) = native::edit(native::EditAction::Copy)
        {
            eprintln!("Native Copy: {error}");
        }
    });
    let weak = window.as_weak();
    window.on_native_select_all(move || {
        if weak
            .upgrade()
            .is_some_and(|window| window.get_native_dialog_active())
            && let Err(error) = native::edit(native::EditAction::SelectAll)
        {
            eprintln!("Native Select All: {error}");
        }
    });
}
fn secondary_windows(
    window: &view::MainWindow,
    interface_font_family: slint::SharedString,
    interface_font_size: f32,
) -> Result<(view::AboutWindow, view::LicenseWindow), Box<dyn std::error::Error>> {
    let about = view::AboutWindow::new()?;
    about.set_interface_font_family(interface_font_family.clone());
    about.set_interface_font_size(interface_font_size);
    about.set_version_label(eml_core::product::VERSION_WITH_BUILD.as_str().into());
    let copyright = include_str!("../../../LICENSE")
        .lines()
        .find_map(|line| line.strip_prefix("Copyright (c) "))
        .ok_or("The project copyright notice is absent")?;
    about.set_copyright(format!("© {copyright}").into());
    about.set_app_icon(native::application_icon()?);
    about.on_website_requested(|| {
        if let Err(error) = native::open_project_website() {
            eprintln!("Project website: {error}");
        }
    });
    let licenses = view::LicenseWindow::new()?;
    licenses.set_interface_font_family(interface_font_family);
    licenses.set_interface_font_size(interface_font_size);
    licenses.set_licenses(include_str!("../../../LICENSE").into());
    let weak_licenses = licenses.as_weak();
    let notice_window = licenses.as_weak();
    licenses.on_third_party_licenses_requested(move || {
        if let Err(error) = native::open_third_party_licenses()
            && let Some(window) = notice_window.upgrade()
        {
            window.set_notice(error.to_string().into());
        }
    });
    let about_licenses = licenses.as_weak();
    about.on_licenses_requested(move || {
        if let Some(licenses) = about_licenses.upgrade()
            && let Err(error) = licenses.show()
        {
            eprintln!("License window: {error}");
        }
    });
    window.on_licenses_requested(move || {
        if let Some(licenses) = weak_licenses.upgrade()
            && let Err(error) = licenses.show()
        {
            eprintln!("License window: {error}");
        }
    });
    Ok((about, licenses))
}
#[derive(Default)]
struct PendingActions {
    actions: std::collections::VecDeque<controller::Action>,
    overflowed: bool,
}
impl PendingActions {
    fn push(&mut self, action: controller::Action) {
        if self.actions.len() < 32 {
            self.actions.push_back(action);
        } else {
            self.overflowed = true;
        }
    }
}
fn bind_actions(window: &view::MainWindow) -> std::rc::Rc<std::cell::RefCell<PendingActions>> {
    let commands = std::rc::Rc::new(std::cell::RefCell::new(PendingActions::default()));
    macro_rules! action {
        ($callback:ident, $action:expr) => {{
            let commands = std::rc::Rc::clone(&commands);
            window.$callback(move || {
                commands.borrow_mut().push($action);
            });
        }};
    }
    action!(on_select_files, Action::Files);
    action!(on_select_folder, Action::Folders);
    action!(on_choose_destination, Action::Destination);
    action!(on_beside_requested, Action::Beside);
    action!(on_stop_requested, Action::Stop);
    action!(on_force_requested, Action::Force);
    action!(on_export_requested, Action::Export);
    action!(on_reveal_requested, Action::Reveal);
    action!(on_cancel_export, Action::CancelExport);
    action!(on_dismiss_requested, Action::Dismiss);
    action!(on_previous_page, Action::Previous);
    action!(on_next_page, Action::Next);
    action!(on_previous_parts, Action::PreviousParts);
    action!(on_next_parts, Action::NextParts);
    action!(on_quit_requested, Action::Quit);
    action!(on_confirm_action, Action::Confirm);
    action!(on_cancel_confirmation, Action::CancelConfirmation);
    action!(on_dismiss_notice, Action::DismissNotice);
    let selected_commands = std::rc::Rc::clone(&commands);
    window.on_select_run(move |index| {
        if let Ok(index) = usize::try_from(index) {
            selected_commands.borrow_mut().push(Action::Select(index));
        }
    });
    let waiting_commands = std::rc::Rc::clone(&commands);
    window.on_remove_waiting(move |index| {
        if let Ok(index) = usize::try_from(index) {
            waiting_commands
                .borrow_mut()
                .push(Action::RemoveWaiting(index));
        }
    });
    let item_commands = std::rc::Rc::clone(&commands);
    window.on_select_item(move |index| {
        if let Ok(index) = usize::try_from(index) {
            item_commands.borrow_mut().push(Action::Item(index));
        }
    });
    commands
}
fn install_event_pump(
    window: &view::MainWindow,
    about: &view::AboutWindow,
    lifecycle: application::ApplicationHooks,
) -> Result<slint::Timer, Box<dyn std::error::Error>> {
    let weak = window.as_weak();
    let weak_about = about.as_weak();
    let commands = bind_actions(window);
    let mut controller = controller::Controller::new()?;
    let lifecycle_timer = slint::Timer::default();
    lifecycle_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(25),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let (overflowed, actions) = {
                let mut pending = commands.borrow_mut();
                (
                    std::mem::take(&mut pending.overflowed),
                    pending.actions.drain(..).collect::<Vec<_>>(),
                )
            };
            if overflowed {
                controller.boundary_failure(
                    "Too many pending UI actions; further processing admission is closed.",
                );
            }
            for action in actions {
                controller.action(&window, action);
            }
            match lifecycle.take_events() {
                Ok(events) => {
                    if events.about
                        && let Some(about) = weak_about.upgrade()
                        && let Err(error) = native::restore(about.window())
                    {
                        controller.system_notice(&format!("About window: {error}"));
                    }
                    if (events.reopen || !events.selections.is_empty())
                        && let Err(error) = native::restore(window.window())
                    {
                        controller.system_notice(&format!("Main window: {error}"));
                    }
                    if events.quit {
                        controller.action(&window, Action::Quit);
                    }
                    for selection in events.selections {
                        controller.ingress(selection);
                    }
                }
                Err(error) => {
                    controller.boundary_failure(error);
                }
            }
            if controller.tick(&window) {
                native::lifecycle_observation(
                    "Confirmed Quit resources settled; requesting loop exit",
                );
                if let Err(error) = slint::quit_event_loop() {
                    controller.boundary_failure(&format!("Quit: {error}"));
                }
            }
        },
    );
    Ok(lifecycle_timer)
}
