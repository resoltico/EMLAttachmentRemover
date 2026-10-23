//! macOS Slint view over the reusable desktop application model.
#[allow(
    unsafe_code,
    unreachable_pub,
    reason = "Only this private module expands Slint-generated component ABI, public generated members and vtable/embedding hooks. Generated public members cannot be made private; authored controller and native bridge code do not inherit these permissions."
)]
mod view {
    slint::include_modules!();
}
use controller::Action;
use slint::ComponentHandle;
use slint::winit_030::winit::platform::macos::{
    EventLoopBuilderExtMacOS, WindowAttributesExtMacOS,
};
#[allow(
    unsafe_code,
    reason = "Typed source-defined application lifecycle callbacks, callback-scoped NSURL conversion and the separately retained Services provider require this narrow native boundary."
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
    let mut event_loop_builder = slint::winit_030::winit::event_loop::EventLoop::with_user_event();
    event_loop_builder.with_application_callback(application::receive_application);
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_event_loop_builder(event_loop_builder)
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
    let commands = bind_actions(&window);
    let _licenses = secondary_windows(
        &window,
        interface_font_family,
        interface_font_size,
        std::rc::Rc::clone(&commands),
    )?;
    let _lifecycle_timer = install_event_pump(&window, lifecycle, commands)?;
    window.show()?;
    native::lifecycle_observation("Main window shown; source-location panic hook installed");
    slint::run_event_loop_until_quit()?;
    native::lifecycle_observation("Main event loop returned successfully");
    Ok(())
}

fn install_native_edit(window: &view::MainWindow) {
    // Dispatch standard native actions synchronously to the active responder.
    // Native controls may finish their visible updates after dispatch returns.
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
    window.on_native_paste(move || {
        if weak
            .upgrade()
            .is_some_and(|window| window.get_native_dialog_active())
            && let Err(error) = native::edit(native::EditAction::Paste)
        {
            eprintln!("Native Paste: {error}");
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
    commands: std::rc::Rc<std::cell::RefCell<PendingActions>>,
) -> Result<view::LicenseWindow, Box<dyn std::error::Error>> {
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
    window.on_licenses_requested(move || {
        if let Some(licenses) = weak_licenses.upgrade()
            && let Err(error) = licenses.show()
        {
            commands
                .borrow_mut()
                .push(Action::SystemNotice(format!("License window: {error}")));
        }
    });
    Ok(licenses)
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
    action!(on_website_requested, Action::Website);
    action!(on_select_folder, Action::Folders);
    action!(on_choose_destination, Action::Destination);
    action!(on_beside_requested, Action::Beside);

    action!(on_export_requested, Action::Export);
    action!(on_start_export, Action::StartExport);

    action!(on_close_export_form, Action::CloseExportForm);
    action!(on_create_from_check, Action::CreateFromCheck);
    let mode_commands = std::rc::Rc::clone(&commands);
    window.on_mode_changed(move |check| mode_commands.borrow_mut().push(Action::Mode(check)));
    let export_commands = std::rc::Rc::clone(&commands);
    window.on_export_name_changed(move |name| {
        export_commands
            .borrow_mut()
            .push(Action::ExportName(name.to_string()))
    });
    action!(on_reveal_requested, Action::Reveal);
    macro_rules! operation_action {
        ($callback:ident, $variant:ident) => {{
            let commands = std::rc::Rc::clone(&commands);
            window.$callback(move |identity| {
                if let Some(id) = identity
                    .parse()
                    .ok()
                    .and_then(eml_protocol::desktop::OperationId::new)
                {
                    commands.borrow_mut().push(Action::$variant(id));
                }
            });
        }};
    }
    operation_action!(on_stop_requested, Stop);
    operation_action!(on_force_requested, Force);
    operation_action!(on_cancel_export, CancelExport);
    operation_action!(on_force_export, ForceExport);
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
    lifecycle: application::ApplicationHooks,
    commands: std::rc::Rc<std::cell::RefCell<PendingActions>>,
) -> Result<slint::Timer, Box<dyn std::error::Error>> {
    let weak = window.as_weak();
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
            let events = lifecycle.take_events();
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
            for diagnostic in events.diagnostics {
                controller.system_notice(diagnostic);
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
