//! Toolkit callbacks dispatch controller actions.
use super::*;
impl DesktopController {
    pub(super) fn bind(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ui.on_new_request(move || {
            if let Some(c) = weak.upgrade() {
                let mut s = c.session.borrow_mut();
                if !s.is_closing() && s.state.confirmation.is_none() {
                    s.state.reviewing = true;
                    s.state.follow_run = None;
                }
                drop(s);
                c.clear_operation();
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.export_ui.on_create_report(move || {
            if let Some(c) = weak.upgrade() {
                c.create_export();
            }
        });
        let weak = Rc::downgrade(self);
        self.export_ui.on_cancel(move || {
            if let Some(c) = weak.upgrade() {
                c.cancel_export();
            }
        });
        let weak = Rc::downgrade(self);
        self.export_ui.window().on_close_requested(move || {
            if let Some(c) = weak.upgrade() {
                if c.export_ui.get_busy() {
                    return slint::CloseRequestResponse::KeepWindowShown;
                }
                c.cancel_export();
            }
            slint::CloseRequestResponse::HideWindow
        });
        let weak = Rc::downgrade(self);
        self.ui.on_start_selection(move || {
            if let Some(c) = weak.upgrade()
                && let Err(error) = c.start_selection()
            {
                c.message(&error.to_string());
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
                s.state.reviewing = true;
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
                s.state.reviewing = true;
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
        self.ui.on_select_run(move |index| {
            if let Some(c) = weak.upgrade() {
                if let Ok(index) = usize::try_from(index)
                    && index < c.session.borrow().entries.len()
                {
                    let mut s = c.session.borrow_mut();
                    let same_context = s.state.selected == index;
                    s.state.selected = index;
                    s.state.reviewing = false;
                    s.entries[index].presentation.visit();
                    s.state.follow_run = None;
                    // A report keeps its view state when reselected, including
                    // after the draft temporarily hid that workspace.
                    if !same_context {
                        s.state.selected_item = 0;
                        if let Some(entry) = s.entries.get_mut(index) {
                            entry.presentation.user_selected_item = false;
                            if let Some(handle) = &entry.report {
                                // Refresh resets the previous report's filter.
                                s.state.selected_item = default_item_selection(handle, false);
                            }
                        }
                    }
                    c.clear_operation();
                }
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_item(move |index| {
            if let Some(c) = weak.upgrade() {
                if let Ok(index) = usize::try_from(index) {
                    c.clear_operation();
                    let mut s = c.session.borrow_mut();
                    s.state.selected_item = index;
                    s.state.follow_run = None;
                    let selected = s.state.selected;
                    if let Some(entry) = s.entries.get_mut(selected) {
                        entry.presentation.user_selected_item = true;
                    }
                }
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_filter_attention(move |enabled| {
            if let Some(c) = weak.upgrade() {
                c.ui.set_messages_scroll_y(0.0);
                c.ui.set_attention_only(enabled);
                let mut s = c.session.borrow_mut();
                if enabled
                    && let Some(handle) = s
                        .entries
                        .get(s.state.selected)
                        .and_then(|entry| entry.report.as_ref())
                {
                    let selected = s.state.selected_item;
                    let first = handle.with_report(|report| {
                        if report
                            .items()
                            .get(selected)
                            .is_some_and(|item| view_model::item(item).attention)
                        {
                            Some(selected)
                        } else {
                            report
                                .items()
                                .iter()
                                .position(|item| view_model::item(item).attention)
                        }
                    });
                    // No selection when the failed-only view has no results.
                    s.state.selected_item = first.flatten().unwrap_or(usize::MAX);
                }
                if !enabled && s.state.selected_item == usize::MAX {
                    s.state.selected_item = 0;
                }
                s.state.follow_run = None;
                drop(s);
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_navigate_message(move |direction| {
            if let Some(c) = weak.upgrade() {
                let rows = c.ui.get_items();
                if rows.row_count() == 0 {
                    return;
                }
                let selected = c.session.borrow().state.selected_item as i32;
                let current = rows
                    .iter()
                    .position(|row| row.original_index == selected)
                    .unwrap_or(0);
                let next = match direction {
                    -2 => 0,
                    2 => rows.row_count() - 1,
                    -1 => current.saturating_sub(1),
                    _ => (current + 1).min(rows.row_count() - 1),
                };
                if let Some(row) = rows.row_data(next) {
                    c.ui.invoke_select_item(row.original_index);
                }
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
                    c.session.borrow_mut().state.queue.remove_waiting(id);
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
            if let Some(c) = weak.upgrade() {
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
                    format!(
                        "Run {}\n\nStops only this task immediately. Waiting requests are not cancelled. Any completed copies remain. The report may be incomplete, and the state of unreported work is unknown.\n\n{}",
                        active.id,
                        vocabulary::FORCE_STOP_NOTICE
                    )
                };
                c.ui.set_force_notice(notice.into());
                let id = c.session.borrow().active.as_ref().expect("active force-stop target").id;
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
                let restore_export = c.confirmation() == Confirmation::Quit;
                c.set_confirmation(None);
                if restore_export && c.export_draft.borrow().is_some() {
                    let _ = c.export_ui.show();
                }
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
