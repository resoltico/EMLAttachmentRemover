//! Run-local selection, filter and sort actions; report order remains immutable.
use super::*;
impl DesktopController {
    pub(super) fn select_run(&self, index: usize) {
        let mut s = self.session.borrow_mut();
        if s.is_closing() || s.state.confirmation.is_some() {
            return;
        }
        let Some(entry) = s.entries.get_mut(index) else {
            return;
        };
        entry.presentation.visit();
        let id = entry.id;
        s.state.view = state::View::Run(id);
        drop(s);
        self.clear_operation();
        self.maintain_reports();
        self.refresh();
    }
    pub(super) fn select_waiting(&self, index: usize) {
        let mut s = self.session.borrow_mut();
        if s.is_closing() || s.state.confirmation.is_some() {
            return;
        }
        let id = s
            .state
            .queue
            .waiting_requests()
            .nth(index)
            .map(|(id, _)| id);
        if let Some(id) = id {
            s.state.view = state::View::Waiting(id);
        }
        drop(s);
        self.clear_operation();
        self.refresh();
    }
    pub(super) fn select_item(&self, index: usize) {
        let mut s = self.session.borrow_mut();
        let Some(entry) = s.selected_entry_mut() else {
            return;
        };
        if entry
            .report
            .as_ref()
            .and_then(|handle| handle.with_report(|report| index < report.items().len()))
            != Some(true)
        {
            return;
        }
        entry.presentation.selected = Some(index);
        entry.presentation.initialized = true;
        drop(s);
        self.clear_operation();
        self.refresh();
    }
    pub(super) fn filter_results(&self, index: i32) {
        let Some(filter) = state::Filter::from_index(index) else {
            return;
        };
        let mut s = self.session.borrow_mut();
        let Some(entry) = s.selected_entry_mut() else {
            return;
        };
        entry.presentation.filter = filter;
        entry.presentation.initialized = true;
        if let Some(categories) = entry.report.as_ref().and_then(|handle| {
            handle.with_report(|report| {
                report
                    .items()
                    .iter()
                    .map(view_model::category)
                    .collect::<Vec<_>>()
            })
        }) {
            entry
                .presentation
                .reconcile(&categories, entry.completion.is_some());
        }
        drop(s);
        self.clear_operation();
        self.ui.set_messages_scroll_y(0.0);
        self.refresh();
    }
    pub(super) fn sort_results(&self, column: i32) {
        let column = match column {
            0 => state::Column::Message,
            1 => state::Column::Folder,
            2 => state::Column::Outcome,
            _ => return,
        };
        let mut s = self.session.borrow_mut();
        let Some(entry) = s.selected_entry_mut() else {
            return;
        };
        entry.presentation.sort.select(column);
        drop(s);
        self.clear_operation();
        self.refresh();
    }
    pub(super) fn navigate_message(&self, direction: i32, page: usize) {
        let rows = self.ui.get_items();
        if rows.row_count() == 0 {
            return;
        }
        let selected = self
            .session
            .borrow()
            .selected_entry()
            .and_then(|entry| entry.presentation.selected);
        let current = rows
            .iter()
            .position(|row| Some(row.original_index as usize) == selected)
            .unwrap_or(0);
        let page = page.max(1);
        let next = match direction {
            -2 => 0,
            2 => rows.row_count() - 1,
            -3 => current.saturating_sub(page),
            3 => (current + page).min(rows.row_count() - 1),
            -1 => current.saturating_sub(1),
            _ => (current + 1).min(rows.row_count() - 1),
        };
        if let Some(row) = rows.row_data(next) {
            self.select_item(row.original_index as usize);
        }
    }
    pub(super) fn type_select(&self, prefix: &str) {
        let prefix = prefix.to_lowercase();
        if let Some(row) = self.ui.get_items().iter().find(|row| {
            format!("{}{}", row.path.prefix, row.path.suffix)
                .to_lowercase()
                .starts_with(&prefix)
        }) {
            self.select_item(row.original_index as usize);
        }
    }
}

/// Selected input roots associated with earlier verified copy receipts, for advisory navigation.
pub(super) fn verified_roots(entry: &Entry) -> Vec<PathBuf> {
    if entry.request.request().mode != RunMode::Create || completion_state(entry).is_none() {
        return Vec::new();
    }
    entry
        .report
        .as_ref()
        .and_then(|handle| {
            handle.with_report(|report| {
                if !(0..report.items().len()).any(|index| report.reveal_path(index).is_some()) {
                    return Vec::new();
                }
                if let eml_core::OutputLocation::Directory(path) = &entry.request.request().output {
                    return vec![path.clone()];
                }
                let mut roots = Vec::new();
                for root in &entry.request.request().paths {
                    let mut candidate = None;
                    for index in 0..report.items().len() {
                        if report.reveal_path(index).is_none() {
                            continue;
                        }
                        if let Some(source) = report.source_path(index) {
                            if source == *root {
                                candidate = root.parent().map(Path::to_owned);
                                break;
                            }
                            if source.starts_with(root) {
                                candidate = Some(root.clone());
                            }
                        }
                    }
                    if let Some(path) = candidate
                        && !roots.contains(&path)
                    {
                        roots.push(path);
                    }
                }
                roots
            })
        })
        .unwrap_or_default()
}
impl DesktopController {
    /// Output roots retain native paths and earlier receipt qualification.
    pub fn output_roots(&self) -> Vec<PathBuf> {
        self.session
            .borrow()
            .selected_entry()
            .map_or_else(Vec::new, verified_roots)
    }
    /// Intended source path for advisory Show Original, including non-UTF-8 native names.
    pub fn selected_original(&self) -> Option<PathBuf> {
        self.selected_path(false)
    }
}

impl DesktopController {
    pub(super) fn can_close_run(&self, s: &SessionRuntime, entry: &Entry) -> bool {
        !s.is_closing()
            && s.state.confirmation.is_none()
            && (completion_state(entry).is_some() || entry.launch_error.is_some())
            && !s.export.as_ref().is_some_and(|job| job.run == entry.id)
            && !self
                .export_draft
                .borrow()
                .as_ref()
                .is_some_and(|draft| draft.source.run == entry.id)
    }
    pub(super) fn select_session(&self, token: &str) {
        let target = {
            let s = self.session.borrow();
            s.entries
                .iter()
                .position(|entry| entry.id.to_string() == token)
                .map(|index| (false, index))
                .or_else(|| {
                    s.state
                        .queue
                        .waiting_requests()
                        .position(|(id, _)| id.to_string() == token)
                        .map(|index| (true, index))
                })
        };
        match target {
            Some((false, index)) => self.select_run(index),
            Some((true, index)) => self.select_waiting(index),
            None => {}
        }
    }
    pub(super) fn close_session(&self, token: &str) {
        let mut s = self.session.borrow_mut();
        if s.is_closing() || s.state.confirmation.is_some() {
            return;
        }
        if let Some(index) = s
            .entries
            .iter()
            .position(|entry| entry.id.to_string() == token)
        {
            if self.can_close_run(&s, &s.entries[index]) {
                drop(s);
                self.dismiss_run(index);
            } else {
                let label = format!(
                    "Run {} has started or is saving a report. Use Stop, or finish saving, before closing it.",
                    s.entries[index].id
                );
                drop(s);
                self.message(&label);
                self.refresh();
            }
            return;
        }
        let id = s
            .state
            .queue
            .waiting_requests()
            .find(|(id, _)| id.to_string() == token)
            .map(|(id, _)| id);
        if let Some(id) = id {
            s.state.queue.remove_waiting(id);
            if s.state.view == state::View::Waiting(id) {
                s.state.view = state::View::Draft;
            }
        }
        drop(s);
        self.clear_operation();
        self.refresh();
    }
}
