//! Route UI events through the controller's authoritative predicates.
use super::*;
impl DesktopController {
    pub(super) fn bind(self: &Rc<Self>) {
        macro_rules! bind {
            ($method:ident,$body:expr) => {{
                let weak = Rc::downgrade(self);
                self.ui.$method(move || {
                    if let Some(c) = weak.upgrade() {
                        ($body)(&c);
                    }
                });
            }};
        }
        bind!(on_start_selection, |c: &Rc<Self>| c.start_selection());
        bind!(on_start_over, |c: &Rc<Self>| c.start_over());
        bind!(on_stop, |c: &Rc<Self>| c.stop());
        bind!(on_beside_source, |c: &Rc<Self>| c.set_destination(None));
        bind!(on_toggle_problems, |c: &Rc<Self>| {
            if c.can_filter() {
                let mut r = c.runtime.borrow_mut();
                r.batch.problems_only = !r.batch.problems_only;
                drop(r);
                c.refresh();
            }
        });
        bind!(on_clear_selected, |c: &Rc<Self>| {
            c.runtime.borrow_mut().batch.selected = None;
            c.refresh();
        });
        bind!(on_close_window, |c: &Rc<Self>| {
            c.request_quit();
        });
        bind!(on_minimize, |c: &Rc<Self>| c
            .ui
            .window()
            .set_minimized(true));
        bind!(on_cancel_quit, |c: &Rc<Self>| c.cancel_quit());
        bind!(on_stop_and_quit, |c: &Rc<Self>| c.stop_and_quit());
        bind!(on_quit_now, |c: &Rc<Self>| c.quit_now());
        bind!(on_about, |c: &Rc<Self>| {
            c.show_about();
        });
        bind!(on_licenses, |c: &Rc<Self>| {
            c.show_licenses(c.ui.get_ui_size());
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_email(move |key| {
            if let Some(c) = weak.upgrade()
                && !c.locked()
            {
                c.selected(&key);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_navigate_email(move |delta, edge| {
            if let Some(c) = weak.upgrade()
                && !c.locked()
            {
                c.navigate(delta, edge);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_recent_destination(move |index| {
            if let Some(c) = weak.upgrade() {
                let destination = c
                    .runtime
                    .borrow()
                    .preferences
                    .recent_destinations()
                    .get(index as usize)
                    .cloned();
                if let Some(path) = destination {
                    c.set_destination(Some(path));
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.window().on_close_requested(move || {
            if let Some(c) = weak.upgrade() {
                c.request_quit();
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
        let weak = Rc::downgrade(self);
        self.about.on_dismiss(move || {
            if let Some(c) = weak.upgrade() {
                let _ = c.about.hide();
            }
        });
        let weak = Rc::downgrade(self);
        self.about.on_licenses(move || {
            if let Some(c) = weak.upgrade() {
                c.show_licenses(c.about.get_ui_size());
            }
        });
        let weak = Rc::downgrade(self);
        self.licenses.on_dismiss(move || {
            if let Some(c) = weak.upgrade() {
                let _ = c.licenses.hide();
            }
        });
    }
}

impl DesktopController {
    pub(super) fn show_about(&self) {
        self.about.set_ui_size(self.ui.get_ui_size());
        match self.about.show() {
            Ok(()) => self.about.invoke_focus_website(),
            Err(e) => self.message(&e.to_string()),
        }
    }
    pub(super) fn show_licenses(&self, size: f32) {
        self.licenses.set_ui_size(size);
        match self.licenses.show() {
            Ok(()) => self.licenses.invoke_focus_start(),
            Err(e) => self.message(&e.to_string()),
        }
    }
}
