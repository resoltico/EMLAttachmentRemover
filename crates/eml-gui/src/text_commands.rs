//! Standard text commands use actual editor focus, independently of Copy Details.
use crate::DesktopController;
use i_slint_core::{
    items::TextInput,
    window::{WindowInner, WindowKind},
};
use slint::ComponentHandle;
use std::rc::Rc;

#[derive(Clone, Copy)]
enum Command {
    Copy,
    SelectAll,
}
fn apply(window: &slint::Window, command: Command) -> bool {
    let focused = WindowInner::from_pub(window).focus_item.borrow().upgrade();
    let Some(item) = focused else { return false };
    let Some(input) = item.clone().downcast::<TextInput>() else {
        return false;
    };
    let input = input.as_pin_ref();
    if !input.has_focus() {
        return false;
    }
    let Some(adapter) = item.window_adapter() else {
        return false;
    };
    match command {
        Command::Copy => input.copy(&adapter, &item),
        Command::SelectAll => input.select_all(&adapter, &item),
    }
    true
}
fn after_menu(window: &slint::Window, action: impl FnOnce() + 'static) {
    let menu_open = WindowInner::from_pub(window)
        .active_popups()
        .iter()
        .any(|popup| matches!(popup.window_kind, WindowKind::Menu));
    if menu_open {
        // Menu activation runs before its popup closes. The normal close path
        // restores editor focus; run once after that stack has completed.
        slint::Timer::single_shot(std::time::Duration::ZERO, action);
    } else {
        action();
    }
}
impl DesktopController {
    pub(super) fn bind_text_commands(self: &Rc<Self>) {
        let weak = self.ui.as_weak();
        self.ui.on_copy(move || {
            let Some(ui) = weak.upgrade() else { return };
            let weak = ui.as_weak();
            after_menu(ui.window(), move || {
                if let Some(ui) = weak.upgrade()
                    && !apply(ui.window(), Command::Copy)
                {
                    ui.invoke_copy_details();
                }
            });
        });
        macro_rules! bind_window {
            ($window:expr) => {{
                let weak = $window.as_weak();
                $window.on_copy(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    let weak = ui.as_weak();
                    after_menu(ui.window(), move || {
                        if let Some(ui) = weak.upgrade() {
                            apply(ui.window(), Command::Copy);
                        }
                    });
                });
                let weak = $window.as_weak();
                $window.on_select_all(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    let weak = ui.as_weak();
                    after_menu(ui.window(), move || {
                        if let Some(ui) = weak.upgrade() {
                            apply(ui.window(), Command::SelectAll);
                        }
                    });
                });
            }};
        }
        bind_window!(self.about);
        bind_window!(self.licenses);
    }
}
