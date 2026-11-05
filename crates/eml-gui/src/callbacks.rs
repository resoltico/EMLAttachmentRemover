//! Toolkit callbacks dispatch controller actions.
use super::*;
impl DesktopController {
    pub(super) fn bind(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ui.on_new_request(move || {
            if let Some(c) = weak.upgrade() {
                let mut s = c.session.borrow_mut();
                if !s.is_closing() && s.state.confirmation.is_none() {
                    s.state.view = state::View::Draft;
                }
                drop(s);
                c.clear_operation();
                c.maintain_reports();
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_start_selection(move || {
            if let Some(c) = weak.upgrade()
                && let Err(error) = c.start_selection()
            {
                c.message(&wording::queue_error(&error));
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_clear_selection(move || {
            if let Some(c) = weak.upgrade() {
                let mut s = c.session.borrow_mut();
                if s.state.confirmation.is_some() || s.is_closing() {
                    return;
                }
                s.state.selection.clear();
                s.state.view = state::View::Draft;
                drop(s);
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_remove_selection(move |index| {
            if let Some(c) = weak.upgrade() {
                if !c.inputs_enabled() {
                    return;
                }
                let mut s = c.session.borrow_mut();
                if let Ok(index) = usize::try_from(index)
                    && index < s.state.selection.len()
                {
                    s.state.selection.remove(index);
                }
                s.state.view = state::View::Draft;
                drop(s);
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_mode_changed(move |check| {
            if let Some(c) = weak.upgrade() {
                if !c.inputs_enabled() {
                    return;
                }
                c.clear_operation();
                c.session.borrow_mut().state.mode = if check {
                    RunMode::DryRun
                } else {
                    RunMode::Create
                };
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_session(move |token| {
            if let Some(c) = weak.upgrade() {
                c.select_session(token.as_str());
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_close_session(move |token| {
            if let Some(c) = weak.upgrade() {
                c.close_session(token.as_str());
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_run(move |index| {
            if let Some(c) = weak.upgrade()
                && let Ok(index) = usize::try_from(index)
            {
                c.select_run(index);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_item(move |index| {
            if let Some(c) = weak.upgrade()
                && let Ok(index) = usize::try_from(index)
            {
                c.select_item(index);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_filter_results(move |filter| {
            if let Some(c) = weak.upgrade() {
                c.filter_results(filter);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_sort_results(move |column| {
            if let Some(c) = weak.upgrade() {
                c.sort_results(column);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_navigate_message(move |direction, page| {
            if let Some(c) = weak.upgrade() {
                c.navigate_message(direction, page.max(1) as usize);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_type_select(move |prefix| {
            if let Some(c) = weak.upgrade() {
                c.type_select(prefix.as_str());
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_waiting(move |index| {
            if let Some(c) = weak.upgrade()
                && let Ok(index) = usize::try_from(index)
            {
                c.select_waiting(index);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_remove_waiting(move |index| {
            if let Some(c) = weak.upgrade() {
                let id = usize::try_from(index).ok().and_then(|index| {
                    c.session
                        .borrow()
                        .state
                        .queue
                        .waiting_requests()
                        .nth(index)
                        .map(|(id, _)| id)
                });
                if let Some(id) = id {
                    let mut s = c.session.borrow_mut();
                    s.state.queue.remove_waiting(id);
                    if s.state.view == state::View::Waiting(id) {
                        s.state.view = state::View::Draft;
                    }
                }
                c.clear_operation();
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_beside_source(move || {
            if let Some(c) = weak.upgrade() {
                c.set_destination(None);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_stop(move || {
            if let Some(c) = weak.upgrade()
                && c.can_stop()
            {
                c.stop();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_force_stop(move || {
            if let Some(c) = weak.upgrade() {
                if c.confirmation() != Confirmation::None {
                    return;
                }
                let notice = {
                    let s = c.session.borrow();
                    if s.is_closing() {
                        return;
                    }
                    let Some(active) = &s.active else {
                        return;
                    };
                    if !active.stop.can_force(now_ms()) {
                        return;
                    }
                    wording::force_notice(active.id)
                };
                c.ui.set_force_notice(notice.into());
                let id = c
                    .session
                    .borrow()
                    .active
                    .as_ref()
                    .expect("active force-stop target")
                    .id;
                c.set_confirmation(Some(state::PendingConfirmation::ForceStop(id)));
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_confirm_force_stop(move || {
            if let Some(c) = weak.upgrade() {
                let valid = {
                    let s = c.session.borrow();
                    matches!(&s.state.confirmation, Some(state::PendingConfirmation::ForceStop(id)) if s.active.as_ref().is_some_and(|active| active.id == *id))
                };
                if !valid { return; }
                c.set_confirmation(None);
                c.force_stop();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_review_request(move || {
            if let Some(c) = weak.upgrade() {
                c.review_request();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_create_from_check(move || {
            if let Some(c) = weak.upgrade() {
                c.create_from_check();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_confirm_replace_draft(move || {
            if let Some(c) = weak.upgrade() {
                let pending = c.session.borrow_mut().state.confirmation.take();
                if let Some(state::PendingConfirmation::ReplaceDraft(snapshot)) = pending {
                    c.ui.set_confirmation(Confirmation::None);
                    c.apply_snapshot(snapshot);
                } else {
                    c.session.borrow_mut().state.confirmation = pending;
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_recent_destination(move |index| {
            if let Some(c) = weak.upgrade() {
                let path = usize::try_from(index).ok().and_then(|index| {
                    c.session
                        .borrow()
                        .preferences
                        .recent_destinations()
                        .get(index)
                        .cloned()
                });
                if let Some(path) = path {
                    c.set_destination(Some(path));
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_close_run(move |index| {
            if let Some(c) = weak.upgrade()
                && let Ok(index) = usize::try_from(index)
            {
                c.dismiss_run(index);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_dismiss(move || {
            if let Some(c) = weak.upgrade() {
                c.dismiss();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_close_window(move || {
            if let Some(c) = weak.upgrade() {
                c.request_quit();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_copy_context(move || {
            if let Some(c) = weak.upgrade() {
                c.copy_context();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_all_text(move || {
            if let Some(c) = weak.upgrade() {
                c.select_all_text();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_minimize(move || {
            if let Some(c) = weak.upgrade() {
                c.ui.window().set_minimized(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_licenses(move || {
            if let Some(c) = weak.upgrade() {
                c.about.invoke_licenses();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_about(move || {
            if let Some(c) = weak.upgrade() {
                c.about.set_ui_size(c.ui.get_ui_size());
                match c.about.show() {
                    Ok(()) => c.about.invoke_focus_website(),
                    Err(error) => c.message(&error.to_string()),
                }
            }
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
                c.licenses.set_ui_size(c.about.get_ui_size());
                match c.licenses.show() {
                    Ok(()) => c.licenses.invoke_focus_start(),
                    Err(error) => c.message(&error.to_string()),
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.licenses.on_dismiss(move || {
            if let Some(c) = weak.upgrade() {
                let _ = c.licenses.hide();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_cancel_confirmation(move || {
            if let Some(c) = weak.upgrade() {
                c.set_confirmation(None);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_stop_and_quit(move || {
            if let Some(c) = weak.upgrade() {
                if c.confirmation() != Confirmation::Quit {
                    return;
                }
                {
                    let mut s = c.session.borrow_mut();
                    s.quit_when_idle = true;
                    s.state.queue.cancel_waiting();
                }
                c.set_confirmation(None);
                c.stop();
                c.tick();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_force_and_quit(move || {
            if let Some(c) = weak.upgrade() {
                if c.confirmation() != Confirmation::Quit {
                    return;
                }
                {
                    let mut s = c.session.borrow_mut();
                    s.force_quit = true;
                    s.state.queue.cancel_waiting();
                }
                c.set_confirmation(None);
                c.force_stop();
                c.tick();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.window().on_close_requested(move || {
            if let Some(c) = weak.upgrade()
                && !c.request_quit()
            {
                return slint::CloseRequestResponse::KeepWindowShown;
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
}
