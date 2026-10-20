//! macOS Slint view over the reusable desktop application model.
#[allow(
    unsafe_code,
    reason = "Slint-generated vtables and embedding hooks only"
)]
mod view {
    slint::include_modules!();
}
use slint::ComponentHandle;
use slint::winit_030::winit::platform::macos::WindowAttributesExtMacOS;
#[allow(
    unsafe_code,
    reason = "Necessary absent-only native lifecycle, file-open and Services methods"
)]
mod application;
mod controller;
#[allow(
    unsafe_code,
    reason = "Necessary native file ingress, dialogs, responder editing and URL/raw-window boundary"
)]
mod native;
#[allow(
    unsafe_code,
    reason = "Typed Foundation String preference and immutable argument-domain constant"
)]
mod preferences;
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
    let window = view::MainWindow::new()?;
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
    let about = view::AboutWindow::new()?;
    about.set_identity(format!("EML Attachment Remover {}\nSoftware: MPL-2.0\nSlint: Royalty-free Desktop, Mobile, and Web Applications License 2.0",env!("CARGO_PKG_VERSION")).into());
    let weak = about.as_weak();
    window.on_about_requested(move || {
        if let Some(about) = weak.upgrade()
            && let Err(error) = about.show()
        {
            eprintln!("About window: {error}");
        }
    });
    let licenses = view::LicenseWindow::new()?;
    licenses.set_licenses(include_str!("../../../LICENSE").into());
    let weak_licenses = licenses.as_weak();
    window.on_licenses_requested(move || {
        if let Some(licenses) = weak_licenses.upgrade()
            && let Err(error) = licenses.show()
        {
            eprintln!("License window: {error}");
        }
    });
    let weak = window.as_weak();
    let commands = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
    let overflowed = std::rc::Rc::new(std::cell::Cell::new(false));
    macro_rules! action {
        ($callback:ident, $action:expr) => {{
            let commands = std::rc::Rc::clone(&commands);
            let overflowed = std::rc::Rc::clone(&overflowed);
            window.$callback(move || {
                let mut commands = commands.borrow_mut();
                if commands.len() < 32 {
                    commands.push_back($action);
                } else {
                    overflowed.set(true);
                }
            });
        }};
    }
    use controller::Action;
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
    let selected_overflowed = std::rc::Rc::clone(&overflowed);
    window.on_select_run(move |index| {
        if let Ok(index) = usize::try_from(index) {
            let mut commands = selected_commands.borrow_mut();
            if commands.len() < 32 {
                commands.push_back(Action::Select(index));
            } else {
                selected_overflowed.set(true);
            }
        }
    });
    let waiting_commands = std::rc::Rc::clone(&commands);
    let waiting_overflowed = std::rc::Rc::clone(&overflowed);
    window.on_remove_waiting(move |index| {
        if let Ok(index) = usize::try_from(index) {
            let mut commands = waiting_commands.borrow_mut();
            if commands.len() < 32 {
                commands.push_back(Action::RemoveWaiting(index));
            } else {
                waiting_overflowed.set(true);
            }
        }
    });
    let item_commands = std::rc::Rc::clone(&commands);
    let item_overflowed = std::rc::Rc::clone(&overflowed);
    window.on_select_item(move |index| {
        if let Ok(index) = usize::try_from(index) {
            let mut commands = item_commands.borrow_mut();
            if commands.len() < 32 {
                commands.push_back(Action::Item(index));
            } else {
                item_overflowed.set(true);
            }
        }
    });
    let mut controller = controller::Controller::new()?;
    let lifecycle_timer = slint::Timer::default();
    lifecycle_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(25),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if overflowed.replace(false) {
                controller.boundary_failure(
                    "Too many pending UI actions; further processing admission is closed.",
                );
            }
            let actions: Vec<_> = commands.borrow_mut().drain(..).collect();
            for action in actions {
                controller.action(&window, action);
            }
            match lifecycle.take_events() {
                Ok(events) => {
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
    window.show()?;
    native::lifecycle_observation("Main window shown; source-location panic hook installed");
    slint::run_event_loop_until_quit()?;
    native::lifecycle_observation("Main event loop returned successfully");
    Ok(())
}
