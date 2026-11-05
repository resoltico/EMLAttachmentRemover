//! One enablement predicate per action, shared by view projection and action boundaries.
use super::*;
impl DesktopController {
    pub(super) fn can_start(&self) -> bool {
        let s = self.session.borrow();
        !s.state.selection.is_empty() && s.admission_error().is_none()
    }
    pub(super) fn can_stop(&self) -> bool {
        self.inputs_enabled()
            && self
                .session
                .borrow()
                .active
                .as_ref()
                .is_some_and(|active| active.stop == state::Stop::Running)
    }
    pub(super) fn can_force(&self) -> bool {
        self.inputs_enabled()
            && self
                .session
                .borrow()
                .active
                .as_ref()
                .is_some_and(|active| active.stop.can_force(now_ms()))
    }
    pub(super) fn can_review(&self) -> bool {
        self.inputs_enabled()
            && self.session.borrow().selected_entry().is_some_and(|entry| {
                completion_state(entry).is_some() || entry.launch_error.is_some()
            })
    }
    pub(super) fn can_convert_check(&self) -> bool {
        self.can_review()
            && self
                .session
                .borrow()
                .selected_entry()
                .is_some_and(can_create)
    }
    pub(super) fn can_save_report(&self) -> bool {
        self.inputs_enabled() && self.pending_save.borrow().is_none() && {
            let s = self.session.borrow();
            s.export.is_none()
                && s.selected_entry()
                    .and_then(|entry| entry.completion.as_ref())
                    .is_some_and(|completion| !completion.jsonl.is_empty())
        }
    }
    pub(super) fn can_filter(&self) -> bool {
        self.inputs_enabled()
            && self
                .session
                .borrow()
                .selected_entry()
                .is_some_and(|entry| entry.report.is_some())
    }
    pub(super) fn can_details(&self) -> bool {
        self.inputs_enabled()
            && self
                .session
                .borrow()
                .selected_entry()
                .is_some_and(|entry| entry.presentation.selected.is_some())
    }
    pub(super) fn can_sort(&self) -> bool {
        self.inputs_enabled()
            && self
                .session
                .borrow()
                .selected_entry()
                .is_some_and(|entry| entry.report.is_some())
    }
    fn main_open_pending(&self) -> bool {
        self.open_jobs
            .borrow()
            .iter()
            .any(|job| job.target == OpenTarget::Main)
    }
    pub(super) fn can_show_output(&self) -> bool {
        self.inputs_enabled() && !self.main_open_pending() && !self.output_roots().is_empty()
    }
    pub(super) fn can_show_copy(&self) -> bool {
        self.inputs_enabled() && !self.main_open_pending() && self.selected_copy().is_some()
    }
    pub(super) fn can_show_original(&self) -> bool {
        self.inputs_enabled() && !self.main_open_pending() && self.selected_original().is_some()
    }
    pub(super) fn can_copy_path(&self, destination: bool) -> bool {
        self.inputs_enabled()
            && self
                .selected_path(destination)
                .is_some_and(|path| path.to_str().is_some())
    }
    /// Native clipboard text is never reconstructed from escaped display wording.
    pub fn clipboard_text(&self, destination: bool) -> Option<String> {
        if !self.can_copy_path(destination) {
            return None;
        }
        self.selected_path(destination)?
            .into_os_string()
            .into_string()
            .ok()
    }
    pub(super) fn can_show_report(&self) -> bool {
        self.inputs_enabled() && !self.main_open_pending() && self.saved_report_path().is_some()
    }
    fn focused_text(&self) -> Option<i_slint_core::item_tree::ItemRc> {
        let item = i_slint_core::window::WindowInner::from_pub(self.ui.window())
            .focus_item
            .borrow()
            .upgrade()?;
        item.downcast::<i_slint_core::items::TextInput>()
            .map(|_| item)
    }
    pub(super) fn update_text_actions(&self) {
        let text = self.focused_text().is_some() && self.inputs_enabled();
        self.ui.set_can_select_all(text);
        self.ui
            .set_can_copy_menu(self.inputs_enabled() && (text || self.can_copy_path(false)));
    }
    pub(super) fn copy_context(&self) {
        if !self.inputs_enabled() {
            return;
        }
        if let Some(item) = self.focused_text() {
            if let Some(text) = item.downcast::<i_slint_core::items::TextInput>() {
                text.as_pin_ref().copy(
                    &i_slint_core::window::WindowInner::from_pub(self.ui.window()).window_adapter(),
                    &item,
                );
            }
        } else if self.can_copy_path(false) {
            self.ui.invoke_copy_source_path();
        }
    }
    pub(super) fn select_all_text(&self) {
        if !self.inputs_enabled() {
            return;
        }
        if let Some(item) = self.focused_text()
            && let Some(text) = item.downcast::<i_slint_core::items::TextInput>()
        {
            text.as_pin_ref().select_all(
                &i_slint_core::window::WindowInner::from_pub(self.ui.window()).window_adapter(),
                &item,
            );
        }
    }
}
