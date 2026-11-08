//! Standard text commands use editor context independently of Copy Details.
use crate::DesktopController;
use i_slint_core::{
    item_tree::{ItemRc, ItemWeak},
    items::TextInput,
    window::{WindowInner, WindowKind},
};
use slint::ComponentHandle;
use std::{cell::RefCell, pin::Pin, rc::Rc};

#[derive(Clone, Copy)]
enum Command {
    Copy,
    SelectAll,
}
fn with_focused_text(
    window: &slint::Window,
    action: impl FnOnce(&ItemRc, Pin<&TextInput>) -> bool,
) -> bool {
    let focused = WindowInner::from_pub(window).focus_item.borrow().upgrade();
    let Some(item) = focused else { return false };
    let Some(input) = item.clone().downcast::<TextInput>() else {
        return false;
    };
    let input = input.as_pin_ref();
    if !input.has_focus() || !item.is_visible() {
        return false;
    }
    action(&item, input)
}
fn apply(window: &slint::Window, command: Command) -> bool {
    with_focused_text(window, |item, input| {
        let Some(adapter) = item.window_adapter() else {
            return false;
        };
        match command {
            Command::Copy => input.copy(&adapter, item),
            Command::SelectAll => input.select_all(&adapter, item),
        }
        true
    })
}
fn menu_open(window: &slint::Window) -> bool {
    WindowInner::from_pub(window)
        .active_popups()
        .iter()
        .any(|popup| matches!(popup.window_kind, WindowKind::Menu))
}
#[derive(Default)]
struct TextContext(RefCell<ItemWeak>);
impl TextContext {
    fn update(&self, window: &slint::Window) -> bool {
        if with_focused_text(window, |item, _| {
            *self.0.borrow_mut() = item.downgrade();
            true
        }) {
            return true;
        }
        // A menu temporarily owns keyboard focus. Its command context remains
        // the editor it displaced, provided that editor still exists visibly.
        let retained = menu_open(window)
            && self
                .0
                .borrow()
                .upgrade()
                .is_some_and(|item| item.is_visible());
        if !retained {
            *self.0.borrow_mut() = ItemWeak::default();
        }
        retained
    }
}
fn after_menu(window: &slint::Window, action: impl FnOnce() + 'static) {
    if menu_open(window) {
        // Activation precedes normal popup closure and restoration of editor focus.
        slint::Timer::single_shot(std::time::Duration::ZERO, action);
    } else {
        action();
    }
}
impl DesktopController {
    pub(super) fn bind_text_commands(self: &Rc<Self>) {
        macro_rules! bind_focus {
            ($window:expr) => {{
                let weak = $window.as_weak();
                let context = TextContext::default();
                let sync = Rc::new(move || {
                    if let Some(ui) = weak.upgrade() {
                        let eligible = context.update(ui.window());
                        if ui.get_text_copy_eligible() != eligible {
                            ui.set_text_copy_eligible(eligible);
                        }
                    }
                });
                sync();
                let immediate = sync.clone();
                $window.on_text_focus_changed(move || immediate());
                // Conditional inputs are retired after their change handlers.
                // Observe that completed tree update once, not on a polling timer.
                $window.on_text_editors_changed(move || {
                    let committed = sync.clone();
                    slint::Timer::single_shot(std::time::Duration::ZERO, move || committed());
                });
            }};
        }
        bind_focus!(self.ui);
        bind_focus!(self.about);
        bind_focus!(self.licenses);
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
